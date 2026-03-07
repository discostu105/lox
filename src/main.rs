mod config;
mod ws;
mod daemon;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use daemon::Automations;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

// ── Scene ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct SceneStep {
    control: String,
    cmd: String,
    #[serde(default)]
    delay_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Scene {
    name: Option<String>,
    description: Option<String>,
    steps: Vec<SceneStep>,
}

impl Scene {
    fn scenes_dir() -> PathBuf { Config::dir().join("scenes") }

    fn load(name: &str) -> Result<Self> {
        let path = Self::scenes_dir().join(format!("{}.yaml", name));
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Scene '{}' not found", name))?;
        Ok(serde_yaml::from_str(&content)?)
    }

    fn list() -> Result<Vec<String>> {
        let dir = Self::scenes_dir();
        if !dir.exists() { return Ok(vec![]); }
        let mut names = vec![];
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.extension().map(|e| e == "yaml").unwrap_or(false) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort();
        Ok(names)
    }
}

// ── HTTP Client ───────────────────────────────────────────────────────────────

struct LoxClient {
    cfg: Config,
    client: Client,
    structure: Option<Value>,
}

#[derive(Debug, Clone)]
struct Control {
    name: String,
    uuid: String,
    typ: String,
    room: Option<String>,
}

impl LoxClient {
    fn new(cfg: Config) -> Self {
        Self {
            cfg,
            client: Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(Duration::from_secs(10))
                .build().unwrap(),
            structure: None,
        }
    }

    fn get_text(&self, path: &str) -> Result<String> {
        let url = format!("{}/{}", self.cfg.host, path.trim_start_matches('/'));
        Ok(self.client.get(&url)
            .basic_auth(&self.cfg.user, Some(&self.cfg.pass))
            .send()?.text()?)
    }

    fn get_json(&self, path: &str) -> Result<Value> {
        let url = format!("{}/{}", self.cfg.host, path.trim_start_matches('/'));
        Ok(self.client.get(&url)
            .basic_auth(&self.cfg.user, Some(&self.cfg.pass))
            .send()?.json::<Value>()?)
    }

    fn get_structure(&mut self) -> Result<&Value> {
        if self.structure.is_none() {
            self.structure = Some(self.get_json("/data/LoxApp3.json")?);
        }
        Ok(self.structure.as_ref().unwrap())
    }

    fn send_cmd(&self, uuid: &str, cmd: &str) -> Result<Value> {
        self.get_json(&format!("/jdev/sps/io/{}/{}", uuid, cmd))
    }

    fn get_all(&self, uuid: &str) -> Result<String> {
        self.get_text(&format!("/dev/sps/io/{}/all", uuid))
    }

    fn list_controls(&mut self, type_filter: Option<&str>, room_filter: Option<&str>) -> Result<Vec<Control>> {
        let structure = self.get_structure()?;
        let mut rooms: HashMap<String, String> = HashMap::new();
        if let Some(map) = structure.get("rooms").and_then(|r| r.as_object()) {
            for (uuid, room) in map {
                if let Some(name) = room.get("name").and_then(|n| n.as_str()) {
                    rooms.insert(uuid.clone(), name.to_string());
                }
            }
        }
        let mut controls = Vec::new();
        if let Some(ctrl_map) = structure.get("controls").and_then(|c| c.as_object()) {
            for (uuid, ctrl) in ctrl_map {
                let name = ctrl.get("name").and_then(|n| n.as_str()).unwrap_or("?").to_string();
                let typ = ctrl.get("type").and_then(|t| t.as_str()).unwrap_or("?").to_string();
                let room_uuid = ctrl.get("room").and_then(|r| r.as_str()).unwrap_or("").to_string();
                let room = rooms.get(&room_uuid).cloned();
                if let Some(tf) = type_filter {
                    if !typ.to_lowercase().contains(&tf.to_lowercase()) { continue; }
                }
                if let Some(rf) = room_filter {
                    if !room.as_deref().unwrap_or("").to_lowercase().contains(&rf.to_lowercase()) { continue; }
                }
                controls.push(Control { name, uuid: uuid.clone(), typ, room });
            }
        }
        controls.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(controls)
    }

    fn resolve(&mut self, name_or_uuid: &str) -> Result<String> {
        if is_uuid(name_or_uuid) { return Ok(name_or_uuid.to_string()); }
        let controls = self.list_controls(None, None)?;
        let matches: Vec<&Control> = controls.iter()
            .filter(|c| c.name.to_lowercase().contains(&name_or_uuid.to_lowercase()))
            .collect();
        match matches.len() {
            0 => bail!("No control matching '{}'", name_or_uuid),
            1 => Ok(matches[0].uuid.clone()),
            _ => {
                for c in &matches {
                    eprintln!("  {:40} [{}]  {}", c.name, c.room.as_deref().unwrap_or("-"), c.uuid);
                }
                bail!("Ambiguous: '{}'", name_or_uuid)
            }
        }
    }

    fn find_control(&mut self, name_or_uuid: &str) -> Result<Control> {
        let controls = self.list_controls(None, None)?;
        if is_uuid(name_or_uuid) {
            return controls.into_iter().find(|c| c.uuid == name_or_uuid).context("UUID not found");
        }
        let matches: Vec<Control> = controls.into_iter()
            .filter(|c| c.name.to_lowercase().contains(&name_or_uuid.to_lowercase()))
            .collect();
        match matches.len() {
            0 => bail!("No control matching '{}'", name_or_uuid),
            1 => Ok(matches.into_iter().next().unwrap()),
            _ => {
                for c in &matches { eprintln!("  {:40} [{}]", c.name, c.room.as_deref().unwrap_or("-")); }
                bail!("Ambiguous: '{}'", name_or_uuid)
            }
        }
    }
}

fn is_uuid(s: &str) -> bool { s.contains('-') && s.len() > 20 }

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
    use std::time::{SystemTime, UNIX_EPOCH};
    let s = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    format!("{:02}:{:02}:{:02}", (s%86400)/3600, (s%3600)/60, s%60)
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
    /// Miniserver health
    Status,
    /// List controls
    Ls { #[arg(long)] r#type: Option<String>, #[arg(long)] room: Option<String> },
    /// List rooms
    Rooms,
    /// Get full state of a control
    Get { name_or_uuid: String },
    /// Send raw command
    Send { name_or_uuid: String, command: String },
    /// Turn on
    On { name_or_uuid: String },
    /// Turn off
    Off { name_or_uuid: String },
    /// Momentary pulse
    Pulse { name_or_uuid: String },
    /// Control blind: up | down | stop | shade | full-up | full-down
    Blind { name_or_uuid: String, action: String },
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
enum ConfigCmd {
    Set {
        #[arg(long)] host: String,
        #[arg(long)] user: String,
        #[arg(long)] pass: String,
        #[arg(long, default_value = "")] serial: String,
    },
    Show,
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
            ConfigCmd::Set { host, user, pass, serial } => {
                Config { host, user, pass, serial }.save()?;
            }
            ConfigCmd::Show => {
                let cfg = Config::load()?;
                println!("host:   {}", cfg.host);
                println!("user:   {}", cfg.user);
                println!("pass:   {}", "*".repeat(cfg.pass.len()));
                if !cfg.serial.is_empty() { println!("serial: {}", cfg.serial); }
            }
        },

        Cmd::Status => {
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
        },

        Cmd::Ls { r#type, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let controls = lox.list_controls(r#type.as_deref(), room.as_deref())?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&controls.iter().map(|c| serde_json::json!({
                    "name": c.name, "uuid": c.uuid, "type": c.typ, "room": c.room
                })).collect::<Vec<_>>())?);
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

        Cmd::Get { name_or_uuid } => {
            let mut lox = LoxClient::new(Config::load()?);
            let ctrl = lox.find_control(&name_or_uuid)?;
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

        Cmd::Send { name_or_uuid, command } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, &command)?;
            print_resp(&resp, cli.json, &name_or_uuid, &command);
        },
        Cmd::On { name_or_uuid } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, "on")?;
            print_resp(&resp, cli.json, &name_or_uuid, "on");
        },
        Cmd::Off { name_or_uuid } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, "off")?;
            print_resp(&resp, cli.json, &name_or_uuid, "off");
        },
        Cmd::Pulse { name_or_uuid } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, "pulse")?;
            print_resp(&resp, cli.json, &name_or_uuid, "pulse");
        },

        Cmd::Blind { name_or_uuid, action } => {
            let mut lox = LoxClient::new(Config::load()?);
            let ctrl = lox.find_control(&name_or_uuid)?;
            if !matches!(ctrl.typ.as_str(), "Jalousie" | "CentralJalousie") {
                bail!("'{}' is type '{}', not a Jalousie", ctrl.name, ctrl.typ);
            }
            let cmd = match action.to_lowercase().as_str() {
                "up"   | "open"  => "PulseUp",
                "down" | "close" => "PulseDown",
                "stop"           => "off",
                "shade" | "auto" => "AutomaticDown",
                "full-up"        => "FullUp",
                "full-down"      => "FullDown",
                other => bail!("Unknown action '{}'. Use: up down stop shade full-up full-down", other),
            };
            let resp = lox.send_cmd(&ctrl.uuid, cmd)?;
            print_resp(&resp, cli.json, &ctrl.name, cmd);
            if !cli.json {
                thread::sleep(Duration::from_millis(400));
                let xml = lox.get_all(&ctrl.uuid)?;
                if let Some(pos) = xml_attr(&xml, "StatePos") {
                    let p: f64 = pos.parse().unwrap_or(0.0);
                    println!("   Position: {:.0}%  {}", p * 100.0, bar(p, 1.0));
                }
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
            let url = format!("{}/jdev/sps/io/{}/{}", lox.cfg.host, uuid, value);
            let resp: Value = lox.client.get(&url)
                .basic_auth(&lox.cfg.user, Some(&lox.cfg.pass))
                .send()?.json()?;
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
