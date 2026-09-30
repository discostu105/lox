//! Loxone token authentication via WS + RSA/AES command encryption

use anyhow::{Result, bail};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use rand::RngCore;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey, pkcs8::DecodePublicKey};
use sha1::Sha1 as Sha1Digest;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

use crate::client::LOXONE_EPOCH_SECS;
use crate::config::Config;
use crate::ws::LoxWsClient;

const TOKEN_EXPIRY_MARGIN_SECS: u64 = 300;
const WS_TIMEOUT_SECS: u64 = 5;
const HTTP_TIMEOUT_SECS: u64 = 10;
/// gettoken permission: 4 = long-lived "app" token (2 = short-lived web token).
const TOKEN_PERMISSION_APP: u8 = 4;
const TOKEN_CLIENT_INFO: &str = "lox-cli";

type HmacSha1 = Hmac<Sha1Digest>;
type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenStore {
    pub token: String,
    pub key: String,
    pub valid_until: u64,
}

impl TokenStore {
    /// Token path for a specific config (context-aware).
    pub fn path_for(cfg: &Config) -> std::path::PathBuf {
        cfg.token_path()
    }
    /// Load token for a specific config (context-aware).
    pub fn load_for(cfg: &Config) -> Option<Self> {
        serde_json::from_str(&std::fs::read_to_string(Self::path_for(cfg)).ok()?).ok()
    }
    /// Save token for a specific config (context-aware).
    pub fn save_for(&self, cfg: &Config) -> Result<()> {
        let path = Self::path_for(cfg);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.write_to(&path)
    }
    fn write_to(&self, path: &std::path::Path) -> Result<()> {
        // The token grants Miniserver access for weeks: keep it owner-only.
        let json = serde_json::to_string_pretty(self)?;
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)?;
            // mode() only applies on create; tighten files written by older versions
            f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            f.write_all(json.as_bytes())?;
        }
        #[cfg(not(unix))]
        std::fs::write(path, json)?;
        Ok(())
    }
    pub fn is_valid(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.valid_until > now + TOKEN_EXPIRY_MARGIN_SECS
    }
}

#[allow(dead_code)]
fn recv_text(
    msg: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
) -> Option<String> {
    match msg? {
        Ok(Message::Text(t)) => Some(t.to_string()),
        _ => None,
    }
}

/// Hash a token using HMAC-SHA256 with the token key, as required by
/// checktoken/refreshtoken/killtoken endpoints.
pub fn hash_token(token: &str, key: &str) -> String {
    let key_bytes = hex::decode(key).unwrap_or_default();
    if key_bytes.is_empty() {
        // Fallback: use token directly if key is not hex
        return token.to_string();
    }
    let mut mac = HmacSha256::new_from_slice(&key_bytes).unwrap();
    mac.update(token.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// The Miniserver WebSocket stream type used by the auth handshake.
pub type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Hash algorithm the Miniserver uses for a user, as reported by `getkey2`.
/// SHA-256 since firmware 10.4; older firmware (and a missing field) is SHA-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlg {
    Sha1,
    Sha256,
}

impl HashAlg {
    fn from_loxone(s: Option<&str>) -> Self {
        match s {
            Some(a) if a.eq_ignore_ascii_case("SHA256") => Self::Sha256,
            _ => Self::Sha1,
        }
    }

    fn digest_upper_hex(self, data: &[u8]) -> String {
        match self {
            Self::Sha1 => format!("{:X}", Sha1Digest::digest(data)),
            Self::Sha256 => format!("{:X}", Sha256::digest(data)),
        }
    }

    fn hmac_hex(self, key: &[u8], data: &[u8]) -> String {
        // HMAC accepts keys of any length, so new_from_slice cannot fail.
        match self {
            Self::Sha1 => {
                let mut mac = HmacSha1::new_from_slice(key).expect("HMAC key");
                mac.update(data);
                hex::encode(mac.finalize().into_bytes())
            }
            Self::Sha256 => {
                let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key");
                mac.update(data);
                hex::encode(mac.finalize().into_bytes())
            }
        }
    }
}

/// One-time key material returned by `jdev/sys/getkey2/{user}`.
struct UserSalt {
    key: Vec<u8>,
    salt: String,
    alg: HashAlg,
}

impl UserSalt {
    fn parse(v: &serde_json::Value) -> Result<Self> {
        let key_hex = v.get("key").and_then(|k| k.as_str()).unwrap_or("");
        Ok(Self {
            key: hex::decode(key_hex).map_err(|e| anyhow::anyhow!("getkey2 key: {}", e))?,
            salt: v
                .get("salt")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            alg: HashAlg::from_loxone(v.get("hashAlg").and_then(|a| a.as_str())),
        })
    }
}

/// Credential hash for `gettoken`:
/// `HMAC-alg(key, user + ":" + UPPER(alg(password + ":" + salt)))`, hex.
/// The salt is used as the raw hex string (not hex-decoded).
fn credential_hash(user: &str, pass: &str, s: &UserSalt) -> String {
    let pw_hash = s
        .alg
        .digest_upper_hex(format!("{}:{}", pass, s.salt).as_bytes());
    s.alg
        .hmac_hex(&s.key, format!("{}:{}", user, pw_hash).as_bytes())
}

/// Token hash for `authwithtoken`: `HMAC-alg(key, token)`, hex.
fn token_auth_hash(token: &str, s: &UserSalt) -> String {
    s.alg.hmac_hex(&s.key, token.as_bytes())
}

/// Fetch the Miniserver's RSA public key over HTTP (not available on the WS).
async fn fetch_public_key(cfg: &Config) -> Result<RsaPublicKey> {
    let cfg2 = cfg.clone();
    let pem: String = tokio::task::spawn_blocking(move || -> Result<String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(crate::client::USER_AGENT)
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .danger_accept_invalid_certs(!cfg2.verify_ssl.unwrap_or(false))
            .build()?;
        let resp: serde_json::Value = client
            .get(format!("{}/jdev/sys/getPublicKey", cfg2.host))
            .basic_auth(&cfg2.user, Some(&cfg2.pass))
            .send()?
            .error_for_status()?
            .json()?;
        Ok(resp
            .pointer("/LL/value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("no public key"))?
            .to_string())
    })
    .await??;
    parse_public_key(&pem)
}

/// Loxone returns SubjectPublicKeyInfo mis-labeled as "CERTIFICATE" without
/// line breaks: strip headers, rewrap base64 at 64 chars, relabel as PUBLIC KEY.
fn parse_public_key(pem_in: &str) -> Result<RsaPublicKey> {
    let b64: String = pem_in
        .replace("-----BEGIN CERTIFICATE-----", "")
        .replace("-----END CERTIFICATE-----", "")
        .replace("-----BEGIN PUBLIC KEY-----", "")
        .replace("-----END PUBLIC KEY-----", "")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let mut pem = String::from("-----BEGIN PUBLIC KEY-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        pem.push('\n');
    }
    pem.push_str("-----END PUBLIC KEY-----");
    RsaPublicKey::from_public_key_pem(&pem).map_err(|e| anyhow::anyhow!("RSA parse: {}", e))
}

/// Send a random AES-256 session key (RSA-PKCS1v15 encrypted) via `keyexchange`.
async fn key_exchange(ws: &mut WsStream, pub_key: &RsaPublicKey) -> Result<()> {
    let mut aes_key = [0u8; 32];
    let mut aes_iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut aes_key);
    rand::thread_rng().fill_bytes(&mut aes_iv);
    // Loxone expects key:iv as HEX strings (not base64)
    let key_info = format!("{}:{}", hex::encode(aes_key), hex::encode(aes_iv));
    let enc = pub_key.encrypt(
        &mut rand::thread_rng(),
        Pkcs1v15Encrypt,
        key_info.as_bytes(),
    )?;
    let encrypted_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &enc);
    ws.send(Message::Text(format!(
        "jdev/sys/keyexchange/{}",
        encrypted_b64
    )))
    .await?;
    ws_read_value(ws, "keyexchange").await?;
    Ok(())
}

async fn get_user_salt(ws: &mut WsStream, user: &str) -> Result<UserSalt> {
    ws.send(Message::Text(format!("jdev/sys/getkey2/{}", user)))
        .await?;
    UserSalt::parse(&ws_read_value(ws, "getkey2").await?)
}

/// Convert a Miniserver `validUntil` (seconds since 2009-01-01) to unix seconds.
fn loxone_to_unix(valid_until: u64) -> u64 {
    if valid_until > 1_500_000_000 {
        valid_until
    } else {
        LOXONE_EPOCH_SECS.saturating_add(valid_until)
    }
}

/// Request a new long-lived (permission 4, "app") token with the password.
/// On success the WS session is authenticated as well.
async fn request_token(ws: &mut WsStream, cfg: &Config) -> Result<TokenStore> {
    let salt = get_user_salt(ws, &cfg.user).await?;
    let sig = credential_hash(&cfg.user, &cfg.pass, &salt);
    let client_uuid = uuid::Uuid::new_v4().to_string();
    ws.send(Message::Text(format!(
        "jdev/sys/gettoken/{}/{}/{}/{}/{}",
        sig, cfg.user, TOKEN_PERMISSION_APP, client_uuid, TOKEN_CLIENT_INFO
    )))
    .await?;
    let val = ws_read_value(ws, "gettoken").await?;
    let token = val
        .get("token")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("no token field in gettoken response"))?;
    Ok(TokenStore {
        token: token.to_string(),
        key: val
            .get("key")
            .and_then(|k| k.as_str())
            .unwrap_or("")
            .to_string(),
        valid_until: loxone_to_unix(val.get("validUntil").and_then(|v| v.as_u64()).unwrap_or(0)),
    })
}

/// Authenticate the WS session with an existing token (`authwithtoken`).
/// Returns the token with its refreshed expiry, or `None` if the Miniserver
/// rejected it (revoked, expired, other user).
async fn auth_with_token(
    ws: &mut WsStream,
    cfg: &Config,
    ts: &TokenStore,
) -> Result<Option<TokenStore>> {
    let salt = get_user_salt(ws, &cfg.user).await?;
    let hash = token_auth_hash(&ts.token, &salt);
    ws.send(Message::Text(format!(
        "authwithtoken/{}/{}",
        hash, cfg.user
    )))
    .await?;
    match ws_reply(ws, "authwithtoken").await? {
        WsReply::Ok(val) => {
            let mut ts = ts.clone();
            if let Some(until) = val.get("validUntil").and_then(|v| v.as_u64()) {
                ts.valid_until = loxone_to_unix(until);
            }
            Ok(Some(ts))
        }
        WsReply::Rejected(_) => Ok(None),
    }
}

/// Set once a stored token was rejected in this process, so reconnect loops
/// (TUI, `otel serve`) don't repeat a failing login against the Miniserver.
static TOKEN_REUSE_REJECTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Whether a stored token was rejected earlier in this process (see
/// [`authenticate_ws`]); later sessions then skip straight to `gettoken`.
pub fn token_reuse_rejected() -> bool {
    TOKEN_REUSE_REJECTED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Authenticate a freshly opened WS session.
///
/// Reuses the stored token via `authwithtoken` when it is still valid, and
/// only requests a new token (and stores it) otherwise — so repeated stream
/// sessions don't pile up long-lived tokens on the Miniserver.
pub async fn authenticate_ws(ws: &mut WsStream, cfg: &Config) -> Result<()> {
    let pub_key = fetch_public_key(cfg).await?;
    key_exchange(ws, &pub_key).await?;

    use std::sync::atomic::Ordering;
    if !TOKEN_REUSE_REJECTED.load(Ordering::Relaxed)
        && let Some(ts) = TokenStore::load_for(cfg).filter(|t| t.is_valid())
    {
        match auth_with_token(ws, cfg, &ts).await {
            Ok(Some(refreshed)) => {
                let _ = refreshed.save_for(cfg);
                return Ok(());
            }
            Ok(None) => {
                TOKEN_REUSE_REJECTED.store(true, Ordering::Relaxed);
                if crate::client::verbose() >= 1 {
                    eprintln!("stored token rejected by the Miniserver; requesting a new one");
                }
            }
            // The socket is likely unusable; the caller reconnects and, with
            // the flag set, goes straight to gettoken.
            Err(e) => {
                TOKEN_REUSE_REJECTED.store(true, Ordering::Relaxed);
                return Err(e);
            }
        }
    }

    let ts = request_token(ws, cfg).await?;
    let _ = ts.save_for(cfg);
    Ok(())
}

pub async fn acquire_token(cfg: &Config) -> Result<TokenStore> {
    // 1. Fetch RSA public key via HTTP
    let pub_key = fetch_public_key(cfg).await?;

    // 2. WS connect (retry up to 3 times)
    let ws_client = LoxWsClient::new(cfg.clone());
    let mut ws = None;
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(200 * (1 << attempt))).await;
        }
        match ws_client.connect_raw().await {
            Ok((stream, _)) => {
                ws = Some(stream);
                break;
            }
            Err(e) if attempt == 2 => return Err(e),
            Err(_) => continue,
        }
    }
    let mut ws = ws.ok_or_else(|| anyhow::anyhow!("WS connect failed after retries"))?;

    // 3. Key exchange, then gettoken on the same connection (getkey2 is session-specific)
    key_exchange(&mut ws, &pub_key).await?;
    let ts = request_token(&mut ws, cfg).await?;
    ts.save_for(cfg)?;
    Ok(ts)
}

enum WsReply {
    Ok(serde_json::Value),
    /// Non-200 response code, with the raw message.
    Rejected(String),
}

/// Read WS messages until a command response (`LL.Code`) arrives. Binary
/// frames and code-less text are skipped.
async fn ws_reply(ws: &mut WsStream, label: &str) -> Result<WsReply> {
    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_secs(WS_TIMEOUT_SECS), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_default();
                let code = v
                    .pointer("/LL/Code")
                    .or_else(|| v.pointer("/LL/code"))
                    .and_then(|c| {
                        c.as_str()
                            .map(|s| s.to_string())
                            .or_else(|| c.as_i64().map(|n| n.to_string()))
                    })
                    .unwrap_or_else(|| "0".to_string());
                if code == "200" {
                    let val = v
                        .pointer("/LL/value")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    // Some firmware returns object values as a JSON string
                    if let Some(s) = val.as_str()
                        && let Ok(obj @ serde_json::Value::Object(_)) = serde_json::from_str(s)
                    {
                        return Ok(WsReply::Ok(obj));
                    }
                    return Ok(WsReply::Ok(val));
                }
                if code != "0" {
                    return Ok(WsReply::Rejected(format!(
                        "{} failed ({}): {}",
                        label, code, t
                    )));
                }
            }
            Ok(Some(Ok(Message::Binary(_)))) => continue,
            Ok(Some(Err(e))) => bail!("WS error during {}: {}", label, e),
            _ => bail!("WS timeout during {}", label),
        }
    }
    bail!("{}: no response after 10 messages", label)
}

/// Like [`ws_reply`], but a non-200 response is an error.
async fn ws_read_value(ws: &mut WsStream, label: &str) -> Result<serde_json::Value> {
    match ws_reply(ws, label).await? {
        WsReply::Ok(v) => Ok(v),
        WsReply::Rejected(msg) => bail!(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store(valid_until: u64) -> TokenStore {
        TokenStore {
            token: "test_token".into(),
            key: "test_key".into(),
            valid_until,
        }
    }

    #[test]
    fn test_token_store_valid() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Valid until 1 hour from now (well past the 300s margin)
        let ts = make_store(now + 3600);
        assert!(ts.is_valid());
    }

    #[test]
    fn test_token_store_expired() {
        let ts = make_store(0);
        assert!(!ts.is_valid());
    }

    #[test]
    fn test_hash_token() {
        // Known HMAC-SHA256 test
        let key_hex = hex::encode(b"test-key-1234567890abcdef");
        let hash = hash_token("my-token", &key_hex);
        // Just verify it produces a 64-char hex string (256 bits)
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_hash_token_empty_key_fallback() {
        // Non-hex key should fall back to returning the token
        let hash = hash_token("my-token", "not-hex!");
        assert_eq!(hash, "my-token");
    }

    #[test]
    fn test_token_store_expiring_soon() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Expires in 100s — within the 300s margin
        let ts = make_store(now + 100);
        assert!(!ts.is_valid());
    }

    fn salt(alg: HashAlg) -> UserSalt {
        UserSalt {
            key: hex::decode("41434538393542463839453337453135").unwrap(),
            salt: "31323334".into(),
            alg,
        }
    }

    // Expected values computed independently with Python's hmac/hashlib.
    #[test]
    fn test_credential_hash_sha256() {
        assert_eq!(
            credential_hash("admin", "secret", &salt(HashAlg::Sha256)),
            "ca4a7059de6e82fab652bda644ba05a4fef7014faead6387707b5c57e6bf9a81"
        );
    }

    #[test]
    fn test_credential_hash_sha1_uses_hmac_sha1() {
        assert_eq!(
            credential_hash("admin", "secret", &salt(HashAlg::Sha1)),
            "bc7e6d3a8d20d7b7ff377f5669d491537f690613"
        );
    }

    #[test]
    fn test_token_auth_hash() {
        assert_eq!(
            token_auth_hash("my-token", &salt(HashAlg::Sha256)),
            "07de40ad0a1f7b97690e265cea766f16087637d9750946a941544e25f1a69664"
        );
        assert_eq!(
            token_auth_hash("my-token", &salt(HashAlg::Sha1)),
            "605b80084c8b7d10c7ae009703b230f9cdc64999"
        );
    }

    #[test]
    fn test_user_salt_parse() {
        let v = serde_json::json!({"key": "4142", "salt": "abcd", "hashAlg": "SHA256"});
        let s = UserSalt::parse(&v).unwrap();
        assert_eq!(s.key, b"AB");
        assert_eq!(s.salt, "abcd");
        assert_eq!(s.alg, HashAlg::Sha256);
        // Old firmware omits hashAlg: SHA-1
        let old = UserSalt::parse(&serde_json::json!({"key": "4142", "salt": "abcd"})).unwrap();
        assert_eq!(old.alg, HashAlg::Sha1);
        assert!(UserSalt::parse(&serde_json::json!({"key": "zz"})).is_err());
    }

    #[test]
    fn test_loxone_to_unix() {
        assert_eq!(loxone_to_unix(100), LOXONE_EPOCH_SECS + 100);
        assert_eq!(loxone_to_unix(1_700_000_000), 1_700_000_000);
    }

    #[cfg(unix)]
    #[test]
    fn test_token_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        make_store(1).write_to(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let back: TokenStore =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.token, "test_token");
    }
}
