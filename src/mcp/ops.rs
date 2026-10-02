//! Miniserver operations behind the MCP tools.
//!
//! Everything here is synchronous (`reqwest::blocking`) and runs on tokio's
//! blocking pool. The output types derive `JsonSchema`, so every tool
//! publishes an `outputSchema` and returns matching `structuredContent`.

use anyhow::{Result, bail};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::actions::Action;
use crate::client::{Control, LoxClient};
use crate::commands::inspect::{is_energy_type, is_sensor_type};
use crate::config::Config;
use crate::json_val_str;
use crate::scene::{Scene, SceneStep};

// ── Errors ────────────────────────────────────────────────────────────────────

/// A tool failure with an explicit error code (on top of the CLI's codes).
#[derive(Debug)]
pub struct ToolError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ToolError {}

pub fn tool_error(code: &'static str, message: impl std::fmt::Display) -> anyhow::Error {
    ToolError {
        code,
        message: message.to_string(),
    }
    .into()
}

pub fn invalid(message: impl std::fmt::Display) -> anyhow::Error {
    tool_error("invalid_arguments", message)
}

/// The CLI's `-o json` error envelope for a failed tool call.
pub fn error_envelope(e: &anyhow::Error) -> Value {
    let code = match e.downcast_ref::<ToolError>() {
        Some(te) => te.code,
        None => crate::categorize_error(e),
    };
    let mut message = format!("{:#}", e);
    if code == "ambiguous_control" {
        // the resolver speaks CLI; tool callers pass `room` instead of `--room`
        message = message.replace(
            "Use [Room] qualifier or --room flag.",
            "Pass `room`, write the name as 'Name [Room]', or use the UUID.",
        );
    }
    json!({ "ok": false, "error": code, "message": message })
}

// ── Output types ──────────────────────────────────────────────────────────────

/// A control as the tools report it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ControlRef {
    /// Control name as configured in Loxone Config
    pub name: String,
    /// Loxone control type, e.g. Jalousie, LightControllerV2, Switch
    #[serde(rename = "type")]
    pub typ: String,
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// The MCP tool that operates this control, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

impl From<&Control> for ControlRef {
    fn from(c: &Control) -> Self {
        Self {
            name: c.name.clone(),
            typ: c.typ.clone(),
            uuid: c.uuid.clone(),
            room: c.room.clone(),
            category: c.cat.clone(),
            tool: tool_for_type(&c.typ).map(str::to_string),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Room {
    pub name: String,
    pub uuid: String,
    /// Number of controls in this room
    pub controls: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RoomList {
    pub count: usize,
    pub rooms: Vec<Room>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ControlList {
    pub count: usize,
    pub controls: Vec<ControlRef>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ControlState {
    #[serde(flatten)]
    pub control: ControlRef,
    /// Main value (number when numeric)
    pub value: Value,
    /// State attributes, e.g. StatePos for blinds (0 = open, 1 = closed)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub states: BTreeMap<String, Value>,
    /// Named outputs of the control
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, Value>,
    /// The control is secured (visualization password)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub secured: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Sensor {
    #[serde(flatten)]
    pub control: ControlRef,
    pub value: Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SensorList {
    pub kind: String,
    pub count: usize,
    pub sensors: Vec<Sensor>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Mood {
    /// Mood ID, for the `light` tool (action=mood)
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MoodList {
    pub control: ControlRef,
    pub moods: Vec<Mood>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SceneInfo {
    /// Scene ID for `run_scene`
    pub scene: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<usize>,
    /// Set when the scene file could not be loaded
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SceneList {
    pub count: usize,
    pub scenes: Vec<SceneInfo>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SystemStatus {
    pub firmware: String,
    /// running, started, error, booting, updating or unknown
    pub plc_state: String,
    pub plc_running: bool,
    /// Raw heap string, e.g. "12000/32000kB"
    pub heap: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heap_used_kb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heap_total_kb: Option<f64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ActionResult {
    pub ok: bool,
    pub control: ControlRef,
    /// The action in words, e.g. "pos 30"
    pub action: String,
    /// Loxone commands sent (or that would be sent); PINs are masked
    pub commands: Vec<String>,
    /// The equivalent `lox` command line
    pub cli: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
    /// High-risk action that needs the user's confirmation
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub needs_confirmation: bool,
    /// The user confirmed this high-risk action
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirmed: bool,
    /// Miniserver response code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<Value>,
    /// Miniserver response value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StepResult {
    pub control: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SceneRun {
    /// All steps succeeded and the run was not cancelled
    pub ok: bool,
    pub scene: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
    /// The client cancelled the run; remaining steps were skipped
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cancelled: bool,
    pub steps: Vec<StepResult>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// A Miniserver value as JSON: numbers become numbers, everything else stays text.
pub fn typed(s: &str) -> Value {
    if let Ok(i) = s.parse::<i64>() {
        return json!(i);
    }
    match s.parse::<f64>() {
        Ok(f) if f.is_finite() => json!(f),
        _ => Value::String(s.to_string()),
    }
}

/// The MCP tool that operates a control type, if any.
pub fn tool_for_type(typ: &str) -> Option<&'static str> {
    Some(match typ {
        "Jalousie" | "CentralJalousie" => "blind",
        "LightControllerV2"
        | "LightController"
        | "CentralLightController"
        | "ColorPickerV2"
        | "ColorPicker"
        | "Dimmer"
        | "EIBDimmer" => "light",
        "Switch" | "Pushbutton" | "TimedSwitch" => "switch",
        "IRoomControllerV2" | "IRoomController" => "thermostat",
        "Gate" | "CentralGate" => "gate",
        "Alarm" => "alarm",
        t if t.contains("DoorLock") => "door",
        _ => return None,
    })
}

type ValueMap = BTreeMap<String, Value>;

/// Parse `/dev/sps/io/{uuid}/all`: the main value, `State*`-style attributes,
/// and named outputs. Outputs come either as `n1`/`v1`, `n2`/`v2`, … attributes
/// on the root element, or as `<output name=".." nr=".." value=".."/>` children
/// (LightControllerV2 circuits); a name used twice gets its `nr` appended.
pub fn parse_all_xml(xml: &str) -> (Option<Value>, ValueMap, ValueMap) {
    use quick_xml::Reader;
    use quick_xml::events::{BytesStart, Event};

    let attrs_of = |e: &BytesStart| -> Vec<(String, String)> {
        e.attributes()
            .flatten()
            .map(|a| {
                let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
                let val = a
                    .unescape_value()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|_| String::from_utf8_lossy(&a.value).to_string());
                (key, val)
            })
            .collect()
    };
    let mut root: Option<Vec<(String, String)>> = None;
    let mut children: Vec<Vec<(String, String)>> = Vec::new();
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if root.is_none() {
                    root = Some(attrs_of(&e));
                } else if e.name().as_ref() == b"output" {
                    children.push(attrs_of(&e));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    let attrs = root.unwrap_or_default();
    let get = |k: &str| attrs.iter().find(|(key, _)| key == k).map(|(_, v)| v);
    let value = get("value").map(|v| typed(v));
    let mut states = BTreeMap::new();
    let mut outputs = BTreeMap::new();
    for (k, v) in &attrs {
        let is_indexed = |p: char| {
            k.strip_prefix(p)
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        };
        if is_indexed('n') {
            if !v.is_empty() {
                let val = get(&format!("v{}", &k[1..]))
                    .map(|x| typed(x))
                    .unwrap_or(Value::Null);
                outputs.insert(v.clone(), val);
            }
        } else if is_indexed('v') || matches!(k.as_str(), "control" | "value" | "Code") {
            continue;
        } else {
            states.insert(k.clone(), typed(v));
        }
    }
    let field = |c: &[(String, String)], k: &str| {
        c.iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let named = |c: &&Vec<(String, String)>| !field(c, "name").is_empty();
    for c in children.iter().filter(named) {
        let name = field(c, "name");
        let shared = children
            .iter()
            .filter(named)
            .filter(|o| field(o, "name") == name)
            .count()
            > 1;
        let key = if shared {
            format!("{} ({})", name, field(c, "nr"))
        } else {
            name
        };
        outputs.insert(key, typed(&field(c, "value")));
    }
    (value, states, outputs)
}

/// Commands as shown to the model: an alarm PIN is never echoed back.
pub fn visible_commands(action: &Action) -> Vec<String> {
    let cmds = action.commands();
    match action {
        Action::Alarm { pin: Some(pin), .. } => cmds
            .into_iter()
            .map(|c| match c.strip_suffix(&format!("/{}", pin)) {
                Some(base) => format!("{}/****", base),
                None => c,
            })
            .collect(),
        _ => cmds,
    }
}

// ── Read operations ───────────────────────────────────────────────────────────

pub fn list_rooms(lox: &mut LoxClient) -> Result<RoomList> {
    let controls = lox.list_controls(None, None)?;
    let structure = lox.get_structure()?;
    let mut rooms: Vec<(String, String)> = structure
        .get("rooms")
        .and_then(|r| r.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(uuid, r)| {
                    r.get("name")
                        .and_then(|n| n.as_str())
                        .map(|n| (n.to_string(), uuid.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    rooms.sort();
    let rooms: Vec<Room> = rooms
        .into_iter()
        .map(|(name, uuid)| {
            let controls = controls
                .iter()
                .filter(|c| c.room.as_deref() == Some(name.as_str()))
                .count();
            Room {
                name,
                uuid,
                controls,
            }
        })
        .collect();
    Ok(RoomList {
        count: rooms.len(),
        rooms,
    })
}

pub struct ControlFilter<'a> {
    pub name: Option<&'a str>,
    pub room: Option<&'a str>,
    pub typ: Option<&'a str>,
    pub category: Option<&'a str>,
    pub favorites_only: bool,
}

pub fn list_controls(lox: &mut LoxClient, f: &ControlFilter) -> Result<ControlList> {
    let name = f.name.map(str::to_lowercase);
    let controls: Vec<ControlRef> = lox
        .list_controls_ext(f.typ, f.room, f.category, f.favorites_only)?
        .iter()
        .filter(|c| {
            name.as_deref()
                .is_none_or(|n| c.name.to_lowercase().contains(n))
        })
        .map(ControlRef::from)
        .collect();
    Ok(ControlList {
        count: controls.len(),
        controls,
    })
}

/// Resolve a control name like the CLI does.
pub fn resolve(lox: &mut LoxClient, name: &str, room: Option<&str>) -> Result<Control> {
    let uuid = lox.resolve_with_room(name, room)?;
    lox.find_control(&uuid)
}

pub fn get_control(lox: &mut LoxClient, name: &str, room: Option<&str>) -> Result<ControlState> {
    let ctrl = resolve(lox, name, room)?;
    let xml = lox.get_all(&ctrl.uuid)?;
    let (value, states, outputs) = parse_all_xml(&xml);
    Ok(ControlState {
        control: ControlRef::from(&ctrl),
        value: value.unwrap_or(Value::Null),
        states,
        outputs,
        secured: ctrl.is_secured,
    })
}

pub fn list_sensors(lox: &mut LoxClient, kind: &str, room: Option<&str>) -> Result<SensorList> {
    let controls = lox.list_controls(None, room)?;
    let mut sensors = Vec::new();
    for c in controls.iter().filter(|c| {
        if kind == "energy" {
            is_energy_type(&c.typ)
        } else {
            is_sensor_type(kind, c)
        }
    }) {
        let xml = lox.get_all(&c.uuid).unwrap_or_default();
        let (value, _, _) = parse_all_xml(&xml);
        sensors.push(Sensor {
            control: ControlRef::from(c),
            value: value.unwrap_or(Value::Null),
        });
    }
    Ok(SensorList {
        kind: kind.to_string(),
        count: sensors.len(),
        sensors,
    })
}

pub fn list_scenes(cfg: &Config) -> Result<SceneList> {
    let mut scenes = Vec::new();
    for id in Scene::list_with_config(cfg)? {
        scenes.push(match Scene::load_with_config(&id, cfg) {
            Ok(s) => SceneInfo {
                scene: id,
                name: s.name,
                description: s.description,
                steps: Some(s.steps.len()),
                error: None,
            },
            Err(e) => SceneInfo {
                scene: id,
                name: None,
                description: None,
                steps: None,
                error: Some(format!("{:#}", e)),
            },
        });
    }
    Ok(SceneList {
        count: scenes.len(),
        scenes,
    })
}

pub fn system_status(lox: &mut LoxClient) -> Result<SystemStatus> {
    let read = |path: &str| -> Result<String> {
        let text = lox.get_text(path)?;
        Ok(crate::xml_attr(&text, "value").unwrap_or("").to_string())
    };
    let firmware = read("/dev/cfg/version")?;
    let plc = read("/dev/sps/state")?;
    let heap = read("/dev/sys/heap")?;
    let plc_state = match plc.as_str() {
        "5" => "running",
        "3" => "started",
        "7" => "error",
        "1" => "booting",
        "8" => "updating",
        _ => "unknown",
    };
    let (heap_used_kb, heap_total_kb) = match heap.split_once('/') {
        Some((used, total)) => {
            let used = used.trim().parse::<f64>().ok();
            let total = total.trim().trim_end_matches("kB").parse::<f64>().ok();
            (used, total.filter(|t| *t > 0.0))
        }
        None => (None, None),
    };
    Ok(SystemStatus {
        firmware,
        plc_state: plc_state.to_string(),
        plc_running: plc == "5",
        heap,
        heap_used_kb,
        heap_total_kb,
    })
}

// ── Actions ───────────────────────────────────────────────────────────────────

/// Control types where a generic action (on/off/pulse/raw) can open a door,
/// move a gate or change the alarm state.
pub fn is_risky_type(typ: &str) -> bool {
    matches!(typ, "Alarm" | "SmokeAlarm" | "Gate" | "CentralGate") || typ.contains("DoorLock")
}

/// A plain switch or push-button that is really a door opener, gate or lock:
/// installations often wire these as `Pushbutton`/`Switch`. Category names are
/// user-chosen and localized, so this goes by the language-independent icons
/// (`IconsFilled/door-open.svg`, `login-key.svg`, `garage-closed-2.svg`) and by
/// Loxone's own `isSecured` flag (the control asks for the visualization password).
pub fn is_access_control(ctrl: &Control) -> bool {
    const ACCESS: &[&str] = &["door", "gate", "garage", "lock", "padlock", "key", "keypad"];
    let is_access_icon = |icon: &Option<String>| {
        icon.as_deref().is_some_and(|path| {
            let file = path.rsplit('/').next().unwrap_or(path);
            let stem = file.split('.').next().unwrap_or(file);
            stem.to_lowercase()
                .split(['-', '_'])
                .any(|t| ACCESS.contains(&t))
        })
    };
    ctrl.is_secured || is_access_icon(&ctrl.icon) || is_access_icon(&ctrl.cat_icon)
}

/// Is this control on the user's `confirm:` list in the config? An entry matches
/// the UUID, an alias of it, or the name as a case-insensitive substring with an
/// optional `[Room]` qualifier ("Pool Abdeckung" covers both cover buttons).
/// Over-matching only means one more question, so the match is deliberately loose.
pub fn on_confirm_list(cfg: &Config, ctrl: &Control) -> bool {
    let contains = |haystack: Option<&str>, needle: &str| {
        haystack
            .unwrap_or("")
            .to_lowercase()
            .contains(&needle.to_lowercase())
    };
    cfg.confirm.iter().map(|e| e.trim()).any(|entry| {
        if entry.is_empty() {
            return false;
        }
        if entry.eq_ignore_ascii_case(&ctrl.uuid) || cfg.aliases.get(entry) == Some(&ctrl.uuid) {
            return true;
        }
        let (name, room) = match entry.strip_suffix(']').and_then(|e| e.rsplit_once('[')) {
            Some((name, room)) => (name.trim(), Some(room.trim())),
            None => (entry, None),
        };
        !name.is_empty()
            && contains(Some(&ctrl.name), name)
            && room.is_none_or(|r| contains(ctrl.room.as_deref(), r))
    })
}

/// Does this action on this control need the user's confirmation?
///
/// The action's own risk (doors, gate open/close, alarm arm/disarm), plus any
/// generic action aimed at a risky control type or an access control, so
/// `switch off` on a door lock, a raw command to a gate, or a pulse to a
/// door-opener push-button cannot bypass the confirmation.
pub fn needs_confirmation(action: &Action, ctrl: &Control) -> bool {
    action.risk() == crate::actions::Risk::Confirm
        || (matches!(
            action,
            Action::On | Action::Off | Action::Pulse | Action::Raw(_) | Action::Value(_)
        ) && (is_risky_type(&ctrl.typ) || is_access_control(ctrl)))
}

/// Resolve the target of an action and check that the action fits its type.
pub fn resolve_for_action(
    lox: &mut LoxClient,
    name: &str,
    room: Option<&str>,
    action: &Action,
) -> Result<Control> {
    let ctrl = resolve(lox, name, room)?;
    action.check_type(&ctrl.name, &ctrl.typ).map_err(invalid)?;
    Ok(ctrl)
}

/// The result of an action before anything is sent.
pub fn planned(ctrl: &Control, action: &Action) -> ActionResult {
    ActionResult {
        ok: true,
        control: ControlRef::from(ctrl),
        action: action.describe(),
        commands: visible_commands(action),
        cli: action.to_cli(&ctrl.name, ctrl.room.as_deref()),
        dry_run: false,
        needs_confirmation: false,
        confirmed: false,
        code: None,
        value: None,
    }
}

/// Send an action's commands and record the Miniserver's answer.
pub fn send(
    lox: &mut LoxClient,
    ctrl: &Control,
    action: &Action,
    out: &mut ActionResult,
) -> Result<()> {
    let mut last = Value::Null;
    for cmd in action.commands() {
        last = lox.send_cmd(&ctrl.uuid, &cmd)?;
    }
    let code = last.pointer("/LL/Code").and_then(json_val_str);
    let value = last.pointer("/LL/value").and_then(json_val_str);
    if let Some(code) = &code
        && code != "200"
    {
        bail!(
            "Miniserver rejected '{}' on '{}' (code {}{})",
            action.describe(),
            ctrl.name,
            code,
            value.map(|v| format!(", value {}", v)).unwrap_or_default()
        );
    }
    out.code = code.map(|c| typed(&c));
    out.value = value.map(|v| typed(&v));
    Ok(())
}

pub fn load_scene(cfg: &Config, id: &str) -> Result<Scene> {
    Scene::load_with_config(id, cfg)
}

/// Run (or dry-run) one scene step. Failures are recorded, not returned.
pub fn run_step(lox: &mut LoxClient, step: &SceneStep, dry_run: bool) -> StepResult {
    let mut out = StepResult {
        control: step.control.clone(),
        command: step.cmd.clone(),
        uuid: None,
        ok: false,
        code: None,
        error: None,
    };
    let uuid = match lox.resolve(&step.control) {
        Ok(u) => u,
        Err(e) => {
            out.error = Some(format!("{:#}", e));
            return out;
        }
    };
    out.uuid = Some(uuid.clone());
    if dry_run {
        out.ok = true;
        return out;
    }
    match lox.send_cmd(&uuid, &step.cmd) {
        Ok(resp) => {
            let code = resp.pointer("/LL/Code").and_then(json_val_str);
            out.ok = code.as_deref().is_none_or(|c| c == "200");
            out.code = code.map(|c| typed(&c));
        }
        Err(e) => out.error = Some(format!("{:#}", e)),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions;

    #[test]
    fn parse_all_xml_collects_value_states_and_outputs() {
        let xml = r#"<LL control="dev/sps/io/x/all" value="0.5" Code="200" StatePos="0.3" StateUp="0" n1="Temperatur" v1="21.5" n2="" v2="x"/>"#;
        let (value, states, outputs) = parse_all_xml(xml);
        assert_eq!(value, Some(json!(0.5)));
        assert_eq!(states.get("StatePos"), Some(&json!(0.3)));
        assert_eq!(states.get("StateUp"), Some(&json!(0)));
        assert!(!states.contains_key("control"));
        assert!(!states.contains_key("Code"));
        assert!(!states.contains_key("n1"));
        assert_eq!(outputs.get("Temperatur"), Some(&json!(21.5)));
        assert_eq!(outputs.len(), 1, "empty output names are skipped");
    }

    #[test]
    fn parse_all_xml_reads_output_children() {
        // LightControllerV2 as a Miniserver 17.2 answers `/all`
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<LL control="dev/sps/io/x/all" value="1.000" Code="200">
    <output name="Relais Lichter Büro" nr="1" Type="Switch" value="383"/>
    <output name="Relais Lichter Büro" nr="2" Type="Switch" value="383"/>
    <output name="Spots" nr="3" Type="Dimmer" value="40"/>
    <output name="" nr="4" Type="Switch" value="383"/>
</LL>"#;
        let (value, states, outputs) = parse_all_xml(xml);
        assert_eq!(value, Some(json!(1.0)));
        assert!(
            states.is_empty(),
            "child attributes are not states: {states:?}"
        );
        assert_eq!(outputs.get("Relais Lichter Büro (1)"), Some(&json!(383)));
        assert_eq!(outputs.get("Relais Lichter Büro (2)"), Some(&json!(383)));
        assert_eq!(outputs.get("Spots"), Some(&json!(40)));
        assert_eq!(outputs.len(), 3, "unnamed outputs are skipped");
    }

    #[test]
    fn typed_values() {
        assert_eq!(typed("1"), json!(1));
        assert_eq!(typed("21.5"), json!(21.5));
        assert_eq!(typed("on"), json!("on"));
        assert_eq!(typed("NaN"), json!("NaN"));
    }

    #[test]
    fn alarm_pin_is_masked() {
        let a = actions::parse_alarm("arm", false, Some("1234".into())).unwrap();
        assert_eq!(visible_commands(&a), vec!["delayedon/1/****"]);
        let b = actions::parse_alarm("disarm", false, Some("0".into())).unwrap();
        assert_eq!(visible_commands(&b), vec!["off/****"]);
    }

    fn ctrl(typ: &str) -> Control {
        Control {
            name: "x".into(),
            uuid: "u".into(),
            typ: typ.into(),
            room: None,
            cat: None,
            is_favorite: false,
            is_secured: false,
            format: None,
            icon: None,
            cat_icon: None,
        }
    }

    #[test]
    fn generic_actions_on_risky_types_need_confirmation() {
        assert!(needs_confirmation(&Action::Off, &ctrl("DoorLock")));
        assert!(needs_confirmation(
            &Action::Raw("open".into()),
            &ctrl("Gate")
        ));
        assert!(needs_confirmation(&Action::Pulse, &ctrl("Alarm")));
        assert!(!needs_confirmation(&Action::On, &ctrl("Switch")));
        let stop = actions::parse_gate("stop").unwrap();
        assert!(!needs_confirmation(&stop, &ctrl("Gate")));
    }

    #[test]
    fn door_opener_push_buttons_need_confirmation() {
        // "Tür öffnen": a Pushbutton in a category with the door-open icon
        let opener = Control {
            cat_icon: Some("IconsFilled/door-open.svg".into()),
            ..ctrl("Pushbutton")
        };
        assert!(needs_confirmation(&Action::Pulse, &opener));
        let keyed = Control {
            icon: Some("IconsFilled/login-key.svg".into()),
            ..ctrl("Pushbutton")
        };
        assert!(needs_confirmation(&Action::Pulse, &keyed));
        let garage = Control {
            cat_icon: Some("IconsFilled/garage-closed-2.svg".into()),
            ..ctrl("Switch")
        };
        assert!(needs_confirmation(&Action::On, &garage));
        let secured = Control {
            is_secured: true,
            ..ctrl("Switch")
        };
        assert!(needs_confirmation(&Action::Off, &secured));
    }

    #[test]
    fn everyday_icons_do_not_need_confirmation() {
        for icon in [
            "IconsFilled/alarm-clock.svg", // "lock" inside "clock"
            "IconsFilled/lightbulb-3.svg",
            "IconsFilled/finger-tapping.svg",
            "IconsFilled/keyboard.svg",
        ] {
            let c = Control {
                cat_icon: Some(icon.into()),
                ..ctrl("Pushbutton")
            };
            assert!(!needs_confirmation(&Action::Pulse, &c), "{icon}");
        }
    }

    #[test]
    fn confirm_list_matches_uuid_alias_name_and_room() {
        let cover = Control {
            name: "Taster Pool Abdeckung Auf".into(),
            uuid: "209765db-028a-3739-ffffed57184a04d2".into(),
            room: Some("Pool".into()),
            ..ctrl("Pushbutton")
        };
        let cfg = |entries: &[&str]| Config {
            confirm: entries.iter().map(|e| e.to_string()).collect(),
            aliases: [("cover".to_string(), cover.uuid.clone())].into(),
            ..Default::default()
        };
        assert!(!on_confirm_list(&cfg(&[]), &cover));
        assert!(on_confirm_list(&cfg(&["pool abdeckung"]), &cover));
        assert!(on_confirm_list(&cfg(&["Pool Abdeckung [Pool]"]), &cover));
        assert!(on_confirm_list(
            &cfg(&["209765DB-028a-3739-ffffed57184a04d2"]),
            &cover
        ));
        assert!(on_confirm_list(&cfg(&["cover"]), &cover));
        assert!(!on_confirm_list(&cfg(&["Pool Abdeckung [Garten]"]), &cover));
        assert!(!on_confirm_list(&cfg(&["Pumpe"]), &cover));
        // a blank entry or a bare room must not match everything
        assert!(!on_confirm_list(&cfg(&["", "  ", "[Pool]"]), &cover));
    }

    #[test]
    fn tool_for_type_maps_common_types() {
        assert_eq!(tool_for_type("Jalousie"), Some("blind"));
        assert_eq!(tool_for_type("LightControllerV2"), Some("light"));
        assert_eq!(tool_for_type("Switch"), Some("switch"));
        assert_eq!(tool_for_type("InfoOnlyAnalog"), None);
    }
}
