use anyhow::{Result, bail};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Cursor;
use std::thread;
use std::time::Duration;

use crate::client::LoxClient;
use crate::commands::RunContext;
use crate::config::Config;
use crate::stream;
use crate::{
    AutopilotCmd, eval_op, find_stats_files, lox_epoch, lox_timestamp_to_string, matches_filters,
    now_hms, parse_stats_entries, parse_weather_entry, print_stream_event, stats_data_offset,
    stats_file_path, stats_period, xml_attr,
};

pub fn cmd_ls(
    ctx: &RunContext,
    r#type: Option<String>,
    room: Option<String>,
    values: bool,
    cat: Option<String>,
    favorites: bool,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let controls = lox.list_controls_ext(
        r#type.as_deref(),
        room.as_deref(),
        cat.as_deref(),
        favorites,
    )?;
    if ctx.json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &controls
                    .iter()
                    .map(|c| serde_json::json!({
                        "name": c.name, "uuid": c.uuid, "type": c.typ, "room": c.room
                    }))
                    .collect::<Vec<_>>()
            )?
        );
    } else if values {
        if !ctx.no_header {
            println!(
                "{:<40} {:<24} {:<22} {:<20} UUID",
                "NAME", "ROOM", "TYPE", "VALUE"
            );
            println!("{}", "─".repeat(140));
        }
        for c in &controls {
            let val = lox
                .get_all(&c.uuid)
                .ok()
                .and_then(|xml| xml_attr(&xml, "value").map(|s| s.to_string()))
                .unwrap_or_else(|| "-".to_string());
            println!(
                "{:<40} {:<24} {:<22} {:<20} {}",
                c.name,
                c.room.as_deref().unwrap_or("─"),
                c.typ,
                val,
                c.uuid
            );
        }
        println!("\n{} controls", controls.len());
    } else {
        if !ctx.no_header {
            println!("{:<40} {:<24} {:<22} UUID", "NAME", "ROOM", "TYPE");
            println!("{}", "─".repeat(120));
        }
        for c in &controls {
            println!(
                "{:<40} {:<24} {:<22} {}",
                c.name,
                c.room.as_deref().unwrap_or("─"),
                c.typ,
                c.uuid
            );
        }
        println!("\n{} controls", controls.len());
    }
    Ok(())
}

pub fn cmd_get(ctx: &RunContext, name_or_uuid: String, room: Option<String>) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let uuid_resolved = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid_resolved)?;
    let xml = lox.get_all(&ctrl.uuid)?;
    let val = xml_attr(&xml, "value").unwrap_or("?");
    let code = xml_attr(&xml, "Code").unwrap_or("?");

    if ctx.json {
        let mut result = serde_json::json!({
            "name": ctrl.name, "uuid": ctrl.uuid,
            "type": ctrl.typ, "room": ctrl.room, "value": val,
        });
        for attr in &[
            "StateUp",
            "StateDown",
            "StatePos",
            "StateShade",
            "StateAutoShade",
            "StateSafety",
        ] {
            if let Some(v) = xml_attr(&xml, attr) {
                result[attr] = Value::String(v.to_string());
            }
        }
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Control:  {} ({})", ctrl.name, ctrl.uuid);
        println!(
            "Type:     {}   Room: {}",
            ctrl.typ,
            ctrl.room.as_deref().unwrap_or("─")
        );
        println!("Value:    {}  [Code {}]", val, code);
        for attr in &[
            "StateUp",
            "StateDown",
            "StatePos",
            "StateShade",
            "StateAutoShade",
        ] {
            if let Some(v) = xml_attr(&xml, attr) {
                println!("  {:<12} {}", attr.trim_start_matches("State"), v);
            }
        }
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
    Ok(())
}

pub fn cmd_info(ctx: &RunContext, name_or_uuid: String, room: Option<String>) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let uuid_resolved = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid_resolved)?;
    let ctrl_json = lox.get_control_json(&ctrl.uuid)?;
    let xml = lox.get_all(&ctrl.uuid).unwrap_or_default();

    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&ctrl_json)?);
    } else {
        println!("Control:    {} ({})", ctrl.name, ctrl.uuid);
        println!(
            "Type:       {}   Room: {}   Cat: {}",
            ctrl.typ,
            ctrl.room.as_deref().unwrap_or("─"),
            ctrl.cat.as_deref().unwrap_or("─"),
        );
        println!(
            "Favorite:   {}   Secured: {}",
            if ctrl.is_favorite { "yes" } else { "no" },
            if ctrl.is_secured { "yes" } else { "no" },
        );
        let val = xml_attr(&xml, "value").unwrap_or("?");
        println!("Value:      {}", val);

        if let Some(subs) = ctrl_json.get("subControls").and_then(|s| s.as_object()) {
            println!("\nSub-controls:");
            for (sub_uuid, sub) in subs {
                let sub_name = sub.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                let sub_type = sub.get("type").and_then(|t| t.as_str()).unwrap_or("?");
                println!("  {:<30} {:<20} {}", sub_name, sub_type, sub_uuid);
            }
        }

        if let Some(states) = ctrl_json.get("states").and_then(|s| s.as_object()) {
            println!("\nStates:");
            let mut state_list: Vec<_> = states.iter().collect();
            state_list.sort_by_key(|(k, _)| k.as_str());
            for (state_name, state_uuid) in &state_list {
                let uuid_str = state_uuid.as_str().unwrap_or("?");
                println!("  {:<30} {}", state_name, uuid_str);
            }
        }

        if let Some(details) = ctrl_json.get("details").and_then(|d| d.as_object())
            && let Some(moods) = details.get("moods").and_then(|m| m.as_array())
        {
            println!("\nMoods:");
            for mood in moods {
                let id = mood.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
                let name = mood.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                println!("  {:<6} {}", id, name);
            }
        }

        if let Some(stat) = ctrl_json.get("statistic")
            && !stat.is_null()
        {
            println!("\nStatistics: enabled");
            if let Some(outputs) = stat.get("outputs").and_then(|o| o.as_object()) {
                for (k, v) in outputs {
                    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                    println!("  {:<30} {}", name, k);
                }
            }
        }
    }
    Ok(())
}

pub fn cmd_watch(_ctx: &RunContext, name_or_uuid: String, interval: u64) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let ctrl = lox.find_control(&name_or_uuid)?;
    println!(
        "Watching '{}' every {}s  (Ctrl+C to stop)",
        ctrl.name, interval
    );
    let mut last = String::new();
    loop {
        if let Ok(xml) = lox.get_all(&ctrl.uuid) {
            let val = xml_attr(&xml, "value").unwrap_or("?").to_string();
            if val != last {
                println!("[{}]  {} = {}", now_hms(), ctrl.name, val);
                last = val;
            }
        }
        thread::sleep(Duration::from_secs(interval));
    }
}

pub fn cmd_stream(
    ctx: &RunContext,
    r#type: Option<String>,
    room: Option<String>,
    control: Option<String>,
    initial: bool,
) -> Result<()> {
    let cfg = Config::load()?;
    let mut lox = LoxClient::new(cfg.clone())?;
    let structure = lox.get_structure()?.clone();
    let state_map = stream::build_state_uuid_map(&structure);
    if !ctx.quiet {
        eprintln!("Streaming state changes (Ctrl+C to stop)...");
    }
    if ctx.csv && !ctx.no_header {
        println!("timestamp,control,state,value,room,type,uuid");
    }

    let type_filter = r#type;
    let room_filter = room;
    let control_filter = control;
    let mut initial_done = false;
    let json = ctx.json;
    let csv = ctx.csv;
    let quiet = ctx.quiet;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(stream::stream_events(&cfg, |events| {
        for event in &events {
            match event {
                stream::StateEvent::ValueState { uuid, value } => {
                    if !initial && !initial_done {
                        continue;
                    }
                    if let Some(info) = state_map.get(uuid) {
                        if !matches_filters(
                            info,
                            type_filter.as_deref(),
                            room_filter.as_deref(),
                            control_filter.as_deref(),
                        ) {
                            continue;
                        }
                        print_stream_event(
                            json,
                            csv,
                            &info.control_name,
                            &info.state_name,
                            &format!("{}", value),
                            info.room.as_deref(),
                            &info.control_type,
                            uuid,
                        );
                    }
                }
                stream::StateEvent::TextState { uuid, text, .. } => {
                    if !initial && !initial_done {
                        continue;
                    }
                    if let Some(info) = state_map.get(uuid) {
                        if !matches_filters(
                            info,
                            type_filter.as_deref(),
                            room_filter.as_deref(),
                            control_filter.as_deref(),
                        ) {
                            continue;
                        }
                        print_stream_event(
                            json,
                            csv,
                            &info.control_name,
                            &info.state_name,
                            text,
                            info.room.as_deref(),
                            &info.control_type,
                            uuid,
                        );
                    }
                }
                stream::StateEvent::OutOfService => {
                    if !quiet {
                        eprintln!("Miniserver going offline (firmware update?). Exiting.");
                    }
                    std::process::exit(0);
                }
                _ => {}
            }
        }
        if !initial_done {
            initial_done = true;
        }
        Ok(())
    }))?;
    Ok(())
}

pub fn cmd_if(
    ctx: &RunContext,
    name_or_uuid: String,
    op: String,
    value: String,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let uuid_resolved = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid_resolved)?;
    let xml = lox.get_all(&ctrl.uuid)?;
    let current = xml_attr(&xml, "value").unwrap_or("").to_string();
    let matches = eval_op(&current, &op, &value)?;
    if !ctx.json {
        println!(
            "{} = {}  →  {} {} {}  →  {}",
            ctrl.name,
            current,
            current,
            op,
            value,
            if matches { "✓ true" } else { "✗ false" }
        );
    } else {
        println!(
            "{}",
            serde_json::json!({
                "control": ctrl.name, "current": current,
                "op": op, "target": value, "result": matches
            })
        );
    }
    std::process::exit(if matches { 0 } else { 1 });
}

pub fn cmd_rooms(ctx: &RunContext) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let structure = lox.get_structure()?;
    if let Some(rooms) = structure.get("rooms").and_then(|r| r.as_object()) {
        let mut entries: Vec<_> = rooms
            .iter()
            .filter_map(|(uuid, r)| {
                r.get("name")
                    .and_then(|n| n.as_str())
                    .map(|name| (uuid.as_str(), name))
            })
            .collect();
        entries.sort_by_key(|(_, name)| *name);
        if ctx.json {
            let arr: Vec<_> = entries
                .iter()
                .map(|(uuid, name)| serde_json::json!({"uuid": uuid, "name": name}))
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr)?);
        } else {
            for (_, name) in entries {
                println!("{}", name);
            }
        }
    }
    Ok(())
}

pub fn cmd_categories(ctx: &RunContext) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let cats = lox.list_categories()?;
    if ctx.json {
        let arr: Vec<_> = cats
            .iter()
            .map(|(uuid, name)| serde_json::json!({"uuid": uuid, "name": name}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        println!("{:<36} NAME", "UUID");
        println!("{}", "─".repeat(60));
        for (uuid, name) in &cats {
            println!("{:<36} {}", uuid, name);
        }
        println!("\n{} categories", cats.len());
    }
    Ok(())
}

pub fn cmd_globals(ctx: &RunContext) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let globals = lox.get_global_states()?;
    if ctx.json {
        let mut map = serde_json::Map::new();
        for (name, uuid) in &globals {
            let val = lox
                .get_all(uuid)
                .ok()
                .and_then(|xml| xml_attr(&xml, "value").map(|s| s.to_string()))
                .unwrap_or_else(|| "?".to_string());
            map.insert(
                name.clone(),
                serde_json::json!({"uuid": uuid, "value": val}),
            );
        }
        println!("{}", serde_json::to_string_pretty(&Value::Object(map))?);
    } else {
        println!("{:<30} {:<36} VALUE", "STATE", "UUID");
        println!("{}", "─".repeat(90));
        for (name, uuid) in &globals {
            let val = lox
                .get_all(uuid)
                .ok()
                .and_then(|xml| xml_attr(&xml, "value").map(|s| s.to_string()))
                .unwrap_or_else(|| "?".to_string());
            println!("{:<30} {:<36} {}", name, uuid, val);
        }
    }
    Ok(())
}

pub fn cmd_modes(ctx: &RunContext) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let modes = lox.get_operating_modes()?;
    let globals = lox.get_global_states().unwrap_or_default();
    let current_mode = globals
        .iter()
        .find(|(name, _)| name == "operatingMode")
        .and_then(|(_, uuid)| {
            lox.get_all(uuid)
                .ok()
                .and_then(|xml| xml_attr(&xml, "value").map(|s| s.to_string()))
        });
    if ctx.json {
        let arr: Vec<_> = modes
            .iter()
            .map(|(id, name)| {
                let active = current_mode.as_deref() == Some(id.as_str());
                serde_json::json!({"id": id, "name": name, "active": active})
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        println!("{:<6} {:<30} STATUS", "ID", "MODE");
        println!("{}", "─".repeat(50));
        for (id, name) in &modes {
            let active = current_mode.as_deref() == Some(id.as_str());
            println!(
                "{:<6} {:<30} {}",
                id,
                name,
                if active { "← active" } else { "" }
            );
        }
    }
    Ok(())
}

pub fn cmd_sensors(ctx: &RunContext, r#type: String, room: Option<String>) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let type_lower = r#type.to_lowercase();
    let type_filter: Option<&str> = match type_lower.as_str() {
        "temperature" | "temp" => Some("InfoOnlyAnalog"),
        "door-window" | "doorwindow" => Some("InfoOnlyDigital"),
        "motion" => None,
        "smoke" => Some("SmokeAlarm"),
        _ => None,
    };
    let controls = lox.list_controls(type_filter, room.as_deref())?;
    let filtered: Vec<_> = controls
        .iter()
        .filter(|c| match type_lower.as_str() {
            "temperature" | "temp" => c.typ == "InfoOnlyAnalog",
            "door-window" | "doorwindow" => c.typ == "InfoOnlyDigital",
            "motion" => {
                matches!(c.typ.as_str(), "PresenceDetector" | "MotionSensor")
            }
            "smoke" => c.typ == "SmokeAlarm",
            _ => matches!(
                c.typ.as_str(),
                "InfoOnlyAnalog"
                    | "InfoOnlyDigital"
                    | "PresenceDetector"
                    | "MotionSensor"
                    | "SmokeAlarm"
                    | "Meter"
            ),
        })
        .collect();
    if ctx.json {
        let mut arr = Vec::new();
        for c in &filtered {
            let xml = lox.get_all(&c.uuid).unwrap_or_default();
            let val = xml_attr(&xml, "value").unwrap_or("?").to_string();
            arr.push(serde_json::json!({
                "name": c.name, "uuid": c.uuid, "type": c.typ,
                "room": c.room, "value": val,
            }));
        }
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else if filtered.is_empty() {
        println!("No sensors found.");
    } else {
        if !ctx.no_header {
            println!("{:<36} {:<20} {:<20} VALUE", "NAME", "TYPE", "ROOM");
            println!("{}", "─".repeat(96));
        }
        for c in &filtered {
            let xml = lox.get_all(&c.uuid).unwrap_or_default();
            let val = xml_attr(&xml, "value").unwrap_or("?");
            println!(
                "{:<36} {:<20} {:<20} {}",
                c.name,
                c.typ,
                c.room.as_deref().unwrap_or("─"),
                val
            );
        }
        println!("\n{} sensors", filtered.len());
    }
    Ok(())
}

pub fn cmd_energy(ctx: &RunContext, room: Option<String>) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let controls = lox.list_controls(None, room.as_deref())?;
    let energy: Vec<_> = controls
        .iter()
        .filter(|c| {
            matches!(
                c.typ.as_str(),
                "Meter" | "EnergyManager" | "EnergyMonitor" | "EnergyFlowMonitor"
            ) || c.typ.contains("Energy")
        })
        .collect();
    if ctx.json {
        let mut arr = Vec::new();
        for c in &energy {
            let xml = lox.get_all(&c.uuid).unwrap_or_default();
            let val = xml_attr(&xml, "value").unwrap_or("?").to_string();
            arr.push(serde_json::json!({
                "name": c.name, "uuid": c.uuid, "type": c.typ,
                "room": c.room, "value": val,
            }));
        }
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else if energy.is_empty() {
        println!("No energy meters found.");
    } else {
        if !ctx.no_header {
            println!("{:<36} {:<20} {:<20} VALUE", "NAME", "TYPE", "ROOM");
            println!("{}", "─".repeat(96));
        }
        for c in &energy {
            let xml = lox.get_all(&c.uuid).unwrap_or_default();
            let val = xml_attr(&xml, "value").unwrap_or("?");
            println!(
                "{:<36} {:<20} {:<20} {}",
                c.name,
                c.typ,
                c.room.as_deref().unwrap_or("─"),
                val
            );
        }
        println!("\n{} energy meters", energy.len());
    }
    Ok(())
}

pub fn cmd_weather(ctx: &RunContext, forecast: bool) -> Result<()> {
    let lox = LoxClient::new(Config::load()?)?;
    let data = lox.get_bytes("/data/weatheru.bin")?;
    if data.is_empty() {
        println!("No weather data available on the Miniserver.");
    } else {
        let entry_size = 108;
        let num_entries = data.len() / entry_size;
        let epoch = lox_epoch();

        let max_display = if forecast {
            num_entries
        } else {
            1.min(num_entries)
        };

        if ctx.json {
            let mut arr = Vec::new();
            for i in 0..max_display {
                let offset = i * entry_size;
                let mut cursor = Cursor::new(&data[offset..offset + entry_size]);
                if let Some(entry) = parse_weather_entry(&mut cursor, &epoch) {
                    arr.push(entry);
                }
            }
            println!("{}", serde_json::to_string_pretty(&arr)?);
        } else {
            println!(
                "{:<20} {:>8} {:>8} {:>8} {:>8} {:>10}",
                "TIME", "TEMP°C", "HUM%", "WIND", "RAIN", "CLOUDS"
            );
            println!("{}", "─".repeat(72));
            for i in 0..max_display {
                let offset = i * entry_size;
                let mut cursor = Cursor::new(&data[offset..offset + entry_size]);
                if let Some(entry) = parse_weather_entry(&mut cursor, &epoch) {
                    println!(
                        "{:<20} {:>8.1} {:>8.0} {:>8.1} {:>8.1} {:>10.0}",
                        entry["timestamp"].as_str().unwrap_or("?"),
                        entry["temperature"].as_f64().unwrap_or(0.0),
                        entry["humidity"].as_f64().unwrap_or(0.0),
                        entry["wind_speed"].as_f64().unwrap_or(0.0),
                        entry["rain"].as_f64().unwrap_or(0.0),
                        entry["clouds"].as_f64().unwrap_or(0.0),
                    );
                }
            }
        }
    }
    Ok(())
}

pub fn cmd_stats(ctx: &RunContext) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let structure = lox.get_structure()?.clone();
    let mut rooms: HashMap<String, String> = HashMap::new();
    if let Some(map) = structure.get("rooms").and_then(|r| r.as_object()) {
        for (uuid, room) in map {
            if let Some(name) = room.get("name").and_then(|n| n.as_str()) {
                rooms.insert(uuid.clone(), name.to_string());
            }
        }
    }
    let mut stats_controls = Vec::new();
    if let Some(ctrl_map) = structure.get("controls").and_then(|c| c.as_object()) {
        for (uuid, ctrl) in ctrl_map {
            if let Some(stat) = ctrl.get("statistic")
                && !stat.is_null()
            {
                let name = ctrl.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                let typ = ctrl.get("type").and_then(|t| t.as_str()).unwrap_or("?");
                let room_uuid = ctrl.get("room").and_then(|r| r.as_str()).unwrap_or("");
                let room = rooms.get(room_uuid).cloned();
                stats_controls.push((name.to_string(), uuid.clone(), typ.to_string(), room));
            }
        }
    }
    stats_controls.sort_by(|a, b| a.0.cmp(&b.0));
    if ctx.json {
        let arr: Vec<_> = stats_controls
            .iter()
            .map(|(n, u, t, r)| serde_json::json!({"name": n, "uuid": u, "type": t, "room": r}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        if !ctx.no_header {
            println!("{:<40} {:<22} {:<24} UUID", "NAME", "TYPE", "ROOM");
            println!("{}", "─".repeat(120));
        }
        for (name, uuid, typ, room) in &stats_controls {
            println!(
                "{:<40} {:<22} {:<24} {}",
                name,
                typ,
                room.as_deref().unwrap_or("─"),
                uuid
            );
        }
        println!("\n{} controls with statistics", stats_controls.len());
    }
    Ok(())
}

pub fn cmd_history(
    ctx: &RunContext,
    name_or_uuid: String,
    month: Option<String>,
    day: Option<String>,
    room: Option<String>,
) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;

    let ctrl_json = lox.get_control_json(&ctrl.uuid).ok();
    let output_names: Vec<String> = ctrl_json
        .as_ref()
        .and_then(|cj| cj.get("statistic"))
        .and_then(|s| s.get("outputs"))
        .and_then(|o| o.as_object())
        .map(|outputs| {
            let mut entries: Vec<_> = outputs.iter().collect();
            entries.sort_by_key(|(k, _)| k.as_str().to_string());
            entries
                .iter()
                .map(|(_, v)| {
                    v.get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("value")
                        .to_string()
                })
                .collect()
        })
        .unwrap_or_else(|| vec!["value".to_string()]);
    let num_outputs = output_names.len();

    let period = stats_period(day.as_deref(), month.as_deref());
    let bare = stats_file_path(&ctrl.uuid, &period);
    let data = match lox.get_bytes(&bare) {
        Ok(d) => d,
        Err(_) => {
            let listing = lox.get_text("/dev/fslist//stats")?;
            let files = find_stats_files(&listing, &ctrl.uuid, &period);
            if files.is_empty() {
                anyhow::bail!(
                    "No statistics files found for {} in period {}",
                    ctrl.uuid,
                    period
                );
            }
            let path = format!("/dev/fsget//stats/{}", files[0]);
            lox.get_bytes(&path)?
        }
    };

    if data.is_empty() {
        println!("No statistics data available for this period.");
    } else {
        let entry_size = 4 + 4 + num_outputs * 8;

        if ctx.csv {
            print!("timestamp");
            for name in &output_names {
                print!(",{}", name);
            }
            println!();
        } else if !ctx.json {
            print!("{:<20}", "TIMESTAMP");
            for name in &output_names {
                print!(" {:>15}", name);
            }
            println!();
            println!("{}", "─".repeat(20 + num_outputs * 16));
        }

        let offset = stats_data_offset(&data, entry_size).unwrap_or(0);
        let entries = parse_stats_entries(&data[offset..], num_outputs);

        let mut json_arr = Vec::new();
        for e in &entries {
            if ctx.csv {
                print!("{}", e.timestamp);
                for v in &e.values {
                    print!(",{:.4}", v);
                }
                println!();
            } else if ctx.json {
                let mut entry = serde_json::json!({"timestamp": e.timestamp});
                for (i, name) in output_names.iter().enumerate() {
                    entry[name] = serde_json::json!(e.values[i]);
                }
                json_arr.push(entry);
            } else {
                print!("{:<20}", e.timestamp);
                for v in &e.values {
                    print!(" {:>15.4}", v);
                }
                println!();
            }
        }
        if ctx.json {
            println!("{}", serde_json::to_string_pretty(&json_arr)?);
        }
    }
    Ok(())
}

pub fn cmd_autopilot(ctx: &RunContext, action: AutopilotCmd) -> Result<()> {
    let mut lox = LoxClient::new(Config::load()?)?;
    match action {
        AutopilotCmd::Ls => {
            let rules = lox.list_autopilot_rules()?;
            if rules.is_empty() {
                println!("No autopilot rules found.");
            } else if ctx.json {
                let arr: Vec<serde_json::Value> = rules
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "name": r.name,
                            "uuid": r.uuid,
                            "state_changed": r.state_changed,
                            "state_history": r.state_history,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else {
                let name_w = rules.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
                println!("{:<width$}  UUID", "NAME", width = name_w);
                for r in &rules {
                    println!("{:<width$}  {}", r.name, r.uuid, width = name_w);
                }
            }
        }
        AutopilotCmd::State { name_or_uuid } => {
            let rule = lox.find_autopilot_rule(&name_or_uuid)?;
            let resp = lox.get_json(&format!("/jdev/sps/io/{}/state", rule.state_changed));
            if ctx.json {
                match resp {
                    Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
                    Err(e) => bail!("{}", e),
                }
            } else {
                let last_run = match resp {
                    Ok(v) => {
                        let raw = v
                            .get("LL")
                            .and_then(|ll| ll.get("value"))
                            .and_then(|val| val.as_str())
                            .unwrap_or("");
                        if raw.is_empty() || raw == "0" {
                            "never".to_string()
                        } else if let Ok(secs) = raw.parse::<i64>() {
                            lox_timestamp_to_string(secs)
                        } else {
                            raw.to_string()
                        }
                    }
                    Err(_) => "unavailable".to_string(),
                };
                println!("Rule:     {}", rule.name);
                println!("Last run: {}", last_run);
            }
        }
    }
    Ok(())
}
