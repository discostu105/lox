mod client;
mod config;
mod daemon;
mod scene;
mod token;
mod ws;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use client::{is_uuid, LoxClient};
use config::Config;
use daemon::Automations;
use reqwest::blocking::Client;
use scene::Scene;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

fn xml_attr<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    let key = format!("{}=\"", attr);
    let start = xml.find(&key)? + key.len();
    let end = xml[start..].find('"')? + start;
    Some(&xml[start..end])
}

fn print_resp(resp: &Value, json: bool, name: &str, cmd: &str) {
    if json {
        println!("{}", serde_json::to_string_pretty(resp).unwrap());
    } else {
        let val = resp.pointer("/LL/value").and_then(|v| v.as_str()).unwrap_or("?");
        let code = resp.pointer("/LL/Code").and_then(|v| v.as_str()).unwrap_or("?");
        println!("{}  {} → {} = {}", if code == "200" { "✓" } else { "✗" }, name, cmd, val);
    }
}

fn bar(v: f64, max: f64) -> String {
    let n = ((v / max * 20.0) as usize).min(20);
    format!("[{}{}] {:.0}%", "█".repeat(n), "░".repeat(20 - n), v / max * 100.0)
}

fn kb_fmt(kb: f64) -> String {
    if kb > 1024.0 { format!("{:.0} MB", kb / 1024.0) }
    else { format!("{:.0} kB", kb) }
}

fn now_hms() -> String {
    use chrono::Timelike;
    let now = chrono::Local::now();
    format!("{:02}:{:02}:{:02}", now.hour(), now.minute(), now.second())
}

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "lox", about = "Loxone Miniserver CLI", version)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Configure connection
    Config { #[command(subcommand)] action: ConfigCmd },
    /// Manage control name aliases
    Alias { #[command(subcommand)] action: AliasCmd },
    /// Miniserver health
    Status { #[arg(long)] energy: bool },
    /// List controls
    Ls { #[arg(long)] r#type: Option<String>, #[arg(long)] room: Option<String>, #[arg(long)] values: bool },
    /// List rooms
    Rooms,
    /// Get full state of a control
    Get { name_or_uuid: String, #[arg(long)] room: Option<String> },
    /// Send raw command
    Send { name_or_uuid: String, command: String, #[arg(long)] room: Option<String> },
    /// Turn on
    On { name_or_uuid: String, #[arg(long)] room: Option<String> },
    /// Turn off
    Off { name_or_uuid: String, #[arg(long)] room: Option<String> },
    /// Momentary pulse
    Pulse { name_or_uuid: String, #[arg(long)] room: Option<String> },
    /// Control blind: up | down | stop | shade | full-up | full-down | pos <0-100>
    Blind { name_or_uuid: String, action: String, #[arg(allow_hyphen_values = true)] pos: Option<f64>, #[arg(long)] room: Option<String> },
    /// Control light moods: plus | minus | off | <mood-id>
    Mood {
        name_or_uuid: String,
        /// Mood action: plus, minus, off, or numeric mood ID
        action: String,
        #[arg(long)] room: Option<String>,
    },
    /// Watch state changes
    Watch { name_or_uuid: String, #[arg(long, default_value = "2")] interval: u64 },
    /// Check state (exit 0=match, 1=no match)
    If { name_or_uuid: String, op: String, value: String },
    /// Run a scene
    Run { scene: String },
    /// Manage scenes
    Scene { #[command(subcommand)] action: SceneCmd },
    /// Start the automation daemon
    Daemon {
        /// Print all state changes
        #[arg(long)]
        verbose: bool,
        /// Use HTTP polling instead of WebSocket (needed without Monitor rights)
        #[arg(long)]
        poll: bool,
        /// Poll interval in seconds (default 3, only with --poll)
        #[arg(long, default_value = "3")]
        interval: u64,
    },
    /// Manage automation rules
    Automation { #[command(subcommand)] action: AutomationCmd },
    /// Fetch Miniserver log
    Log { #[arg(long, default_value = "50")] lines: usize },
    /// Set analog/virtual input value
    Set {
        /// Control name or UUID
        name_or_uuid: String,
        /// Value to send (numeric or text)
        value: String,
    },
    /// Manage systemd daemon service
    Service { #[command(subcommand)] action: ServiceCmd },
    /// Manage local cache
    Cache { #[command(subcommand)] action: CacheCmd },
    /// Manage auth token (more secure than Basic Auth)
    Token { #[command(subcommand)] action: TokenCmd },
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// Install systemd service
    Install,
    /// Show service status
    Status,
    /// Show service logs
    Logs,
    /// Uninstall service
    Uninstall,
}

#[derive(Subcommand)]
enum TokenCmd {
    /// Fetch and save a new token (valid 20 days)
    Fetch,
    /// Show current token status
    Info,
    /// Delete saved token (revert to Basic Auth)
    Clear,
}

#[derive(Subcommand)]
enum CacheCmd {
    /// Show cache info
    Info,
    /// Clear structure cache (forces fresh fetch)
    Clear,
    /// Refresh structure cache now
    Refresh,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Set one or more config fields (omitted fields are preserved)
    Set {
        #[arg(long)] host: Option<String>,
        #[arg(long)] user: Option<String>,
        /// Password (or set LOX_PASS env var to avoid it appearing in the process table)
        #[arg(long, env = "LOX_PASS")] pass: Option<String>,
        #[arg(long)] serial: Option<String>,
        /// IANA timezone (e.g. Europe/Vienna) for automation time windows
        #[arg(long)] timezone: Option<String>,
    },
    Show,
}

#[derive(Subcommand)]
enum AliasCmd {
    /// Add or update an alias
    Add { name: String, uuid: String },
    /// Remove an alias
    Remove { name: String },
    /// List all aliases
    List,
}

#[derive(Subcommand)]
enum SceneCmd {
    List,
    Show { name: String },
    New { name: String },
}

#[derive(Subcommand)]
enum AutomationCmd {
    /// List loaded rules
    List,
    /// Create/show the automations file
    Edit,
    /// Test: print what events would trigger (dry run, no WS)
    Check,
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.cmd {
        Cmd::Config { action } => match action {
            ConfigCmd::Set { host, user, pass, serial, timezone } => {
                let mut cfg = Config::load().unwrap_or_default();
                if let Some(h) = host { cfg.host = h; }
                if let Some(u) = user { cfg.user = u; }
                if let Some(p) = pass { cfg.pass = p; }
                if let Some(s) = serial { cfg.serial = s; }
                if let Some(tz) = timezone { cfg.timezone = Some(tz); }
                cfg.save()?;
            }
            ConfigCmd::Show => {
                let cfg = Config::load()?;
                println!("host:   {}", cfg.host);
                println!("user:   {}", cfg.user);
                println!("pass:   {}", "*".repeat(cfg.pass.len()));
                if !cfg.serial.is_empty() { println!("serial: {}", cfg.serial); }
                if let Some(tz) = &cfg.timezone { println!("timezone: {}", tz); }
                if !cfg.aliases.is_empty() {
                    println!("aliases:");
                    let mut aliases: Vec<_> = cfg.aliases.iter().collect();
                    aliases.sort_by_key(|(k, _)| k.as_str());
                    for (name, uuid) in aliases { println!("  {}: {}", name, uuid); }
                }
            }
        },

        Cmd::Alias { action } => {
            let mut cfg = Config::load()?;
            match action {
                AliasCmd::Add { name, uuid } => {
                    cfg.aliases.insert(name.clone(), uuid.clone());
                    cfg.save()?;
                    println!("✓  alias '{}' → {}", name, uuid);
                }
                AliasCmd::Remove { name } => {
                    if cfg.aliases.remove(&name).is_some() {
                        cfg.save()?;
                        println!("✓  removed alias '{}'", name);
                    } else {
                        println!("No alias named '{}'", name);
                    }
                }
                AliasCmd::List => {
                    if cfg.aliases.is_empty() {
                        println!("No aliases. Add with: lox alias add <name> <uuid>");
                    } else {
                        let mut aliases: Vec<_> = cfg.aliases.iter().collect();
                        aliases.sort_by_key(|(k, _)| k.as_str());
                        println!("{:<24} {}", "ALIAS", "UUID");
                        println!("{}", "─".repeat(60));
                        for (name, uuid) in aliases { println!("{:<24} {}", name, uuid); }
                    }
                }
            }
        },

        Cmd::Status { energy } => {
            let lox = LoxClient::new(Config::load()?);
            let version = lox.get_text("/dev/cfg/version")?;
            let heap    = lox.get_text("/dev/sys/heap")?;
            let sps     = lox.get_text("/dev/sps/state")?;
            let check   = lox.get_text("/dev/sys/check")?;
            let status  = lox.get_text("/data/status")?;

            let ver  = xml_attr(&version, "value").unwrap_or("?");
            let heap_val = xml_attr(&heap, "value").unwrap_or("?");
            let sps_num  = xml_attr(&sps, "value").unwrap_or("?");
            let conn = xml_attr(&check, "value").unwrap_or("?");

            let sps_label = match sps_num {
                "5" => "✓ Running", "3" => "Started", "7" => "⚠ Error",
                "1" => "Booting",   "8" => "Updating", n => n,
            };
            let heap_display = if let Some((used, total)) = heap_val.split_once('/') {
                let t_str = total.trim_end_matches("kB");
                let u: f64 = used.parse().unwrap_or(0.0);
                let t: f64 = t_str.parse().unwrap_or(1.0);
                format!("{} / {}  {}", kb_fmt(u), kb_fmt(t), bar(u, t))
            } else { heap_val.to_string() };

            let ms_name = xml_attr(&status, "Name").unwrap_or("Loxone");
            let ms_ip   = xml_attr(&status, "IP").unwrap_or("?");
            let online  = if xml_attr(&status, "Offline").unwrap_or("false") == "false" { "✓ Online" } else { "✗ Offline" };

            if cli.json {
                println!("{}", serde_json::json!({
                    "name": ms_name, "ip": ms_ip, "version": ver,
                    "plc": sps_label, "heap": heap_val, "connections": conn,
                }));
            } else {
                println!("┌─ Loxone Miniserver ─────────────────────────────────");
                println!("│  Name:        {} ({} {})", ms_name, ms_ip, online);
                println!("│  Firmware:    {}", ver);
                println!("│  PLC:         {}", sps_label);
                println!("│  Memory:      {}", heap_display);
                println!("│  Connections: {}", conn);
                println!("└─────────────────────────────────────────────────────");
            }
            if energy {
                let mut lox_mut = LoxClient::new(Config::load()?);
                let meters = lox_mut.list_controls(Some("meter"), None)
                    .unwrap_or_default()
                    .into_iter()
                    .chain(lox_mut.list_controls(Some("energymanager"), None).unwrap_or_default())
                    .collect::<Vec<_>>();
                if meters.is_empty() {
                    println!("No energy meters found in structure (type 'Meter' or 'EnergyManager').");
                } else {
                    println!("┌─ Energy Meters ─────────────────────────────────────");
                    for ctrl in &meters {
                        let val = lox_mut.get_all(&ctrl.uuid)
                            .ok()
                            .and_then(|xml| xml_attr(&xml, "value").map(|s| s.to_string()))
                            .unwrap_or_else(|| "-".to_string());
                        let v: f64 = val.parse().unwrap_or(0.0);
                        println!("│  {:<24} {:>8.3} kW  [{}]",
                            ctrl.name, v, ctrl.room.as_deref().unwrap_or("─"));
                    }
                    println!("└─────────────────────────────────────────────────────");
                }
            }
        },

        Cmd::Ls { r#type, room, values } => {
            let mut lox = LoxClient::new(Config::load()?);
            let controls = lox.list_controls(r#type.as_deref(), room.as_deref())?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&controls.iter().map(|c| serde_json::json!({
                    "name": c.name, "uuid": c.uuid, "type": c.typ, "room": c.room
                })).collect::<Vec<_>>())?);
            } else if values {
                println!("{:<40} {:<24} {:<22} {:<20} {}", "NAME", "ROOM", "TYPE", "VALUE", "UUID");
                println!("{}", "─".repeat(140));
                for c in &controls {
                    let val = lox.get_all(&c.uuid)
                        .ok()
                        .and_then(|xml| xml_attr(&xml, "value").map(|s| s.to_string()))
                        .unwrap_or_else(|| "-".to_string());
                    println!("{:<40} {:<24} {:<22} {:<20} {}",
                        c.name, c.room.as_deref().unwrap_or("─"), c.typ, val, c.uuid);
                }
                println!("\n{} controls", controls.len());
            } else {
                println!("{:<40} {:<24} {:<22} {}", "NAME", "ROOM", "TYPE", "UUID");
                println!("{}", "─".repeat(120));
                for c in &controls {
                    println!("{:<40} {:<24} {:<22} {}",
                        c.name, c.room.as_deref().unwrap_or("─"), c.typ, c.uuid);
                }
                println!("\n{} controls", controls.len());
            }
        },

        Cmd::Rooms => {
            let mut lox = LoxClient::new(Config::load()?);
            let structure = lox.get_structure()?;
            if let Some(rooms) = structure.get("rooms").and_then(|r| r.as_object()) {
                let mut names: Vec<&str> = rooms.values()
                    .filter_map(|r| r.get("name").and_then(|n| n.as_str())).collect();
                names.sort();
                for n in names { println!("{}", n); }
            }
        },

        Cmd::Get { name_or_uuid, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid_resolved = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid_resolved)?;
            let xml = lox.get_all(&ctrl.uuid)?;
            let val = xml_attr(&xml, "value").unwrap_or("?");
            let code = xml_attr(&xml, "Code").unwrap_or("?");

            if cli.json {
                let mut result = serde_json::json!({
                    "name": ctrl.name, "uuid": ctrl.uuid,
                    "type": ctrl.typ, "room": ctrl.room, "value": val,
                });
                for attr in &["StateUp","StateDown","StatePos","StateShade","StateAutoShade","StateSafety"] {
                    if let Some(v) = xml_attr(&xml, attr) {
                        result[attr] = Value::String(v.to_string());
                    }
                }
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Control:  {} ({})", ctrl.name, ctrl.uuid);
                println!("Type:     {}   Room: {}", ctrl.typ, ctrl.room.as_deref().unwrap_or("─"));
                println!("Value:    {}  [Code {}]", val, code);
                for attr in &["StateUp","StateDown","StatePos","StateShade","StateAutoShade"] {
                    if let Some(v) = xml_attr(&xml, attr) {
                        println!("  {:<12} {}", attr.trim_start_matches("State"), v);
                    }
                }
                let mut i = 1;
                loop {
                    let Some(n) = xml_attr(&xml, &format!("n{}", i)) else { break; };
                    let v = xml_attr(&xml, &format!("v{}", i)).unwrap_or("?");
                    if !n.is_empty() { println!("  {:<30} = {}", n, v); }
                    i += 1;
                }
            }
        },

        Cmd::Send { name_or_uuid, command, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let resp = lox.send_cmd(&uuid, &command)?;
            print_resp(&resp, cli.json, &name_or_uuid, &command);
        },
        Cmd::On { name_or_uuid, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let resp = lox.send_cmd(&uuid, "on")?;
            print_resp(&resp, cli.json, &name_or_uuid, "on");
        },
        Cmd::Off { name_or_uuid, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let resp = lox.send_cmd(&uuid, "off")?;
            print_resp(&resp, cli.json, &name_or_uuid, "off");
        },
        Cmd::Pulse { name_or_uuid, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let resp = lox.send_cmd(&uuid, "pulse")?;
            print_resp(&resp, cli.json, &name_or_uuid, "pulse");
        },

        Cmd::Blind { name_or_uuid, action, pos, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !matches!(ctrl.typ.as_str(), "Jalousie" | "CentralJalousie") {
                bail!("'{}' is type '{}', not a Jalousie", ctrl.name, ctrl.typ);
            }
            let cmd_owned: String;
            let cmd: &str = match action.to_lowercase().as_str() {
                "up"   | "open"  => "PulseUp",
                "down" | "close" => "PulseDown",
                "stop"           => "off",
                "shade" | "auto" => "AutomaticDown",
                "full-up"        => "FullUp",
                "full-down"      => "FullDown",
                "pos" | "position" => {
                    let pct = pos.ok_or_else(|| anyhow::anyhow!("pos requires a value 0-100"))?;
                    if !(0.0..=100.0).contains(&pct) { bail!("Position must be 0-100"); }
                    cmd_owned = format!("manualPosition/{:.4}", pct / 100.0);
                    &cmd_owned
                },
                other => {
                    // Try numeric mood-style: pos 50
                    if let Ok(pct) = other.parse::<f64>() {
                        if (0.0..=100.0).contains(&pct) {
                            cmd_owned = format!("manualPosition/{:.4}", pct / 100.0);
                            &cmd_owned
                        } else { bail!("Position must be 0-100"); }
                    } else {
                        bail!("Unknown action '{}'. Use: up down stop shade full-up full-down pos <0-100>", other)
                    }
                },
            };
            let resp = lox.send_cmd(&ctrl.uuid, cmd)?;
            print_resp(&resp, cli.json, &ctrl.name, cmd);
            if !cli.json {
                thread::sleep(Duration::from_millis(800));
                let xml = lox.get_all(&ctrl.uuid)?;
                if let Some(pos_str) = xml_attr(&xml, "StatePos") {
                    let p: f64 = pos_str.parse().unwrap_or(0.0);
                    println!("   Position: {:.0}%  {}", p * 100.0, bar(p, 1.0));
                }
            }
        },

        Cmd::Mood { name_or_uuid, action, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !matches!(ctrl.typ.as_str(), "LightControllerV2" | "LightController") {
                bail!("'{}' is type '{}', not a LightController", ctrl.name, ctrl.typ);
            }
            let cmd_owned: String;
            let cmd: &str = match action.to_lowercase().as_str() {
                "plus" | "next" | "+" => "plus",
                "minus" | "prev" | "-" => "minus",
                "off"  => "setMood/778",
                other => {
                    if let Ok(id) = other.parse::<u32>() {
                        cmd_owned = format!("setMood/{}", id);
                        &cmd_owned
                    } else {
                        bail!("Unknown mood action '{}'. Use: plus, minus, off, or a numeric mood ID", other)
                    }
                },
            };
            let resp = lox.send_cmd(&ctrl.uuid, cmd)?;
            if cli.json {
                print_resp(&resp, true, &ctrl.name, cmd);
            } else {
                println!("✓  {} → mood {}", ctrl.name, action);
                thread::sleep(Duration::from_millis(400));
                let xml = lox.get_all(&ctrl.uuid)?;
                let state = xml_attr(&xml, "value").unwrap_or_default();
                // State encodes active moods as a packed integer
                // 200002700 = off, other values = mood active
                let is_off = state.starts_with("200002700") || state == "0";
                println!("   State: {}  ({})", state, if is_off { "off" } else { "active" });
            }
        },

        Cmd::Watch { name_or_uuid, interval } => {
            let mut lox = LoxClient::new(Config::load()?);
            let ctrl = lox.find_control(&name_or_uuid)?;
            println!("Watching '{}' every {}s  (Ctrl+C to stop)", ctrl.name, interval);
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
        },

        Cmd::If { name_or_uuid, op, value } => {
            let mut lox = LoxClient::new(Config::load()?);
            let ctrl = lox.find_control(&name_or_uuid)?;
            let xml = lox.get_all(&ctrl.uuid)?;
            let current = xml_attr(&xml, "value").unwrap_or("").to_string();
            let matches = eval_op(&current, &op, &value)?;
            if !cli.json {
                println!("{} = {}  →  {} {} {}  →  {}",
                    ctrl.name, current, current, op, value,
                    if matches { "✓ true" } else { "✗ false" });
            } else {
                println!("{}", serde_json::json!({
                    "control": ctrl.name, "current": current,
                    "op": op, "target": value, "result": matches
                }));
            }
            std::process::exit(if matches { 0 } else { 1 });
        },

        Cmd::Run { scene } => {
            let s = Scene::load(&scene)?;
            let mut lox = LoxClient::new(Config::load()?);
            println!("▶  {}", s.name.as_deref().unwrap_or(&scene));
            if let Some(d) = &s.description { println!("   {}", d); }
            println!();
            for (i, step) in s.steps.iter().enumerate() {
                let uuid = match lox.resolve(&step.control) {
                    Ok(u) => u,
                    Err(e) => { eprintln!("Step {}: {}", i+1, e); continue; }
                };
                let resp = lox.send_cmd(&uuid, &step.cmd)?;
                print_resp(&resp, cli.json, &step.control, &step.cmd);
                if step.delay_ms > 0 { thread::sleep(Duration::from_millis(step.delay_ms)); }
            }
        },

        Cmd::Scene { action } => match action {
            SceneCmd::List => {
                let names = Scene::list()?;
                if names.is_empty() { println!("No scenes. Create: lox scene new <name>"); }
                else {
                    for name in &names {
                        let desc = Scene::load(name).ok().and_then(|s| s.description).unwrap_or_default();
                        println!("  {:<20}  {}", name, desc);
                    }
                }
            },
            SceneCmd::Show { name } => {
                println!("{}", fs::read_to_string(Scene::scenes_dir().join(format!("{}.yaml", name)))
                    .with_context(|| format!("Scene '{}' not found", name))?);
            },
            SceneCmd::New { name } => {
                let dir = Scene::scenes_dir();
                fs::create_dir_all(&dir)?;
                let path = dir.join(format!("{}.yaml", name));
                if path.exists() { bail!("Already exists"); }
                fs::write(&path, format!(
                    "name: \"{name}\"\ndescription: \"\"\nsteps:\n  - control: \"\"\n    cmd: \"on\"\n"))?;
                println!("✓  {:?}", path);
            },
        },

        Cmd::Daemon { verbose, poll, interval } => {
            let cfg = Config::load()?;
            println!("🏠 lox daemon starting...");
            let rt = tokio::runtime::Runtime::new()?;
            if poll {
                rt.block_on(daemon::run_polling_daemon(cfg, verbose, interval))?;
            } else {
                rt.block_on(daemon::run_daemon(cfg, verbose))?;
            }
        },

        Cmd::Automation { action } => match action {
            AutomationCmd::List => {
                let auto = Automations::load()?;
                if auto.rules.is_empty() {
                    println!("No rules. Edit: {:?}", Automations::path());
                } else {
                    println!("{} rule(s) in {:?}:", auto.rules.len(), Automations::path());
                    for (i, r) in auto.rules.iter().enumerate() {
                        let cond = if r.op == "changes" {
                            format!("changes")
                        } else {
                            format!("{} {} {}", r.op, r.value.as_deref().unwrap_or("?"), "")
                        };
                        println!("  {}. when '{}' {}→ {}", i+1, r.when, cond, r.run);
                        if let Some(d) = &r.description { println!("     ({})", d); }
                    }
                }
            },
            AutomationCmd::Edit => {
                let path = Automations::path();
                if !path.exists() {
                    fs::create_dir_all(path.parent().unwrap())?;
                    fs::write(&path, Automations::template())?;
                    println!("✓  Created {:?}", path);
                }
                println!("Edit: {}", path.display());
            },
            AutomationCmd::Check => {
                let auto = Automations::load()?;
                println!("Checking {} rule(s)...", auto.rules.len());
                let mut lox = LoxClient::new(Config::load()?);
                for rule in &auto.rules {
                    let uuid = lox.resolve(&rule.when);
                    match uuid {
                        Ok(u) => println!("  ✓  '{}' → {}", rule.when, u),
                        Err(e) => println!("  ✗  '{}' → {}", rule.when, e),
                    }
                }
            },
        },

        Cmd::Set { name_or_uuid, value } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, &encode_path_value(&value))?;
            let code = resp.pointer("/LL/Code").and_then(|v| v.as_str()).unwrap_or("?");
            let val  = resp.pointer("/LL/value").and_then(|v| v.as_str()).unwrap_or("?");
            if code == "200" {
                println!("✓  {} = {}", name_or_uuid, val);
            } else {
                bail!("Error {}: {}", code, val);
            }
        },
        Cmd::Service { action } => {
            let binary = std::env::current_exe()
                .unwrap_or_else(|_| PathBuf::from("lox"));
            match action {
                ServiceCmd::Install => {
                    let unit = format!(r#"[Unit]
Description=Loxone Automation Daemon
After=network-online.target
Wants=network-online.target

[Service]
ExecStart={bin} daemon --poll
Restart=always
RestartSec=5
Environment=HOME={home}

[Install]
WantedBy=multi-user.target
"#,
                        bin = binary.display(),
                        home = dirs::home_dir().unwrap_or_default().display(),
                    );
                    let unit_path = PathBuf::from("/etc/systemd/system/lox-daemon.service");
                    if unit_path.exists() {
                        bail!("Service already installed at {:?}. Remove first with: lox service uninstall", unit_path);
                    }
                    // Write to tmp, then ask to move with sudo
                    let tmp = PathBuf::from("/tmp/lox-daemon.service");
                    fs::write(&tmp, &unit)?;
                    println!("Service file written to {:?}", tmp);
                    println!("
To install (requires sudo):");
                    println!("  sudo mv /tmp/lox-daemon.service /etc/systemd/system/");
                    println!("  sudo systemctl daemon-reload");
                    println!("  sudo systemctl enable --now lox-daemon");
                    println!("
Or run as user service:");
                    let user_dir = dirs::home_dir().unwrap_or_default().join(".config/systemd/user");
                    fs::create_dir_all(&user_dir)?;
                    let user_path = user_dir.join("lox-daemon.service");
                    fs::write(&user_path, &unit)?;
                    println!("  systemctl --user daemon-reload");
                    println!("  systemctl --user enable --now lox-daemon");
                    println!("
User service written to: {:?}", user_path);
                }
                ServiceCmd::Status => {
                    let out = std::process::Command::new("systemctl")
                        .args(["--user", "status", "lox-daemon"])
                        .output();
                    match out {
                        Ok(o) => print!("{}", String::from_utf8_lossy(&o.stdout)),
                        Err(_) => println!("systemctl not available"),
                    }
                }
                ServiceCmd::Logs => {
                    let out = std::process::Command::new("journalctl")
                        .args(["--user", "-u", "lox-daemon", "-n", "50", "--no-pager"])
                        .output();
                    match out {
                        Ok(o) => print!("{}", String::from_utf8_lossy(&o.stdout)),
                        Err(_) => println!("journalctl not available"),
                    }
                }
                ServiceCmd::Uninstall => {
                    let user_path = dirs::home_dir().unwrap_or_default()
                        .join(".config/systemd/user/lox-daemon.service");
                    if user_path.exists() {
                        let _ = std::process::Command::new("systemctl")
                            .args(["--user", "stop", "lox-daemon"]).status();
                        let _ = std::process::Command::new("systemctl")
                            .args(["--user", "disable", "lox-daemon"]).status();
                        fs::remove_file(&user_path)?;
                        let _ = std::process::Command::new("systemctl")
                            .args(["--user", "daemon-reload"]).status();
                        println!("✓ User service removed");
                    } else {
                        println!("No user service found. For system service, remove manually:");
                        println!("  sudo systemctl disable --now lox-daemon");
                        println!("  sudo rm /etc/systemd/system/lox-daemon.service");
                    }
                }
            }
        },
        Cmd::Token { action } => {
            match action {
                TokenCmd::Fetch => {
                    let cfg = Config::load()?;
                    println!("Fetching token from Miniserver...");
                    let rt = tokio::runtime::Runtime::new()?;
                    match rt.block_on(token::acquire_token(&cfg)) {
                        Ok(ts) => {
                            let exp = std::time::UNIX_EPOCH + std::time::Duration::from_secs(ts.valid_until);
                            let days = ts.valid_until.saturating_sub(
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
                            ) / 86400;
                            println!("✓ Token saved to {:?}", token::TokenStore::path());
                            println!("  Valid for: {} days", days);
                        }
                        Err(e) => bail!("Token fetch failed: {}", e),
                    }
                }
                TokenCmd::Info => {
                    match token::TokenStore::load() {
                        Some(ts) => {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                            let days_left = ts.valid_until.saturating_sub(now) / 86400;
                            println!("Token: {}...{}", &ts.token[..8], &ts.token[ts.token.len()-4..]);
                            if ts.is_valid() {
                                println!("Status: ✓ valid ({} days remaining)", days_left);
                            } else {
                                println!("Status: ⚠ expired — run: lox token fetch");
                            }
                        }
                        None => println!("No token saved. Using Basic Auth. Run: lox token fetch"),
                    }
                }
                TokenCmd::Clear => {
                    let path = token::TokenStore::path();
                    if path.exists() {
                        fs::remove_file(&path)?;
                        println!("✓ Token cleared (reverting to Basic Auth)");
                    } else {
                        println!("No token to clear");
                    }
                }
            }
        },
        Cmd::Cache { action } => {
            let cfg = Config::load()?;
            let cache = LoxClient::cache_path(&cfg);
            match action {
                CacheCmd::Info => {
                    if cache.exists() {
                        let meta = cache.metadata()?;
                        let age = std::time::SystemTime::now()
                            .duration_since(meta.modified()?).unwrap_or_default();
                        let size = meta.len();
                        println!("Cache: {:?}", cache);
                        println!("Size:  {:.1} KB", size as f64 / 1024.0);
                        println!("Age:   {}m {}s", age.as_secs() / 60, age.as_secs() % 60);
                        if age.as_secs() < 86400 {
                            println!("Status: ✓ valid ({} until refresh)", {
                                let remaining = 86400u64.saturating_sub(age.as_secs());
                                format!("{}h {}m", remaining / 3600, (remaining % 3600) / 60)
                            });
                        } else {
                            println!("Status: ⚠ stale (will refresh on next command)");
                        }
                    } else {
                        println!("No cache. Will be created on first command.");
                    }
                }
                CacheCmd::Clear => {
                    if cache.exists() {
                        fs::remove_file(&cache)?;
                        println!("✓ Cache cleared");
                    } else {
                        println!("No cache to clear");
                    }
                }
                CacheCmd::Refresh => {
                    let client = Client::builder()
                        .danger_accept_invalid_certs(true)
                        .timeout(Duration::from_secs(15))
                        .build()?;
                    if cache.exists() { let _ = fs::remove_file(&cache); }
                    LoxClient::load_or_fetch_structure(&cfg, &client)?;
                    println!("✓ Structure cache refreshed ({:?})", cache);
                }
            }
        },
        Cmd::Log { lines } => {
            let lox = LoxClient::new(Config::load()?);
            let log = lox.get_text("/dev/fsget/log/def.log")?;
            let all: Vec<&str> = log.lines().collect();
            for line in &all[all.len().saturating_sub(lines)..] {
                println!("{}", line);
            }
        },
    }

    Ok(())
}

fn eval_op(current: &str, op: &str, target: &str) -> Result<bool> {
    Ok(match op {
        "eq"|"==" => current == target,
        "ne"|"!=" => current != target,
        "contains" => current.contains(target),
        "gt"|">" => parse_f(current)? > parse_f(target)?,
        "lt"|"<" => parse_f(current)? < parse_f(target)?,
        "ge"|">=" => parse_f(current)? >= parse_f(target)?,
        "le"|"<=" => parse_f(current)? <= parse_f(target)?,
        _ => bail!("Unknown op '{}'", op),
    })
}

fn parse_f(s: &str) -> Result<f64> {
    s.parse::<f64>().with_context(|| format!("Not a number: '{}'", s))
}

/// Percent-encode characters that would corrupt an HTTP path segment.
/// Does NOT encode '/' so that Loxone command separators (e.g. "manualPosition/0.5") pass through.
fn encode_path_value(s: &str) -> String {
    s.chars().flat_map(|c| match c {
        '%' => vec!['%', '2', '5'],
        ' ' => vec!['%', '2', '0'],
        '#' => vec!['%', '2', '3'],
        '?' => vec!['%', '3', 'F'],
        '+' => vec!['%', '2', 'B'],
        c   => vec![c],
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── eval_op ──────────────────────────────────────────────────────────────

    #[test]
    fn test_eval_op_eq() {
        assert!(eval_op("1", "eq", "1").unwrap());
        assert!(!eval_op("1", "eq", "2").unwrap());
        assert!(eval_op("hello", "==", "hello").unwrap());
    }

    #[test]
    fn test_eval_op_ne() {
        assert!(eval_op("1", "ne", "2").unwrap());
        assert!(!eval_op("1", "!=", "1").unwrap());
    }

    #[test]
    fn test_eval_op_numeric_comparisons() {
        assert!(eval_op("10", "gt", "5").unwrap());
        assert!(eval_op("5", "lt", "10").unwrap());
        assert!(eval_op("5", "ge", "5").unwrap());
        assert!(eval_op("5", "le", "5").unwrap());
        assert!(!eval_op("4", ">", "5").unwrap());
    }

    #[test]
    fn test_eval_op_contains() {
        assert!(eval_op("hello world", "contains", "world").unwrap());
        assert!(!eval_op("hello", "contains", "world").unwrap());
    }

    #[test]
    fn test_eval_op_unknown_returns_err() {
        assert!(eval_op("1", "bogus", "1").is_err());
    }

    // ── encode_path_value ────────────────────────────────────────────────────

    #[test]
    fn test_encode_path_value_plain() {
        assert_eq!(encode_path_value("on"), "on");
        assert_eq!(encode_path_value("manualPosition/0.5"), "manualPosition/0.5");
    }

    #[test]
    fn test_encode_path_value_space() {
        assert_eq!(encode_path_value("hello world"), "hello%20world");
    }

    #[test]
    fn test_encode_path_value_percent() {
        assert_eq!(encode_path_value("50%"), "50%25");
    }

    #[test]
    fn test_encode_path_value_plus() {
        assert_eq!(encode_path_value("a+b"), "a%2Bb");
    }

    // ── is_uuid ──────────────────────────────────────────────────────────────

    #[test]
    fn test_is_uuid() {
        assert!(is_uuid("1fbc668c-005c-7471-ffffed57184a04d2"));
        assert!(!is_uuid("Licht Wohnzimmer"));
        assert!(!is_uuid("short-str"));
    }
}
