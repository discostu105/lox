//! MCP tool definitions and handlers.
//!
//! Tools call `LoxClient` and the shared action layer directly: the CLI command
//! functions print to stdout, which is the protocol channel here.

use anyhow::{Result, anyhow};
use serde_json::{Map, Value, json};
use std::thread;
use std::time::Duration;

use super::{McpServer, ServerOptions};
use crate::actions::{self, Action, Risk};
use crate::client::Control;
use crate::commands::control::fetch_light_moods;
use crate::commands::inspect::{is_energy_type, is_sensor_type};
use crate::json_val_str;
use crate::scene::Scene;

// ── Errors ────────────────────────────────────────────────────────────────────

/// A tool failure with an explicit error code (on top of the CLI's codes).
#[derive(Debug)]
struct ToolError {
    code: &'static str,
    message: String,
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ToolError {}

fn invalid(message: impl std::fmt::Display) -> anyhow::Error {
    ToolError {
        code: "invalid_arguments",
        message: message.to_string(),
    }
    .into()
}

/// The CLI's `-o json` error envelope for a failed tool call.
pub fn error_envelope(e: &anyhow::Error) -> Value {
    let code = match e.downcast_ref::<ToolError>() {
        Some(te) => te.code,
        None => crate::categorize_error(e),
    };
    json!({ "ok": false, "error": code, "message": format!("{:#}", e) })
}

// ── Argument helpers ──────────────────────────────────────────────────────────

fn opt_str(args: &Value, key: &str) -> Result<Option<String>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        // numbers are accepted where a string is expected ("value": 50)
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(_) => Err(invalid(format!("'{}' must be a string", key))),
    }
}

fn req_str(args: &Value, key: &str) -> Result<String> {
    opt_str(args, key)?.ok_or_else(|| invalid(format!("Missing required argument '{}'", key)))
}

fn opt_f64(args: &Value, key: &str) -> Result<Option<f64>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => Ok(n.as_f64()),
        Some(Value::String(s)) => s
            .trim()
            .parse::<f64>()
            .map(Some)
            .map_err(|_| invalid(format!("'{}' must be a number, got '{}'", key, s))),
        Some(_) => Err(invalid(format!("'{}' must be a number", key))),
    }
}

fn opt_bool(args: &Value, key: &str) -> Result<bool> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(Value::String(s)) if s == "true" => Ok(true),
        Some(Value::String(s)) if s == "false" => Ok(false),
        Some(_) => Err(invalid(format!("'{}' must be true or false", key))),
    }
}

// ── Output helpers ────────────────────────────────────────────────────────────

/// A Miniserver value as JSON: numbers become numbers, everything else stays text.
fn typed(s: &str) -> Value {
    if let Ok(i) = s.parse::<i64>() {
        return json!(i);
    }
    match s.parse::<f64>() {
        Ok(f) if f.is_finite() => json!(f),
        _ => Value::String(s.to_string()),
    }
}

/// The MCP tool that operates a control type, if any.
fn tool_for_type(typ: &str) -> Option<&'static str> {
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

fn control_json(c: &Control) -> Value {
    let mut v = json!({ "name": c.name, "type": c.typ, "uuid": c.uuid });
    if let Some(room) = &c.room {
        v["room"] = json!(room);
    }
    if let Some(cat) = &c.cat {
        v["category"] = json!(cat);
    }
    if let Some(tool) = tool_for_type(&c.typ) {
        v["tool"] = json!(tool);
    }
    v
}

/// Parse `/dev/sps/io/{uuid}/all`: the main value, `State*`-style attributes,
/// and named outputs (`n1`/`v1`, `n2`/`v2`, …).
fn parse_all_xml(xml: &str) -> (Option<Value>, Map<String, Value>, Map<String, Value>) {
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
    let mut states = Map::new();
    let mut outputs = Map::new();
    for (k, v) in &attrs {
        let is_indexed = |p: char| {
            k.strip_prefix(p)
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        };
        if is_indexed('n') {
            let idx = &k[1..];
            if !v.is_empty() {
                let val = get(&format!("v{}", idx))
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

// ── Tool definitions ──────────────────────────────────────────────────────────

enum Kind {
    Read,
    Action,
    Risky,
}

fn tool(name: &str, title: &str, description: &str, schema: Value, kind: Kind) -> Value {
    let annotations = match kind {
        Kind::Read => json!({ "title": title, "readOnlyHint": true, "openWorldHint": false }),
        Kind::Action => json!({
            "title": title, "readOnlyHint": false, "destructiveHint": false, "openWorldHint": false
        }),
        Kind::Risky => json!({
            "title": title, "readOnlyHint": false, "destructiveHint": true, "openWorldHint": false
        }),
    };
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": schema,
        "annotations": annotations,
    })
}

fn name_prop() -> Value {
    json!({
        "type": "string",
        "description": "Control name: case-insensitive substring, 'Name [Room]', alias, or UUID"
    })
}

fn room_prop() -> Value {
    json!({ "type": "string", "description": "Room name (substring) to disambiguate the control" })
}

fn dry_run_prop() -> Value {
    json!({
        "type": "boolean",
        "description": "Resolve the control and return the commands without sending them"
    })
}

/// An action tool schema: `name`, `room`, `dry_run` plus tool-specific properties.
fn action_schema(extra: Value, required: &[&str]) -> Value {
    let mut props = json!({ "name": name_prop(), "room": room_prop() });
    if let Value::Object(extra) = extra {
        for (k, v) in extra {
            props[k] = v;
        }
    }
    props["dry_run"] = dry_run_prop();
    let mut req = vec!["name"];
    req.extend_from_slice(required);
    json!({ "type": "object", "properties": props, "required": req, "additionalProperties": false })
}

/// Tools exposed under the given policy, in display order.
pub fn tool_defs(opts: &ServerOptions) -> Vec<Value> {
    let mut tools = vec![
        tool(
            "list_rooms",
            "List rooms",
            "List all rooms of the Loxone installation with the number of controls in each.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
            Kind::Read,
        ),
        tool(
            "list_controls",
            "List controls",
            "List controls (lights, blinds, switches, sensors, …) with type, room, category and \
             UUID. Filters are case-insensitive substrings and can be combined. Each control names \
             the `tool` that operates it.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Filter by control name" },
                    "room": { "type": "string", "description": "Filter by room" },
                    "type": {
                        "type": "string",
                        "description": "Filter by Loxone type, e.g. Jalousie, LightControllerV2, Switch"
                    },
                    "category": { "type": "string", "description": "Filter by category" },
                    "favorites_only": { "type": "boolean", "description": "Only favorites" }
                },
                "additionalProperties": false
            }),
            Kind::Read,
        ),
        tool(
            "get_control",
            "Get control state",
            "Read the live state of one control: main value, state attributes such as blind \
             position, and named outputs.",
            json!({
                "type": "object",
                "properties": { "name": name_prop(), "room": room_prop() },
                "required": ["name"],
                "additionalProperties": false
            }),
            Kind::Read,
        ),
        tool(
            "list_sensors",
            "List sensor readings",
            "Current readings of sensors: temperature, door/window contacts, motion/presence, \
             smoke, or energy meters.",
            json!({
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["all", "temperature", "door-window", "motion", "smoke", "energy"],
                        "description": "Sensor kind (default: all except energy)"
                    },
                    "room": { "type": "string", "description": "Filter by room" }
                },
                "additionalProperties": false
            }),
            Kind::Read,
        ),
        tool(
            "list_light_moods",
            "List light moods",
            "List the moods (scenes) of a lighting controller with their numeric IDs, for use \
             with the `light` tool (action=mood).",
            json!({
                "type": "object",
                "properties": { "name": name_prop(), "room": room_prop() },
                "required": ["name"],
                "additionalProperties": false
            }),
            Kind::Read,
        ),
        tool(
            "list_scenes",
            "List scenes",
            "List the user's lox scenes (named multi-step command sequences) for `run_scene`.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
            Kind::Read,
        ),
        tool(
            "system_status",
            "Miniserver status",
            "Miniserver firmware version, PLC state and memory usage.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
            Kind::Read,
        ),
    ];
    if opts.read_only {
        return tools;
    }

    let risky_note = if opts.allow_risky {
        ""
    } else {
        " This server refuses these actions: it was started without --allow-risky."
    };

    tools.extend([
        tool(
            "switch",
            "Switch on/off",
            "Turn a control on or off, or send a pulse (push-button). Works for switches, \
             lighting controllers and most digital controls.",
            action_schema(
                json!({ "state": { "type": "string", "enum": ["on", "off", "pulse"] } }),
                &["state"],
            ),
            Kind::Action,
        ),
        tool(
            "blind",
            "Move blinds",
            "Move a blind/shade (Jalousie). Position and slats are 0-100 where 100 is fully \
             closed/down. 'shade' starts automatic shading.",
            action_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["up", "down", "stop", "shade", "position", "slats"]
                    },
                    "value": {
                        "type": "number", "minimum": 0, "maximum": 100,
                        "description": "Required for position and slats (0-100, 100 = closed)"
                    }
                }),
                &["action"],
            ),
            Kind::Action,
        ),
        tool(
            "light",
            "Control lights",
            "Change a light: switch a lighting controller's mood (plus, minus, off, or a mood ID \
             from list_light_moods), dim (0-100), or set a color (#RRGGBB, hsv(h,s,v), or \
             temp(brightness,kelvin)) on a color picker.",
            action_schema(
                json!({
                    "action": { "type": "string", "enum": ["mood", "dim", "color"] },
                    "value": {
                        "type": ["string", "number"],
                        "description": "mood: plus|minus|off|<id>; dim: 0-100; color: #RRGGBB|hsv(h,s,v)|temp(b,k)"
                    }
                }),
                &["action", "value"],
            ),
            Kind::Action,
        ),
        tool(
            "thermostat",
            "Set room climate",
            "Set a room controller: comfort temperature (temp, °C), operating mode (mode: auto, \
             auto-heat, auto-cool, manual, manual-heat, manual-cool), or a temporary override \
             temperature (override, °C, for `minutes`, default 60).",
            action_schema(
                json!({
                    "action": { "type": "string", "enum": ["temp", "mode", "override"] },
                    "value": {
                        "type": ["string", "number"],
                        "description": "°C for temp/override, mode name for mode"
                    },
                    "minutes": { "type": "integer", "minimum": 1, "description": "Override duration" }
                }),
                &["action", "value"],
            ),
            Kind::Action,
        ),
        tool(
            "gate",
            "Operate gate",
            &format!(
                "Open, close or stop a gate or garage door. open and close are high-risk.{}",
                risky_note
            ),
            action_schema(
                json!({ "action": { "type": "string", "enum": ["open", "close", "stop"] } }),
                &["action"],
            ),
            Kind::Risky,
        ),
        tool(
            "alarm",
            "Operate alarm",
            &format!(
                "Arm, arm in home mode, disarm, or acknowledge (quit) the burglar alarm. All \
                 but quit are high-risk.{}",
                risky_note
            ),
            action_schema(
                json!({
                    "action": { "type": "string", "enum": ["arm", "arm-home", "disarm", "quit"] },
                    "no_motion": { "type": "boolean", "description": "Arm without motion detectors" },
                    "code": { "type": "string", "description": "Alarm PIN, if the alarm requires one" }
                }),
                &["action"],
            ),
            Kind::Risky,
        ),
        tool(
            "door",
            "Operate door lock",
            &format!("Lock, unlock or open a door lock. High-risk.{}", risky_note),
            action_schema(
                json!({ "action": { "type": "string", "enum": ["lock", "unlock", "open"] } }),
                &["action"],
            ),
            Kind::Risky,
        ),
        tool(
            "run_scene",
            "Run scene",
            "Run one of the user's lox scenes (see list_scenes). Steps run in order with their \
             configured delays.",
            json!({
                "type": "object",
                "properties": {
                    "scene": { "type": "string", "description": "Scene name from list_scenes" },
                    "dry_run": dry_run_prop()
                },
                "required": ["scene"],
                "additionalProperties": false
            }),
            Kind::Action,
        ),
    ]);

    if opts.allow_raw {
        tools.push(tool(
            "send_command",
            "Send raw command",
            "Send a raw Loxone command string to a control (/jdev/sps/io/{uuid}/{command}), \
             e.g. 'on', 'FullUp', 'setComfortTemperature/21'. Prefer the typed tools.",
            action_schema(
                json!({ "command": { "type": "string", "description": "Raw Loxone command" } }),
                &["command"],
            ),
            Kind::Risky,
        ));
    }
    tools
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

pub fn call(
    server: &mut McpServer,
    opts: &ServerOptions,
    name: &str,
    args: &Value,
) -> Result<Value> {
    match name {
        "list_rooms" => list_rooms(server),
        "list_controls" => list_controls(server, args),
        "get_control" => get_control(server, args),
        "list_sensors" => list_sensors(server, args),
        "list_light_moods" => list_light_moods(server, args),
        "list_scenes" => list_scenes(server),
        "system_status" => system_status(server),
        "switch" => {
            let action = match req_str(args, "state")?.to_lowercase().as_str() {
                "on" => Action::On,
                "off" => Action::Off,
                "pulse" => Action::Pulse,
                other => {
                    return Err(invalid(format!(
                        "Unknown state '{}'. Use: on, off, pulse",
                        other
                    )));
                }
            };
            exec_action(server, opts, args, action)
        }
        "blind" => {
            let act = req_str(args, "action")?.to_lowercase();
            let value = opt_f64(args, "value")?;
            let action = match act.as_str() {
                "position" | "pos" => actions::parse_blind("pos", value),
                "slats" => match value {
                    Some(v) => actions::parse_blind("shade", Some(v)),
                    None => Err(anyhow!("slats requires a value 0-100")),
                },
                "shade" => actions::parse_blind("shade", None),
                other => actions::parse_blind(other, None),
            }
            .map_err(invalid)?;
            exec_action(server, opts, args, action)
        }
        "light" => {
            let act = req_str(args, "action")?.to_lowercase();
            let value = req_str(args, "value")?;
            let action = match act.as_str() {
                "mood" => actions::parse_mood(&value),
                "dim" => value
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| anyhow!("dim value must be a number 0-100, got '{}'", value))
                    .and_then(actions::parse_dim),
                "color" => actions::parse_color(&value),
                other => Err(anyhow!(
                    "Unknown light action '{}'. Use: mood, dim, color",
                    other
                )),
            }
            .map_err(invalid)?;
            exec_action(server, opts, args, action)
        }
        "thermostat" => {
            let act = req_str(args, "action")?;
            let value = req_str(args, "value")?;
            let minutes = opt_f64(args, "minutes")?.map(|m| m.max(1.0).round() as u64);
            let action = actions::parse_thermostat(&act, Some(&value), minutes).map_err(invalid)?;
            exec_action(server, opts, args, action)
        }
        "gate" => {
            let action = actions::parse_gate(&req_str(args, "action")?).map_err(invalid)?;
            exec_action(server, opts, args, action)
        }
        "alarm" => {
            let action = actions::parse_alarm(
                &req_str(args, "action")?,
                opt_bool(args, "no_motion")?,
                opt_str(args, "code")?,
            )
            .map_err(invalid)?;
            exec_action(server, opts, args, action)
        }
        "door" => {
            let action = actions::parse_door(&req_str(args, "action")?).map_err(invalid)?;
            exec_action(server, opts, args, action)
        }
        "run_scene" => run_scene(server, opts, args),
        "send_command" => {
            let action = Action::Raw(req_str(args, "command")?);
            exec_action(server, opts, args, action)
        }
        other => Err(invalid(format!("Unknown tool '{}'", other))),
    }
}

// ── Read tools ────────────────────────────────────────────────────────────────

fn list_rooms(server: &mut McpServer) -> Result<Value> {
    let lox = server.client()?;
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
    let list: Vec<Value> = rooms
        .iter()
        .map(|(name, uuid)| {
            let n = controls
                .iter()
                .filter(|c| c.room.as_deref() == Some(name.as_str()))
                .count();
            json!({ "name": name, "uuid": uuid, "controls": n })
        })
        .collect();
    Ok(json!({ "count": list.len(), "rooms": list }))
}

fn list_controls(server: &mut McpServer, args: &Value) -> Result<Value> {
    let name = opt_str(args, "name")?.map(|n| n.to_lowercase());
    let room = opt_str(args, "room")?;
    let typ = opt_str(args, "type")?;
    let cat = opt_str(args, "category")?;
    let favorites = opt_bool(args, "favorites_only")?;
    let lox = server.client()?;
    let controls =
        lox.list_controls_ext(typ.as_deref(), room.as_deref(), cat.as_deref(), favorites)?;
    let list: Vec<Value> = controls
        .iter()
        .filter(|c| {
            name.as_deref()
                .is_none_or(|n| c.name.to_lowercase().contains(n))
        })
        .map(control_json)
        .collect();
    Ok(json!({ "count": list.len(), "controls": list }))
}

fn get_control(server: &mut McpServer, args: &Value) -> Result<Value> {
    let name = req_str(args, "name")?;
    let room = opt_str(args, "room")?;
    let lox = server.client()?;
    let uuid = lox.resolve_with_room(&name, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    let xml = lox.get_all(&ctrl.uuid)?;
    let (value, states, outputs) = parse_all_xml(&xml);
    let mut out = control_json(&ctrl);
    out["value"] = value.unwrap_or(Value::Null);
    if !states.is_empty() {
        out["states"] = Value::Object(states);
    }
    if !outputs.is_empty() {
        out["outputs"] = Value::Object(outputs);
    }
    if ctrl.is_secured {
        out["secured"] = json!(true);
    }
    Ok(out)
}

fn list_sensors(server: &mut McpServer, args: &Value) -> Result<Value> {
    let kind = opt_str(args, "kind")?
        .unwrap_or_else(|| "all".to_string())
        .to_lowercase();
    if !matches!(
        kind.as_str(),
        "all" | "temperature" | "temp" | "door-window" | "motion" | "smoke" | "energy"
    ) {
        return Err(invalid(format!(
            "Unknown sensor kind '{}'. Use: all, temperature, door-window, motion, smoke, energy",
            kind
        )));
    }
    let room = opt_str(args, "room")?;
    let lox = server.client()?;
    let controls = lox.list_controls(None, room.as_deref())?;
    let mut list = Vec::new();
    for c in controls.iter().filter(|c| {
        if kind == "energy" {
            is_energy_type(&c.typ)
        } else {
            is_sensor_type(&kind, &c.typ)
        }
    }) {
        let xml = lox.get_all(&c.uuid).unwrap_or_default();
        let (value, _, _) = parse_all_xml(&xml);
        list.push(json!({
            "name": c.name, "type": c.typ, "room": c.room, "uuid": c.uuid,
            "value": value.unwrap_or(Value::Null),
        }));
    }
    Ok(json!({ "kind": kind, "count": list.len(), "sensors": list }))
}

fn list_light_moods(server: &mut McpServer, args: &Value) -> Result<Value> {
    let name = req_str(args, "name")?;
    let room = opt_str(args, "room")?;
    let lox = server.client()?;
    let (ctrl, moods) = fetch_light_moods(lox, &name, room.as_deref())?;
    let list: Vec<Value> = moods
        .iter()
        .map(|m| json!({ "id": m.id, "name": m.name }))
        .collect();
    Ok(json!({ "control": control_json(&ctrl), "moods": list }))
}

fn list_scenes(server: &mut McpServer) -> Result<Value> {
    let cfg = server.client()?.cfg.clone();
    let mut list = Vec::new();
    for id in Scene::list_with_config(&cfg)? {
        match Scene::load_with_config(&id, &cfg) {
            Ok(s) => list.push(json!({
                "scene": id,
                "name": s.name,
                "description": s.description,
                "steps": s.steps.len(),
            })),
            Err(e) => list.push(json!({ "scene": id, "error": format!("{:#}", e) })),
        }
    }
    Ok(json!({ "count": list.len(), "scenes": list }))
}

fn system_status(server: &mut McpServer) -> Result<Value> {
    let lox = server.client()?;
    let read = |path: &str| -> Result<String> {
        let text = lox.get_text(path)?;
        Ok(crate::xml_attr(&text, "value").unwrap_or("").to_string())
    };
    let version = read("/dev/cfg/version")?;
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
    let mut out = json!({
        "firmware": version,
        "plc_state": plc_state,
        "plc_running": plc == "5",
        "heap": heap,
    });
    if let Some((used, total)) = heap.split_once('/') {
        let used: f64 = used.trim().parse().unwrap_or(0.0);
        let total: f64 = total.trim().trim_end_matches("kB").parse().unwrap_or(0.0);
        if total > 0.0 {
            out["heap_used_kb"] = json!(used);
            out["heap_total_kb"] = json!(total);
        }
    }
    Ok(out)
}

// ── Action tools ──────────────────────────────────────────────────────────────

/// Commands as shown to the model: an alarm PIN is never echoed back.
fn visible_commands(action: &Action) -> Vec<String> {
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

fn exec_action(
    server: &mut McpServer,
    opts: &ServerOptions,
    args: &Value,
    action: Action,
) -> Result<Value> {
    let name = req_str(args, "name")?;
    let room = opt_str(args, "room")?;
    let dry_run = opts.dry_run || opt_bool(args, "dry_run")?;
    let lox = server.client()?;
    let uuid = lox.resolve_with_room(&name, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    action.check_type(&ctrl.name, &ctrl.typ).map_err(invalid)?;

    let allowed = action.risk() != Risk::Confirm || opts.allow_risky;
    let mut out = json!({
        "ok": true,
        "control": control_json(&ctrl),
        "action": action.describe(),
        "commands": visible_commands(&action),
        "cli": action.to_cli(&ctrl.name, ctrl.room.as_deref()),
    });
    if dry_run {
        out["dry_run"] = json!(true);
        out["allowed"] = json!(allowed);
        return Ok(out);
    }
    if !allowed {
        return Err(ToolError {
            code: "action_not_allowed",
            message: format!(
                "Refused: '{}' on '{}' is a high-risk action (doors, gates, alarm). The MCP server \
                 was started without --allow-risky. The user can enable it by restarting the \
                 server with `lox mcp serve --allow-risky`, or run it themselves: {}",
                action.describe(),
                ctrl.name,
                action.to_cli(&ctrl.name, ctrl.room.as_deref()),
            ),
        }
        .into());
    }

    let mut last = Value::Null;
    for cmd in action.commands() {
        last = lox.send_cmd(&ctrl.uuid, &cmd)?;
    }
    let code = last.pointer("/LL/Code").and_then(json_val_str);
    let value = last.pointer("/LL/value").and_then(json_val_str);
    if let Some(code) = &code
        && code != "200"
    {
        anyhow::bail!(
            "Miniserver rejected '{}' on '{}' (code {}{})",
            action.describe(),
            ctrl.name,
            code,
            value.map(|v| format!(", value {}", v)).unwrap_or_default()
        );
    }
    out["code"] = code.map(|c| typed(&c)).unwrap_or(Value::Null);
    out["value"] = value.map(|v| typed(&v)).unwrap_or(Value::Null);
    Ok(out)
}

fn run_scene(server: &mut McpServer, opts: &ServerOptions, args: &Value) -> Result<Value> {
    let id = req_str(args, "scene")?;
    let dry_run = opts.dry_run || opt_bool(args, "dry_run")?;
    let lox = server.client()?;
    let scene = Scene::load_with_config(&id, &lox.cfg)?;
    let mut steps = Vec::new();
    let mut all_ok = true;
    for step in &scene.steps {
        let mut entry = json!({ "control": step.control, "command": step.cmd });
        let uuid = match lox.resolve(&step.control) {
            Ok(u) => u,
            Err(e) => {
                all_ok = false;
                entry["ok"] = json!(false);
                entry["error"] = json!(format!("{:#}", e));
                steps.push(entry);
                continue;
            }
        };
        entry["uuid"] = json!(uuid);
        if dry_run {
            entry["ok"] = json!(true);
        } else {
            match lox.send_cmd(&uuid, &step.cmd) {
                Ok(resp) => {
                    let code = resp.pointer("/LL/Code").and_then(json_val_str);
                    let ok = code.as_deref().is_none_or(|c| c == "200");
                    all_ok &= ok;
                    entry["ok"] = json!(ok);
                    entry["code"] = code.map(|c| typed(&c)).unwrap_or(Value::Null);
                }
                Err(e) => {
                    all_ok = false;
                    entry["ok"] = json!(false);
                    entry["error"] = json!(format!("{:#}", e));
                }
            }
            if step.delay_ms > 0 {
                thread::sleep(Duration::from_millis(step.delay_ms));
            }
        }
        steps.push(entry);
    }
    let mut out = json!({
        "ok": all_ok,
        "scene": id,
        "name": scene.name,
        "steps": steps,
    });
    if dry_run {
        out["dry_run"] = json!(true);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn tool_for_type_maps_common_types() {
        assert_eq!(tool_for_type("Jalousie"), Some("blind"));
        assert_eq!(tool_for_type("LightControllerV2"), Some("light"));
        assert_eq!(tool_for_type("Switch"), Some("switch"));
        assert_eq!(tool_for_type("InfoOnlyAnalog"), None);
    }
}
