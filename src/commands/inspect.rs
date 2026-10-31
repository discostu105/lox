use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Cursor;
use std::thread;
use std::time::{Duration, Instant};

use crate::commands::RunContext;
use crate::config::Config;
use crate::util::{now_hms, xml_attr};
use crate::{AutopilotCmd, eval_op, matches_filters, print_stream_event};
use loxone_client::client::Control;
use loxone_client::stats::{
    StatsEntry, find_stats_files, parse_stats_entries, parse_weather_entry, stats_data_offset,
    stats_file_path, stats_period,
};
use loxone_client::stream;
use loxone_client::time::lox_epoch;

pub fn cmd_ls(
    ctx: &RunContext,
    r#type: Option<String>,
    room: Option<String>,
    values: bool,
    cat: Option<String>,
    favorites: bool,
) -> Result<()> {
    let mut lox = Config::load()?.client()?;
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
    let mut lox = Config::load()?.client()?;
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
        while let Some(n) = xml_attr(&xml, &format!("n{}", i)) {
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
    let mut lox = Config::load()?.client()?;
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
    let mut lox = Config::load()?.client()?;
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

/// `lox stream --reconnect`: wait between attempts, doubling up to the max.
const RECONNECT_MIN_SECS: u64 = 1;
const RECONNECT_MAX_SECS: u64 = 30;
/// First wait after the Miniserver announced it is going offline: it may
/// still answer for a moment before it actually shuts down.
const RECONNECT_AFTER_OUT_OF_SERVICE_SECS: u64 = 5;
/// Give up when the Miniserver stays unreachable this long.
const RECONNECT_GIVE_UP_SECS: u64 = 30 * 60;
/// Connecting and authenticating one stream session.
const STREAM_CONNECT_TIMEOUT_SECS: u64 = 20;

pub fn cmd_stream(
    ctx: &RunContext,
    r#type: Option<String>,
    room: Option<String>,
    control: Option<String>,
    initial: bool,
    reconnect: bool,
) -> Result<()> {
    let cfg = Config::load()?;
    let mut lox = cfg.client()?;
    let mut state_map = stream::build_state_uuid_map(lox.get_structure()?);
    if !ctx.quiet {
        eprintln!("Streaming state changes (Ctrl+C to stop)...");
    }
    if ctx.csv && !ctx.no_header {
        println!("timestamp,control,state,value,room,type,uuid");
    }

    let type_filter = r#type;
    let room_filter = room;
    let control_filter = control;
    let json = ctx.json;
    let csv = ctx.csv;
    let mut snapshot = SnapshotFilter::new(initial);
    let mut ever_live = false;
    // While the stream is down: since when, and the wait before the next attempt
    let mut offline_since: Option<Instant> = None;
    let mut backoff = Backoff::new(RECONNECT_MIN_SECS);

    let rt = tokio::runtime::Runtime::new()?;
    loop {
        let mut out_of_service = false;
        let result = rt.block_on(stream::stream_events_while(
            &cfg.connection(),
            Duration::from_secs(STREAM_CONNECT_TIMEOUT_SECS),
            |events| {
                ever_live = true;
                if let Some(since) = offline_since.take() {
                    print_reconnected(ctx, since.elapsed());
                }
                let dump = snapshot.is_dump(&events);
                for event in &events {
                    let (uuid, value) = match event {
                        stream::StateEvent::ValueState { uuid, value } => (uuid, value.to_string()),
                        stream::StateEvent::TextState { uuid, text, .. } => (uuid, text.clone()),
                        stream::StateEvent::OutOfService => {
                            out_of_service = true;
                            return Ok(false);
                        }
                        _ => continue,
                    };
                    let Some(info) = state_map.get(uuid) else {
                        continue;
                    };
                    if !matches_filters(
                        info,
                        type_filter.as_deref(),
                        room_filter.as_deref(),
                        control_filter.as_deref(),
                    ) || !snapshot.keep(uuid, &value, dump)
                    {
                        continue;
                    }
                    print_stream_event(
                        json,
                        csv,
                        &info.control_name,
                        &info.state_name,
                        &value,
                        info.room.as_deref(),
                        &info.control_type,
                        uuid,
                    );
                }
                Ok::<_, anyhow::Error>(true)
            },
        ));

        // Without --reconnect, and when the very first connection fails
        // (wrong host or credentials), end as before.
        if !reconnect || (!ever_live && result.is_err()) {
            if out_of_service {
                if !ctx.quiet {
                    eprintln!(
                        "Miniserver going offline (firmware update?). Exiting (--reconnect keeps streaming)."
                    );
                }
                return Ok(());
            }
            return Ok(result?);
        }

        let (reason, mut message) = match &result {
            _ if out_of_service => (
                "out_of_service",
                "Miniserver going offline (reboot or firmware update)".to_string(),
            ),
            Ok(()) => ("closed", "connection closed by the Miniserver".to_string()),
            Err(e) => ("error", format!("{:#}", e)),
        };
        let since = match offline_since {
            Some(since) => {
                if loxone_client::client::verbose() >= 1 {
                    eprintln!("  still offline: {}", message);
                }
                since
            }
            None => {
                print_disconnected(ctx, reason, &message);
                backoff = Backoff::new(if out_of_service {
                    RECONNECT_AFTER_OUT_OF_SERVICE_SECS
                } else {
                    RECONNECT_MIN_SECS
                });
                *offline_since.insert(Instant::now())
            }
        };

        // Wait until the Miniserver answers again. The structure is re-read on
        // every attempt: after a reboot it may run a new program (controls
        // added, renamed or removed), and the download doubles as the probe.
        loop {
            if since.elapsed() >= Duration::from_secs(RECONNECT_GIVE_UP_SECS) {
                bail!(
                    "Miniserver unreachable for {} min, giving up (last error: {})",
                    RECONNECT_GIVE_UP_SECS / 60,
                    message
                );
            }
            thread::sleep(backoff.next_delay());
            match fetch_fresh_structure(&cfg) {
                Ok(structure) => {
                    state_map = stream::build_state_uuid_map(&structure);
                    break;
                }
                Err(e) => {
                    message = format!("{:#}", e);
                    if loxone_client::client::verbose() >= 1 {
                        eprintln!("  still offline: {}", message);
                    }
                }
            }
        }
        snapshot.new_session();
    }
}

/// Re-read the structure from the Miniserver, bypassing the cache.
fn fetch_fresh_structure(cfg: &Config) -> Result<Value> {
    let _ = std::fs::remove_file(cfg.structure_cache_path());
    Ok(cfg.client()?.get_structure()?.clone())
}

/// Gap marker: a status line on stderr and, with `-o json`, an event line on
/// stdout between the state lines (CSV has no room for it).
fn print_disconnected(ctx: &RunContext, reason: &str, message: &str) {
    if !ctx.quiet {
        eprintln!(
            "[{}]  ✗ disconnected: {} — reconnecting (Ctrl+C to stop)...",
            now_hms(),
            message
        );
    }
    if ctx.json {
        println!(
            "{}",
            serde_json::json!({
                "timestamp": chrono::Local::now().to_rfc3339(),
                "event": "disconnected",
                "reason": reason,
                "message": message,
            })
        );
    }
}

fn print_reconnected(ctx: &RunContext, offline: Duration) {
    if !ctx.quiet {
        eprintln!(
            "[{}]  ✓ reconnected after {} offline",
            now_hms(),
            fmt_offline(offline)
        );
    }
    if ctx.json {
        println!(
            "{}",
            serde_json::json!({
                "timestamp": chrono::Local::now().to_rfc3339(),
                "event": "reconnected",
                "offline_secs": offline.as_secs(),
            })
        );
    }
}

fn fmt_offline(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{}s", s),
        60..3600 => format!("{}m {}s", s / 60, s % 60),
        _ => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

/// Wait schedule between reconnect attempts: doubling, capped.
struct Backoff {
    next_secs: u64,
}

impl Backoff {
    fn new(first_secs: u64) -> Self {
        Self {
            next_secs: first_secs,
        }
    }

    fn next_delay(&mut self) -> Duration {
        let secs = self.next_secs;
        self.next_secs = (secs * 2).min(RECONNECT_MAX_SECS);
        Duration::from_secs(secs)
    }
}

/// Splits a stream session's initial state dump from live changes.
///
/// After subscribing, the Miniserver sends every state once (a value-state
/// table, then a text-state table, each as one batch) and only changes after
/// that. With `--initial` every dump is printed, also after a reconnect: a
/// fresh snapshot of the rebooted Miniserver. Without it the first dump is the
/// silent baseline, and a later one prints only the states that changed (or
/// appeared) while the stream was down.
struct SnapshotFilter {
    initial: bool,
    reconnected: bool,
    value_dump_seen: bool,
    text_dump_seen: bool,
    /// Last value seen per state UUID, across sessions
    last: HashMap<String, String>,
}

impl SnapshotFilter {
    fn new(initial: bool) -> Self {
        Self {
            initial,
            reconnected: false,
            value_dump_seen: false,
            text_dump_seen: false,
            last: HashMap::new(),
        }
    }

    /// A new session after a reconnect starts with a new dump.
    fn new_session(&mut self) {
        self.reconnected = true;
        self.value_dump_seen = false;
        self.text_dump_seen = false;
    }

    /// Whether this batch is the session's dump of its table type.
    fn is_dump(&mut self, batch: &[stream::StateEvent]) -> bool {
        let seen = match batch.first() {
            Some(stream::StateEvent::ValueState { .. }) => &mut self.value_dump_seen,
            Some(stream::StateEvent::TextState { .. }) => &mut self.text_dump_seen,
            _ => return false,
        };
        !std::mem::replace(seen, true)
    }

    /// Whether to print this state value; `dump` as from [`Self::is_dump`].
    fn keep(&mut self, uuid: &str, value: &str, dump: bool) -> bool {
        let previous = self.last.insert(uuid.to_string(), value.to_string());
        if !dump || self.initial {
            return true;
        }
        self.reconnected && previous.as_deref() != Some(value)
    }
}

pub fn cmd_if(
    ctx: &RunContext,
    name_or_uuid: String,
    op: String,
    value: String,
    room: Option<String>,
) -> Result<()> {
    let mut lox = Config::load()?.client()?;
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
    let mut lox = Config::load()?.client()?;
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
    let mut lox = Config::load()?.client()?;
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
    let mut lox = Config::load()?.client()?;
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
    let mut lox = Config::load()?.client()?;
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

/// Does a control belong to a sensor kind (`temperature`, `door-window`,
/// `motion`, `smoke`; anything else means all sensor types)?
/// Shared by `lox sensors` and the MCP `list_sensors` tool.
pub fn is_sensor_type(kind: &str, c: &Control) -> bool {
    let typ = c.typ.as_str();
    match kind {
        // InfoOnlyAnalog also carries CO2 (ppm), humidity (%), …: go by the unit
        "temperature" | "temp" => {
            typ == "InfoOnlyAnalog" && c.format.as_deref().is_none_or(|f| f.contains('°'))
        }
        "door-window" | "doorwindow" => typ == "InfoOnlyDigital",
        "motion" => matches!(typ, "PresenceDetector" | "MotionSensor"),
        "smoke" => typ == "SmokeAlarm",
        _ => matches!(
            typ,
            "InfoOnlyAnalog"
                | "InfoOnlyDigital"
                | "PresenceDetector"
                | "MotionSensor"
                | "SmokeAlarm"
                | "Meter"
        ),
    }
}

/// Energy meters and managers (`lox energy`, MCP `list_sensors kind=energy`).
pub fn is_energy_type(typ: &str) -> bool {
    matches!(
        typ,
        "Meter" | "EnergyManager" | "EnergyMonitor" | "EnergyFlowMonitor"
    ) || typ.contains("Energy")
}

pub fn cmd_sensors(ctx: &RunContext, r#type: String, room: Option<String>) -> Result<()> {
    let mut lox = Config::load()?.client()?;
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
        .filter(|c| is_sensor_type(&type_lower, c))
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
    let mut lox = Config::load()?.client()?;
    let controls = lox.list_controls(None, room.as_deref())?;
    let energy: Vec<_> = controls.iter().filter(|c| is_energy_type(&c.typ)).collect();
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
    let lox = Config::load()?.client()?;
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
    let mut lox = Config::load()?.client()?;
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
            let legacy = ctrl.get("statistic").is_some_and(|s| !s.is_null());
            if legacy || !loxone_client::statv2::groups(ctrl).is_empty() {
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

/// Names of a control's statistic outputs. Real structure files use an array of
/// `{id, name, …}`; older ones an object keyed by index. Both are accepted.
pub(crate) fn statistic_output_names(outputs: &serde_json::Value) -> Vec<String> {
    let name = |v: &serde_json::Value| {
        v.get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("value")
            .to_string()
    };
    if let Some(arr) = outputs.as_array() {
        let mut entries: Vec<_> = arr.iter().collect();
        entries.sort_by_key(|v| v.get("id").and_then(|i| i.as_u64()).unwrap_or(0));
        entries.into_iter().map(name).collect()
    } else if let Some(obj) = outputs.as_object() {
        let mut entries: Vec<_> = obj.iter().collect();
        entries.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(u64::MAX));
        entries.into_iter().map(|(_, v)| name(v)).collect()
    } else {
        Vec::new()
    }
}

pub fn cmd_history(
    ctx: &RunContext,
    name_or_uuid: String,
    month: Option<String>,
    day: Option<String>,
    room: Option<String>,
) -> Result<()> {
    let mut lox = Config::load()?.client()?;
    let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
    let ctrl = lox.find_control(&uuid)?;

    let ctrl_json = lox.get_control_json(&ctrl.uuid).ok();
    if let Some(cj) = &ctrl_json
        && let Some(group) = loxone_client::statv2::groups(cj)
            .into_iter()
            .find(|g| !g.accumulated)
    {
        let (from, to) = stats_v2_range(day.as_deref(), month.as_deref())?;
        let data = lox.get_bytes(&loxone_client::statv2::raw_path(
            &ctrl.uuid, from, to, &group.id, None,
        ))?;
        let names: Vec<String> = group.points.iter().map(|(_, t)| t.clone()).collect();
        let entries: Vec<StatsEntry> = loxone_client::statv2::parse(&data, names.len())
            .into_iter()
            .map(|(t, values)| StatsEntry {
                timestamp: chrono::DateTime::from_timestamp(t, 0)
                    .map(|d| {
                        d.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_default(),
                values,
            })
            .collect();
        return print_history(ctx, &names, &entries);
    }
    let output_names: Vec<String> = ctrl_json
        .as_ref()
        .and_then(|cj| cj.get("statistic"))
        .and_then(|s| s.get("outputs"))
        .map(statistic_output_names)
        .filter(|names| !names.is_empty())
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
        return Ok(());
    }
    let entry_size = 4 + 4 + num_outputs * 8;
    let offset = stats_data_offset(&data, entry_size).unwrap_or(0);
    let entries = parse_stats_entries(&data[offset..], num_outputs);
    print_history(ctx, &output_names, &entries)
}

/// Local-time range [from, to] (unix seconds) for Statistics V2: a day, a month,
/// or (default) the current month up to now.
fn stats_v2_range(day: Option<&str>, month: Option<&str>) -> Result<(i64, i64)> {
    use chrono::{Datelike, Local, NaiveDate, TimeZone};
    let start = |d: NaiveDate| -> Result<i64> {
        Local
            .from_local_datetime(&d.and_hms_opt(0, 0, 0).context("bad date")?)
            .earliest()
            .map(|t| t.timestamp())
            .context("date does not exist in the local time zone")
    };
    let next_month = |d: NaiveDate| {
        if d.month() == 12 {
            NaiveDate::from_ymd_opt(d.year() + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1)
        }
    };
    if let Some(d) = day {
        let d = NaiveDate::parse_from_str(d, "%Y-%m-%d").context("--day must be YYYY-MM-DD")?;
        let next = d.succ_opt().context("bad date")?;
        return Ok((start(d)?, start(next)? - 1));
    }
    let first = match month {
        Some(m) => NaiveDate::parse_from_str(&format!("{}-01", m), "%Y-%m-%d")
            .context("--month must be YYYY-MM")?,
        None => Local::now().date_naive().with_day(1).context("bad date")?,
    };
    let end = start(next_month(first).context("bad date")?)? - 1;
    Ok((start(first)?, end.min(Local::now().timestamp())))
}

fn print_history(ctx: &RunContext, output_names: &[String], entries: &[StatsEntry]) -> Result<()> {
    let num_outputs = output_names.len();
    if entries.is_empty() {
        println!("No statistics data available for this period.");
        return Ok(());
    }
    if ctx.csv {
        print!("timestamp");
        for name in output_names {
            print!(",{}", name);
        }
        println!();
    } else if !ctx.json {
        print!("{:<20}", "TIMESTAMP");
        for name in output_names {
            print!(" {:>15}", name);
        }
        println!();
        println!("{}", "─".repeat(20 + num_outputs * 16));
    }

    let mut json_arr = Vec::new();
    for e in entries {
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
    Ok(())
}

pub fn cmd_autopilot(ctx: &RunContext, action: AutopilotCmd) -> Result<()> {
    match action {
        AutopilotCmd::Ls { src } => {
            let (snap, l) = super::config_insight::load(&src)?;
            let rules = l.autopilot_rules();
            if ctx.json {
                let v = serde_json::json!({"config": snap.label, "rules": rules});
                println!("{}", serde_json::to_string_pretty(&v)?);
                return Ok(());
            }
            print!("{}", rules_table(&rules, &snap.label, !ctx.no_header));
        }
        AutopilotCmd::Changed { explain, since } => {
            let cfg = Config::load()?;
            let ap = cfg.client()?.autopilot()?;
            let (secs, at) = parse_changed(&read_state(&cfg, &ap.state_changed)?)?;
            let why = explain
                .then(|| explain_changes(&cfg, since.as_deref()))
                .transpose()?;
            if ctx.json {
                let mut v = serde_json::json!({
                    "changed": secs,
                    "changed_at": at.map(|t| t.to_rfc3339()),
                    "uuid": ap.uuid,
                });
                if let Some(w) = &why {
                    v["program"] = serde_json::json!({
                        "newest": w.newest,
                        "compared_with": w.compared_with,
                        "rules": w.kinds.rules,
                        "access": w.kinds.access,
                        "other": w.kinds.other,
                    });
                    v["custom_changes"] = serde_json::to_value(&w.custom)?;
                    v["rules_changed"] = w.rules_changed().into();
                }
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                match at {
                    Some(t) => println!(
                        "Rules last changed: {} ({})",
                        t.format("%Y-%m-%d %H:%M:%S"),
                        secs
                    ),
                    None => println!("Rules last changed: never"),
                }
                if let Some(w) = &why {
                    print!("{}", w.text());
                }
            }
        }
        AutopilotCmd::State { name_or_uuid } => {
            let cfg = Config::load()?;
            let ap = cfg.client()?.autopilot()?;
            let runs = parse_autopilot_history(&read_state(&cfg, &ap.state_history)?);
            let hits = runs_of(&runs, &name_or_uuid)?;
            let last = hits.iter().max_by(|a, b| a.time.cmp(&b.time));
            if ctx.json {
                let v = serde_json::json!({
                    "name": last.map_or(name_or_uuid.as_str(), |r| r.name.as_str()),
                    "uuid": last.map(|r| r.uuid.as_str()),
                    "last_run": last.map(|r| r.time.as_str()),
                    "runs_in_history": hits.len(),
                });
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                match last {
                    Some(r) => {
                        println!("Rule:     {}", r.name);
                        println!("Last run: {}", r.time);
                    }
                    None => {
                        println!("Rule:     {}", name_or_uuid);
                        println!(
                            "Last run: not in the Miniserver's rule history (it keeps the recent runs)"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// The `lox autopilot ls` table, with a count line and the broken rules.
fn rules_table(rules: &[crate::logic::AutopilotRuleInfo], label: &str, header: bool) -> String {
    use std::fmt::Write;
    if rules.is_empty() {
        return format!("No automatic rules in {label}.\n");
    }
    let name_w = rules
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(4)
        .max(4);
    let mut out = String::new();
    if header {
        let _ = writeln!(
            out,
            "{:<name_w$}  {:<7}  {:<6}  UUID",
            "NAME", "ACTIVE", "JSON"
        );
    }
    for r in rules {
        let active = match (r.invalid, r.active) {
            (Some(true), _) => "invalid",
            (_, Some(true)) => "yes",
            (_, Some(false)) => "no",
            _ => "?",
        };
        let json = if r.apc_valid { "ok" } else { "broken" };
        let _ = writeln!(
            out,
            "{:<name_w$}  {:<7}  {:<6}  {}",
            r.name, active, json, r.uuid
        );
    }
    let _ = writeln!(out, "\n{} rules ({})", rules.len(), label);
    for r in rules.iter().filter(|r| !r.apc_valid) {
        let _ = writeln!(
            out,
            "✗  {}: rule JSON does not parse ({})",
            r.name,
            r.apc_error.as_deref().unwrap_or("")
        );
    }
    out
}

/// What `lox autopilot changed --explain` found: the newest program file
/// against a baseline, and the pending custom changes.
struct ChangeReport {
    /// The newest program file in /prog
    newest: String,
    /// The baseline: the program file before it, or `--since`; none when
    /// /prog holds a single file
    compared_with: Option<String>,
    kinds: crate::logic::ChangeKinds,
    custom: Vec<crate::custom_changes::CustomChange>,
}

impl ChangeReport {
    /// A rule pending in /sys/custom, or one whose type is unknown (the file
    /// did not read), counts as a rule change.
    fn rules_changed(&self) -> bool {
        !self.kinds.rules.is_empty()
            || self
                .custom
                .iter()
                .any(|c| c.block_type.as_deref().is_none_or(|t| t == "AutopilotRule"))
    }

    fn text(&self) -> String {
        let mut s = format!("\nNewest program file: {}\n", self.newest);
        match &self.compared_with {
            None => s.push_str("  the only one in /prog: nothing to compare with\n"),
            Some(base) if self.kinds.is_empty() => {
                s.push_str(&format!("  no changes against {base}\n"))
            }
            Some(base) => {
                s.push_str(&format!("  against {base}:\n"));
                for r in &self.kinds.rules {
                    s.push_str(&format!("    rule {}: {}\n", r.change, r.name));
                }
                for a in &self.kinds.access {
                    s.push_str(&format!("    {a}\n"));
                }
                for o in &self.kinds.other {
                    s.push_str(&format!("    {}\n", o.text));
                }
            }
        }
        if self.custom.is_empty() {
            s.push_str("Custom changes (/sys/custom): none\n");
        } else {
            s.push_str("Custom changes (/sys/custom), deleted by the next upload:\n");
            for c in &self.custom {
                s.push_str(&format!("    {}\n", c.label()));
            }
        }
        s.push_str(if self.rules_changed() {
            "\nRule changes found.\n"
        } else {
            "\nNo rule changes.\n"
        });
        s
    }
}

/// Read-only: the two newest program files of /prog (or the newest and
/// `since`) and the files of /sys/custom.
fn explain_changes(cfg: &Config, since: Option<&str>) -> Result<ChangeReport> {
    use crate::{ftp, logic::Logic, snapshot};
    use lxir::archive;
    let backups = ftp::list_backups(&cfg.connection())?;
    let newest = backups
        .first()
        .context("no program files in /prog on the Miniserver")?;
    let new_xml = archive::extract_and_decompress(&ftp::download_backup(
        &cfg.connection(),
        &newest.filename,
    )?)?;
    let base = match since {
        Some(f) => Some(snapshot::file(f)?),
        None => match backups.get(1) {
            Some(b) => Some(snapshot::Snapshot {
                bytes: archive::extract_and_decompress(&ftp::download_backup(
                    &cfg.connection(),
                    &b.filename,
                )?)?,
                label: b.filename.clone(),
            }),
            None => None,
        },
    };
    let new = Logic::parse(&new_xml).with_context(|| format!("cannot read {}", newest.filename))?;
    let kinds = match &base {
        Some(b) => {
            let old = Logic::parse(&b.bytes).with_context(|| format!("cannot read {}", b.label))?;
            crate::logic::change_kinds(&old, &new)
        }
        None => Default::default(),
    };
    let files = ftp::download_custom_changes(&cfg.connection())?;
    Ok(ChangeReport {
        newest: newest.filename.clone(),
        compared_with: base.map(|b| b.label),
        kinds,
        custom: crate::custom_changes::describe(&files, Some(&new_xml)),
    })
}

/// The autopilot `changed` state: unix seconds, and the local time unless 0
/// (never changed).
fn parse_changed(raw: &str) -> Result<(i64, Option<chrono::DateTime<chrono::Local>>)> {
    let secs = raw
        .trim()
        .parse::<f64>()
        .with_context(|| format!("autopilot `changed` is not a number: {raw:?}"))?
        as i64;
    let at = (secs > 0)
        .then(|| chrono::DateTime::from_timestamp(secs, 0))
        .flatten()
        .map(|t| t.with_timezone(&chrono::Local));
    Ok((secs, at))
}

/// The runs of one rule, by UUID or a part of its name; a name part that
/// matches several rules is an error.
fn runs_of<'a>(runs: &'a [AutopilotRun], name_or_uuid: &str) -> Result<Vec<&'a AutopilotRun>> {
    let q = name_or_uuid.to_lowercase();
    let hits: Vec<&AutopilotRun> = runs
        .iter()
        .filter(|r| r.uuid == name_or_uuid || r.name.to_lowercase().contains(&q))
        .collect();
    let mut names: Vec<&str> = hits.iter().map(|r| r.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    if names.len() > 1 {
        bail!(
            "Ambiguous: '{}' matches {} rules in the history:\n  {}",
            name_or_uuid,
            names.len(),
            names.join("\n  ")
        );
    }
    Ok(hits)
}

/// One state's current value from the WebSocket state dump (max 10 s): text
/// states as text, value states as a number.
fn read_state(cfg: &Config, uuid: &str) -> Result<String> {
    let target = uuid.to_string();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let found = tokio::time::timeout(
            Duration::from_secs(10),
            stream::stream_events_until(&cfg.connection(), |ev| match ev {
                stream::StateEvent::ValueState { uuid, value } if *uuid == target => {
                    Some(value.to_string())
                }
                stream::StateEvent::TextState { uuid, text, .. } if *uuid == target => {
                    Some(text.clone())
                }
                _ => None,
            }),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Timeout waiting for state {} (10 s)", target))??;
        Ok(found)
    })
}

/// One entry of the autopilot `history` state.
#[derive(Debug, PartialEq)]
struct AutopilotRun {
    /// Local time, `YYYY-MM-DD HH:MM:SS`
    time: String,
    uuid: String,
    name: String,
}

/// The autopilot `history` state: `|`-separated runs, each
/// `YYYY-MM-DD HH:MM:SS <rule uuid> <rule name>`.
fn parse_autopilot_history(text: &str) -> Vec<AutopilotRun> {
    text.split('|')
        .filter_map(|e| {
            let e = e.trim();
            let time = e.get(..19)?;
            let (uuid, name) = e.get(19..)?.trim_start().split_once(' ')?;
            Some(AutopilotRun {
                time: time.to_string(),
                uuid: uuid.to_string(),
                name: name.trim().to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        AutopilotRun, Backoff, ChangeReport, SnapshotFilter, fmt_offline, parse_autopilot_history,
        parse_changed, rules_table, runs_of, statistic_output_names,
    };
    use loxone_client::stream::StateEvent;
    use serde_json::json;
    use std::time::Duration;

    fn custom(block_type: Option<&str>) -> crate::custom_changes::CustomChange {
        crate::custom_changes::CustomChange {
            uuid: "u1".into(),
            block_type: block_type.map(str::to_string),
            title: Some("T".into()),
            page: None,
            new: false,
            removed: false,
        }
    }

    #[test]
    fn change_report_rules_changed() {
        let mut r = ChangeReport {
            newest: "sps_2_20261029195728.zip".into(),
            compared_with: Some("sps_1_20261029194421.zip".into()),
            kinds: crate::logic::ChangeKinds {
                access: vec!["~ user   loxcli · visualization password set".into()],
                ..Default::default()
            },
            custom: vec![custom(Some("DayTimer"))],
        };
        // a password change and a timer edited in the app: no rule change
        assert!(!r.rules_changed());
        let t = r.text();
        assert!(t.contains("against sps_1_20261029194421.zip:"), "{t}");
        assert!(t.contains("~ user   loxcli"), "{t}");
        assert!(t.contains("\"T\" (DayTimer)"), "{t}");
        assert!(t.ends_with("No rule changes.\n"), "{t}");
        // a rule pending in /sys/custom, or a file that did not read
        r.custom = vec![custom(Some("AutopilotRule"))];
        assert!(r.rules_changed());
        r.custom = vec![custom(None)];
        assert!(r.rules_changed());
        // a rule changed in the program file
        r.custom.clear();
        r.kinds.rules.push(crate::logic::RuleChange {
            uuid: "r1".into(),
            name: "Morning".into(),
            change: "changed",
        });
        assert!(r.rules_changed());
        assert!(r.text().contains("rule changed: Morning"));
        r.compared_with = None;
        assert!(r.text().contains("nothing to compare with"));
    }

    fn values(pairs: &[(&str, f64)]) -> Vec<StateEvent> {
        pairs
            .iter()
            .map(|(uuid, value)| StateEvent::ValueState {
                uuid: uuid.to_string(),
                value: *value,
            })
            .collect()
    }

    fn texts(pairs: &[(&str, &str)]) -> Vec<StateEvent> {
        pairs
            .iter()
            .map(|(uuid, text)| StateEvent::TextState {
                uuid: uuid.to_string(),
                icon_uuid: String::new(),
                text: text.to_string(),
            })
            .collect()
    }

    /// The states of `batch` the filter lets through.
    fn printed(f: &mut SnapshotFilter, batch: &[StateEvent]) -> Vec<String> {
        let dump = f.is_dump(batch);
        batch
            .iter()
            .filter_map(|e| match e {
                StateEvent::ValueState { uuid, value } => Some((uuid, value.to_string())),
                StateEvent::TextState { uuid, text, .. } => Some((uuid, text.clone())),
                _ => None,
            })
            .filter(|(uuid, value)| f.keep(uuid, value, dump))
            .map(|(uuid, value)| format!("{}={}", uuid, value))
            .collect()
    }

    #[test]
    fn snapshot_without_initial_hides_both_dumps() {
        let mut f = SnapshotFilter::new(false);
        assert!(printed(&mut f, &values(&[("a", 1.0), ("b", 0.0)])).is_empty());
        // the text-state dump is a second batch, still part of the snapshot
        assert!(printed(&mut f, &texts(&[("t", "idle")])).is_empty());
        assert_eq!(printed(&mut f, &values(&[("a", 2.0)])), ["a=2"]);
        assert_eq!(printed(&mut f, &texts(&[("t", "busy")])), ["t=busy"]);
    }

    #[test]
    fn snapshot_with_initial_prints_dumps() {
        let mut f = SnapshotFilter::new(true);
        assert_eq!(printed(&mut f, &values(&[("a", 1.0)])), ["a=1"]);
        assert_eq!(printed(&mut f, &texts(&[("t", "idle")])), ["t=idle"]);
        assert_eq!(printed(&mut f, &values(&[("a", 2.0)])), ["a=2"]);
        // after a reconnect: a full fresh snapshot
        f.new_session();
        assert_eq!(
            printed(&mut f, &values(&[("a", 2.0), ("b", 0.0)])),
            ["a=2", "b=0"]
        );
        assert_eq!(printed(&mut f, &texts(&[("t", "idle")])), ["t=idle"]);
    }

    #[test]
    fn snapshot_after_reconnect_prints_what_changed_offline() {
        let mut f = SnapshotFilter::new(false);
        printed(&mut f, &values(&[("a", 1.0), ("b", 0.0)]));
        printed(&mut f, &texts(&[("t", "idle")]));
        assert_eq!(printed(&mut f, &values(&[("b", 1.0)])), ["b=1"]);
        f.new_session();
        // a changed while offline, b did not, c is new (a new program)
        assert_eq!(
            printed(&mut f, &values(&[("a", 5.0), ("b", 1.0), ("c", 0.0)])),
            ["a=5", "c=0"]
        );
        assert_eq!(
            printed(&mut f, &texts(&[("t", "idle")])),
            Vec::<String>::new()
        );
        assert_eq!(printed(&mut f, &values(&[("a", 6.0)])), ["a=6"]);
    }

    #[test]
    fn snapshot_ignores_other_batches() {
        let mut f = SnapshotFilter::new(false);
        assert!(!f.is_dump(&[StateEvent::Keepalive]));
        assert!(f.is_dump(&values(&[("a", 1.0)])));
        assert!(!f.is_dump(&values(&[("a", 2.0)])));
        assert!(f.is_dump(&texts(&[("t", "x")])));
        assert!(!f.is_dump(&texts(&[("t", "y")])));
    }

    #[test]
    fn reconnect_backoff_doubles_up_to_cap() {
        let mut b = Backoff::new(1);
        let secs: Vec<u64> = (0..7).map(|_| b.next_delay().as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 16, 30, 30]);
        let mut b = Backoff::new(5);
        let secs: Vec<u64> = (0..4).map(|_| b.next_delay().as_secs()).collect();
        assert_eq!(secs, [5, 10, 20, 30]);
    }

    #[test]
    fn offline_duration_format() {
        assert_eq!(fmt_offline(Duration::from_secs(42)), "42s");
        assert_eq!(fmt_offline(Duration::from_secs(134)), "2m 14s");
        assert_eq!(fmt_offline(Duration::from_secs(3720)), "1h 2m");
    }

    #[test]
    fn autopilot_history_runs() {
        let h = "2026-07-26 17:01:00 0a000000-0000-0001-ffff000000000001 Pump off|\
                 2026-07-27 08:52:00 0a000000-0000-0002-ffff000000000002 Open the cover now|garbage";
        assert_eq!(
            parse_autopilot_history(h),
            [
                AutopilotRun {
                    time: "2026-07-26 17:01:00".into(),
                    uuid: "0a000000-0000-0001-ffff000000000001".into(),
                    name: "Pump off".into(),
                },
                AutopilotRun {
                    time: "2026-07-27 08:52:00".into(),
                    uuid: "0a000000-0000-0002-ffff000000000002".into(),
                    name: "Open the cover now".into(),
                },
            ]
        );
        assert!(parse_autopilot_history("").is_empty());
    }

    #[test]
    fn autopilot_runs_of_one_rule() {
        let runs = parse_autopilot_history(
            "2026-07-26 17:01:00 0a000000-0000-0001-ffff000000000001 Pump off|\
             2026-07-27 09:00:00 0a000000-0000-0003-ffff000000000003 Pump on|\
             2026-07-27 17:01:00 0a000000-0000-0001-ffff000000000001 Pump off",
        );
        let off = runs_of(&runs, "pump OFF").unwrap();
        assert_eq!(off.len(), 2);
        assert!(off.iter().all(|r| r.name == "Pump off"));
        let by_uuid = runs_of(&runs, "0a000000-0000-0003-ffff000000000003").unwrap();
        assert_eq!(by_uuid[0].name, "Pump on");
        let e = runs_of(&runs, "pump").unwrap_err().to_string();
        assert!(e.contains("matches 2 rules"), "{}", e);
        assert!(runs_of(&runs, "heating").unwrap().is_empty());
    }

    #[test]
    fn autopilot_changed_seconds() {
        let (secs, at) = parse_changed("1793275220").unwrap();
        assert_eq!(secs, 1793275220);
        assert_eq!(at.unwrap().timestamp(), 1793275220);
        assert_eq!(parse_changed("0").unwrap(), (0, None));
        assert!(parse_changed("soon").is_err());
    }

    #[test]
    fn autopilot_rules_table_rows() {
        use crate::logic::AutopilotRuleInfo;
        let rule = |name: &str, active, invalid, err: Option<&str>| AutopilotRuleInfo {
            uuid: format!("uuid-{name}"),
            name: name.into(),
            active,
            invalid,
            apc_valid: err.is_none(),
            apc_error: err.map(str::to_string),
        };
        let rules = [
            rule("Morning", Some(true), Some(false), None),
            rule("Stale", Some(true), Some(true), None),
            rule("Broken", None, None, Some("expected value")),
        ];
        let t = rules_table(&rules, "test.Loxone", true);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "NAME     ACTIVE   JSON    UUID");
        assert_eq!(lines[1], "Morning  yes      ok      uuid-Morning");
        assert_eq!(lines[2], "Stale    invalid  ok      uuid-Stale");
        assert_eq!(lines[3], "Broken   ?        broken  uuid-Broken");
        assert_eq!(lines[5], "3 rules (test.Loxone)");
        assert_eq!(
            lines[6],
            "✗  Broken: rule JSON does not parse (expected value)"
        );
        assert!(!rules_table(&rules, "x", false).starts_with("NAME"));
        assert_eq!(rules_table(&[], "x", true), "No automatic rules in x.\n");
    }

    #[test]
    fn statistic_outputs_array_and_object() {
        let arr = json!([{"id": 1, "name": "Humidity"}, {"id": 0, "name": "Temperature"}]);
        assert_eq!(statistic_output_names(&arr), ["Temperature", "Humidity"]);
        let obj = json!({"10": {"name": "B"}, "2": {"name": "A"}});
        assert_eq!(statistic_output_names(&obj), ["A", "B"]);
        assert!(statistic_output_names(&json!(null)).is_empty());
    }
}
