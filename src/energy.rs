//! The house's energy flow: which meters are grid, production, storage and load
//! (the energy flow monitor's nodes, or a guess from the meters), and the flow
//! breakdown computed from their power: production, consumption, grid
//! import/export, battery charge/discharge and state of charge.
//!
//! Shared by `lox energy flow`, the MCP server and the TUI's energy screen; each
//! supplies the live values its own way (HTTP `/all`, or the WebSocket state
//! store) and gets the same interpretation back.
//!
//! Values are what the Miniserver reports (the EFM's `actualFormat`, usually
//! `%.3fkW`). Loxone reports the grid with + import / − export and the storage
//! like a source (+ discharging); [`Flow::batt`] flips the latter so that +
//! means charging.

use std::collections::BTreeMap;

use anyhow::Result;
use loxone_client::LoxClient;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Below this (in the Miniserver's power unit) a flow counts as idle.
pub const IDLE: f64 = 0.01;

/// Below this production the self-use share is not meaningful.
const PV_MIN: f64 = 0.05;

/// Energy role of a node of the energy flow monitor (`details.nodes[].nodeType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Grid,
    Production,
    Storage,
    Load,
}

impl Role {
    pub fn from_node_type(t: &str) -> Option<Role> {
        Some(match t {
            "Grid" => Role::Grid,
            "Production" => Role::Production,
            "Storage" => Role::Storage,
            "Load" => Role::Load,
            _ => return None,
        })
    }

    /// A role as written in `tui.yaml` (`roles:`).
    pub fn parse(s: &str) -> Option<Role> {
        Some(match s.to_lowercase().as_str() {
            "grid" => Role::Grid,
            "pv" | "production" | "solar" => Role::Production,
            "battery" | "storage" => Role::Storage,
            "load" | "home" | "consumption" => Role::Load,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Grid => "grid",
            Role::Production => "production",
            Role::Storage => "storage",
            Role::Load => "load",
        }
    }

    /// The EFM output that carries the sum of this role's nodes.
    pub fn efm_state(self) -> Option<&'static str> {
        match self {
            Role::Grid => Some("Gpwr"),
            Role::Production => Some("Ppwr"),
            Role::Storage => Some("Spwr"),
            Role::Load => None,
        }
    }
}

/// Control types of the energy flow monitor.
pub fn is_flow_monitor(typ: &str) -> bool {
    matches!(typ, "EFM" | "EnergyManager2")
}

/// A node of the energy flow: its role, the meter behind it, the EFM state
/// carrying its power.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeSpec {
    pub role: Role,
    /// UUID of the meter control behind this node
    pub ctrl: Option<String>,
    /// UUID of the EFM state carrying this node's power (`actualN`)
    pub power_state: Option<String>,
    /// The node's title in the EFM
    pub title: Option<String>,
}

/// A top-level meter of the structure.
#[derive(Debug, Clone, PartialEq)]
pub struct MeterInfo {
    pub uuid: String,
    pub name: String,
    /// `details.type`: `unidirectional`, `bidirectional` or `storage`
    pub meter_type: String,
}

/// Where the energy flow comes from in a structure.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    /// UUID of the energy flow monitor, if the installation has one
    pub efm: Option<String>,
    pub nodes: Vec<NodeSpec>,
    /// All top-level meters
    pub meters: Vec<MeterInfo>,
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(String::from)
}

/// The top-level nodes of an EFM (`details.nodes`); nested loads are parts of
/// their parent and not counted again.
pub fn efm_nodes(details: &Value) -> Vec<NodeSpec> {
    details
        .get("nodes")
        .and_then(|n| n.as_array())
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|n| {
                    Some(NodeSpec {
                        role: Role::from_node_type(n.get("nodeType")?.as_str()?)?,
                        ctrl: text(n, "ctrlUuid"),
                        power_state: text(n, "actualEfmState"),
                        title: text(n, "title"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Without an EFM: guess a meter's role from its type and name.
pub fn heuristic_role(name: &str, meter_type: &str) -> Option<Role> {
    let lname = name.to_lowercase();
    if meter_type == "storage" {
        Some(Role::Storage)
    } else if lname.contains("pv") || lname.contains("solar") || lname.contains("photovolt") {
        Some(Role::Production)
    } else if meter_type == "bidirectional"
        && (lname.contains("grid") || lname.contains("netz") || lname.contains("bezug"))
    {
        Some(Role::Grid)
    } else {
        None
    }
}

impl Layout {
    /// The EFM (the first top-level one) and its nodes, else nodes guessed from
    /// the meters. Role overrides are applied separately ([`apply_overrides`]).
    pub fn from_structure(st: &Value) -> Layout {
        let mut controls: Vec<(&String, &Value)> = st
            .get("controls")
            .and_then(|c| c.as_object())
            .map(|o| o.iter().collect())
            .unwrap_or_default();
        controls.sort_by(|a, b| {
            let name = |c: &Value| text(c, "name").unwrap_or_default();
            name(a.1).cmp(&name(b.1)).then(a.0.cmp(b.0))
        });
        let typ = |c: &Value| text(c, "type").unwrap_or_default();
        let meters: Vec<MeterInfo> = controls
            .iter()
            .filter(|(_, c)| typ(c) == "Meter")
            .map(|(u, c)| MeterInfo {
                uuid: u.to_string(),
                name: text(c, "name").unwrap_or_default(),
                meter_type: c
                    .pointer("/details/type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
            .collect();
        let efm = controls.iter().find(|(_, c)| is_flow_monitor(&typ(c)));
        let mut nodes = efm
            .and_then(|(_, c)| c.get("details"))
            .map(efm_nodes)
            .unwrap_or_default();
        if nodes.is_empty() {
            nodes = meters
                .iter()
                .filter_map(|m| {
                    Some(NodeSpec {
                        role: heuristic_role(&m.name, &m.meter_type)?,
                        ctrl: Some(m.uuid.clone()),
                        power_state: None,
                        title: None,
                    })
                })
                .collect();
        }
        Layout {
            efm: efm.map(|(u, _)| u.to_string()),
            nodes,
            meters,
        }
    }
}

/// Apply role overrides (`tui.yaml` `roles:`, control name or UUID → role): the
/// named meter becomes a node of that role, replacing a node it already was.
/// `is_control` says whether a key is the UUID of a control; other keys match a
/// meter name (case-insensitive).
pub fn apply_overrides(
    nodes: &mut Vec<NodeSpec>,
    overrides: &BTreeMap<String, String>,
    meters: &[MeterInfo],
    is_control: impl Fn(&str) -> bool,
) {
    for (key, role) in overrides {
        let Some(role) = Role::parse(role) else {
            continue;
        };
        let Some(uuid) = (if is_control(key) {
            Some(key.clone())
        } else {
            meters
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(key))
                .map(|m| m.uuid.clone())
        }) else {
            continue;
        };
        nodes.retain(|n| n.ctrl.as_deref() != Some(uuid.as_str()));
        nodes.push(NodeSpec {
            role,
            ctrl: Some(uuid),
            power_state: None,
            title: None,
        });
    }
}

#[derive(Default, Deserialize)]
struct RolePrefs {
    #[serde(default)]
    roles: BTreeMap<String, String>,
}

/// Role overrides from `~/.lox/tui.yaml` (`roles:`); none when it is missing.
pub fn load_overrides() -> BTreeMap<String, String> {
    std::fs::read_to_string(crate::config::Config::dir().join("tui.yaml"))
        .ok()
        .and_then(|s| serde_yaml::from_str::<RolePrefs>(&s).ok())
        .unwrap_or_default()
        .roles
}

// ── The flow ────────────────────────────────────────────────────────────────

/// A node's live values.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeReading {
    pub role: Role,
    /// Power: the EFM node state, else the meter's
    pub power: Option<f64>,
    /// State of charge (%) of a storage meter
    pub soc: Option<f64>,
    /// The state the power was read from (or would be), for history
    pub uuid: Option<String>,
}

/// Current power flows. Grid: + import / − export. Battery: + charging.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Flow {
    pub pv: Option<f64>,
    pub grid: Option<f64>,
    pub batt: Option<f64>,
    pub soc: Option<f64>,
    pub home: Option<f64>,
    /// State UUIDs behind the values (sparklines)
    pub pv_uuid: Option<String>,
    pub grid_uuid: Option<String>,
    pub home_uuid: Option<String>,
}

type Efm<'a> = &'a dyn Fn(&str) -> Option<(f64, Option<String>)>;

/// Sum of a role's nodes; the EFM's aggregate output as the last resort.
fn sum(nodes: &[NodeReading], role: Role, efm: Efm) -> (Option<f64>, Option<String>) {
    let mut total = None;
    let mut first: Option<String> = None;
    for n in nodes.iter().filter(|n| n.role == role) {
        if let Some(p) = n.power {
            *total.get_or_insert(0.0) += p;
        }
        if first.is_none() {
            first = n.uuid.clone();
        }
    }
    if total.is_none()
        && let Some((v, u)) = role.efm_state().and_then(efm)
    {
        total = Some(v);
        if first.is_none() {
            first = u;
        }
    }
    (total, first)
}

/// The flow from the nodes' readings. `efm` reads an output of the energy
/// flow monitor by name (`Gpwr`, `Ppwr`, `Spwr`, `Ssoc`): value and state UUID.
pub fn flow(nodes: &[NodeReading], efm: impl Fn(&str) -> Option<(f64, Option<String>)>) -> Flow {
    let efm: Efm = &efm;
    let (pv, pv_uuid) = sum(nodes, Role::Production, efm);
    let (grid, grid_uuid) = sum(nodes, Role::Grid, efm);
    // Loxone reports storage like a source (+ discharging, as the grid's +
    // import); flipped so + means charging
    let batt = sum(nodes, Role::Storage, efm).0.map(|b| -b);
    let soc = nodes
        .iter()
        .filter(|n| n.role == Role::Storage)
        .find_map(|n| n.soc)
        .or_else(|| efm("Ssoc").map(|(v, _)| v));
    // Home: the Load node if it carries power, else production + grid − storage
    let (load, home_uuid) = sum(nodes, Role::Load, efm);
    let home = load.or_else(|| {
        if pv.is_none() && grid.is_none() {
            None
        } else {
            Some(pv.unwrap_or(0.0) + grid.unwrap_or(0.0) - batt.unwrap_or(0.0))
        }
    });
    Flow {
        pv,
        grid,
        batt,
        soc,
        home,
        pv_uuid,
        grid_uuid,
        home_uuid,
    }
}

impl Flow {
    /// Share of production not exported (%), while there is production.
    pub fn self_use(&self) -> Option<f64> {
        let pv = self.pv.filter(|p| *p > PV_MIN)?;
        let export = self.grid.map(|g| (-g).max(0.0)).unwrap_or(0.0);
        Some((((pv - export) / pv) * 100.0).clamp(0.0, 100.0))
    }

    /// `importing`, `exporting` or `idle`.
    pub fn grid_direction(&self) -> Option<&'static str> {
        self.grid.map(|g| {
            if g > IDLE {
                "importing"
            } else if g < -IDLE {
                "exporting"
            } else {
                "idle"
            }
        })
    }

    /// `charging`, `discharging` or `idle`.
    pub fn battery_direction(&self) -> Option<&'static str> {
        self.batt.map(|b| {
            if b > IDLE {
                "charging"
            } else if b < -IDLE {
                "discharging"
            } else {
                "idle"
            }
        })
    }
}

// ── Reading over HTTP (`/dev/sps/io/{uuid}/all`) ────────────────────────────

/// What `/dev/sps/io/{uuid}/all` reports, as numbers. An EFM answers its
/// outputs (`Gpwr`, `Ppwr`, `Cpwr`, `Spwr`, …) as `<output>` children and its
/// node states (`actualN`) as `<SpecialState uuid=..>`; a meter its power as
/// the main value and its state of charge as output `Slvl`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AllValues {
    pub value: Option<f64>,
    pub outputs: BTreeMap<String, f64>,
    /// SpecialState UUID → value
    pub special: BTreeMap<String, f64>,
}

pub fn parse_all(xml: &str) -> AllValues {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut out = AllValues::default();
    let mut root = true;
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let attr = |k: &str| {
                    e.attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == k.as_bytes())
                        .and_then(|a| a.unescape_value().ok().map(|v| v.to_string()))
                };
                let num = |k: &str| attr(k).and_then(|v| v.trim().parse::<f64>().ok());
                if root {
                    out.value = num("value");
                    root = false;
                } else {
                    match e.name().as_ref() {
                        b"output" => {
                            if let (Some(n), Some(v)) = (attr("name"), num("value")) {
                                out.outputs.insert(n, v);
                            }
                        }
                        b"SpecialState" => {
                            if let (Some(u), Some(v)) = (attr("uuid"), num("value")) {
                                out.special.insert(u, v);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// The unit of a Loxone number format: `%.3fkW` → `kW`, `%.1f%%` → `%`.
pub fn unit_of(format: &str) -> Option<String> {
    let i = format.find('%')?;
    let rest = &format[i + 1..];
    let conv = rest.find(|c: char| c.is_ascii_alphabetic())?;
    let unit = rest[conv + 1..].replace("%%", "%").trim().to_string();
    (!unit.is_empty()).then_some(unit)
}

/// A control as the reports name it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ControlInfo {
    pub name: String,
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
}

/// One node of the flow.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct NodePower {
    pub role: Role,
    /// The node's title in the energy flow monitor, else the meter's name
    pub name: String,
    /// UUID of the meter behind the node
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    /// Power as the Miniserver reports it (storage: + discharging)
    pub power: Option<f64>,
}

/// The energy flow of the house right now.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FlowReport {
    /// The energy flow monitor; absent when the roles were guessed from meter names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor: Option<ControlInfo>,
    /// Unit of the power values, from the Miniserver's format (usually kW)
    pub unit: Option<String>,
    /// Production (PV)
    pub production: Option<f64>,
    /// Consumption of the house
    pub consumption: Option<f64>,
    /// Grid: + import, − export
    pub grid: Option<f64>,
    /// importing, exporting or idle
    pub grid_direction: Option<String>,
    /// Battery: + charging, − discharging
    pub battery: Option<f64>,
    /// charging, discharging or idle
    pub battery_direction: Option<String>,
    /// Battery state of charge (%)
    pub battery_soc: Option<f64>,
    /// Share of production used in the house, not exported (%)
    pub self_use: Option<f64>,
    pub nodes: Vec<NodePower>,
}

fn control_name(st: &Value, uuid: &str) -> Option<String> {
    st.pointer(&format!("/controls/{}/name", uuid))
        .and_then(|n| n.as_str())
        .map(String::from)
}

fn room_name(st: &Value, uuid: &str) -> Option<String> {
    let room = st.pointer(&format!("/controls/{}/room", uuid))?.as_str()?;
    st.pointer(&format!("/rooms/{}/name", room))?
        .as_str()
        .map(String::from)
}

fn format_of(st: &Value, uuid: &str) -> Option<String> {
    st.pointer(&format!("/controls/{}/details/actualFormat", uuid))
        .and_then(|f| f.as_str())
        .map(String::from)
}

/// Whether a UUID names a control or a sub-control of the structure.
fn is_control(st: &Value, uuid: &str) -> bool {
    let Some(ctrls) = st.get("controls").and_then(|c| c.as_object()) else {
        return false;
    };
    ctrls.contains_key(uuid)
        || ctrls.values().any(|c| {
            c.get("subControls")
                .and_then(|s| s.as_object())
                .is_some_and(|s| s.contains_key(uuid))
        })
}

/// The flow from the structure and the `/all` answers (`fetch` by UUID).
pub fn report(
    st: &Value,
    overrides: &BTreeMap<String, String>,
    mut fetch: impl FnMut(&str) -> AllValues,
) -> Result<FlowReport> {
    let mut layout = Layout::from_structure(st);
    apply_overrides(&mut layout.nodes, overrides, &layout.meters, |u| {
        is_control(st, u)
    });
    if layout.nodes.is_empty() {
        anyhow::bail!(
            "no energy flow found: the structure has no Energy Flow Monitor and no meter \
             named like PV, grid or battery. Assign roles in ~/.lox/tui.yaml (roles: meter name → \
             grid|pv|battery|load) or list the meters with `lox energy`."
        );
    }
    let efm_all = layout.efm.as_deref().map(&mut fetch).unwrap_or_default();
    let mut readings = Vec::new();
    let mut nodes = Vec::new();
    for n in &layout.nodes {
        let from_efm = n
            .power_state
            .as_ref()
            .and_then(|u| efm_all.special.get(u).copied());
        // the meter: when the EFM has no value for the node, and for the
        // state of charge of a storage
        let meter = n
            .ctrl
            .as_deref()
            .filter(|_| from_efm.is_none() || n.role == Role::Storage)
            .map(&mut fetch);
        let power = from_efm.or_else(|| meter.as_ref().and_then(|m| m.value));
        let soc = meter
            .as_ref()
            .filter(|_| n.role == Role::Storage)
            .and_then(|m| m.outputs.get("Slvl").copied());
        readings.push(NodeReading {
            role: n.role,
            power,
            soc,
            uuid: None,
        });
        nodes.push(NodePower {
            role: n.role,
            name: n
                .title
                .clone()
                .or_else(|| n.ctrl.as_deref().and_then(|u| control_name(st, u)))
                .unwrap_or_default(),
            uuid: n.ctrl.clone(),
            power,
        });
    }
    let f = flow(&readings, |name| {
        efm_all.outputs.get(name).map(|v| (*v, None))
    });
    let unit = layout
        .efm
        .iter()
        .chain(layout.nodes.iter().filter_map(|n| n.ctrl.as_ref()))
        .find_map(|u| format_of(st, u))
        .and_then(|f| unit_of(&f));
    Ok(FlowReport {
        monitor: layout.efm.as_deref().map(|u| ControlInfo {
            name: control_name(st, u).unwrap_or_default(),
            uuid: u.to_string(),
            room: room_name(st, u),
        }),
        unit,
        production: f.pv,
        consumption: f.home,
        grid: f.grid,
        grid_direction: f.grid_direction().map(String::from),
        battery: f.batt,
        battery_direction: f.battery_direction().map(String::from),
        battery_soc: f.soc,
        self_use: f.self_use(),
        nodes,
    })
}

/// Read the flow from the Miniserver.
pub fn read_flow(lox: &mut LoxClient) -> Result<FlowReport> {
    let st = lox.get_structure()?.clone();
    report(&st, &load_overrides(), |u| {
        parse_all(&lox.get_all(u).unwrap_or_default())
    })
}

// ── Ranking ─────────────────────────────────────────────────────────────────

/// A meter's current power.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MeterPower {
    pub rank: usize,
    pub name: String,
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    /// Current power as the Miniserver reports it
    pub power: f64,
    /// Unit of `power`, from the meter's format
    pub unit: Option<String>,
}

/// Sort by power, highest first (ties by name), and keep the first `top`.
pub fn rank(mut meters: Vec<MeterPower>, top: usize) -> Vec<MeterPower> {
    meters.sort_by(|a, b| b.power.total_cmp(&a.power).then(a.name.cmp(&b.name)));
    meters.truncate(top);
    for (i, m) in meters.iter_mut().enumerate() {
        m.rank = i + 1;
    }
    meters
}

/// The `top` meters by current power, and how many meters there are.
pub fn top_meters(
    lox: &mut LoxClient,
    room: Option<&str>,
    top: usize,
) -> Result<(Vec<MeterPower>, usize)> {
    let st = lox.get_structure()?.clone();
    let meters: Vec<_> = lox
        .list_controls(None, room)?
        .into_iter()
        .filter(|c| c.typ == "Meter")
        .collect();
    let total = meters.len();
    let mut out = Vec::new();
    for c in meters {
        let Some(power) = parse_all(&lox.get_all(&c.uuid).unwrap_or_default()).value else {
            continue;
        };
        out.push(MeterPower {
            rank: 0,
            unit: format_of(&st, &c.uuid).and_then(|f| unit_of(&f)),
            name: c.name,
            uuid: c.uuid,
            room: c.room,
            power,
        });
    }
    Ok((rank(out, top), total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `/all` of a real EFM (Miniserver Gen 2, 15.x).
    const EFM_ALL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<LL control="dev/sps/io/efm/all" value="2.213" Code="200">
    <output name="Gpwr" nr="1"  value="2.213"/>
    <output name="Ppwr" nr="2"  value="0.109"/>
    <output name="Cpwr" nr="3"  value="2.313"/>
    <output name="Spwr" nr="4"  value="-0.009"/>
    <output name="Rest" nr="12"  value="-0.000"/>
    <output name="API" nr="13"  value=""/>
    <SpecialState uuid="s-grid" nr="1"  value="2.213"/>
    <SpecialState uuid="s-pv" nr="2"  value="0.109"/>
    <SpecialState uuid="s-balcony" nr="3"  value="0.000"/>
    <SpecialState uuid="s-batt" nr="4"  value="-0.009"/>
    <SpecialState uuid="s-load" nr="5"  value="2.313"/> </LL>"#;

    const BATT_ALL: &str = r#"<LL control="dev/sps/io/batt/all" value="-0.009" Code="200">
    <output name="Pf" nr="1"  value="-0.009"/>
    <output name="Slvl" nr="16"  value="9.000"/>
    <output name="API" nr="17"  value=""/> </LL>"#;

    fn node(t: &str, title: &str, ctrl: &str, st: &str) -> Value {
        json!({ "nodeType": t, "title": title, "ctrlUuid": ctrl, "actualEfmState": st })
    }

    fn structure() -> Value {
        json!({
            "rooms": { "r1": { "name": "Zentral" } },
            "controls": {
                "efm": {
                    "name": "Energiefluss", "type": "EFM", "room": "r1",
                    "details": { "actualFormat": "%.3fkW", "nodes": [
                        node("Grid", "Netzbezug", "grid", "s-grid"),
                        node("Production", "PV-Strom", "pv", "s-pv"),
                        node("Production", "Balkonkraftwerk", "balcony", "s-balcony"),
                        node("Storage", "Speicher", "batt", "s-batt"),
                        { "nodeType": "Load", "title": "Stromverbrauch", "ctrlUuid": "load",
                          "actualEfmState": "s-load",
                          "nodes": [ node("Load", "Heizung", "heat", "s-heat") ] },
                    ] },
                },
                "grid": { "name": "Netzbezug", "type": "Meter", "details": { "type": "bidirectional", "actualFormat": "%.3fkW" } },
                "pv": { "name": "PV-Strom", "type": "Meter", "details": { "type": "unidirectional" } },
                "balcony": { "name": "Balkonkraftwerk", "type": "Meter", "details": { "type": "unidirectional" } },
                "batt": { "name": "Speicher", "type": "Meter", "details": { "type": "storage" } },
                "load": { "name": "Stromverbrauch", "type": "Meter", "details": { "type": "unidirectional" } },
                "heat": { "name": "Heizung", "type": "Meter", "details": { "type": "unidirectional" } },
            }
        })
    }

    #[test]
    fn parse_all_reads_outputs_and_special_states() {
        let a = parse_all(EFM_ALL);
        assert_eq!(a.value, Some(2.213));
        assert_eq!(a.outputs.get("Gpwr"), Some(&2.213));
        assert_eq!(a.outputs.get("Spwr"), Some(&-0.009));
        assert!(!a.outputs.contains_key("API"), "empty values are skipped");
        assert_eq!(a.special.get("s-load"), Some(&2.313));
        assert_eq!(parse_all(BATT_ALL).outputs.get("Slvl"), Some(&9.0));
        assert_eq!(parse_all("").value, None);
    }

    #[test]
    fn units_from_formats() {
        assert_eq!(unit_of("%.3fkW").as_deref(), Some("kW"));
        assert_eq!(unit_of("%.1f%%").as_deref(), Some("%"));
        assert_eq!(unit_of("%.0f W").as_deref(), Some("W"));
        assert_eq!(unit_of("%.3f"), None);
        assert_eq!(unit_of(""), None);
    }

    #[test]
    fn layout_takes_the_efm_top_level_nodes() {
        let l = Layout::from_structure(&structure());
        assert_eq!(l.efm.as_deref(), Some("efm"));
        let roles: Vec<Role> = l.nodes.iter().map(|n| n.role).collect();
        assert_eq!(
            roles,
            [
                Role::Grid,
                Role::Production,
                Role::Production,
                Role::Storage,
                Role::Load
            ]
        );
        assert_eq!(l.meters.len(), 6);
    }

    #[test]
    fn layout_without_efm_guesses_from_meter_names() {
        let st = json!({ "controls": {
            "a": { "name": "PV Dach", "type": "Meter", "details": { "type": "unidirectional" } },
            "b": { "name": "Netz", "type": "Meter", "details": { "type": "bidirectional" } },
            "c": { "name": "Akku", "type": "Meter", "details": { "type": "storage" } },
            "d": { "name": "Netzwerk", "type": "Meter", "details": { "type": "unidirectional" } },
        }});
        let l = Layout::from_structure(&st);
        assert!(l.efm.is_none());
        let got: Vec<(Role, &str)> = l
            .nodes
            .iter()
            .map(|n| (n.role, n.ctrl.as_deref().unwrap()))
            .collect();
        assert_eq!(
            got,
            [
                (Role::Storage, "c"),
                (Role::Grid, "b"),
                (Role::Production, "a")
            ]
        );
    }

    #[test]
    fn overrides_replace_a_node_by_name_or_uuid() {
        let l = Layout::from_structure(&structure());
        let mut nodes = l.nodes.clone();
        let ov: BTreeMap<String, String> = [
            ("heizung".to_string(), "load".to_string()),
            ("balcony".to_string(), "storage".to_string()),
            ("nope".to_string(), "grid".to_string()),
            ("PV-Strom".to_string(), "bogus".to_string()),
        ]
        .into();
        apply_overrides(&mut nodes, &ov, &l.meters, |u| u == "balcony");
        assert_eq!(nodes.len(), 6);
        let b = nodes
            .iter()
            .find(|n| n.ctrl.as_deref() == Some("balcony"))
            .unwrap();
        assert_eq!(b.role, Role::Storage);
        assert!(b.power_state.is_none());
        assert!(nodes.iter().any(|n| n.ctrl.as_deref() == Some("heat")));
    }

    fn reading(role: Role, power: Option<f64>) -> NodeReading {
        NodeReading {
            role,
            power,
            soc: None,
            uuid: None,
        }
    }

    #[test]
    fn flow_sums_roles_and_derives_home() {
        let nodes = [
            reading(Role::Production, Some(3.0)),
            reading(Role::Production, Some(0.5)),
            reading(Role::Grid, Some(-1.0)),
            reading(Role::Storage, Some(-0.5)),
        ];
        let f = flow(&nodes, |_| None);
        assert_eq!(f.pv, Some(3.5));
        assert_eq!(f.grid, Some(-1.0));
        assert_eq!(f.batt, Some(0.5), "storage − discharging → + charging");
        // no load node: production + grid − charging
        assert_eq!(f.home, Some(2.0));
        assert_eq!(f.grid_direction(), Some("exporting"));
        assert_eq!(f.battery_direction(), Some("charging"));
        let su = f.self_use().unwrap();
        assert!((su - 2.5 / 3.5 * 100.0).abs() < 1e-9, "{}", su);
    }

    #[test]
    fn flow_falls_back_to_efm_aggregates() {
        let nodes = [
            reading(Role::Production, None),
            reading(Role::Grid, None),
            reading(Role::Storage, None),
        ];
        let f = flow(&nodes, |name| match name {
            "Ppwr" => Some((0.0, Some("u-ppwr".into()))),
            "Gpwr" => Some((0.004, None)),
            "Spwr" => Some((1.5, None)),
            "Ssoc" => Some((74.0, None)),
            _ => None,
        });
        assert_eq!(f.pv, Some(0.0));
        assert_eq!(f.pv_uuid.as_deref(), Some("u-ppwr"));
        assert_eq!(f.batt, Some(-1.5));
        assert_eq!(f.soc, Some(74.0));
        assert_eq!(f.grid_direction(), Some("idle"));
        assert_eq!(f.battery_direction(), Some("discharging"));
        assert_eq!(f.self_use(), None, "no production");
    }

    #[test]
    fn flow_without_values_is_empty() {
        let f = flow(&[reading(Role::Grid, None)], |_| None);
        assert_eq!(f, Flow::default());
        assert_eq!(f.grid_direction(), None);
    }

    #[test]
    fn report_from_a_real_efm() {
        let st = structure();
        let mut fetched = Vec::new();
        let r = report(&st, &BTreeMap::new(), |u| {
            fetched.push(u.to_string());
            match u {
                "efm" => parse_all(EFM_ALL),
                "batt" => parse_all(BATT_ALL),
                _ => AllValues::default(),
            }
        })
        .unwrap();
        // only the EFM and the storage (state of charge) are fetched
        assert_eq!(fetched, ["efm", "batt"]);
        assert_eq!(r.monitor.as_ref().unwrap().name, "Energiefluss");
        assert_eq!(r.monitor.as_ref().unwrap().room.as_deref(), Some("Zentral"));
        assert_eq!(r.unit.as_deref(), Some("kW"));
        assert_eq!(r.production, Some(0.109));
        assert_eq!(r.consumption, Some(2.313));
        assert_eq!(r.grid, Some(2.213));
        assert_eq!(r.grid_direction.as_deref(), Some("importing"));
        assert_eq!(r.battery, Some(0.009));
        assert_eq!(r.battery_direction.as_deref(), Some("idle"));
        assert_eq!(r.battery_soc, Some(9.0));
        assert_eq!(r.self_use, Some(100.0));
        assert_eq!(r.nodes.len(), 5);
        assert_eq!(r.nodes[2].name, "Balkonkraftwerk");
        assert_eq!(r.nodes[2].power, Some(0.0));
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["nodes"][3]["role"], "storage");
    }

    #[test]
    fn report_reads_meters_when_the_efm_has_no_node_value() {
        let st = structure();
        let r = report(&st, &BTreeMap::new(), |u| match u {
            "grid" => parse_all(r#"<LL value="-1.25" Code="200"/>"#),
            _ => AllValues::default(),
        })
        .unwrap();
        assert_eq!(r.grid, Some(-1.25));
        assert_eq!(r.grid_direction.as_deref(), Some("exporting"));
        assert_eq!(r.production, None);
    }

    #[test]
    fn report_without_any_flow_is_an_error() {
        let st = json!({ "controls": {
            "x": { "name": "Waschmaschine", "type": "Meter", "details": { "type": "unidirectional" } }
        }});
        let e = report(&st, &BTreeMap::new(), |_| AllValues::default()).unwrap_err();
        assert!(e.to_string().contains("tui.yaml"), "{}", e);
    }

    #[test]
    fn rank_sorts_by_power_and_truncates() {
        let m = |name: &str, power: f64| MeterPower {
            rank: 0,
            name: name.into(),
            uuid: name.into(),
            room: None,
            power,
            unit: Some("kW".into()),
        };
        let r = rank(vec![m("a", 0.1), m("b", 2.0), m("c", -0.5), m("d", 2.0)], 3);
        let got: Vec<(usize, &str)> = r.iter().map(|m| (m.rank, m.name.as_str())).collect();
        assert_eq!(got, [(1, "b"), (2, "d"), (3, "a")]);
    }
}
