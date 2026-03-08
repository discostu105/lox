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
use crate::token::TokenStore;
use crate::ws::{LoxWsClient, StateEvent};

// ── Automation rule ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Condition {
    pub control: String,
    pub op: String,
    pub value: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Rule {
    /// Primary control name or UUID to watch
    pub when: String,
    /// Operator: eq ne gt lt ge le changes
    #[serde(default = "default_op")]
    pub op: String,
    /// Value to compare against (omit for "changes")
    pub value: Option<String>,
    /// Additional conditions that must also be true (AND)
    #[serde(default)]
    pub also: Vec<Condition>,
    /// Only fire between these hours (e.g. "08:00-22:00")
    pub only_between: Option<String>,
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

  # Example: multi-condition (AND) — shade only if hot AND not already shaded
  # - when: "Temperatur Wohnzimmer"
  #   op: gt
  #   value: "25"
  #   also:
  #     - control: "Beschattung"
  #       op: eq
  #       value: "0"
  #   run: "lox blind 'Beschattung' shade"
  #   cooldown_secs: 600

  # Example: time window — only ring alert during day
  # - when: "Türklingel"
  #   op: eq
  #   value: "1"
  #   only_between: "08:00-22:00"
  #   run: "notify-send 'Doorbell!'"
  #   cooldown_secs: 10
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

fn cmp_value(op: &str, target: &str, new: f64) -> bool {
    match op {
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

fn in_time_window(window: &str, timezone: Option<&str>) -> bool {
    // Format: "HH:MM-HH:MM"
    let parts: Vec<&str> = window.split('-').collect();
    if parts.len() != 2 { return true; }
    fn parse_hm(s: &str) -> Option<u32> {
        let mut p = s.splitn(2, ':');
        let h: u32 = p.next()?.parse().ok()?;
        let m: u32 = p.next()?.parse().ok()?;
        Some(h * 60 + m)
    }
    let (start, end) = match (parse_hm(parts[0]), parse_hm(parts[1])) {
        (Some(s), Some(e)) => (s, e),
        _ => return true,
    };
    use chrono::Timelike;
    let local_min: u32 = if let Some(tz_name) = timezone {
        if let Ok(tz) = tz_name.parse::<chrono_tz::Tz>() {
            let now = chrono::Utc::now().with_timezone(&tz);
            now.hour() * 60 + now.minute()
        } else {
            let now = chrono::Local::now();
            now.hour() * 60 + now.minute()
        }
    } else {
        let now = chrono::Local::now();
        now.hour() * 60 + now.minute()
    };
    if start <= end { local_min >= start && local_min <= end }
    else { local_min >= start || local_min <= end }  // overnight window
}

fn eval_rule(rule: &Rule, old: Option<f64>, new: f64, registry: &StateRegistry, timezone: Option<&str>) -> bool {
    // Time window check
    if let Some(window) = &rule.only_between {
        if !in_time_window(window, timezone) { return false; }
    }

    // Primary condition
    let primary = match rule.op.as_str() {
        "changes" => old.map(|o| (o - new).abs() > 1e-9).unwrap_or(true),
        op => cmp_value(op, rule.value.as_deref().unwrap_or("0"), new),
    };
    if !primary { return false; }

    // Additional AND conditions
    for cond in &rule.also {
        let uuid = registry.resolve_name(&cond.control)
            .unwrap_or_else(|| cond.control.clone());
        let cur = registry.values.get(&uuid).copied().unwrap_or(f64::NAN);
        if !cmp_value(&cond.op, cond.value.as_deref().unwrap_or("0"), cur) {
            return false;
        }
    }

    true
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
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        let url = format!("{}/data/LoxApp3.json", cfg.host);
        let pass = TokenStore::load()
            .filter(|t| t.is_valid())
            .map(|t| t.token)
            .unwrap_or_else(|| cfg.pass.clone());
        client.get(&url)
            .basic_auth(&cfg.user, Some(&pass))
            .send().await?
            .json::<serde_json::Value>().await?
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
    // Track previous match state per rule (for non-"changes" rules: only fire on transition)

    println!("\n🔌 Connecting to Miniserver...");

    let timezone: Option<String> = cfg.timezone.clone();
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
        let timezone2 = timezone.clone();
        let result = ws.run(move |events: Vec<StateEvent>| {
        let rules = &rules2;
        let rule_uuids = &rule_uuids2;
        let registry2 = &registry3;
        let cooldowns2 = &cooldowns3;
        let verbose = verbose2;
        let tz = timezone2.as_deref();
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

                if !eval_rule(rule, old, ev.value, &registry2.read().unwrap(), tz) { continue; }

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

// ── HTTP Polling Daemon (Fallback wenn WS nicht geht) ────────────────────────

pub async fn run_polling_daemon(cfg: Config, verbose: bool, interval_secs: u64) -> Result<()> {
    let automations = Automations::load()?;
    println!("Loaded {} rule(s)", automations.rules.len());

    let pass = TokenStore::load()
        .filter(|t| t.is_valid())
        .map(|t| t.token)
        .unwrap_or_else(|| cfg.pass.clone());
    let structure = {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        let url = format!("{}/data/LoxApp3.json", cfg.host);
        client.get(&url)
            .basic_auth(&cfg.user, Some(&pass))
            .send().await?
            .json::<serde_json::Value>().await?
    };

    let registry = Arc::new(RwLock::new(StateRegistry::default()));
    {
        let mut reg = registry.write().unwrap();
        reg.populate_from_structure(&structure);
        println!("Loaded {} controls", reg.name_to_uuid.len());
    }

    // Pre-resolve rule UUIDs
    let rule_uuids: Vec<Option<String>> = {
        let reg = registry.read().unwrap();
        automations.rules.iter().map(|rule| {
            let uuid = reg.resolve_name(&rule.when);
            if uuid.is_none() { eprintln!("  ⚠  Not found: '{}'", rule.when); }
            uuid
        }).collect()
    };

    let cooldowns: Arc<RwLock<HashMap<usize, u64>>> = Arc::new(RwLock::new(HashMap::new()));
    // Track previous match state per rule (for non-"changes" rules: only fire on transition)
    let prev_matched: Arc<RwLock<HashMap<usize, bool>>> = Arc::new(RwLock::new(HashMap::new()));
    let rules = Arc::new(automations.rules);

    // Collect all UUIDs we need to poll
    let watch_uuids: Vec<String> = rule_uuids.iter()
        .filter_map(|u| u.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter().collect();

    println!("\n🔄 Polling {} controls every {}s  (Ctrl+C to stop)\n",
        watch_uuids.len(), interval_secs);

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    loop {
        for uuid in &watch_uuids {
            let url = format!("{}/dev/sps/io/{}/all", cfg.host, uuid);
            let xml = match client.get(&url)
                .basic_auth(&cfg.user, Some(&pass))
                .send().await.and_then(|r| Ok(r))
            {
                Ok(resp) => match resp.text().await {
                    Ok(x) => x,
                    Err(e) => { eprintln!("Poll error {}: {}", &uuid[..8], e); continue; }
                },
                Err(e) => { eprintln!("Poll error {}: {}", &uuid[..8], e); continue; }
            };

            // Extract value from XML attr
            fn xml_val(xml: &str) -> Option<f64> {
                let key = "value=\"";
                let start = xml.find(key)? + key.len();
                let end = xml[start..].find('"')? + start;
                xml[start..end].parse().ok()
            }

            let Some(new_val) = xml_val(&xml) else { continue; };

            let old_val = {
                let mut reg = registry.write().unwrap();
                reg.update(uuid, new_val)
            };

            if verbose && old_val != Some(new_val) {
                let reg = registry.read().unwrap();
                println!("[{}] {} = {}", now_ts(), reg.name_for(uuid), new_val);
            }

            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

            for (i, rule) in rules.iter().enumerate() {
                let Some(ref rule_uuid) = rule_uuids[i] else { continue; };
                if rule_uuid != uuid { continue; }
                let matches = eval_rule(rule, old_val, new_val, &registry.read().unwrap(), cfg.timezone.as_deref());
                // For non-"changes" rules: only trigger on false→true transition
                let was_matched = *prev_matched.read().unwrap().get(&i).unwrap_or(&false);
                let is_edge = if rule.op == "changes" { matches } else { matches && !was_matched };
                prev_matched.write().unwrap().insert(i, matches);
                if !is_edge { continue; }
                let last = cooldowns.read().unwrap().get(&i).copied().unwrap_or(0);
                if now_secs - last < rule.cooldown_secs { continue; }
                cooldowns.write().unwrap().insert(i, now_secs);
                let reg = registry.read().unwrap();
                println!("\n⚡ Rule triggered: {} = {}", reg.name_for(uuid), new_val);
                fire_rule(rule);
            }
        }

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

fn now_ts() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let s = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    format!("{:02}:{:02}:{:02}", (s%86400)/3600, (s%3600)/60, s%60)
}
