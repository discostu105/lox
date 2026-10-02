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
    json!({ "ok": false, "error": code, "message": format!("{:#}", e) })
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
/// and named outputs (`n1`/`v1`, `n2`/`v2`, …).
pub fn parse_all_xml(xml: &str) -> (Option<Value>, ValueMap, ValueMap) {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                for a in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
                    let val = a
                        .unescape_value()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|_| String::from_utf8_lossy(&a.value).to_string());
                    attrs.push((key, val));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

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
            is_sensor_type(kind, &c.typ)
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

/// Does this action on this control need the user's confirmation?
///
/// The action's own risk (doors, gate open/close, alarm arm/disarm), plus any
/// generic action aimed at a risky control type, so `switch off` on a door lock
/// or a raw command to a gate cannot bypass the confirmation.
pub fn needs_confirmation(action: &Action, ctrl: &Control) -> bool {
    action.risk() == crate::actions::Risk::Confirm
        || (matches!(
            action,
            Action::On | Action::Off | Action::Pulse | Action::Raw(_) | Action::Value(_)
        ) && is_risky_type(&ctrl.typ))
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

    #[test]
    fn generic_actions_on_risky_types_need_confirmation() {
        let ctrl = |typ: &str| Control {
            name: "x".into(),
            uuid: "u".into(),
            typ: typ.into(),
            room: None,
            cat: None,
            is_favorite: false,
            is_secured: false,
        };
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
    fn tool_for_type_maps_common_types() {
        assert_eq!(tool_for_type("Jalousie"), Some("blind"));
        assert_eq!(tool_for_type("LightControllerV2"), Some("light"));
        assert_eq!(tool_for_type("Switch"), Some("switch"));
        assert_eq!(tool_for_type("InfoOnlyAnalog"), None);
    }
}
