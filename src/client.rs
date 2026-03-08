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
    pub cat: Option<String>,
    pub is_favorite: bool,
    pub is_secured: bool,
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
                .build()
                .unwrap(),
            structure: None,
        }
    }

    pub fn apply_auth(
        &self,
        rb: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
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
        Ok(self
            .apply_auth(self.client.get(&url))
            .send()?
            .json::<Value>()?)
    }

    pub fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let url = format!("{}/{}", self.cfg.host, path.trim_start_matches('/'));
        Ok(self
            .apply_auth(self.client.get(&url))
            .send()?
            .bytes()?
            .to_vec())
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
        // Skip disk cache for localhost hosts (used in tests with mock servers)
        let use_cache = !cfg.host.contains("127.0.0.1") && !cfg.host.contains("localhost");
        if use_cache {
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
        }
        let url = format!("{}/data/LoxApp3.json", cfg.host);
        let pass = TokenStore::load()
            .filter(|t| t.is_valid())
            .map(|t| t.token)
            .unwrap_or_else(|| cfg.pass.clone());
        let resp = client
            .get(&url)
            .basic_auth(&cfg.user, Some(&pass))
            .send()?
            .bytes()?;
        let v: Value = serde_json::from_slice(&resp)?;
        if use_cache {
            if let Some(parent) = cache.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&cache, &resp);
        }
        Ok(v)
    }

    pub fn send_cmd(&self, uuid: &str, cmd: &str) -> Result<Value> {
        self.get_json(&format!("/jdev/sps/io/{}/{}", uuid, cmd))
    }

    pub fn get_all(&self, uuid: &str) -> Result<String> {
        self.get_text(&format!("/dev/sps/io/{}/all", uuid))
    }

    pub fn list_controls(
        &mut self,
        type_filter: Option<&str>,
        room_filter: Option<&str>,
    ) -> Result<Vec<Control>> {
        self.list_controls_ext(type_filter, room_filter, None, false)
    }

    pub fn list_controls_ext(
        &mut self,
        type_filter: Option<&str>,
        room_filter: Option<&str>,
        cat_filter: Option<&str>,
        favorites_only: bool,
    ) -> Result<Vec<Control>> {
        let structure = self.get_structure()?;
        let mut rooms: HashMap<String, String> = HashMap::new();
        if let Some(map) = structure.get("rooms").and_then(|r| r.as_object()) {
            for (uuid, room) in map {
                if let Some(name) = room.get("name").and_then(|n| n.as_str()) {
                    rooms.insert(uuid.clone(), name.to_string());
                }
            }
        }
        let mut cats: HashMap<String, String> = HashMap::new();
        if let Some(map) = structure.get("cats").and_then(|c| c.as_object()) {
            for (uuid, cat) in map {
                if let Some(name) = cat.get("name").and_then(|n| n.as_str()) {
                    cats.insert(uuid.clone(), name.to_string());
                }
            }
        }
        let mut controls = Vec::new();
        if let Some(ctrl_map) = structure.get("controls").and_then(|c| c.as_object()) {
            for (uuid, ctrl) in ctrl_map {
                let name = ctrl
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("?")
                    .to_string();
                let typ = ctrl
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("?")
                    .to_string();
                let room_uuid = ctrl
                    .get("room")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string();
                let cat_uuid = ctrl
                    .get("cat")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                let room = rooms.get(&room_uuid).cloned();
                let cat = cats.get(&cat_uuid).cloned();
                let is_favorite = ctrl
                    .get("isFavorite")
                    .and_then(|f| f.as_bool())
                    .unwrap_or(false);
                let is_secured = ctrl
                    .get("isSecured")
                    .and_then(|f| f.as_bool())
                    .unwrap_or(false);
                if let Some(tf) = type_filter {
                    if !typ.to_lowercase().contains(&tf.to_lowercase()) {
                        continue;
                    }
                }
                if let Some(rf) = room_filter {
                    if !room
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&rf.to_lowercase())
                    {
                        continue;
                    }
                }
                if let Some(cf) = cat_filter {
                    if !cat
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&cf.to_lowercase())
                    {
                        continue;
                    }
                }
                if favorites_only && !is_favorite {
                    continue;
                }
                controls.push(Control {
                    name,
                    uuid: uuid.clone(),
                    typ,
                    room,
                    cat,
                    is_favorite,
                    is_secured,
                });
            }
        }
        controls.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(controls)
    }

    pub fn list_categories(&mut self) -> Result<Vec<(String, String)>> {
        let structure = self.get_structure()?;
        let mut result = Vec::new();
        if let Some(map) = structure.get("cats").and_then(|c| c.as_object()) {
            for (uuid, cat) in map {
                if let Some(name) = cat.get("name").and_then(|n| n.as_str()) {
                    result.push((uuid.clone(), name.to_string()));
                }
            }
        }
        result.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(result)
    }

    pub fn resolve(&mut self, name_or_uuid: &str) -> Result<String> {
        self.resolve_with_room(name_or_uuid, None)
    }

    pub fn resolve_with_room(
        &mut self,
        name_or_uuid: &str,
        room_filter: Option<&str>,
    ) -> Result<String> {
        if is_uuid(name_or_uuid) {
            return Ok(name_or_uuid.to_string());
        }
        if let Some(uuid) = self.cfg.aliases.get(name_or_uuid) {
            return Ok(uuid.clone());
        }
        let (name_part, room_part) = if let Some(idx) = name_or_uuid.rfind('[') {
            if name_or_uuid.ends_with(']') {
                let name = name_or_uuid[..idx].trim();
                let room = &name_or_uuid[idx + 1..name_or_uuid.len() - 1];
                (name, Some(room))
            } else {
                (name_or_uuid, None)
            }
        } else {
            (name_or_uuid, None)
        };
        let effective_room = room_part.or(room_filter);
        let controls = self.list_controls(None, None)?;
        let matches: Vec<&Control> = controls
            .iter()
            .filter(|c| c.name.to_lowercase().contains(&name_part.to_lowercase()))
            .filter(|c| {
                if let Some(rf) = effective_room {
                    c.room
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&rf.to_lowercase())
                } else {
                    true
                }
            })
            .collect();
        match matches.len() {
            0 => bail!("No control matching '{}'", name_or_uuid),
            1 => Ok(matches[0].uuid.clone()),
            _ => {
                for c in &matches {
                    eprintln!(
                        "  {:40} [{}]  {}",
                        c.name,
                        c.room.as_deref().unwrap_or("-"),
                        c.uuid
                    );
                }
                bail!(
                    "Ambiguous: '{}'. Use [Room] qualifier or --room flag.",
                    name_or_uuid
                )
            }
        }
    }

    pub fn get_control_json(&mut self, uuid: &str) -> Result<Value> {
        let structure = self.get_structure()?;
        structure
            .get("controls")
            .and_then(|c| c.get(uuid))
            .cloned()
            .context("Control not found in structure")
    }

    pub fn get_global_states(&mut self) -> Result<Vec<(String, String)>> {
        let structure = self.get_structure()?;
        let mut result = Vec::new();
        if let Some(map) = structure.get("globalStates").and_then(|g| g.as_object()) {
            for (name, uuid_val) in map {
                if let Some(uuid) = uuid_val.as_str() {
                    result.push((name.clone(), uuid.to_string()));
                }
            }
        }
        result.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(result)
    }

    pub fn get_operating_modes(&mut self) -> Result<Vec<(String, String)>> {
        let structure = self.get_structure()?;
        let mut result = Vec::new();
        if let Some(map) = structure.get("operatingModes").and_then(|o| o.as_object()) {
            for (id, name_val) in map {
                if let Some(name) = name_val.as_str() {
                    result.push((id.clone(), name.to_string()));
                }
            }
        }
        result.sort_by(|a, b| {
            a.0.parse::<i32>()
                .unwrap_or(0)
                .cmp(&b.0.parse::<i32>().unwrap_or(0))
        });
        Ok(result)
    }

    pub fn find_control(&mut self, name_or_uuid: &str) -> Result<Control> {
        let controls = self.list_controls(None, None)?;
        if is_uuid(name_or_uuid) {
            return controls
                .into_iter()
                .find(|c| c.uuid == name_or_uuid)
                .context("UUID not found");
        }
        let matches: Vec<Control> = controls
            .into_iter()
            .filter(|c| c.name.to_lowercase().contains(&name_or_uuid.to_lowercase()))
            .collect();
        match matches.len() {
            0 => bail!("No control matching '{}'", name_or_uuid),
            1 => Ok(matches.into_iter().next().unwrap()),
            _ => {
                for c in &matches {
                    eprintln!("  {:40} [{}]", c.name, c.room.as_deref().unwrap_or("-"));
                }
                bail!("Ambiguous: '{}'", name_or_uuid)
            }
        }
    }
}

pub fn is_uuid(s: &str) -> bool {
    s.contains('-') && s.len() > 20
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn mock_config(server: &MockServer) -> Config {
        Config {
            host: server.base_url(),
            user: "test".into(),
            pass: "test".into(),
            ..Default::default()
        }
    }

    // ── list_controls ─────────────────────────────────────────────────────────

    #[test]
    fn test_list_controls_parses_structure() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": { "r1": { "name": "Living Room" } },
                "controls": {
                    "u1": { "name": "Main Light", "type": "LightControllerV2", "room": "r1" },
                    "u2": { "name": "Roller Blind", "type": "Jalousie", "room": "r1" }
                }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server));
        let controls = client.list_controls(None, None).unwrap();
        assert_eq!(controls.len(), 2);
        let light = controls.iter().find(|c| c.name == "Main Light").unwrap();
        assert_eq!(light.room.as_deref(), Some("Living Room"));
    }

    #[test]
    fn test_list_controls_type_filter() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": {},
                "controls": {
                    "u1": { "name": "Light", "type": "LightControllerV2", "room": "" },
                    "u2": { "name": "Blind", "type": "Jalousie", "room": "" }
                }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server));
        let lights = client.list_controls(Some("light"), None).unwrap();
        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].name, "Light");
    }

    // ── send_cmd ──────────────────────────────────────────────────────────────

    #[test]
    fn test_send_cmd_success() {
        let server = MockServer::start();
        let _s = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200)
                .json_body(serde_json::json!({ "rooms": {}, "controls": {} }));
        });
        let _c = server.mock(|when, then| {
            when.method(GET).path("/jdev/sps/io/test-uuid/on");
            then.status(200).json_body(serde_json::json!({
                "LL": { "Code": "200", "value": "1" }
            }));
        });
        let client = LoxClient::new(mock_config(&server));
        let resp = client.send_cmd("test-uuid", "on").unwrap();
        assert_eq!(
            resp.pointer("/LL/Code").and_then(|v| v.as_str()),
            Some("200")
        );
    }

    // ── cache ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_structure_fetched_only_once_per_client() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": {},
                "controls": { "c1": { "name": "Light", "type": "Switch", "room": "" } }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server));
        let _ = client.list_controls(None, None).unwrap();
        let _ = client.list_controls(None, None).unwrap();
        mock.assert_hits(1);
    }

    // ── resolve_with_room ─────────────────────────────────────────────────────

    #[test]
    fn test_resolve_exact_match() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": {},
                "controls": { "uuid-abc": { "name": "Kitchen Light", "type": "Switch", "room": "" } }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server));
        let uuid = client.resolve_with_room("Kitchen Light", None).unwrap();
        assert_eq!(uuid, "uuid-abc");
    }

    #[test]
    fn test_resolve_uuid_passthrough() {
        let server = MockServer::start();
        // No mock needed — UUID bypasses HTTP lookup
        let mut client = LoxClient::new(mock_config(&server));
        let uuid = "1fbc668c-005c-7471-ffffed57184a04d2";
        assert_eq!(client.resolve_with_room(uuid, None).unwrap(), uuid);
    }

    #[test]
    fn test_resolve_ambiguous_returns_err() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": { "r1": { "name": "Kitchen" }, "r2": { "name": "Bedroom" } },
                "controls": {
                    "u1": { "name": "Light", "type": "Switch", "room": "r1" },
                    "u2": { "name": "Light", "type": "Switch", "room": "r2" }
                }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server));
        assert!(client.resolve_with_room("Light", None).is_err());
    }

    #[test]
    fn test_resolve_room_qualifier_disambiguates() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": { "r1": { "name": "Kitchen" }, "r2": { "name": "Bedroom" } },
                "controls": {
                    "u1": { "name": "Light", "type": "Switch", "room": "r1" },
                    "u2": { "name": "Light", "type": "Switch", "room": "r2" }
                }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server));
        let uuid = client.resolve_with_room("Light [Kitchen]", None).unwrap();
        assert_eq!(uuid, "u1");
    }

    // ── favorites / categories ─────────────────────────────────────────────────

    #[test]
    fn test_list_controls_favorites_filter() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": {},
                "cats": {},
                "controls": {
                    "u1": { "name": "Fav Light", "type": "Switch", "room": "", "isFavorite": true },
                    "u2": { "name": "Other Light", "type": "Switch", "room": "", "isFavorite": false }
                }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server));
        let favs = client.list_controls_ext(None, None, None, true).unwrap();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].name, "Fav Light");
        assert!(favs[0].is_favorite);
    }

    #[test]
    fn test_list_controls_category_filter() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": {},
                "cats": { "c1": { "name": "Lighting" }, "c2": { "name": "Shading" } },
                "controls": {
                    "u1": { "name": "Lamp", "type": "Switch", "room": "", "cat": "c1" },
                    "u2": { "name": "Blind", "type": "Jalousie", "room": "", "cat": "c2" }
                }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server));
        let lights = client
            .list_controls_ext(None, None, Some("Lighting"), false)
            .unwrap();
        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].name, "Lamp");
        assert_eq!(lights[0].cat.as_deref(), Some("Lighting"));
    }

    #[test]
    fn test_list_categories() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": {},
                "cats": { "c1": { "name": "Lighting" }, "c2": { "name": "Shading" } },
                "controls": {}
            }));
        });
        let mut client = LoxClient::new(mock_config(&server));
        let cats = client.list_categories().unwrap();
        assert_eq!(cats.len(), 2);
        assert_eq!(cats[0].1, "Lighting");
        assert_eq!(cats[1].1, "Shading");
    }

    #[test]
    fn test_get_global_states() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": {}, "controls": {},
                "globalStates": {
                    "operatingMode": "0f1e2d3c-0000-0000-0000000000000000",
                    "sunrise": "1a2b3c4d-0000-0000-0000000000000000"
                }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server));
        let globals = client.get_global_states().unwrap();
        assert_eq!(globals.len(), 2);
        assert!(globals.iter().any(|(n, _)| n == "operatingMode"));
        assert!(globals.iter().any(|(n, _)| n == "sunrise"));
    }

    #[test]
    fn test_get_operating_modes() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": {}, "controls": {},
                "operatingModes": {
                    "0": "Auto", "1": "Manual", "2": "Comfort"
                }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server));
        let modes = client.get_operating_modes().unwrap();
        assert_eq!(modes.len(), 3);
        assert_eq!(modes[0], ("0".to_string(), "Auto".to_string()));
        assert_eq!(modes[1], ("1".to_string(), "Manual".to_string()));
    }

    #[test]
    fn test_get_control_json() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": {},
                "controls": {
                    "u1": {
                        "name": "Light", "type": "Switch", "room": "",
                        "states": { "active": "state-uuid-1" },
                        "subControls": { "sub1": { "name": "Dimmer", "type": "Dimmer" } }
                    }
                }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server));
        let ctrl_json = client.get_control_json("u1").unwrap();
        assert_eq!(
            ctrl_json.get("name").and_then(|v| v.as_str()),
            Some("Light")
        );
        assert!(ctrl_json.get("subControls").is_some());
        assert!(ctrl_json.get("states").is_some());
    }

    // ── alias ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_resolve_alias() {
        let server = MockServer::start();
        // No HTTP call expected — alias resolves immediately
        let mut cfg = mock_config(&server);
        cfg.aliases
            .insert("mylight".into(), "alias-uuid-1234567890".into());
        let mut client = LoxClient::new(cfg);
        let uuid = client.resolve_with_room("mylight", None).unwrap();
        assert_eq!(uuid, "alias-uuid-1234567890");
    }
}
