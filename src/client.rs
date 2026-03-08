//! Loxone HTTP client, control resolution, and structure cache

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::config::Config;
use crate::token::TokenStore;

// ── Control ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Control {
    pub name: String,
    pub uuid: String,
    pub typ: String,
    pub room: Option<String>,
}

// ── LoxClient ─────────────────────────────────────────────────────────────────

pub struct LoxClient {
    pub cfg: Config,
    pub client: Client,
    structure: Option<Value>,
}

impl LoxClient {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            client: Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(Duration::from_secs(10))
                .build().unwrap(),
            structure: None,
        }
    }

    pub fn apply_auth(&self, rb: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
        if let Some(ts) = TokenStore::load().filter(|t| t.is_valid()) {
            rb.basic_auth(&self.cfg.user, Some(&ts.token))
        } else {
            rb.basic_auth(&self.cfg.user, Some(&self.cfg.pass))
        }
    }

    pub fn get_text(&self, path: &str) -> Result<String> {
        let url = format!("{}/{}", self.cfg.host, path.trim_start_matches('/'));
        Ok(self.apply_auth(self.client.get(&url)).send()?.text()?)
    }

    pub fn get_json(&self, path: &str) -> Result<Value> {
        let url = format!("{}/{}", self.cfg.host, path.trim_start_matches('/'));
        Ok(self.apply_auth(self.client.get(&url)).send()?.json::<Value>()?)
    }

    pub fn get_structure(&mut self) -> Result<&Value> {
        if self.structure.is_none() {
            self.structure = Some(Self::load_or_fetch_structure(&self.cfg, &self.client)?);
        }
        Ok(self.structure.as_ref().unwrap())
    }

    pub fn cache_path(_cfg: &Config) -> PathBuf {
        Config::dir().join("cache").join("structure.json")
    }

    pub fn load_or_fetch_structure(cfg: &Config, client: &Client) -> Result<Value> {
        let cache = Self::cache_path(cfg);
        if let Ok(meta) = cache.metadata() {
            if let Ok(modified) = meta.modified() {
                let age = std::time::SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or_default();
                if age.as_secs() < 86400 {
                    if let Ok(data) = fs::read_to_string(&cache) {
                        if let Ok(v) = serde_json::from_str::<Value>(&data) {
                            return Ok(v);
                        }
                    }
                }
            }
        }
        let url = format!("{}/data/LoxApp3.json", cfg.host);
        let pass = TokenStore::load()
            .filter(|t| t.is_valid())
            .map(|t| t.token)
            .unwrap_or_else(|| cfg.pass.clone());
        let resp = client.get(&url)
            .basic_auth(&cfg.user, Some(&pass))
            .send()?.bytes()?;
        let v: Value = serde_json::from_slice(&resp)?;
        if let Some(parent) = cache.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&cache, &resp);
        Ok(v)
    }

    pub fn send_cmd(&self, uuid: &str, cmd: &str) -> Result<Value> {
        self.get_json(&format!("/jdev/sps/io/{}/{}", uuid, cmd))
    }

    pub fn get_all(&self, uuid: &str) -> Result<String> {
        self.get_text(&format!("/dev/sps/io/{}/all", uuid))
    }

    pub fn list_controls(&mut self, type_filter: Option<&str>, room_filter: Option<&str>) -> Result<Vec<Control>> {
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

    pub fn resolve(&mut self, name_or_uuid: &str) -> Result<String> {
        self.resolve_with_room(name_or_uuid, None)
    }

    pub fn resolve_with_room(&mut self, name_or_uuid: &str, room_filter: Option<&str>) -> Result<String> {
        if is_uuid(name_or_uuid) { return Ok(name_or_uuid.to_string()); }
        if let Some(uuid) = self.cfg.aliases.get(name_or_uuid) {
            return Ok(uuid.clone());
        }
        let (name_part, room_part) = if let Some(idx) = name_or_uuid.rfind('[') {
            if name_or_uuid.ends_with(']') {
                let name = name_or_uuid[..idx].trim();
                let room = &name_or_uuid[idx+1..name_or_uuid.len()-1];
                (name, Some(room))
            } else {
                (name_or_uuid, None)
            }
        } else {
            (name_or_uuid, None)
        };
        let effective_room = room_part.or(room_filter);
        let controls = self.list_controls(None, None)?;
        let matches: Vec<&Control> = controls.iter()
            .filter(|c| c.name.to_lowercase().contains(&name_part.to_lowercase()))
            .filter(|c| {
                if let Some(rf) = effective_room {
                    c.room.as_deref().unwrap_or("").to_lowercase().contains(&rf.to_lowercase())
                } else { true }
            })
            .collect();
        match matches.len() {
            0 => bail!("No control matching '{}'", name_or_uuid),
            1 => Ok(matches[0].uuid.clone()),
            _ => {
                for c in &matches {
                    eprintln!("  {:40} [{}]  {}", c.name, c.room.as_deref().unwrap_or("-"), c.uuid);
                }
                bail!("Ambiguous: '{}'. Use [Room] qualifier or --room flag.", name_or_uuid)
            }
        }
    }

    pub fn find_control(&mut self, name_or_uuid: &str) -> Result<Control> {
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

pub fn is_uuid(s: &str) -> bool { s.contains('-') && s.len() > 20 }
