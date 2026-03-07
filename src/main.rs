use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use dirs::home_dir;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use std::thread;

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Config {
    host: String,
    user: String,
    pass: String,
}

impl Config {
    fn dir() -> PathBuf {
        home_dir().unwrap_or_default().join(".lox")
    }

    fn path() -> PathBuf {
        Self::dir().join("config.yaml")
    }

    fn load() -> Result<Self> {
        let path = Self::path();
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Config not found. Run: lox config set --host ... --user ... --pass ..."))?;
        Ok(serde_yaml::from_str(&content)?)
    }

    fn save(&self) -> Result<()> {
        let path = Self::path();
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, serde_yaml::to_string(self)?)?;
        println!("✓  Config saved to {:?}", path);
        Ok(())
    }
}

// ── Scene ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct SceneStep {
    /// Control name or UUID
    control: String,
    /// Command to send (on/off/pulse/...)
    cmd: String,
    /// Optional delay in ms after this step
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
    fn scenes_dir() -> PathBuf {
        Config::dir().join("scenes")
    }

    fn load(name: &str) -> Result<Self> {
        let path = Self::scenes_dir().join(format!("{}.yaml", name));
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Scene '{}' not found at {:?}", name, path))?;
        Ok(serde_yaml::from_str(&content)?)
    }

    fn list() -> Result<Vec<String>> {
        let dir = Self::scenes_dir();
        if !dir.exists() {
            return Ok(vec![]);
        }
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

// ── Loxone API ────────────────────────────────────────────────────────────────

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
    states: HashMap<String, String>,
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

    fn get_structure(&mut self) -> Result<&Value> {
        if self.structure.is_none() {
            let url = format!("{}/data/LoxApp3.json", self.cfg.host);
            let resp = self.client
                .get(&url)
                .basic_auth(&self.cfg.user, Some(&self.cfg.pass))
                .send()?
                .json::<Value>()?;
            self.structure = Some(resp);
        }
        Ok(self.structure.as_ref().unwrap())
    }

    fn send_cmd(&self, uuid: &str, cmd: &str) -> Result<Value> {
        let url = format!("{}/jdev/sps/io/{}/{}", self.cfg.host, uuid, cmd);
        let resp = self.client
            .get(&url)
            .basic_auth(&self.cfg.user, Some(&self.cfg.pass))
            .send()?
            .json::<Value>()?;
        Ok(resp)
    }

    fn get_state(&self, state_uuid: &str) -> Result<String> {
        let url = format!("{}/jdev/sps/io/{}/state", self.cfg.host, state_uuid);
        let resp = self.client
            .get(&url)
            .basic_auth(&self.cfg.user, Some(&self.cfg.pass))
            .send()?
            .json::<Value>()?;
        let val = resp.pointer("/LL/value")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string();
        Ok(val)
    }

    fn list_controls(&mut self, type_filter: Option<&str>, room_filter: Option<&str>) -> Result<Vec<Control>> {
        let structure = self.get_structure()?;

        // Build room map
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

                // Collect state UUIDs
                let mut states: HashMap<String, String> = HashMap::new();
                if let Some(s) = ctrl.get("states").and_then(|s| s.as_object()) {
                    for (k, v) in s {
                        if let Some(suuid) = v.as_str() {
                            states.insert(k.clone(), suuid.to_string());
                        }
                    }
                }

                if let Some(tf) = type_filter {
                    if !typ.to_lowercase().contains(&tf.to_lowercase()) { continue; }
                }
                if let Some(rf) = room_filter {
                    let room_name = room.as_deref().unwrap_or("");
                    if !room_name.to_lowercase().contains(&rf.to_lowercase()) { continue; }
                }

                controls.push(Control { name, uuid: uuid.clone(), typ, room, states });
            }
        }

        controls.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(controls)
    }

    fn resolve(&mut self, name_or_uuid: &str) -> Result<String> {
        // UUID-like: contains dashes and is long
        if name_or_uuid.contains('-') && name_or_uuid.len() > 20 {
            return Ok(name_or_uuid.to_string());
        }

        let controls = self.list_controls(None, None)?;
        let matches: Vec<&Control> = controls.iter()
            .filter(|c| c.name.to_lowercase().contains(&name_or_uuid.to_lowercase()))
            .collect();

        match matches.len() {
            0 => bail!("No control found matching '{}'", name_or_uuid),
            1 => Ok(matches[0].uuid.clone()),
            _ => {
                eprintln!("Multiple matches for '{}', be more specific:", name_or_uuid);
                for c in &matches {
                    eprintln!("  {} ({})  [{}]", c.name, c.uuid,
                        c.room.as_deref().unwrap_or("-"));
                }
                bail!("Ambiguous name")
            }
        }
    }

    fn find_control(&mut self, name_or_uuid: &str) -> Result<Control> {
        if name_or_uuid.contains('-') && name_or_uuid.len() > 20 {
            let controls = self.list_controls(None, None)?;
            return controls.into_iter().find(|c| c.uuid == name_or_uuid)
                .context("UUID not found in structure");
        }
        let controls = self.list_controls(None, None)?;
        let matches: Vec<Control> = controls.into_iter()
            .filter(|c| c.name.to_lowercase().contains(&name_or_uuid.to_lowercase()))
            .collect();
        match matches.len() {
            0 => bail!("No control found matching '{}'", name_or_uuid),
            1 => Ok(matches.into_iter().next().unwrap()),
            _ => {
                eprintln!("Multiple matches for '{}':", name_or_uuid);
                for c in &matches {
                    eprintln!("  {} ({})  [{}]", c.name, c.uuid,
                        c.room.as_deref().unwrap_or("-"));
                }
                bail!("Ambiguous name")
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn print_response(resp: &Value, json: bool, name: &str, cmd: &str) {
    if json {
        println!("{}", serde_json::to_string_pretty(resp).unwrap());
    } else {
        let val = resp.pointer("/LL/value").and_then(|v| v.as_str()).unwrap_or("?");
        let code = resp.pointer("/LL/Code").and_then(|v| v.as_str()).unwrap_or("?");
        let ok = code == "200";
        let icon = if ok { "✓" } else { "✗" };
        println!("{icon}  {name} → {cmd} = {val}");
    }
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
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
    /// List controls
    Ls {
        #[arg(long)] r#type: Option<String>,
        #[arg(long)] room: Option<String>,
    },
    /// List rooms
    Rooms,
    /// Send a raw command
    Send { name_or_uuid: String, command: String },
    /// Turn on
    On { name_or_uuid: String },
    /// Turn off
    Off { name_or_uuid: String },
    /// Pulse (momentary trigger)
    Pulse { name_or_uuid: String },
    /// Get current state
    Get { name_or_uuid: String },
    /// Watch state changes (polling)
    Watch {
        name_or_uuid: String,
        /// Poll interval in seconds (default: 2)
        #[arg(long, default_value = "2")]
        interval: u64,
    },
    /// Check state — exits 0 if matches, 1 if not (for shell scripting)
    If {
        name_or_uuid: String,
        /// Operator: eq, ne, gt, lt, ge, le, contains
        op: String,
        value: String,
    },
    /// Run a scene
    Run { scene: String },
    /// List or create scenes
    Scene {
        #[command(subcommand)]
        action: SceneCmd,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    Set {
        #[arg(long)] host: String,
        #[arg(long)] user: String,
        #[arg(long)] pass: String,
    },
    Show,
}

#[derive(Subcommand)]
enum SceneCmd {
    /// List available scenes
    List,
    /// Show scene contents
    Show { name: String },
    /// Create a minimal scene template
    New { name: String },
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.cmd {
        Cmd::Config { action } => match action {
            ConfigCmd::Set { host, user, pass } => {
                Config { host, user, pass }.save()?;
            }
            ConfigCmd::Show => {
                let cfg = Config::load()?;
                println!("host: {}\nuser: {}\npass: {}", cfg.host, cfg.user, "*".repeat(cfg.pass.len()));
            }
        },

        Cmd::Ls { r#type, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let controls = lox.list_controls(r#type.as_deref(), room.as_deref())?;
            if cli.json {
                let out: Vec<Value> = controls.iter().map(|c| serde_json::json!({
                    "name": c.name, "uuid": c.uuid, "type": c.typ, "room": c.room
                })).collect();
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("{:<40} {:<24} {:<22} {}", "NAME", "ROOM", "TYPE", "UUID");
                println!("{}", "-".repeat(120));
                for c in &controls {
                    println!("{:<40} {:<24} {:<22} {}",
                        c.name,
                        c.room.as_deref().unwrap_or("-"),
                        c.typ, c.uuid);
                }
                println!("\n{} controls", controls.len());
            }
        },

        Cmd::Rooms => {
            let mut lox = LoxClient::new(Config::load()?);
            let structure = lox.get_structure()?;
            if let Some(rooms) = structure.get("rooms").and_then(|r| r.as_object()) {
                let mut names: Vec<&str> = rooms.values()
                    .filter_map(|r| r.get("name").and_then(|n| n.as_str()))
                    .collect();
                names.sort();
                for name in names { println!("{}", name); }
            }
        },

        Cmd::Send { name_or_uuid, command } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, &command)?;
            print_response(&resp, cli.json, &name_or_uuid, &command);
        },

        Cmd::On { name_or_uuid } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, "on")?;
            print_response(&resp, cli.json, &name_or_uuid, "on");
            let _ = resp;
        },

        Cmd::Off { name_or_uuid } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, "off")?;
            print_response(&resp, cli.json, &name_or_uuid, "off");
        },

        Cmd::Pulse { name_or_uuid } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, "pulse")?;
            print_response(&resp, cli.json, &name_or_uuid, "pulse");
        },

        Cmd::Get { name_or_uuid } => {
            let mut lox = LoxClient::new(Config::load()?);
            let ctrl = lox.find_control(&name_or_uuid)?;
            if ctrl.states.is_empty() {
                println!("No states available for '{}'", ctrl.name);
            } else if cli.json {
                let mut result: HashMap<String, String> = HashMap::new();
                for (key, state_uuid) in &ctrl.states {
                    if let Ok(val) = lox.get_state(state_uuid) {
                        result.insert(key.clone(), val);
                    }
                }
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Control: {} ({})", ctrl.name, ctrl.uuid);
                println!("Type:    {}", ctrl.typ);
                println!("Room:    {}", ctrl.room.as_deref().unwrap_or("-"));
                println!("States:");
                for (key, state_uuid) in &ctrl.states {
                    match lox.get_state(state_uuid) {
                        Ok(val) => println!("  {:<20} = {}", key, val),
                        Err(_)  => println!("  {:<20} = (unavailable)", key),
                    }
                }
            }
        },

        Cmd::Watch { name_or_uuid, interval } => {
            let mut lox = LoxClient::new(Config::load()?);
            let ctrl = lox.find_control(&name_or_uuid)?;
            println!("Watching '{}' every {}s (Ctrl+C to stop)...", ctrl.name, interval);

            // Track active state (first state key, usually "active" or "value")
            let state_key = ctrl.states.keys()
                .find(|k| *k == "active" || *k == "value")
                .or_else(|| ctrl.states.keys().next())
                .cloned();

            let Some(key) = state_key else {
                bail!("No states available for '{}'", ctrl.name);
            };
            let state_uuid = ctrl.states[&key].clone();

            let mut last = String::new();
            loop {
                match lox.get_state(&state_uuid) {
                    Ok(val) => {
                        if val != last {
                            let ts = chrono_now();
                            if cli.json {
                                println!("{}", serde_json::json!({"time": ts, "control": ctrl.name, "key": key, "value": val}));
                            } else {
                                println!("[{}] {} {} = {}", ts, ctrl.name, key, val);
                            }
                            last = val;
                        }
                    }
                    Err(e) => eprintln!("Error: {}", e),
                }
                thread::sleep(Duration::from_secs(interval));
            }
        },

        Cmd::If { name_or_uuid, op, value } => {
            let mut lox = LoxClient::new(Config::load()?);
            let ctrl = lox.find_control(&name_or_uuid)?;

            // Get primary state
            let state_key = ctrl.states.keys()
                .find(|k| *k == "active" || *k == "value")
                .or_else(|| ctrl.states.keys().next())
                .cloned()
                .context("No states available")?;
            let state_uuid = &ctrl.states[&state_key];
            let current = lox.get_state(state_uuid)?;

            let matches = eval_op(&current, &op, &value)?;

            if !cli.json {
                println!("{} {} {} {}  →  {}",
                    ctrl.name, state_key, op, value,
                    if matches { "✓ true" } else { "✗ false" });
            } else {
                println!("{}", serde_json::json!({
                    "control": ctrl.name,
                    "state": current,
                    "op": op,
                    "target": value,
                    "result": matches
                }));
            }

            std::process::exit(if matches { 0 } else { 1 });
        },

        Cmd::Run { scene } => {
            let s = Scene::load(&scene)?;
            let mut lox = LoxClient::new(Config::load()?);
            let name = s.name.as_deref().unwrap_or(&scene);
            println!("Running scene: {}", name);
            if let Some(desc) = &s.description {
                println!("  {}", desc);
            }
            println!();

            for (i, step) in s.steps.iter().enumerate() {
                let uuid = match lox.resolve(&step.control) {
                    Ok(u) => u,
                    Err(e) => {
                        eprintln!("Step {}: {}", i + 1, e);
                        continue;
                    }
                };
                let resp = lox.send_cmd(&uuid, &step.cmd)?;
                print_response(&resp, cli.json, &step.control, &step.cmd);

                if step.delay_ms > 0 {
                    thread::sleep(Duration::from_millis(step.delay_ms));
                }
            }
        },

        Cmd::Scene { action } => match action {
            SceneCmd::List => {
                let names = Scene::list()?;
                if names.is_empty() {
                    println!("No scenes found. Create one in: {:?}", Scene::scenes_dir());
                } else {
                    for name in &names {
                        match Scene::load(name) {
                            Ok(s) => println!("  {:20}  {}", name,
                                s.description.as_deref().unwrap_or("")),
                            Err(_) => println!("  {}", name),
                        }
                    }
                }
            },
            SceneCmd::Show { name } => {
                let path = Scene::scenes_dir().join(format!("{}.yaml", name));
                let content = fs::read_to_string(&path)
                    .with_context(|| format!("Scene '{}' not found", name))?;
                println!("{}", content);
            },
            SceneCmd::New { name } => {
                let dir = Scene::scenes_dir();
                fs::create_dir_all(&dir)?;
                let path = dir.join(format!("{}.yaml", name));
                if path.exists() {
                    bail!("Scene '{}' already exists", name);
                }
                let template = format!(
r#"name: "{name}"
description: "Describe your scene"
steps:
  - control: "Control Name or UUID"
    cmd: "on"
  - control: "Another Control"
    cmd: "off"
    delay_ms: 500  # optional delay after this step
"#);
                fs::write(&path, &template)?;
                println!("✓  Scene template created: {:?}", path);
                println!("Edit it and run: lox run {}", name);
            },
        },
    }

    Ok(())
}

fn eval_op(current: &str, op: &str, target: &str) -> Result<bool> {
    Ok(match op {
        "eq" | "==" => current == target,
        "ne" | "!=" => current != target,
        "contains"  => current.contains(target),
        "gt" | ">"  => parse_f(current)? > parse_f(target)?,
        "lt" | "<"  => parse_f(current)? < parse_f(target)?,
        "ge" | ">=" => parse_f(current)? >= parse_f(target)?,
        "le" | "<=" => parse_f(current)? <= parse_f(target)?,
        _ => bail!("Unknown operator '{}'. Use: eq ne gt lt ge le contains", op),
    })
}

fn parse_f(s: &str) -> Result<f64> {
    s.parse::<f64>().with_context(|| format!("Cannot parse '{}' as number", s))
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    // Simple HH:MM:SS — no dep needed
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}
