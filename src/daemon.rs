//! Automation daemon — watches state events and fires rules

use anyhow::Result;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, RwLock};

use crate::config::Config;
use crate::ws::{LoxWsClient, StateEvent};

// ── Automation rule ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Rule {
    /// Control name or UUID to watch
    pub when: String,
    /// Operator: eq ne gt lt ge le changes
    #[serde(default = "default_op")]
    pub op: String,
    /// Value to compare against (omit for "changes")
    pub value: Option<String>,
    /// Shell command to run
    pub run: String,
    /// Optional description
    pub description: Option<String>,
    /// Minimum seconds between triggers (debounce)
    #[serde(default = "default_cooldown")]
    pub cooldown_secs: u64,
}

fn default_op() -> String { "changes".into() }
fn default_cooldown() -> u64 { 5 }

#[derive(Debug, Serialize, Deserialize)]
pub struct Automations {
    pub rules: Vec<Rule>,
}

impl Automations {
    pub fn path() -> PathBuf {
        Config::dir().join("automations.yaml")
    }

    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Self { rules: vec![] });
        }
        let content = fs::read_to_string(&path)?;
        Ok(serde_yaml::from_str(&content)?)
    }

    pub fn template() -> &'static str {
r#"# ~/.lox/automations.yaml
# Rules are evaluated on every state change from the Miniserver.
#
# Operators: eq ne gt lt ge le changes
# "changes" triggers on any value change (no value field needed)
#
# cooldown_secs: minimum seconds between re-triggers (default 5)

rules:
  # Example: turn on entrance light when doorbell rings
  # - when: "Tür öffnen"
  #   op: eq
  #   value: "1"
  #   run: "lox on 'Licht Eingang'"
  #   cooldown_secs: 30

  # Example: shade windows when it gets hot
  # - when: "Temperatur Wohnzimmer"
  #   op: gt
  #   value: "25"
  #   run: "lox blind 'Beschattung Zentral EG' shade"
  #   cooldown_secs: 300

  # Example: log all light changes
  # - when: "Lichtsteuerung"
  #   op: changes
  #   run: "echo 'Light changed' >> /tmp/lox.log"
"#
    }
}

// ── State registry ────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct StateRegistry {
    /// uuid → (value, last_triggered_secs)
    values: HashMap<String, f64>,
    /// uuid → name (populated from structure)
    uuid_to_name: HashMap<String, String>,
    /// name/substr → uuid (for rule matching)
    name_to_uuid: HashMap<String, String>,
}

impl StateRegistry {
    pub fn populate_from_structure(&mut self, structure: &serde_json::Value) {
        if let Some(controls) = structure.get("controls").and_then(|c| c.as_object()) {
            for (uuid, ctrl) in controls {
                if let Some(name) = ctrl.get("name").and_then(|n| n.as_str()) {
                    self.uuid_to_name.insert(uuid.clone(), name.to_string());
                    self.name_to_uuid.insert(name.to_lowercase(), uuid.clone());
                }
            }
        }
    }

    pub fn resolve_name(&self, name_or_uuid: &str) -> Option<String> {
        // Direct UUID
        if name_or_uuid.contains('-') && name_or_uuid.len() > 20 {
            return Some(name_or_uuid.to_string());
        }
        // Exact name match
        let lower = name_or_uuid.to_lowercase();
        if let Some(uuid) = self.name_to_uuid.get(&lower) {
            return Some(uuid.clone());
        }
        // Substring match
        let matches: Vec<&str> = self.name_to_uuid.keys()
            .filter(|k| k.contains(&lower))
            .map(|k| k.as_str())
            .collect();
        if matches.len() == 1 {
            return self.name_to_uuid.get(matches[0]).cloned();
        }
        None
    }

    pub fn name_for(&self, uuid: &str) -> String {
        self.uuid_to_name.get(uuid).cloned().unwrap_or_else(|| uuid.to_string())
    }

    pub fn update(&mut self, uuid: &str, value: f64) -> Option<f64> {
        let old = self.values.get(uuid).copied();
        self.values.insert(uuid.to_string(), value);
        old
    }

    pub fn get(&self, uuid: &str) -> Option<f64> {
        self.values.get(uuid).copied()
    }
}

// ── Rule engine ───────────────────────────────────────────────────────────────

fn eval_rule(rule: &Rule, old: Option<f64>, new: f64) -> bool {
    let target = rule.value.as_deref().unwrap_or("0");
    match rule.op.as_str() {
        "changes" => old.map(|o| (o - new).abs() > 1e-9).unwrap_or(true),
        "eq" | "==" => {
            if let Ok(t) = target.parse::<f64>() { (new - t).abs() < 1e-9 }
            else { new.to_string() == target }
        },
        "ne" | "!=" => {
            if let Ok(t) = target.parse::<f64>() { (new - t).abs() > 1e-9 }
            else { new.to_string() != target }
        },
        "gt" | ">"  => target.parse::<f64>().map(|t| new > t).unwrap_or(false),
        "lt" | "<"  => target.parse::<f64>().map(|t| new < t).unwrap_or(false),
        "ge" | ">=" => target.parse::<f64>().map(|t| new >= t).unwrap_or(false),
        "le" | "<=" => target.parse::<f64>().map(|t| new <= t).unwrap_or(false),
        _ => false,
    }
}

fn fire_rule(rule: &Rule) {
    println!("  ▶  {}", rule.run);
    if let Some(desc) = &rule.description {
        println!("     ({})", desc);
    }
    let output = Command::new("sh")
        .arg("-c")
        .arg(&rule.run)
        .output();
    match output {
        Ok(o) => {
            if !o.status.success() {
                eprintln!("  ✗  Command failed (exit {}): {}", o.status,
                    String::from_utf8_lossy(&o.stderr).trim());
            }
        }
        Err(e) => eprintln!("  ✗  Failed to run command: {}", e),
    }
}

// ── Daemon ────────────────────────────────────────────────────────────────────

pub async fn run_daemon(cfg: Config, verbose: bool) -> Result<()> {
    let automations = Automations::load()?;
    println!("Loaded {} rule(s) from {:?}", automations.rules.len(), Automations::path());

    // Fetch structure for name→uuid mapping
    let structure = {
        let cfg2 = cfg.clone();
        tokio::task::spawn_blocking(move || {
            let client = reqwest::blocking::Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(std::time::Duration::from_secs(15))
                .build()?;
            let url = format!("{}/data/LoxApp3.json", cfg2.host);
            client.get(&url)
                .basic_auth(&cfg2.user, Some(&cfg2.pass))
                .send()?.json::<serde_json::Value>()
        }).await??
    };

    let registry = Arc::new(RwLock::new(StateRegistry::default()));
    {
        let mut reg = registry.write().unwrap();
        reg.populate_from_structure(&structure);
        println!("Loaded {} controls from structure", reg.name_to_uuid.len());
    }

    // Pre-resolve rule UUIDs
    let mut rule_uuids: Vec<Option<String>> = Vec::new();
    {
        let reg = registry.read().unwrap();
        for rule in &automations.rules {
            let uuid = reg.resolve_name(&rule.when);
            if uuid.is_none() {
                eprintln!("  ⚠  Rule target not found: '{}'", rule.when);
            }
            rule_uuids.push(uuid);
        }
    }

    // Cooldown tracking: rule_index → last_fired unix secs
    let cooldowns: Arc<RwLock<HashMap<usize, u64>>> = Arc::new(RwLock::new(HashMap::new()));

    println!("\n🔌 Connecting to Miniserver...");

    let rules = Arc::new(automations.rules);
    let rule_uuids = Arc::new(rule_uuids);
    let registry2 = registry.clone();
    let cooldowns2 = cooldowns.clone();

    let ws = LoxWsClient::new(cfg);
    let mut retry = 0u32;
    loop {
        let rules2 = rules.clone();
        let rule_uuids2 = rule_uuids.clone();
        let registry3 = registry2.clone();
        let cooldowns3 = cooldowns2.clone();
        let verbose2 = verbose;
        let result = ws.run(move |events: Vec<StateEvent>| {
        let rules = &rules2;
        let rule_uuids = &rule_uuids2;
        let registry2 = &registry3;
        let cooldowns2 = &cooldowns3;
        let verbose = verbose2;
        for ev in events {
            let old = {
                let mut reg = registry2.write().unwrap();
                reg.update(&ev.uuid, ev.value)
            };

            if verbose {
                let reg = registry2.read().unwrap();
                let name = reg.name_for(&ev.uuid);
                println!("[state] {} = {}", name, ev.value);
            }

            // Check rules
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

            for (i, rule) in rules.iter().enumerate() {
                let Some(ref rule_uuid) = rule_uuids[i] else { continue; };
                if rule_uuid != &ev.uuid { continue; }

                if !eval_rule(rule, old, ev.value) { continue; }

                // Cooldown check
                let last = cooldowns2.read().unwrap().get(&i).copied().unwrap_or(0);
                if now_secs - last < rule.cooldown_secs { continue; }
                cooldowns2.write().unwrap().insert(i, now_secs);

                let reg = registry2.read().unwrap();
                let name = reg.name_for(&ev.uuid);
                println!("\n⚡ Rule triggered: {} = {}", name, ev.value);
                fire_rule(rule);
            }
        }
        }).await;
        match result {
            Ok(_) => break,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("reconnect") || msg.contains("close_notify") || msg.contains("closed") {
                    retry += 1;
                    let delay = (retry * 2).min(30);
                    eprintln!("⚠  Disconnected, reconnecting in {}s... ({})", delay, msg);
                    tokio::time::sleep(Duration::from_secs(delay as u64)).await;
                } else {
                    return Err(e);
                }
            }
        }
    }
    Ok(())
}
