//! Loxone HTTP client, control resolution, and structure cache

pub use crate::error::HttpStatusError;
use crate::error::{Context, Error, Result, bail};
use reqwest::blocking::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

// ── Constants ────────────────────────────────────────────────────────────────

const HTTP_TIMEOUT_SECS: u64 = 10;
const RETRY_DELAYS_MS: [u64; 3] = [200, 400, 800];
const CACHE_TTL_SECS: u64 = 86400;
pub use crate::time::LOXONE_EPOCH_SECS;

// ── Global verbosity ─────────────────────────────────────────────────────────

static VERBOSE: AtomicU8 = AtomicU8::new(0);

/// Set the global verbosity level: 0 = quiet, 1 = log requests to stderr,
/// 2 = also their response bodies.
pub fn set_verbose(level: u8) {
    VERBOSE.store(level, Ordering::Relaxed);
}

pub fn verbose() -> u8 {
    VERBOSE.load(Ordering::Relaxed)
}

/// Redact credentials from a URL string for safe logging.
/// Removes basic-auth passwords and known sensitive query parameters.
pub fn redact_url(url: &str) -> String {
    // Redact userinfo password (http://user:pass@host → http://user:***@host)
    let mut result = if let Ok(mut parsed) = reqwest::Url::parse(url) {
        if parsed.password().is_some() {
            let _ = parsed.set_password(Some("***"));
        }
        parsed.to_string()
    } else {
        url.to_string()
    };
    // Redact all occurrences of known sensitive query params.
    // Only match whole parameter names (preceded by '?' or '&') to avoid
    // false positives like "passthrough=" matching the "pass=" pattern.
    for param in &["token", "key", "pass", "password", "autht"] {
        let pattern = format!("{}=", param);
        let mut search_from = 0;
        while let Some(idx) = result[search_from..].find(&pattern) {
            let abs_idx = search_from + idx;
            // Ensure this is a whole query parameter name, not a substring
            if abs_idx > 0 {
                let prev = result.as_bytes()[abs_idx - 1];
                if prev != b'?' && prev != b'&' {
                    search_from = abs_idx + pattern.len();
                    continue;
                }
            }
            let start = abs_idx + pattern.len();
            let end = result[start..]
                .find('&')
                .map_or(result.len(), |i| start + i);
            result.replace_range(start..end, "***");
            search_from = start + 3; // skip past "***"
        }
    }
    result
}

/// Log a response body at -vv level, truncating large responses.
fn log_body(body: &str) {
    if body.len() > 500 {
        // Find a UTF-8 safe truncation point at or before byte 500
        let truncated = &body[..body.floor_char_boundary(500)];
        eprintln!("  body: {}… ({} bytes total)", truncated, body.len());
    } else {
        eprintln!("  body: {}", body);
    }
}

use crate::connection::Connection;

/// The User-Agent of every HTTP request.
pub const USER_AGENT: &str = concat!("loxone-client/", env!("CARGO_PKG_VERSION"));

// ── Autopilot ───────────────────────────────────────────────────────────────

/// The structure file's `autopilot` entry: one container for all Automatic
/// Designer rules, with the states `changed` (unix seconds of the last rule
/// edit) and `history` (recent runs). The rules themselves are only in the
/// program file (`AutopilotRule` objects).
#[derive(Debug, Clone)]
pub struct Autopilot {
    pub uuid: String,
    pub state_changed: String,
    pub state_history: String,
}

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
    /// Value format from `details.format`, e.g. `%.1f°C` or `%.0fppm`
    pub format: Option<String>,
    /// The control's `defaultIcon`, e.g. `IconsFilled/login-key.svg`
    pub icon: Option<String>,
    /// The category's `image`, e.g. `IconsFilled/door-open.svg`
    pub cat_icon: Option<String>,
}

// ── LoxClient ─────────────────────────────────────────────────────────────────

pub struct LoxClient {
    conn: Connection,
    pub client: Client,
    /// Name → UUID shortcuts, checked before the structure (`with_aliases`)
    aliases: HashMap<String, String>,
    structure: Option<Value>,
}

impl LoxClient {
    /// Build a redirect policy that blocks cross-origin redirects.
    /// Loxone Miniserver Gen 2 redirects local HTTP requests to a cloud
    /// DynDNS URL (e.g. `https://{IP}.{serial}.dyndns.loxonecloud.com`).
    /// Following such redirects causes 401 errors because credentials
    /// are for the local Miniserver, not the cloud gateway.
    pub fn same_origin_redirect_policy(host: &str) -> reqwest::redirect::Policy {
        // Extract the host string from the configured URL.  Try parsing as a
        // full URL first; fall back to treating bare hostnames/IPs by
        // prepending a dummy scheme so Url::parse succeeds.
        let configured_host = reqwest::Url::parse(host)
            .or_else(|_| reqwest::Url::parse(&format!("http://{}", host)))
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
            .unwrap_or_default();
        reqwest::redirect::Policy::custom(move |attempt| {
            if configured_host.is_empty() {
                // Could not determine configured host — allow redirect
                return attempt.follow();
            }
            let target_host = attempt.url().host_str().unwrap_or_default().to_string();
            if target_host != configured_host {
                return attempt.error(std::io::Error::other(format!(
                    "Blocked cross-origin redirect from {} to {}. \
                     If using a local IP, ensure your config uses https:// \
                     or check your Miniserver's network settings.",
                    configured_host, target_host
                )));
            }
            attempt.follow()
        })
    }

    pub fn new(conn: Connection) -> Result<Self> {
        let verify_ssl = conn.verify_ssl;
        let redirect_policy = Self::same_origin_redirect_policy(&conn.host);
        let lox = Self {
            client: Client::builder()
                .user_agent(USER_AGENT)
                .danger_accept_invalid_certs(!verify_ssl)
                .redirect(redirect_policy)
                .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
                .build()
                .context("failed to build HTTP client")?,
            conn,
            aliases: HashMap::new(),
            structure: None,
        };

        if !verify_ssl && verbose() >= 1 {
            eprintln!("warning: TLS certificate verification is disabled (verify_ssl not set)");
        }

        Ok(lox)
    }

    /// Name → UUID aliases that [`Self::resolve`] checks before the structure.
    pub fn with_aliases(mut self, aliases: HashMap<String, String>) -> Self {
        self.aliases = aliases;
        self
    }

    /// The UUID an alias names.
    pub fn alias(&self, name: &str) -> Option<&str> {
        self.aliases.get(name).map(String::as_str)
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn apply_auth(
        &self,
        rb: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        // Always use password for HTTP basic auth.
        // Loxone tokens are for WebSocket auth only — they can't be
        // used as HTTP basic auth passwords.
        rb.basic_auth(&self.conn.user, Some(&self.conn.pass))
    }

    pub fn get_text(&self, path: &str) -> Result<String> {
        self.get_text_with(Retry::Read, path)
    }

    pub(crate) fn get_text_with(&self, policy: Retry, path: &str) -> Result<String> {
        let url = self.conn.url(path);
        if verbose() >= 1 {
            eprintln!("GET {}", redact_url(&url));
        }
        self.retry(policy, || {
            let resp = self.apply_auth(self.client.get(&url)).send()?;
            let status = resp.status();
            if verbose() >= 1 {
                eprintln!(
                    "  → {} {}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("")
                );
            }
            let body = resp.text()?;
            if verbose() >= 2 {
                log_body(&body);
            }
            if !status.is_success() {
                return Err(HttpStatusError {
                    status: status.as_u16(),
                    path: path.to_string(),
                }
                .into());
            }
            Ok(body)
        })
    }

    pub fn get_json(&self, path: &str) -> Result<Value> {
        self.get_json_with(Retry::Read, path)
    }

    fn get_json_with(&self, policy: Retry, path: &str) -> Result<Value> {
        let url = self.conn.url(path);
        if verbose() >= 1 {
            eprintln!("GET {}", redact_url(&url));
        }
        self.retry(policy, || {
            let resp = self.apply_auth(self.client.get(&url)).send()?;
            let status = resp.status();
            if verbose() >= 1 {
                eprintln!(
                    "  → {} {}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("")
                );
            }
            if !status.is_success() {
                return Err(HttpStatusError {
                    status: status.as_u16(),
                    path: path.to_string(),
                }
                .into());
            }
            if verbose() >= 2 {
                let text = resp.text()?;
                log_body(&text);
                serde_json::from_str::<Value>(&text).map_err(Into::into)
            } else {
                resp.json::<Value>().map_err(Into::into)
            }
        })
    }

    pub fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let url = self.conn.url(path);
        if verbose() >= 1 {
            eprintln!("GET {}", redact_url(&url));
        }
        self.request_with_retry(|| {
            let resp = self.apply_auth(self.client.get(&url)).send()?;
            let status = resp.status();
            if verbose() >= 1 {
                eprintln!(
                    "  → {} {}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("")
                );
            }
            let bytes = resp.bytes()?.to_vec();
            if verbose() >= 2 {
                eprintln!("  body: <{} bytes>", bytes.len());
            }
            if !status.is_success() {
                return Err(HttpStatusError {
                    status: status.as_u16(),
                    path: path.to_string(),
                }
                .into());
            }
            Ok(bytes)
        })
    }

    /// One GET that changes something on the Miniserver: no retry (a retry
    /// could apply the change twice), and the answer comes back whatever its
    /// status, for the caller to interpret. `mask` is (secret part of the
    /// path, what to show instead): it keeps a secret out of the -v/-vv log,
    /// the returned answer (which may echo the path) and errors.
    pub fn get_once(&self, path: &str, mask: Option<(&str, &str)>) -> Result<(u16, String)> {
        let hide = |s: &str| match mask {
            Some((secret, shown)) => s.replace(secret, shown),
            None => s.to_string(),
        };
        let url = self.conn.url(path);
        if verbose() >= 1 {
            eprintln!("GET {}", redact_url(&hide(&url)));
        }
        // reqwest's errors name the URL; a masked path must not reappear there
        let resp = self
            .apply_auth(self.client.get(&url))
            .send()
            .map_err(|e| e.without_url())?;
        let status = resp.status();
        if verbose() >= 1 {
            eprintln!(
                "  → {} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("")
            );
        }
        let body = hide(&resp.text().map_err(|e| e.without_url())?);
        if verbose() >= 2 {
            log_body(&body);
        }
        Ok((status.as_u16(), body))
    }

    /// Retry a read up to 3 times with exponential backoff on network errors.
    /// Does not retry on 4xx client errors (only on connection/timeout/5xx).
    fn request_with_retry<T, F>(&self, f: F) -> Result<T>
    where
        F: Fn() -> Result<T>,
    {
        self.retry(Retry::Read, f)
    }

    fn retry<T, F>(&self, policy: Retry, f: F) -> Result<T>
    where
        F: Fn() -> Result<T>,
    {
        let delays = RETRY_DELAYS_MS;
        let mut last_err = None;
        for (attempt, delay) in std::iter::once(&0u64).chain(delays.iter()).enumerate() {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(*delay));
            }
            match f() {
                Ok(val) => return Ok(val),
                Err(e) => {
                    if !policy.allows(&e) {
                        return Err(e);
                    }
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.expect("retry loop must execute at least once"))
    }

    pub fn get_structure(&mut self) -> Result<&Value> {
        if self.structure.is_none() {
            self.structure = Some(Self::load_or_fetch_structure(&self.conn, &self.client)?);
        }
        self.structure
            .as_ref()
            .ok_or_else(|| crate::error::err!("structure not loaded"))
    }

    /// The structure file: from `conn.structure_cache` while it is younger
    /// than 24 hours, else fetched (and cached).
    pub fn load_or_fetch_structure(conn: &Connection, client: &Client) -> Result<Value> {
        if let Some(cache) = &conn.structure_cache
            && let Ok(meta) = cache.metadata()
            && let Ok(modified) = meta.modified()
        {
            let age = std::time::SystemTime::now()
                .duration_since(modified)
                .unwrap_or_default();
            if age.as_secs() < CACHE_TTL_SECS
                && let Ok(data) = fs::read_to_string(cache)
                && let Ok(v) = serde_json::from_str::<Value>(&data)
            {
                if verbose() >= 1 {
                    eprintln!(
                        "Using cached structure ({} bytes, age {}s)",
                        data.len(),
                        age.as_secs()
                    );
                }
                return Ok(v);
            }
        }
        let url = conn.url("/data/LoxApp3.json");
        if verbose() >= 1 {
            eprintln!("GET {}", redact_url(&url));
        }
        let resp = client
            .get(&url)
            .basic_auth(&conn.user, Some(&conn.pass))
            .send()?;
        if verbose() >= 1 {
            eprintln!(
                "  → {} {}",
                resp.status().as_u16(),
                resp.status().canonical_reason().unwrap_or("")
            );
        }
        let resp = resp.error_for_status()?.bytes()?;
        if verbose() >= 2 {
            eprintln!("  body: <{} bytes>", resp.len());
        }
        let v: Value = serde_json::from_slice(&resp)?;
        if let Some(cache) = &conn.structure_cache {
            // The structure names every room, control and user of the house.
            if let Some(parent) = cache.parent() {
                let _ = crate::private_file::create_dir_all(parent);
            }
            let _ = crate::private_file::write(cache, &resp);
        }
        Ok(v)
    }

    /// Send `cmd` to the control `uuid`. Sent once: after a lost answer the
    /// Miniserver may have run the command, and a second `pulse` or `toggle`
    /// would undo it. Only a failed connect is retried.
    pub fn send_cmd(&self, uuid: &str, cmd: &str) -> Result<Value> {
        validate_uuid(uuid)?;
        validate_cmd(cmd)?;
        self.get_json_with(Retry::Command, &format!("/jdev/sps/io/{}/{}", uuid, cmd))
    }

    /// Send `cmd` to the control `uuid` as a secured command. `hash` is the
    /// visualization password hashed with a salt from the Miniserver
    /// ([`crate::token::visu_hash`]).
    pub fn send_secured_cmd(&self, hash: &str, uuid: &str, cmd: &str) -> Result<Value> {
        if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("Invalid hash for a secured command: it must be hex digits");
        }
        validate_uuid(uuid)?;
        validate_cmd(cmd)?;
        self.get_json_with(
            Retry::Command,
            &format!("/jdev/sps/ios/{}/{}/{}", hash, uuid, cmd),
        )
    }

    /// Drop the cached structure file and fetch it again.
    pub fn refresh_structure(&mut self) -> Result<&Value> {
        if let Some(cache) = &self.conn.structure_cache {
            let _ = fs::remove_file(cache);
        }
        self.structure = None;
        self.get_structure()
    }

    pub fn get_all(&self, uuid: &str) -> Result<String> {
        validate_uuid(uuid)?;
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
        // category name and icon: names are user-chosen, icons are language-independent
        let mut cats: HashMap<String, (String, Option<String>)> = HashMap::new();
        if let Some(map) = structure.get("cats").and_then(|c| c.as_object()) {
            for (uuid, cat) in map {
                if let Some(name) = cat.get("name").and_then(|n| n.as_str()) {
                    let image = cat.get("image").and_then(|i| i.as_str()).map(String::from);
                    cats.insert(uuid.clone(), (name.to_string(), image));
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
                let (cat, cat_icon) = match cats.get(&cat_uuid) {
                    Some((name, image)) => (Some(name.clone()), image.clone()),
                    None => (None, None),
                };
                let format = ctrl
                    .pointer("/details/format")
                    .and_then(|f| f.as_str())
                    .map(String::from);
                let icon = ctrl
                    .get("defaultIcon")
                    .and_then(|i| i.as_str())
                    .map(String::from);
                let is_favorite = ctrl
                    .get("isFavorite")
                    .and_then(|f| f.as_bool())
                    .unwrap_or(false);
                let is_secured = ctrl
                    .get("isSecured")
                    .and_then(|f| f.as_bool())
                    .unwrap_or(false);
                if let Some(tf) = type_filter
                    && !typ.to_lowercase().contains(&tf.to_lowercase())
                {
                    continue;
                }
                if let Some(rf) = room_filter
                    && !room
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&rf.to_lowercase())
                {
                    continue;
                }
                if let Some(cf) = cat_filter
                    && !cat
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&cf.to_lowercase())
                {
                    continue;
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
                    format,
                    icon,
                    cat_icon,
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
        if let Some(uuid) = self.aliases.get(name_or_uuid) {
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
        // an exact name wins over names that merely contain it ("Zentral" vs "Jalousie EG Zentral")
        let exact: Vec<&&Control> = matches
            .iter()
            .filter(|c| c.name.to_lowercase() == name_part.to_lowercase())
            .collect();
        if matches.len() > 1 && exact.len() == 1 {
            return Ok(exact[0].uuid.clone());
        }
        match matches.len() {
            0 => bail!("No control matching '{}'", name_or_uuid),
            1 => Ok(matches[0].uuid.clone()),
            _ => {
                // candidates go into the error, not stderr: the TUI resolves too
                let list: Vec<String> = matches
                    .iter()
                    .map(|c| {
                        format!(
                            "  {:40} [{}]  {}",
                            c.name,
                            c.room.as_deref().unwrap_or("-"),
                            c.uuid
                        )
                    })
                    .collect();
                bail!(
                    "Ambiguous: '{}'. Use [Room] qualifier or --room flag.\n{}",
                    name_or_uuid,
                    list.join("\n")
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

    /// The `autopilot` entry of the structure file.
    pub fn autopilot(&mut self) -> Result<Autopilot> {
        let structure = self.get_structure()?;
        let (uuid, entry) = structure
            .get("autopilot")
            .and_then(|a| a.as_object())
            .and_then(|m| m.iter().next())
            .context(
                "The structure file has no autopilot entry: this Miniserver has no Automatic Designer",
            )?;
        let state = |k: &str| {
            entry
                .get("states")
                .and_then(|s| s.get(k))
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string()
        };
        Ok(Autopilot {
            uuid: uuid.clone(),
            state_changed: state("changed"),
            state_history: state("history"),
        })
    }

    /// Resolve all controls in a room, optionally filtered by type.
    pub fn resolve_all_in_room(
        &mut self,
        room: &str,
        type_filter: Option<&str>,
    ) -> Result<Vec<Control>> {
        let controls = self.list_controls(type_filter, Some(room))?;
        if controls.is_empty() {
            bail!("No controls found in room '{}'", room);
        }
        Ok(controls)
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
                let list: Vec<String> = matches
                    .iter()
                    .map(|c| format!("  {:40} [{}]", c.name, c.room.as_deref().unwrap_or("-")))
                    .collect();
                bail!("Ambiguous: '{}'\n{}", name_or_uuid, list.join("\n"))
            }
        }
    }
}

/// A control UUID, optionally with one sub-control suffix
/// (`<uuid>/masterValue`, `<uuid>/masterColor` — LightControllerV2 masters).
pub fn is_uuid(s: &str) -> bool {
    let (base, sub) = match s.split_once('/') {
        Some((b, sub)) => (b, Some(sub)),
        None => (s, None),
    };
    base.len() > 20
        && base.contains('-')
        && base.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && sub.is_none_or(|sub| !sub.is_empty() && sub.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// Which failed requests are sent again.
#[derive(Clone, Copy)]
pub(crate) enum Retry {
    /// A read: anything but a 4xx answer.
    Read,
    /// A command that changes something: only when it never reached the
    /// Miniserver. After a timeout or a 5xx it may have run.
    Command,
}

impl Retry {
    fn allows(self, e: &Error) -> bool {
        match self {
            Retry::Read => !e
                .http_status()
                .is_some_and(|s| (400..500).contains(&s.status)),
            Retry::Command => matches!(e, Error::Http(e) if e.is_connect()),
        }
    }
}

/// Validate that a command stays inside its control's URL path.
///
/// The HTTP client resolves `.` and `..` path segments before it sends, so
/// `../<other uuid>/on` would reach another control and `../../../sys/reboot`
/// another endpoint. `?` and `#` would cut the command short.
pub fn validate_cmd(cmd: &str) -> Result<()> {
    if cmd.is_empty() {
        bail!("Empty command");
    }
    if cmd
        .chars()
        .any(|c| c.is_control() || matches!(c, '\\' | '?' | '#'))
    {
        bail!(
            "Invalid characters in command '{}': no '\\', '?', '#' or control characters",
            cmd.escape_debug()
        );
    }
    // `%2e` counts as a dot for the HTTP client; the Miniserver may read
    // `%2f` as a slash.
    let decoded = percent_decode(cmd);
    if decoded
        .split(['/', '\\'])
        .any(|segment| segment == "." || segment == "..")
    {
        bail!("Invalid command '{}': no '.' or '..' path segments", cmd);
    }
    Ok(())
}

/// `s` with its `%XX` escapes resolved; malformed escapes stay as they are.
fn percent_decode(s: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(hi), Some(lo)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            )
        {
            out.push(hi << 4 | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Validate that a UUID is safe to embed in a URL path.
/// Rejects path traversal attempts and non-hex characters; allows a single
/// alphanumeric sub-control suffix (`<uuid>/masterColor`).
pub fn validate_uuid(uuid: &str) -> Result<()> {
    if uuid.is_empty() || uuid.len() > 80 {
        bail!("Invalid UUID length: '{}'", uuid);
    }
    let (base, sub) = match uuid.split_once('/') {
        Some((b, sub)) => (b, Some(sub)),
        None => (uuid, None),
    };
    if uuid.contains("..") || uuid.contains('\\') || uuid.contains('%') {
        bail!("Invalid characters in UUID: '{}'", uuid);
    }
    if sub.is_some_and(|sub| sub.is_empty() || !sub.chars().all(|c| c.is_ascii_alphanumeric())) {
        bail!("Invalid characters in UUID: '{}'", uuid);
    }
    if base.is_empty() || !base.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        bail!("UUID contains non-hex characters: '{}'", uuid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn mock_config(server: &MockServer) -> Connection {
        Connection::new(server.base_url(), "test", "test")
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
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
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
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
        let lights = client.list_controls(Some("light"), None).unwrap();
        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].name, "Light");
    }

    // ── send_cmd ──────────────────────────────────────────────────────────────

    #[test]
    fn test_send_cmd_success() {
        let server = MockServer::start();
        let test_uuid = "0f1e2d3c-a1b2-c3d4-e5f6a7b8c9d0e1f2";
        let _s = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200)
                .json_body(serde_json::json!({ "rooms": {}, "controls": {} }));
        });
        let _c = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/jdev/sps/io/{}/on", test_uuid));
            then.status(200).json_body(serde_json::json!({
                "LL": { "Code": "200", "value": "1" }
            }));
        });
        let client = LoxClient::new(mock_config(&server)).unwrap();
        let resp = client.send_cmd(test_uuid, "on").unwrap();
        assert_eq!(
            resp.pointer("/LL/Code").and_then(|v| v.as_str()),
            Some("200")
        );
    }

    #[test]
    fn test_send_cmd_rejects_path_traversal() {
        let server = MockServer::start();
        let client = LoxClient::new(mock_config(&server)).unwrap();
        assert!(client.send_cmd("../../../etc/passwd", "on").is_err());
        assert!(client.send_cmd("uuid-with-slash/attack", "on").is_err());
        assert!(client.send_cmd("", "on").is_err());
    }

    #[test]
    fn test_validate_uuid() {
        assert!(validate_uuid("0f1e2d3c-a1b2-c3d4-e5f6a7b8c9d0e1f2").is_ok());
        assert!(validate_uuid("1fbc668c-005c-7471-ffffed57184a04d2").is_ok());
        assert!(validate_uuid("").is_err());
        assert!(validate_uuid("../attack").is_err());
        assert!(validate_uuid("uuid/with/slashes").is_err());
        assert!(validate_uuid("has spaces in it-----").is_err());
        // LightControllerV2 master sub-controls
        assert!(validate_uuid("2034c059-0222-5381-ffffed57184a04d2/masterColor").is_ok());
        assert!(validate_uuid("2034c059-0222-5381-ffffed57184a04d2/masterValue").is_ok());
        assert!(validate_uuid("2034c059-0222-5381-ffffed57184a04d2/a/b").is_err());
        assert!(validate_uuid("2034c059-0222-5381-ffffed57184a04d2/").is_err());
        assert!(validate_uuid("2034c059-0222-5381-ffffed57184a04d2/..").is_err());
        assert!(validate_uuid("/masterColor").is_err());
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
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
        let _ = client.list_controls(None, None).unwrap();
        let _ = client.list_controls(None, None).unwrap();
        mock.assert_calls(1);
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
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
        let uuid = client.resolve_with_room("Kitchen Light", None).unwrap();
        assert_eq!(uuid, "uuid-abc");
    }

    #[test]
    fn test_resolve_uuid_passthrough() {
        let server = MockServer::start();
        // No mock needed — UUID bypasses HTTP lookup
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
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
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
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
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
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
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
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
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
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
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
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
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
        let globals = client.get_global_states().unwrap();
        assert_eq!(globals.len(), 2);
        assert!(globals.iter().any(|(n, _)| n == "operatingMode"));
        assert!(globals.iter().any(|(n, _)| n == "sunrise"));
    }

    #[test]
    fn autopilot_entry_from_structure() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": {}, "controls": {},
                "autopilot": {
                    "0a000000-0000-0000-ffff000000000010": {
                        "name": "Automatic rules",
                        "uuidAction": "0a000000-0000-0000-ffff000000000010",
                        "states": {
                            "changed": "0a000000-0000-0000-ffff000000000011",
                            "history": "0a000000-0000-0000-ffff000000000012"
                        }
                    }
                }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
        let ap = client.autopilot().unwrap();
        assert_eq!(ap.uuid, "0a000000-0000-0000-ffff000000000010");
        assert_eq!(ap.state_changed, "0a000000-0000-0000-ffff000000000011");
        assert_eq!(ap.state_history, "0a000000-0000-0000-ffff000000000012");
    }

    #[test]
    fn autopilot_entry_missing() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200)
                .json_body(serde_json::json!({"rooms": {}, "controls": {}}));
        });
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
        let e = client.autopilot().unwrap_err().to_string();
        assert!(e.contains("no autopilot entry"), "{}", e);
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
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
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
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
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
        let aliases = HashMap::from([("mylight".into(), "alias-uuid-1234567890".into())]);
        let mut client = LoxClient::new(mock_config(&server))
            .unwrap()
            .with_aliases(aliases);
        let uuid = client.resolve_with_room("mylight", None).unwrap();
        assert_eq!(uuid, "alias-uuid-1234567890");
    }

    // ── resolve_all_in_room ─────────────────────────────────────────────────

    #[test]
    fn test_resolve_all_in_room() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": { "r1": { "name": "Kitchen" }, "r2": { "name": "Bedroom" } },
                "controls": {
                    "u1": { "name": "Light 1", "type": "Switch", "room": "r1" },
                    "u2": { "name": "Blind 1", "type": "Jalousie", "room": "r1" },
                    "u3": { "name": "Light 2", "type": "Switch", "room": "r2" }
                }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
        let controls = client.resolve_all_in_room("Kitchen", None).unwrap();
        assert_eq!(controls.len(), 2);
        assert!(
            controls
                .iter()
                .all(|c| c.room.as_deref() == Some("Kitchen"))
        );
    }

    #[test]
    fn test_resolve_all_in_room_with_type_filter() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": { "r1": { "name": "Kitchen" } },
                "controls": {
                    "u1": { "name": "Light 1", "type": "Switch", "room": "r1" },
                    "u2": { "name": "Blind 1", "type": "Jalousie", "room": "r1" }
                }
            }));
        });
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
        let controls = client
            .resolve_all_in_room("Kitchen", Some("Switch"))
            .unwrap();
        assert_eq!(controls.len(), 1);
        assert_eq!(controls[0].typ, "Switch");
    }

    #[test]
    fn test_resolve_all_in_room_empty_returns_err() {
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(serde_json::json!({
                "rooms": {}, "controls": {}
            }));
        });
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
        assert!(client.resolve_all_in_room("NonExistent", None).is_err());
    }

    // ── is_uuid (strict) ────────────────────────────────────────────────────

    #[test]
    fn test_is_uuid_strict() {
        // Valid Loxone UUIDs
        assert!(is_uuid("1fbc668c-005c-7471-ffffed57184a04d2"));
        assert!(is_uuid("0f1e2d3c-a1b2-c3d4-e5f6a7b8c9d0e1f2"));
        // Too short
        assert!(!is_uuid("short-str"));
        // Contains non-hex characters
        assert!(!is_uuid("1fbc668c-005c-7471-xxxxed57184a04d2"));
        // No dashes
        assert!(!is_uuid("1fbc668c005c7471ffffed57184a04d2"));
        // Human-readable strings should NOT match
        assert!(!is_uuid("Licht Wohnzimmer"));
        assert!(!is_uuid("Kitchen Light [Room]"));
        // Sub-control suffix
        assert!(is_uuid("2034c059-0222-5381-ffffed57184a04d2/masterColor"));
        assert!(!is_uuid("2034c059-0222-5381-ffffed57184a04d2/a/b"));
        assert!(!is_uuid("Licht/masterColor"));
    }

    // ── retry logic ─────────────────────────────────────────────────────────

    const UUID: &str = "0f1e2d3c-a1b2-c3d4-e5f6a7b8c9d0e1f2";

    #[test]
    fn a_read_is_retried_after_a_server_error() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/jdev/cfg/version");
            then.status(503);
        });
        let client = LoxClient::new(mock_config(&server)).unwrap();
        let err = client.get_json("/jdev/cfg/version").unwrap_err();
        assert_eq!(err.http_status().map(|s| s.status), Some(503));
        mock.assert_calls(1 + RETRY_DELAYS_MS.len());
    }

    #[test]
    fn a_read_is_not_retried_after_a_client_error() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/jdev/cfg/version");
            then.status(404);
        });
        let client = LoxClient::new(mock_config(&server)).unwrap();
        assert!(client.get_json("/jdev/cfg/version").is_err());
        mock.assert_calls(1);
    }

    // A second `pulse` after a lost answer would switch the output twice.
    #[test]
    fn a_command_is_sent_once_even_when_the_answer_is_an_error() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/jdev/sps/io/{}/pulse", UUID));
            then.status(500);
        });
        let client = LoxClient::new(mock_config(&server)).unwrap();
        let err = client.send_cmd(UUID, "pulse").unwrap_err();
        assert_eq!(err.http_status().map(|s| s.status), Some(500));
        mock.assert_calls(1);
    }

    #[test]
    fn a_command_is_sent_once_when_the_answer_is_not_json() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/jdev/sps/io/{}/pulse", UUID));
            then.status(200).body("<html>busy</html>");
        });
        let client = LoxClient::new(mock_config(&server)).unwrap();
        assert!(client.send_cmd(UUID, "pulse").is_err());
        mock.assert_calls(1);
    }

    #[test]
    fn a_command_that_cannot_connect_is_retried() {
        // Nothing listens on the port, so no attempt reaches a Miniserver.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let conn = Connection::new(format!("http://127.0.0.1:{port}"), "test", "test");
        let client = LoxClient::new(conn).unwrap();
        let started = std::time::Instant::now();
        let err = client.send_cmd(UUID, "on").unwrap_err();
        assert!(
            matches!(err, Error::Http(ref e) if e.is_connect()),
            "{err:?}"
        );
        let backoff: u64 = RETRY_DELAYS_MS.iter().sum();
        assert!(started.elapsed() >= Duration::from_millis(backoff));
    }

    // ── command validation ──────────────────────────────────────────────────

    #[test]
    fn commands_stay_inside_their_control() {
        for ok in [
            "on",
            "FullUp",
            "manualPosition/50",
            "override/22.5/60",
            "hsv(120,50,80)",
            "temp(80,2700)",
            "Guten%20Morgen",
            "changeTo/778",
            "1.5",
            "-3",
            "..5",
            "a..b",
        ] {
            assert!(validate_cmd(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "..",
            ".",
            "../1fbc668c-005c-7471-ffffed57184a04d2/open",
            "on/..",
            "on/../../../sys/reboot",
            "on/./x",
            "%2e%2e/x",
            "%2E./x",
            ".%2e/x",
            "..%2fx",
            "..%5cx",
            "..\\x",
            "on?x=1",
            "on#x",
            "o\tn",
            "on\r\nHost: x",
        ] {
            assert!(validate_cmd(bad).is_err(), "{bad:?}");
        }
    }

    // Why validate_cmd exists: the request for `<uuid>/../<other>/open` goes
    // to `<other>/open`.
    #[test]
    fn the_http_client_resolves_dot_segments() {
        let server = MockServer::start();
        let other = "1fbc668c-005c-7471-ffffed57184a04d2";
        let mock = server.mock(|when, then| {
            when.method(GET).path(format!("/jdev/sps/io/{other}/open"));
            then.status(200).json_body(serde_json::json!({"LL": {}}));
        });
        let client = LoxClient::new(mock_config(&server)).unwrap();
        for dots in ["..", "%2e%2e"] {
            client
                .get_json(&format!("/jdev/sps/io/{UUID}/{dots}/{other}/open"))
                .unwrap();
        }
        mock.assert_calls(2);
    }

    #[test]
    fn a_command_with_dot_segments_is_never_sent() {
        let server = MockServer::start();
        let other = "1fbc668c-005c-7471-ffffed57184a04d2";
        let reached = server.mock(|when, then| {
            when.method(GET);
            then.status(200)
                .json_body(serde_json::json!({"LL": {"Code": "200", "value": "1"}}));
        });
        let client = LoxClient::new(mock_config(&server)).unwrap();
        for cmd in [
            format!("../{other}/open"),
            "../../../sys/reboot".to_string(),
            format!("%2e%2e/{other}/open"),
        ] {
            assert!(client.send_cmd(UUID, &cmd).is_err(), "{cmd}");
            assert!(
                client.send_secured_cmd("abc123", UUID, &cmd).is_err(),
                "{cmd}"
            );
        }
        reached.assert_calls(0);
    }

    #[test]
    fn a_secured_command_carries_the_hash_and_needs_a_hex_one() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/jdev/sps/ios/abc123/{}/open", UUID));
            then.status(200)
                .json_body(serde_json::json!({"LL": {"Code": "200", "value": "1"}}));
        });
        let client = LoxClient::new(mock_config(&server)).unwrap();
        client.send_secured_cmd("abc123", UUID, "open").unwrap();
        mock.assert_calls(1);
        assert!(client.send_secured_cmd("../x", UUID, "open").is_err());
        assert!(client.send_secured_cmd("", UUID, "open").is_err());
    }

    #[test]
    fn refresh_structure_drops_the_cache_and_fetches() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("structure.json");
        fs::write(&cache, r#"{"controls":{},"msInfo":{"msName":"old"}}"#).unwrap();
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200)
                .json_body(serde_json::json!({"controls": {}, "msInfo": {"msName": "new"}}));
        });
        let mut conn = mock_config(&server);
        conn.structure_cache = Some(cache.clone());
        let mut client = LoxClient::new(conn).unwrap();
        let name = |v: &Value| {
            v.pointer("/msInfo/msName")
                .and_then(Value::as_str)
                .map(String::from)
        };
        assert_eq!(
            name(client.get_structure().unwrap()).as_deref(),
            Some("old")
        );
        assert_eq!(
            name(client.refresh_structure().unwrap()).as_deref(),
            Some("new")
        );
        mock.assert_calls(1);
        assert!(fs::read_to_string(&cache).unwrap().contains("new"));
    }

    // ── cross-origin redirect blocking ──────────────────────────────────────

    #[test]
    fn test_cross_origin_redirect_blocked() {
        // Simulate Miniserver Gen 2 redirecting to cloud DynDNS URL
        let server = MockServer::start();
        let _m = server.mock(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(302).header(
                "Location",
                "https://192-168-10-222.504f94d08fa8.dyndns.loxonecloud.com/data/LoxApp3.json",
            );
        });
        let mut client = LoxClient::new(mock_config(&server)).unwrap();
        let result = client.get_structure();
        assert!(result.is_err(), "Cross-origin redirect should be blocked");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Blocked cross-origin redirect") || err_msg.contains("redirect"),
            "Error should mention redirect blocking, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_same_origin_redirect_allowed() {
        let server = MockServer::start();
        // Redirect to same host (different path) should be followed
        let _redirect = server.mock(|when, then| {
            when.method(GET).path("/old-path");
            then.status(302)
                .header("Location", format!("{}/new-path", server.base_url()));
        });
        let _target = server.mock(|when, then| {
            when.method(GET).path("/new-path");
            then.status(200).body("ok");
        });
        let client = LoxClient::new(mock_config(&server)).unwrap();
        let result = client.get_text("/old-path").unwrap();
        assert_eq!(result, "ok");
    }

    // ── redact_url ──────────────────────────────────────────────────────────

    #[test]
    fn test_redact_url_plain() {
        let url = "https://192.168.1.5/data/LoxApp3.json";
        assert_eq!(redact_url(url), url);
    }

    #[test]
    fn test_redact_url_basic_auth_password() {
        assert_eq!(
            redact_url("https://admin:secret@192.168.1.5/data/LoxApp3.json"),
            "https://admin:***@192.168.1.5/data/LoxApp3.json"
        );
    }

    #[test]
    fn test_redact_url_query_token() {
        assert_eq!(
            redact_url("https://host/path?token=abc123&other=ok"),
            "https://host/path?token=***&other=ok"
        );
    }

    #[test]
    fn test_redact_url_query_password() {
        assert_eq!(
            redact_url("https://host/path?pass=secret"),
            "https://host/path?pass=***"
        );
    }

    #[test]
    fn test_redact_url_duplicate_params() {
        assert_eq!(
            redact_url("https://host/path?token=abc&other=ok&token=def"),
            "https://host/path?token=***&other=ok&token=***"
        );
    }
}
