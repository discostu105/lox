use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use std::thread;
use std::time::Duration;

use crate::client::{LoxClient, USER_AGENT};
use crate::commands::RunContext;
use crate::config::Config;
use crate::scene::Scene;
use crate::stream::{self, StateEvent};
use crate::{
    InputCmd, LightCmd, MusicCmd, bar, encode_path_value, print_dry_run, print_resp, rgb_to_hsv,
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
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    if !matches!(ctrl.typ.as_str(), "Jalousie" | "CentralJalousie") {
        bail!("'{}' is type '{}', not a Jalousie", ctrl.name, ctrl.typ);
    }
    let cmd_owned: String;
    let cmd: &str = match action.to_lowercase().as_str() {
        "up" | "open" => "FullUp",
        "down" | "close" => "FullDown",
        "stop" => "off",
        "shade" | "auto" => {
            if let Some(pct) = pos {
                if !(0.0..=100.0).contains(&pct) {
                    bail!("Position must be 0-100");
                }
                cmd_owned = format!("manualLamella/{:.4}", pct);
                &cmd_owned
            } else {
                "AutomaticDown"
            }
        }
        "pos" | "position" => {
            let pct = pos.ok_or_else(|| anyhow::anyhow!("pos requires a value 0-100"))?;
            if !(0.0..=100.0).contains(&pct) {
                bail!("Position must be 0-100");
            }
            cmd_owned = format!("manualPosition/{:.4}", pct);
            &cmd_owned
        }
        other => {
            if let Ok(pct) = other.parse::<f64>() {
                if (0.0..=100.0).contains(&pct) {
                    cmd_owned = format!("manualPosition/{:.4}", pct);
                    &cmd_owned
                } else {
                    bail!("Position must be 0-100");
                }
            } else {
                bail!(
                    "Unknown action '{}'. Use: up down stop shade [<0-100>] pos <0-100>",
                    other
                )
            }
        }
    };
    if ctx.dry_run {
        print_dry_run(
            ctx.json,
            ctx.quiet,
            &ctrl.uuid,
            cmd,
            &ctrl.name,
            ctrl.room.as_deref(),
        );
        return Ok(());
    }
    let resp = lox.send_cmd(&ctrl.uuid, cmd)?;
    print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, cmd);
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

/// Mood entry parsed from the moodList TextState JSON.
#[derive(Debug)]
struct MoodEntry {
    id: u64,
    name: String,
}

/// Fetch the list of available moods for a LightControllerV2 via WebSocket.
///
/// The `moodList` state is a TextState sent as JSON during the initial state dump:
/// `[{"name":"Viel Licht","id":777,"static":false},{"name":"Aus","id":778,"static":true}]`
pub fn cmd_light_moods(ctx: &RunContext, name_or_uuid: String, room: Option<String>) -> Result<()> {
    let cfg = Config::load()?;
    let mut lox = LoxClient::new(cfg.clone())?;
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    if !matches!(ctrl.typ.as_str(), "LightControllerV2" | "LightController") {
        bail!(
            "'{}' is type '{}', not a LightController",
            ctrl.name,
            ctrl.typ
        );
    }

    // Get the moodList state UUID from the structure
    let structure = lox.get_structure()?.clone();
    let mood_list_uuid = structure
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

    // Connect via WebSocket and wait for the initial moodList TextState
    let rt = tokio::runtime::Runtime::new()?;
    let moods_json: String = rt.block_on(async {
        use std::time::Duration;
        use tokio::time::timeout;

        timeout(
            Duration::from_secs(10),
            fetch_mood_list_via_ws(&cfg, &mood_list_uuid),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Timeout waiting for moodList state (10s)"))?
    })?;

    // Parse the JSON array
    let entries: Vec<serde_json::Value> = serde_json::from_str(&moods_json)
        .map_err(|e| anyhow::anyhow!("Failed to parse moodList JSON: {}", e))?;

    let mut moods: Vec<MoodEntry> = entries
        .iter()
        .filter_map(|v| {
            let id = v.get("id").and_then(|i| i.as_u64())?;
            let name = v.get("name").and_then(|n| n.as_str())?.to_string();
            Some(MoodEntry { id, name })
        })
        .collect();

    // Sort by id for consistent output
    moods.sort_by_key(|m| m.id);

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

/// Connect to the Miniserver WebSocket, subscribe to binary states,
/// and return the text content of the moodList state UUID on first receipt.
async fn fetch_mood_list_via_ws(cfg: &Config, mood_list_uuid: &str) -> Result<String> {
    use crate::stream::parse_binary_payload;
    use crate::ws::LoxWsClient;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let ws_client = LoxWsClient::new(cfg.clone());
    let (mut ws_stream, _resp) = ws_client.connect_raw().await?;

    // Authenticate
    stream::ws_authenticate_pub(&mut ws_stream, cfg).await?;

    // Subscribe to binary status updates
    ws_stream
        .send(Message::Text("jdev/sps/enablebinstatusupdate".to_string()))
        .await?;

    let target_uuid = mood_list_uuid.to_string();
    let mut pending_header: Option<(u8, u32)> = None;

    while let Some(msg_result) = ws_stream.next().await {
        let msg = msg_result?;
        match msg {
            Message::Binary(data) => {
                if let Some((msg_type, _)) = pending_header.take() {
                    let events = parse_binary_payload(msg_type, &data)?;
                    for event in events {
                        if let StateEvent::TextState { uuid, text, .. } = event
                            && uuid == target_uuid
                        {
                            return Ok(text);
                        }
                    }
                } else if data.len() >= 8 && data[0] == 0x03 {
                    let msg_type = data[1];
                    let estimated = data[2] & 0x01 != 0;
                    let payload_len = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                    if estimated {
                        continue;
                    }
                    if payload_len == 0 {
                        // no payload
                    } else if data.len() > 8 {
                        // header + payload in same frame
                        let events = parse_binary_payload(msg_type, &data[8..])?;
                        for event in events {
                            if let StateEvent::TextState { uuid, text, .. } = event
                                && uuid == target_uuid
                            {
                                return Ok(text);
                            }
                        }
                    } else {
                        pending_header = Some((msg_type, payload_len));
                    }
                }
            }
            Message::Text(_) => {}
            Message::Ping(data) => {
                ws_stream.send(Message::Pong(data)).await?;
            }
            Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }

    bail!("moodList state not received in initial state dump")
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
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !matches!(ctrl.typ.as_str(), "LightControllerV2" | "LightController") {
                bail!(
                    "'{}' is type '{}', not a LightController",
                    ctrl.name,
                    ctrl.typ
                );
            }
            let cmd_owned: String;
            let cmd: &str = match action.to_lowercase().as_str() {
                "plus" | "next" | "+" => "plus",
                "minus" | "prev" | "-" => "minus",
                "off" => "setMood/778",
                other => {
                    if let Ok(id) = other.parse::<u32>() {
                        cmd_owned = format!("setMood/{}", id);
                        &cmd_owned
                    } else {
                        bail!(
                            "Unknown mood action '{}'. Use: plus, minus, off, or a numeric mood ID",
                            other
                        )
                    }
                }
            };
            if let Some(resp) = send_or_dry_run(
                &lox,
                &ctrl.uuid,
                cmd,
                &ctrl.name,
                ctrl.room.as_deref(),
                ctx.dry_run,
                ctx.json,
                ctx.quiet,
            )? {
                if ctx.json {
                    print_resp(&resp, true, ctx.quiet, &ctrl.name, cmd);
                } else if !ctx.quiet {
                    println!("✓  {} → mood {}", ctrl.name, action);
                }
            }
        }
        LightCmd::Dim {
            name_or_uuid,
            level,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?)?;
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !(0.0..=100.0).contains(&level) {
                bail!("Dimmer level must be 0-100");
            }
            let dim_cmd = format!("{}", level);
            if let Some(resp) = send_or_dry_run(
                &lox,
                &ctrl.uuid,
                &dim_cmd,
                &ctrl.name,
                ctrl.room.as_deref(),
                ctx.dry_run,
                ctx.json,
                ctx.quiet,
            )? {
                print_resp(
                    &resp,
                    ctx.json,
                    ctx.quiet,
                    &ctrl.name,
                    &format!("dim={}", level),
                );
            }
        }
        LightCmd::Color {
            name_or_uuid,
            value,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?)?;
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !matches!(ctrl.typ.as_str(), "ColorPickerV2" | "ColorPicker") {
                bail!("'{}' is type '{}', not a ColorPicker", ctrl.name, ctrl.typ);
            }
            let cmd = if value.starts_with('#') {
                let hex = value.trim_start_matches('#');
                if hex.len() != 6 {
                    bail!("Hex color must be 6 digits: #RRGGBB");
                }
                let r = u8::from_str_radix(&hex[0..2], 16)?;
                let g = u8::from_str_radix(&hex[2..4], 16)?;
                let b = u8::from_str_radix(&hex[4..6], 16)?;
                let (h, s, v) = rgb_to_hsv(r, g, b);
                format!("hsv({},{},{})", h, s, v)
            } else {
                value
            };
            if let Some(resp) = send_or_dry_run(
                &lox,
                &ctrl.uuid,
                &cmd,
                &ctrl.name,
                ctrl.room.as_deref(),
                ctx.dry_run,
                ctx.json,
                ctx.quiet,
            )? {
                print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, &cmd);
            }
        }
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
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    if !matches!(ctrl.typ.as_str(), "LightControllerV2" | "LightController") {
        bail!(
            "'{}' is type '{}', not a LightController",
            ctrl.name,
            ctrl.typ
        );
    }
    let cmd_owned: String;
    let cmd: &str = match action.to_lowercase().as_str() {
        "plus" | "next" | "+" => "plus",
        "minus" | "prev" | "-" => "minus",
        "off" => "setMood/778",
        other => {
            if let Ok(id) = other.parse::<u32>() {
                cmd_owned = format!("setMood/{}", id);
                &cmd_owned
            } else {
                bail!(
                    "Unknown mood action '{}'. Use: plus, minus, off, or a numeric mood ID",
                    other
                )
            }
        }
    };
    if let Some(resp) = send_or_dry_run(
        &lox,
        &ctrl.uuid,
        cmd,
        &ctrl.name,
        ctrl.room.as_deref(),
        ctx.dry_run,
        ctx.json,
        ctx.quiet,
    )? {
        if ctx.json {
            print_resp(&resp, true, ctx.quiet, &ctrl.name, cmd);
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
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    if !(0.0..=100.0).contains(&level) {
        bail!("Dimmer level must be 0-100");
    }
    let dim_cmd = format!("{}", level);
    if let Some(resp) = send_or_dry_run(
        &lox,
        &ctrl.uuid,
        &dim_cmd,
        &ctrl.name,
        ctrl.room.as_deref(),
        ctx.dry_run,
        ctx.json,
        ctx.quiet,
    )? {
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
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    if !matches!(ctrl.typ.as_str(), "Gate" | "CentralGate") {
        bail!("'{}' is type '{}', not a Gate", ctrl.name, ctrl.typ);
    }
    let cmd = match action.to_lowercase().as_str() {
        "open" => "open",
        "close" => "close",
        "stop" => "stop",
        other => bail!("Unknown gate action '{}'. Use: open, close, stop", other),
    };
    if let Some(resp) = send_or_dry_run(
        &lox,
        &ctrl.uuid,
        cmd,
        &ctrl.name,
        ctrl.room.as_deref(),
        ctx.dry_run,
        ctx.json,
        ctx.quiet,
    )? {
        print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, cmd);
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
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    if !matches!(ctrl.typ.as_str(), "ColorPickerV2" | "ColorPicker") {
        bail!("'{}' is type '{}', not a ColorPicker", ctrl.name, ctrl.typ);
    }
    let cmd = if value.starts_with('#') {
        let hex = value.trim_start_matches('#');
        if hex.len() != 6 {
            bail!("Hex color must be 6 digits: #RRGGBB");
        }
        let r = u8::from_str_radix(&hex[0..2], 16).context("Invalid red component")?;
        let g = u8::from_str_radix(&hex[2..4], 16).context("Invalid green component")?;
        let b = u8::from_str_radix(&hex[4..6], 16).context("Invalid blue component")?;
        let (h, s, v) = rgb_to_hsv(r, g, b);
        format!("hsv({},{},{})", h, s, v)
    } else {
        value.clone()
    };
    if let Some(resp) = send_or_dry_run(
        &lox,
        &ctrl.uuid,
        &cmd,
        &ctrl.name,
        ctrl.room.as_deref(),
        ctx.dry_run,
        ctx.json,
        ctx.quiet,
    )? {
        print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, &cmd);
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
        match act.to_lowercase().as_str() {
            "temp" | "temperature" => {
                let t: f64 = value
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("Usage: lox thermostat <name> temp <°C>"))?
                    .parse()
                    .context("Temperature must be a number")?;
                let temp_cmd = format!("setComfortTemperature/{}", t);
                if let Some(resp) = send_or_dry_run(
                    &lox,
                    &ctrl.uuid,
                    &temp_cmd,
                    &ctrl.name,
                    ctrl.room.as_deref(),
                    ctx.dry_run,
                    ctx.json,
                    ctx.quiet,
                )? {
                    print_resp(
                        &resp,
                        ctx.json,
                        ctx.quiet,
                        &ctrl.name,
                        &format!("temp={}", t),
                    );
                }
            }
            "mode" => {
                let m = value.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Usage: lox thermostat <name> mode <auto|manual|comfort|eco>")
                })?;
                let lower = m.to_lowercase();
                let mode_id = match lower.as_str() {
                    "auto" | "automatic" => "0",
                    "manual" => "1",
                    "comfort" => "2",
                    "eco" | "economy" => "3",
                    "building-protection" | "building" => "4",
                    other => other,
                };
                let mode_cmd = format!("setOperatingMode/{}", mode_id);
                if let Some(resp) = send_or_dry_run(
                    &lox,
                    &ctrl.uuid,
                    &mode_cmd,
                    &ctrl.name,
                    ctrl.room.as_deref(),
                    ctx.dry_run,
                    ctx.json,
                    ctx.quiet,
                )? {
                    print_resp(
                        &resp,
                        ctx.json,
                        ctx.quiet,
                        &ctrl.name,
                        &format!("mode={}", m),
                    );
                }
            }
            "override" => {
                let temp_override: f64 = value
                    .as_deref()
                    .ok_or_else(|| {
                        anyhow::anyhow!("Usage: lox thermostat <name> override <°C> [minutes]")
                    })?
                    .parse()
                    .context("Override temperature must be a number")?;
                let dur = duration.unwrap_or(60);
                let override_cmd = format!("override/{}/{}", temp_override, dur);
                if let Some(resp) = send_or_dry_run(
                    &lox,
                    &ctrl.uuid,
                    &override_cmd,
                    &ctrl.name,
                    ctrl.room.as_deref(),
                    ctx.dry_run,
                    ctx.json,
                    ctx.quiet,
                )? {
                    print_resp(
                        &resp,
                        ctx.json,
                        ctx.quiet,
                        &ctrl.name,
                        &format!("override={}°/{}min", temp_override, dur),
                    );
                }
            }
            other => bail!(
                "Unknown thermostat action '{}'. Use: temp, mode, override",
                other
            ),
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
            loop {
                let Some(n) = xml_attr(&xml, &format!("n{}", i)) else {
                    break;
                };
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
            loop {
                let Some(n) = xml_attr(&xml, &format!("n{}", i)) else {
                    break;
                };
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
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    if !matches!(ctrl.typ.as_str(), "Alarm") {
        bail!("'{}' is type '{}', not an Alarm", ctrl.name, ctrl.typ);
    }
    let cmd_owned: String;
    let cmd: &str = match action.to_lowercase().as_str() {
        "arm" | "on" => {
            let base = if no_motion {
                "delayedon/0"
            } else {
                "delayedon/1"
            };
            if let Some(ref pin) = code {
                cmd_owned = format!("{}/{}", base, pin);
                &cmd_owned
            } else {
                base
            }
        }
        "arm-home" | "home" => {
            if let Some(ref pin) = code {
                cmd_owned = format!("delayedon/0/{}", pin);
                &cmd_owned
            } else {
                "delayedon/0"
            }
        }
        "disarm" | "off" => {
            if let Some(ref pin) = code {
                cmd_owned = format!("off/{}", pin);
                &cmd_owned
            } else {
                "off"
            }
        }
        "quit" | "ack" | "acknowledge" => "quit",
        other => bail!(
            "Unknown alarm action '{}'. Use: arm, arm-home, disarm, quit",
            other
        ),
    };
    if let Some(resp) = send_or_dry_run(
        &lox,
        &ctrl.uuid,
        cmd,
        &ctrl.name,
        ctrl.room.as_deref(),
        ctx.dry_run,
        ctx.json,
        ctx.quiet,
    )? {
        print_resp(&resp, ctx.json, ctx.quiet, &ctrl.name, cmd);
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
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    if !ctrl.typ.contains("DoorLock") && !ctrl.typ.contains("Lock") {
        bail!("'{}' is type '{}', not a DoorLock", ctrl.name, ctrl.typ);
    }
    let cmd = match action.to_lowercase().as_str() {
        "lock" => "on",
        "unlock" => "off",
        "open" => "open",
        other => bail!(
            "Unknown doorlock action '{}'. Use: lock, unlock, open",
            other
        ),
    };
    if let Some(resp) = send_or_dry_run(
        &lox,
        &ctrl.uuid,
        cmd,
        &ctrl.name,
        ctrl.room.as_deref(),
        ctx.dry_run,
        ctx.json,
        ctx.quiet,
    )? {
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
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    if !ctrl.typ.contains("Intercom") {
        bail!("'{}' is type '{}', not an Intercom", ctrl.name, ctrl.typ);
    }
    let cmd = match action.to_lowercase().as_str() {
        "answer" => "answer",
        "hangup" | "decline" => "hangup",
        "open" => "open",
        other => bail!(
            "Unknown intercom action '{}'. Use: answer, hangup, open",
            other
        ),
    };
    if let Some(resp) = send_or_dry_run(
        &lox,
        &ctrl.uuid,
        cmd,
        &ctrl.name,
        ctrl.room.as_deref(),
        ctx.dry_run,
        ctx.json,
        ctx.quiet,
    )? {
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
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    if !ctrl.typ.contains("Charger") && !ctrl.typ.contains("EV") {
        bail!("'{}' is type '{}', not a Charger", ctrl.name, ctrl.typ);
    }
    let cmd_owned: String;
    let cmd: &str = match action.to_lowercase().as_str() {
        "start" => {
            if let Some(kwh) = limit {
                cmd_owned = format!("start/{:.1}", kwh);
                &cmd_owned
            } else {
                "start"
            }
        }
        "stop" => "stop",
        "pause" => "pause",
        other => bail!(
            "Unknown charger action '{}'. Use: start, stop, pause",
            other
        ),
    };
    if let Some(resp) = send_or_dry_run(
        &lox,
        &ctrl.uuid,
        cmd,
        &ctrl.name,
        ctrl.room.as_deref(),
        ctx.dry_run,
        ctx.json,
        ctx.quiet,
    )? {
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
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;
    let lock_cmd = format!("lockcontrol/1/{}", encode_path_value(&reason));
    if let Some(resp) = send_or_dry_run(
        &lox,
        &ctrl.uuid,
        &lock_cmd,
        &ctrl.name,
        ctrl.room.as_deref(),
        ctx.dry_run,
        ctx.json,
        ctx.quiet,
    )? {
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
    let s = Scene::load(&scene)?;
    let mut lox = LoxClient::new(Config::load()?)?;
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
