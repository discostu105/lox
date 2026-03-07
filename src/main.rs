use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use dirs::home_dir;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Config {
    host: String,
    user: String,
    pass: String,
}

impl Config {
    fn path() -> PathBuf {
        home_dir().unwrap_or_default().join(".lox").join("config.yaml")
    }

    fn load() -> Result<Self> {
        let path = Self::path();
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Config not found at {:?}. Run: lox config set --host ... --user ... --pass ...", path))?;
        let cfg: Config = serde_yaml::from_str(&content)?;
        Ok(cfg)
    }

    fn save(&self) -> Result<()> {
        let path = Self::path();
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, serde_yaml::to_string(self)?)?;
        println!("Config saved to {:?}", path);
        Ok(())
    }
}

// ── Loxone API ────────────────────────────────────────────────────────────────

struct LoxClient {
    cfg: Config,
    client: Client,
}

#[derive(Debug)]
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
            client: Client::builder().danger_accept_invalid_certs(true).build().unwrap(),
        }
    }

    fn get_structure(&self) -> Result<Value> {
        let url = format!("{}/data/LoxApp3.json", self.cfg.host);
        let resp = self.client
            .get(&url)
            .basic_auth(&self.cfg.user, Some(&self.cfg.pass))
            .send()?
            .json::<Value>()?;
        Ok(resp)
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

    fn list_controls(&self, type_filter: Option<&str>, room_filter: Option<&str>) -> Result<Vec<Control>> {
        let structure = self.get_structure()?;

        // Build room map: uuid -> name
        let mut rooms: HashMap<String, String> = HashMap::new();
        if let Some(r) = structure.get("msInfo").and_then(|_| structure.get("rooms")) {
            if let Some(map) = r.as_object() {
                for (uuid, room) in map {
                    if let Some(name) = room.get("name").and_then(|n| n.as_str()) {
                        rooms.insert(uuid.clone(), name.to_string());
                    }
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
                    if !typ.to_lowercase().contains(&tf.to_lowercase()) {
                        continue;
                    }
                }
                if let Some(rf) = room_filter {
                    let room_name = room.as_deref().unwrap_or("");
                    if !room_name.to_lowercase().contains(&rf.to_lowercase()) {
                        continue;
                    }
                }

                controls.push(Control { name, uuid: uuid.clone(), typ, room });
            }
        }

        controls.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(controls)
    }

    fn resolve(&self, name_or_uuid: &str) -> Result<String> {
        // If it looks like a UUID (contains dashes and hex), use directly
        if name_or_uuid.contains('-') && name_or_uuid.len() > 20 {
            return Ok(name_or_uuid.to_string());
        }

        let controls = self.list_controls(None, None)?;
        let matches: Vec<&Control> = controls.iter()
            .filter(|c| c.name.to_lowercase().contains(&name_or_uuid.to_lowercase()))
            .collect();

        match matches.len() {
            0 => anyhow::bail!("No control found matching '{}'", name_or_uuid),
            1 => Ok(matches[0].uuid.clone()),
            _ => {
                eprintln!("Multiple matches for '{}', be more specific:", name_or_uuid);
                for c in &matches {
                    eprintln!("  {} ({})", c.name, c.uuid);
                }
                anyhow::bail!("Ambiguous name")
            }
        }
    }
}

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "lox", about = "Loxone Miniserver CLI", version)]
struct Cli {
    /// Output JSON
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
    /// Send a raw command to a control
    Send {
        name_or_uuid: String,
        command: String,
    },
    /// Turn a control on
    On { name_or_uuid: String },
    /// Turn a control off
    Off { name_or_uuid: String },
    /// Get current state
    Get { name_or_uuid: String },
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
            let lox = LoxClient::new(Config::load()?);
            let controls = lox.list_controls(r#type.as_deref(), room.as_deref())?;

            if cli.json {
                let out: Vec<Value> = controls.iter().map(|c| serde_json::json!({
                    "name": c.name, "uuid": c.uuid, "type": c.typ,
                    "room": c.room
                })).collect();
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("{:<40} {:<20} {:<50}", "NAME", "TYPE", "UUID");
                println!("{}", "-".repeat(112));
                for c in &controls {
                    println!("{:<40} {:<20} {:<50}  [{}]",
                        c.name, c.typ, c.uuid,
                        c.room.as_deref().unwrap_or("-"));
                }
                println!("\n{} controls", controls.len());
            }
        },

        Cmd::Rooms => {
            let lox = LoxClient::new(Config::load()?);
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
            let lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, &command)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                let val = resp.get("LL").and_then(|ll| ll.get("value"))
                    .and_then(|v| v.as_str()).unwrap_or("?");
                println!("✓  {} → {} = {}", name_or_uuid, command, val);
            }
        },

        Cmd::On { name_or_uuid } => {
            let lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, "on")?;
            let val = resp.get("LL").and_then(|ll| ll.get("value"))
                .and_then(|v| v.as_str()).unwrap_or("?");
            println!("✓  {} → on = {}", name_or_uuid, val);
        },

        Cmd::Off { name_or_uuid } => {
            let lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, "off")?;
            let val = resp.get("LL").and_then(|ll| ll.get("value"))
                .and_then(|v| v.as_str()).unwrap_or("?");
            println!("✓  {} → off = {}", name_or_uuid, val);
        },

        Cmd::Get { name_or_uuid } => {
            let lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, "state")?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
        },
    }

    Ok(())
}
