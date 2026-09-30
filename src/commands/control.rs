use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use std::thread;
use std::time::Duration;

use crate::actions::{self, Action, ThermoCmd};
use crate::client::{LoxClient, USER_AGENT};
use crate::commands::RunContext;
use crate::config::Config;
use crate::scene::Scene;
use crate::stream::{self, StateEvent};
use crate::{
    InputCmd, LightCmd, MusicCmd, bar, encode_path_value, json_val_str, print_dry_run, print_resp,
    send_or_dry_run, xml_attr,
};

pub fn cmd_on(
    ctx: &RunContext,
    name_or_uuid: Option<String>,
    room: Option<String>,
    all_in_room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    if let Some(room_name) = all_in_room {
        let controls = lox.resolve_all_in_room(&room_name, None)?;
        for ctrl in &controls {
            if ctx.dry_run {
                print_dry_run(
                    ctx.json,
                    ctx.quiet,
                    &ctrl.uuid,
                    "on",
                    &ctrl.name,
                    ctrl.room.as_deref(),
                );
            } else {
                match lox.send_cmd(&ctrl.uuid, "on") {
                    Ok(resp) => print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, "on"),
                    Err(e) => eprintln!("  {} — {}", ctrl.name, e),
                }
            }
        }
    } else {
        let name = name_or_uuid
            .ok_or_else(|| anyhow::anyhow!("Provide a control name or --all-in-room"))?;
        let uuid = lox.resolve_with_room(&name, room.as_deref())?;
        if ctx.dry_run {
            let ctrl = lox.find_control(&uuid).ok();
            let room = ctrl.as_ref().and_then(|c| c.room.as_deref());
            print_dry_run(ctx.json, ctx.quiet, &uuid, "on", &name, room);
        } else {
            let resp = lox.send_cmd(&uuid, "on")?;
            print_resp(&resp, ctx.json, ctx.quiet, &name, "on");
        }
    }
    Ok(())
}

pub fn cmd_off(
    ctx: &RunContext,
    name_or_uuid: Option<String>,
    room: Option<String>,
    all_in_room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    if let Some(room_name) = all_in_room {
        let controls = lox.resolve_all_in_room(&room_name, None)?;
        for ctrl in &controls {
            if ctx.dry_run {
                print_dry_run(
                    ctx.json,
                    ctx.quiet,
                    &ctrl.uuid,
                    "off",
                    &ctrl.name,
                    ctrl.room.as_deref(),
                );
            } else {
                match lox.send_cmd(&ctrl.uuid, "off") {
                    Ok(resp) => print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, "off"),
                    Err(e) => eprintln!("  {} — {}", ctrl.name, e),
                }
            }
        }
    } else {
        let name = name_or_uuid
            .ok_or_else(|| anyhow::anyhow!("Provide a control name or --all-in-room"))?;
        let uuid = lox.resolve_with_room(&name, room.as_deref())?;
        if ctx.dry_run {
            let ctrl = lox.find_control(&uuid).ok();
            let room = ctrl.as_ref().and_then(|c| c.room.as_deref());
            print_dry_run(ctx.json, ctx.quiet, &uuid, "off", &name, room);
        } else {
            let resp = lox.send_cmd(&uuid, "off")?;
            print_resp(&resp, ctx.json, ctx.quiet, &name, "off");
        }
    }
    Ok(())
}

pub fn cmd_pulse(ctx: &RunContext, name_or_uuid: String, room: Option<String>) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    if ctx.dry_run {
        let ctrl = lox.find_control(&uuid).ok();
        let room = ctrl.as_ref().and_then(|c| c.room.as_deref());
        print_dry_run(ctx.json, ctx.quiet, &uuid, "pulse", &name_or_uuid, room);
    } else {
        let resp = lox.send_cmd(&uuid, "pulse")?;
        print_resp(&resp, ctx.json, ctx.quiet, &name_or_uuid, "pulse");
    }
    Ok(())
}

pub fn cmd_blind(
    ctx: &RunContext,
    name_or_uuid: String,
    action: String,
    pos: Option<f64>,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let act = actions::parse_blind(&action, pos)?;
    let (ctrl, resp) = run_action(ctx, &mut lox, &name_or_uuid, room.as_deref(), &act)?;
    let Some(resp) = resp else { return Ok(()) };
    let cmd = act.main_command();
    print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, &cmd);
    if !ctx.json {
        if cmd.starts_with("manualLamella") {
            thread::sleep(Duration::from_millis(800));
            let xml = lox.get_all(&ctrl.uuid)?;
            if let Some(p) = xml_attr(&xml, "StatePos").and_then(|v| v.parse::<f64>().ok()) {
                println!("   Position: {:.0}%  {}", p * 100.0, bar(p, 1.0));
            }
            if let Some(s) = xml_attr(&xml, "StateShade").and_then(|v| v.parse::<f64>().ok()) {
                println!("   Shade:    {:.0}%  {}", s * 100.0, bar(s, 1.0));
            }
        } else {
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            let mut prev_pos: Option<f64> = None;
            loop {
                thread::sleep(Duration::from_millis(500));
                let xml = lox.get_all(&ctrl.uuid)?;
                let cur_pos = xml_attr(&xml, "StatePos").and_then(|v| v.parse::<f64>().ok());
                let timed_out = std::time::Instant::now() >= deadline;
                let stable =
                    matches!((prev_pos, cur_pos), (Some(a), Some(b)) if (a - b).abs() < 0.005);
                if stable || timed_out {
                    if let Some(p) = cur_pos {
                        let suffix = if timed_out && !stable {
                            "  (moving…)"
                        } else {
                            ""
                        };
                        println!("   Position: {:.0}%  {}{}", p * 100.0, bar(p, 1.0), suffix);
                    }
                    break;
                }
                prev_pos = cur_pos;
            }
        }
    }
    Ok(())
}

/// Resolve a control, check that the action fits its type, and send the action's
/// commands (or print a dry run). Returns the control and the response of the main
/// command, or `None` for a dry run.
fn run_action(
    ctx: &RunContext,
    lox: &mut LoxClient,
    name_or_uuid: &str,
    room: Option<&str>,
    action: &Action,
) -> Result<(crate::client::Control, Option<serde_json::Value>)> {
    let uuid = lox.resolve_with_room(name_or_uuid, room)?;
    let ctrl = lox.find_control(&uuid)?;
    action.check_type(&ctrl.name, &ctrl.typ)?;
    let cmds = action.commands();
    let main = action.main_command();
    if ctx.dry_run {
        print_dry_run(
            ctx.json,
            ctx.quiet,
            &ctrl.uuid,
            &main,
            &ctrl.name,
            ctrl.room.as_deref(),
        );
        return Ok((ctrl, None));
    }
    let mut last = None;
    for cmd in &cmds {
        last = Some(lox.send_cmd(&ctrl.uuid, cmd)?);
    }
    Ok((ctrl, last))
}

/// Fetch the list of available moods for a LightControllerV2 via WebSocket.
///
/// The `moodList` state is a TextState sent as JSON during the initial state dump:
/// `[{"name":"Viel Licht","id":777,"static":false},{"name":"Aus","id":778,"static":true}]`
pub fn cmd_light_moods(ctx: &RunContext, name_or_uuid: String, room: Option<String>) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let (ctrl, moods) = fetch_light_moods(&mut lox, &name_or_uuid, room.as_deref())?;

    if ctx.json {
        let json_moods: Vec<serde_json::Value> = moods
            .iter()
            .map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "name": m.name,
                    "control": ctrl.name,
                    "control_uuid": ctrl.uuid,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_moods)?);
    } else {
        if !ctx.no_header {
            println!("{:<6}  NAME", "ID");
            println!("{}", "─".repeat(30));
        }
        for mood in &moods {
            println!("{:<6}  {}", mood.id, mood.name);
        }
        if !ctx.quiet {
            println!("\n{} moods", moods.len());
        }
    }

    Ok(())
}

/// Resolve a lighting controller and fetch its mood list via WebSocket.
pub fn fetch_light_moods(
    lox: &mut LoxClient,
    name_or_uuid: &str,
    room: Option<&str>,
) -> Result<(crate::client::Control, Vec<actions::MoodEntry>)> {
    let (ctrl, mood_list_uuid) = resolve_mood_list(lox, name_or_uuid, room)?;
    let cfg = lox.cfg.clone();
    let rt = tokio::runtime::Runtime::new()?;
    let moods_json = rt.block_on(fetch_mood_list_json(&cfg, &mood_list_uuid))?;
    Ok((ctrl, actions::parse_mood_list(&moods_json)?))
}

/// Resolve a lighting controller and the UUID of its `moodList` text state.
///
/// Shared by `lox light moods` and the MCP `list_light_moods` tool.
pub fn resolve_mood_list(
    lox: &mut LoxClient,
    name_or_uuid: &str,
    room: Option<&str>,
) -> Result<(crate::client::Control, String)> {
    let uuid = lox.resolve_with_room(name_or_uuid, room)?;
    let ctrl = lox.find_control(&uuid)?;
    if !matches!(ctrl.typ.as_str(), "LightControllerV2" | "LightController") {
        bail!(
            "'{}' is type '{}', not a LightController",
            ctrl.name,
            ctrl.typ
        );
    }
    let mood_list_uuid = lox
        .get_structure()?
        .get("controls")
        .and_then(|c| c.as_object())
        .and_then(|m| m.get(&uuid))
        .and_then(|c| c.get("states"))
        .and_then(|s| s.as_object())
        .and_then(|s| s.get("moodList"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "'{}' has no moodList state — is it a LightControllerV2?",
                ctrl.name
            )
        })?
        .to_string();
    Ok((ctrl, mood_list_uuid))
}

/// Wait (max 10 s) for the `moodList` text state in the WebSocket state dump.
pub async fn fetch_mood_list_json(cfg: &Config, mood_list_uuid: &str) -> Result<String> {
    tokio::time::timeout(
        Duration::from_secs(10),
        fetch_mood_list_via_ws(cfg, mood_list_uuid),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Timeout waiting for moodList state (10s)"))?
}

/// Connect to the Miniserver WebSocket, subscribe to binary states,
/// and return the text content of the moodList state UUID on first receipt.
async fn fetch_mood_list_via_ws(cfg: &Config, mood_list_uuid: &str) -> Result<String> {
    let target = mood_list_uuid.to_string();
    stream::stream_events_until(cfg, |event| {
        if let StateEvent::TextState { uuid, text, .. } = event
            && *uuid == target
        {
            Some(text.clone())
        } else {
            None
        }
    })
    .await
    .context("moodList state not received in initial state dump")
}

pub fn cmd_light(ctx: &RunContext, action: LightCmd) -> Result<()> {
    match action {
        LightCmd::Moods { name_or_uuid, room } => {
            cmd_light_moods(ctx, name_or_uuid, room)?;
        }
        LightCmd::Mood {
            name_or_uuid,
            action,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?)?;
            let act = actions::parse_mood(&action)?;
            let (ctrl, resp) = run_action(ctx, &mut lox, &name_or_uuid, room.as_deref(), &act)?;
            if let Some(resp) = resp {
                if ctx.json {
                    print_resp(&resp, true, ctx.quiet, &ctrl.name, &act.main_command());
                } else if !ctx.quiet {
                    println!("✓  {} → mood {}", ctrl.name, action);
                }
            }
        }
        LightCmd::Dim {
            name_or_uuid,
            level,
            room,
        } => cmd_dimmer(ctx, name_or_uuid, level, room)?,
        LightCmd::Color {
            name_or_uuid,
            value,
            room,
        } => cmd_color(ctx, name_or_uuid, value, room)?,
    }
    Ok(())
}

pub fn cmd_input(ctx: &RunContext, action: InputCmd) -> Result<()> {
    match action {
        InputCmd::Set {
            name_or_uuid,
            value,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?)?;
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let encoded = encode_path_value(&value);
            if ctx.dry_run {
                print_dry_run(ctx.json, ctx.quiet, &uuid, &encoded, &name_or_uuid, None);
            } else {
                let resp = lox.send_cmd(&uuid, &encoded)?;
                let code = resp
                    .pointer("/LL/Code")
                    .and_then(json_val_str)
                    .unwrap_or_else(|| "?".to_string());
                let val = resp
                    .pointer("/LL/value")
                    .and_then(json_val_str)
                    .unwrap_or_else(|| "?".to_string());
                if code == "200" {
                    if !ctx.quiet {
                        println!("✓  {} = {}", name_or_uuid, val);
                    }
                } else {
                    bail!("Error {}: {}", code, val);
                }
            }
        }
        InputCmd::Pulse { name_or_uuid, room } => {
            let mut lox = LoxClient::new(Config::load()?)?;
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            if let Some(resp) = send_or_dry_run(
                &lox,
                &uuid,
                "pulse",
                &name_or_uuid,
                None,
                ctx.dry_run,
                ctx.json,
                ctx.quiet,
            )? {
                print_resp(&resp, ctx.json, ctx.quiet, &name_or_uuid, "pulse");
            }
        }
    }
    Ok(())
}

pub fn cmd_mood(
    ctx: &RunContext,
    name_or_uuid: String,
    action: String,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let act = actions::parse_mood(&action)?;
    let (ctrl, resp) = run_action(ctx, &mut lox, &name_or_uuid, room.as_deref(), &act)?;
    if let Some(resp) = resp {
        if ctx.json {
            print_resp(&resp, true, ctx.quiet, &ctrl.name, &act.main_command());
        } else {
            println!("✓  {} → mood {}", ctrl.name, action);
            thread::sleep(Duration::from_millis(400));
            let xml = lox.get_all(&ctrl.uuid)?;
            let state = xml_attr(&xml, "value").unwrap_or_default();
            let is_off = state.starts_with("200002700") || state == "0";
            println!(
                "   State: {}  ({})",
                state,
                if is_off { "off" } else { "active" }
            );
        }
    }
    Ok(())
}

pub fn cmd_dimmer(
    ctx: &RunContext,
    name_or_uuid: String,
    level: f64,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let act = actions::parse_dim(level)?;
    let (ctrl, resp) = run_action(ctx, &mut lox, &name_or_uuid, room.as_deref(), &act)?;
    if let Some(resp) = resp {
        print_resp(
            &resp,
            ctx.json,
            ctx.quiet,
            &ctrl.name,
            &format!("dim={}", level),
        );
    }
    Ok(())
}

pub fn cmd_gate(
    ctx: &RunContext,
    name_or_uuid: String,
    action: String,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let act = actions::parse_gate(&action)?;
    let (ctrl, resp) = run_action(ctx, &mut lox, &name_or_uuid, room.as_deref(), &act)?;
    if let Some(resp) = resp {
        print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, &act.main_command());
    }
    Ok(())
}

pub fn cmd_color(
    ctx: &RunContext,
    name_or_uuid: String,
    value: String,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let act = actions::parse_color(&value)?;
    let (ctrl, resp) = run_action(ctx, &mut lox, &name_or_uuid, room.as_deref(), &act)?;
    if let Some(resp) = resp {
        print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, &act.main_command());
    }
    Ok(())
}

pub fn cmd_thermostat(
    ctx: &RunContext,
    name_or_uuid: String,
    action: Option<String>,
    value: Option<String>,
    duration: Option<u64>,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    if !matches!(
        ctrl.typ.as_str(),
        "IRoomControllerV2" | "IRoomController" | "Fronius"
    ) {
        bail!(
            "'{}' is type '{}', not a room controller",
            ctrl.name,
            ctrl.typ
        );
    }
    if let Some(act) = action {
        let action = actions::parse_thermostat(&act, value.as_deref(), duration)?;
        action.check_type(&ctrl.name, &ctrl.typ)?;
        let label = match &action {
            Action::Thermostat(ThermoCmd::ComfortTemp(t)) => format!("temp={}", t),
            Action::Thermostat(ThermoCmd::Mode(_)) => {
                format!("mode={}", value.as_deref().unwrap_or(""))
            }
            Action::Thermostat(ThermoCmd::Override { temp, minutes }) => {
                format!("override={}°/{}min", temp, minutes)
            }
            _ => action.main_command(),
        };
        if let Some(resp) = send_or_dry_run(
            &lox,
            &ctrl.uuid,
            &action.main_command(),
            &ctrl.name,
            ctrl.room.as_deref(),
            ctx.dry_run,
            ctx.json,
            ctx.quiet,
        )? {
            print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, &label);
        }
    } else {
        // Show current thermostat state
        let xml = lox.get_all(&ctrl.uuid)?;
        let val = xml_attr(&xml, "value").unwrap_or("?");
        if ctx.json {
            let mut result = serde_json::json!({
                "name": ctrl.name, "uuid": ctrl.uuid,
                "type": ctrl.typ, "value": val,
            });
            let mut i = 1;
            while let Some(n) = xml_attr(&xml, &format!("n{}", i)) {
                let v = xml_attr(&xml, &format!("v{}", i)).unwrap_or("?");
                if !n.is_empty() {
                    result[n] = serde_json::Value::String(v.to_string());
                }
                i += 1;
            }
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!("Thermostat: {} ({})", ctrl.name, ctrl.uuid);
            println!("Room:       {}", ctrl.room.as_deref().unwrap_or("─"));
            let mut i = 1;
            while let Some(n) = xml_attr(&xml, &format!("n{}", i)) {
                let v = xml_attr(&xml, &format!("v{}", i)).unwrap_or("?");
                if !n.is_empty() {
                    println!("  {:<30} = {}", n, v);
                }
                i += 1;
            }
        }
    }
    Ok(())
}

pub fn cmd_alarm(
    ctx: &RunContext,
    name_or_uuid: String,
    action: String,
    no_motion: bool,
    code: Option<String>,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let act = actions::parse_alarm(&action, no_motion, code)?;
    let (ctrl, resp) = run_action(ctx, &mut lox, &name_or_uuid, room.as_deref(), &act)?;
    if let Some(resp) = resp {
        print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, &act.main_command());
    }
    Ok(())
}

pub fn cmd_door(
    ctx: &RunContext,
    name_or_uuid: String,
    action: String,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let act = actions::parse_door(&action)?;
    let (ctrl, resp) = run_action(ctx, &mut lox, &name_or_uuid, room.as_deref(), &act)?;
    if let Some(resp) = resp {
        print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, &action);
    }
    Ok(())
}

pub fn cmd_intercom(
    ctx: &RunContext,
    name_or_uuid: String,
    action: String,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let act = actions::parse_intercom(&action)?;
    let (ctrl, resp) = run_action(ctx, &mut lox, &name_or_uuid, room.as_deref(), &act)?;
    if let Some(resp) = resp {
        print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, &action);
    }
    Ok(())
}

pub fn cmd_charger(
    ctx: &RunContext,
    name_or_uuid: String,
    action: String,
    limit: Option<f64>,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let act = actions::parse_charger(&action, limit)?;
    let (ctrl, resp) = run_action(ctx, &mut lox, &name_or_uuid, room.as_deref(), &act)?;
    if let Some(resp) = resp {
        print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, &action);
    }
    Ok(())
}

pub fn cmd_lock(
    ctx: &RunContext,
    name_or_uuid: String,
    reason: String,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let act = Ok::<_, anyhow::Error>(Action::LockControl(reason))?;
    let (ctrl, resp) = run_action(ctx, &mut lox, &name_or_uuid, room.as_deref(), &act)?;
    if let Some(resp) = resp {
        print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, "lock");
    }
    Ok(())
}

pub fn cmd_unlock(ctx: &RunContext, name_or_uuid: String, room: Option<String>) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    if let Some(resp) = send_or_dry_run(
        &lox,
        &ctrl.uuid,
        "unlockcontrol",
        &ctrl.name,
        ctrl.room.as_deref(),
        ctx.dry_run,
        ctx.json,
        ctx.quiet,
    )? {
        print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, "unlock");
    }
    Ok(())
}

pub fn cmd_send(
    ctx: &RunContext,
    name_or_uuid: String,
    command: String,
    room: Option<String>,
    secured: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    if ctx.dry_run {
        let ctrl = lox.find_control(&uuid).ok();
        let name = ctrl
            .as_ref()
            .map(|c| c.name.as_str())
            .unwrap_or(&name_or_uuid);
        let room = ctrl.as_ref().and_then(|c| c.room.as_deref());
        print_dry_run(ctx.json, ctx.quiet, &uuid, &command, name, room);
    } else {
        let resp = if let Some(hash) = secured {
            lox.get_json(&format!("/jdev/sps/ios/{}/{}/{}", hash, uuid, command))?
        } else {
            lox.send_cmd(&uuid, &command)?
        };
        print_resp(&resp, ctx.json, ctx.quiet, &name_or_uuid, &command);
    }
    Ok(())
}

pub fn cmd_set(
    ctx: &RunContext,
    name_or_uuid: String,
    value: String,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let encoded = encode_path_value(&value);
    if ctx.dry_run {
        print_dry_run(ctx.json, ctx.quiet, &uuid, &encoded, &name_or_uuid, None);
    } else {
        let resp = lox.send_cmd(&uuid, &encoded)?;
        let code = resp
            .pointer("/LL/Code")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let val = resp
            .pointer("/LL/value")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        if code == "200" {
            if !ctx.quiet {
                println!("✓  {} = {}", name_or_uuid, val);
            }
        } else {
            bail!("Error {}: {}", code, val);
        }
    }
    Ok(())
}

pub fn cmd_run(ctx: &RunContext, scene: String, scene_dry_run: bool) -> Result<()> {
    let cfg = Config::load()?;
    let s = Scene::load_with_config(&scene, &cfg)?;
    let mut lox = LoxClient::new(cfg)?;
    if ctx.dry_run || scene_dry_run {
        println!("▶  {} (dry run)", s.name.as_deref().unwrap_or(&scene));
        if let Some(d) = &s.description {
            println!("   {}", d);
        }
        println!();
        for (i, step) in s.steps.iter().enumerate() {
            let resolved = match lox.resolve(&step.control) {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Step {}: {} (resolve failed: {})", i + 1, step.control, e);
                    continue;
                }
            };
            println!(
                "  {}. {} → {}{}",
                i + 1,
                step.control,
                step.cmd,
                if step.delay_ms > 0 {
                    format!(" (delay {}ms)", step.delay_ms)
                } else {
                    String::new()
                }
            );
            let _ = resolved;
        }
    } else {
        if !ctx.quiet {
            println!("▶  {}", s.name.as_deref().unwrap_or(&scene));
            if let Some(d) = &s.description {
                println!("   {}", d);
            }
            println!();
        }
        for (i, step) in s.steps.iter().enumerate() {
            let uuid = match lox.resolve(&step.control) {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Step {}: {}", i + 1, e);
                    continue;
                }
            };
            if let Some(resp) = send_or_dry_run(
                &lox,
                &uuid,
                &step.cmd,
                &step.control,
                None,
                ctx.dry_run,
                ctx.json,
                ctx.quiet,
            )? {
                print_resp(&resp, ctx.json, ctx.quiet, &step.control, &step.cmd);
            }
            if step.delay_ms > 0 && !ctx.dry_run {
                thread::sleep(Duration::from_millis(step.delay_ms));
            }
        }
    }
    Ok(())
}

pub fn cmd_music(ctx: &RunContext, action: MusicCmd) -> Result<()> {
    let cfg = Config::load()?;
    let music_base = format!(
        "{}:7091",
        cfg.host
            .trim_end_matches('/')
            .replace("https://", "http://")
    );
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(10))
        .build()?;
    let (zone, cmd_path) = match action {
        MusicCmd::Play { zone } => (zone, "play".to_string()),
        MusicCmd::Pause { zone } => (zone, "pause".to_string()),
        MusicCmd::Stop { zone } => (zone, "stop".to_string()),
        MusicCmd::Volume { level, zone } => {
            if level > 100 {
                bail!("Volume must be 0-100");
            }
            (zone, format!("volume/{}", level))
        }
        MusicCmd::Next { zone } => (zone, "queueplus".to_string()),
        MusicCmd::Prev { zone } => (zone, "queueminus".to_string()),
        MusicCmd::Mute { zone } => (zone, "mute".to_string()),
        MusicCmd::Unmute { zone } => (zone, "unmute".to_string()),
    };
    let url = format!("{}/zone/{}/{}", music_base, zone, cmd_path);
    match client.get(&url).send() {
        Ok(resp) => {
            let body = resp.text().unwrap_or_default();
            if ctx.json {
                println!(
                    "{}",
                    serde_json::json!({"zone": zone, "command": cmd_path, "response": body})
                );
            } else {
                println!("✓ Zone {} → {}", zone, cmd_path);
            }
        }
        Err(e) => bail!("Music server error: {}", e),
    }
    Ok(())
}
