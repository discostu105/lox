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
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
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

pub async fn acquire_token(cfg: &Config) -> Result<TokenStore> {
    // 1. Fetch RSA public key via HTTP
    let cfg2 = cfg.clone();
    let pub_key_pem: String = tokio::task::spawn_blocking(move || -> Result<String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(crate::client::USER_AGENT)
            .danger_accept_invalid_certs(true)
            .build()?;
        let resp: serde_json::Value = client
            .get(format!("{}/jdev/sys/getPublicKey", cfg2.host))
            .basic_auth(&cfg2.user, Some(&cfg2.pass))
            .send()?
            .json()?;
        Ok(resp
            .pointer("/LL/value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("no public key"))?
            .to_string())
    })
    .await??;

    // Loxone returns SubjectPublicKeyInfo mis-labeled as "CERTIFICATE" without line breaks
    // Strip headers, reformat base64 at 64 chars/line, relabel as PUBLIC KEY
    let pub_key: RsaPublicKey = {
        let b64: String = pub_key_pem
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
        RsaPublicKey::from_public_key_pem(&pem).map_err(|e| anyhow::anyhow!("RSA parse: {}", e))?
    };

    // 2. Generate AES-256 key + IV
    let mut aes_key = [0u8; 32];
    let mut aes_iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut aes_key);
    rand::thread_rng().fill_bytes(&mut aes_iv);
    // Loxone expects key:iv as HEX strings (not base64)
    let key_info = format!("{}:{}", hex::encode(aes_key), hex::encode(aes_iv));

    // 3. RSA-PKCS1v15 encrypt key info
    let encrypted_b64 = {
        let enc = pub_key.encrypt(
            &mut rand::thread_rng(),
            Pkcs1v15Encrypt,
            key_info.as_bytes(),
        )?;
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &enc)
    };

    // 4. WS connect + key exchange (retry up to 3 times)
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
    ws.send(Message::Text(format!(
        "jdev/sys/keyexchange/{}",
        encrypted_b64
    )))
    .await?;

    // Read keyexchange response (may get binary header first)
    for _ in 0..5 {
        match tokio::time::timeout(Duration::from_secs(WS_TIMEOUT_SECS), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_default();
                let code = v
                    .pointer("/LL/Code")
                    .and_then(|c| {
                        c.as_str()
                            .map(|s| s.to_string())
                            .or_else(|| c.as_i64().map(|n| n.to_string()))
                    })
                    .unwrap_or_else(|| "0".to_string());
                if code != "200" {
                    bail!("keyexchange failed ({}): {}", code, t);
                }
                break;
            }
            Ok(Some(Ok(Message::Binary(_)))) => continue,
            Ok(Some(Err(e))) => bail!("WS error: {}", e),
            _ => bail!("WS timeout/closed during keyexchange"),
        }
    }

    // 5. Get one-time key via WS (must be same connection as gettoken)
    ws.send(Message::Text(format!("jdev/sys/getkey2/{}", cfg.user)))
        .await?;
    let key2_val = ws_read_json_value(&mut ws, "getkey2").await?;
    let key_hex = key2_val.get("key").and_then(|v| v.as_str()).unwrap_or("");
    let salt_hex = key2_val.get("salt").and_then(|v| v.as_str()).unwrap_or("");
    let hash_alg = key2_val
        .get("hashAlg")
        .and_then(|v| v.as_str())
        .unwrap_or("SHA1");

    let key_b = hex::decode(key_hex)?;

    // Hash: hashAlg(password + ":" + salt_hex).toUpperCase()
    // Note: salt is used as raw hex string (NOT hex-decoded)
    let pw_hash = if hash_alg == "SHA256" {
        format!(
            "{:X}",
            Sha256::digest(format!("{}:{}", cfg.pass, salt_hex).as_bytes())
        )
    } else {
        let mut h1 = Sha1Digest::new();
        h1.update(format!("{}:{}", cfg.pass, salt_hex).as_bytes());
        format!("{:X}", h1.finalize())
    };

    let mut mac = HmacSha256::new_from_slice(&key_b)?;
    mac.update(format!("{}:{}", cfg.user, pw_hash).as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());

    // 6. Send gettoken command
    let client_uuid = uuid::Uuid::new_v4().to_string();
    let cmd = format!(
        "jdev/sys/gettoken/{}/{}/4/{}/lox-cli",
        sig, cfg.user, client_uuid
    );
    ws.send(Message::Text(cmd)).await?;

    // 7. Read token response
    let mut token_resp: Option<serde_json::Value> = None;
    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_secs(WS_TIMEOUT_SECS), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t)
                    && (v.pointer("/LL/Code").is_some() || v.pointer("/LL/code").is_some())
                {
                    token_resp = Some(v);
                    break;
                }
            }
            Ok(Some(Ok(Message::Binary(_)))) => continue,
            Ok(Some(Err(e))) => bail!("WS: {}", e),
            _ => bail!("WS timeout waiting for token"),
        }
    }
    let token_resp =
        token_resp.ok_or_else(|| anyhow::anyhow!("no token response after 10 messages"))?;

    let code = token_resp
        .pointer("/LL/Code")
        .or_else(|| token_resp.pointer("/LL/code"))
        .and_then(|c| {
            c.as_str()
                .map(|s| s.to_string())
                .or_else(|| c.as_i64().map(|n| n.to_string()))
        })
        .unwrap_or_else(|| "0".to_string());
    if code != "200" {
        bail!("gettoken failed ({}): {}", code, token_resp);
    }

    let val = token_resp
        .pointer("/LL/value")
        .ok_or_else(|| anyhow::anyhow!("no value in token response"))?;
    let token = val
        .get("token")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("no token field"))?;
    let token_key = val.get("key").and_then(|k| k.as_str()).unwrap_or("");
    let valid_until = val.get("validUntil").and_then(|v| v.as_u64()).unwrap_or(0);
    let unix_until = if valid_until > 1_500_000_000 {
        valid_until
    } else {
        LOXONE_EPOCH_SECS.saturating_add(valid_until)
    };

    let ts = TokenStore {
        token: token.to_string(),
        key: token_key.to_string(),
        valid_until: unix_until,
    };
    ts.save_for(cfg)?;
    Ok(ts)
}

/// Read a WS text response and return the parsed JSON value field.
async fn ws_read_json_value(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    label: &str,
) -> Result<serde_json::Value> {
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
                    return Ok(v
                        .pointer("/LL/value")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null));
                }
                if code != "0" {
                    bail!("{} failed ({}): {}", label, code, t);
                }
            }
            Ok(Some(Ok(Message::Binary(_)))) => continue,
            Ok(Some(Err(e))) => bail!("WS error during {}: {}", label, e),
            _ => bail!("WS timeout during {}", label),
        }
    }
    bail!("{}: no response after 10 messages", label)
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
}
