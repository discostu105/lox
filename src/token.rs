//! Loxone token authentication via WS + RSA/AES command encryption

use anyhow::{bail, Result};
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use rand::RngCore;
use rsa::{pkcs8::DecodePublicKey, pkcs1::DecodeRsaPublicKey, Pkcs1v15Encrypt, RsaPublicKey};
use x509_cert::{Certificate, der::{Decode, pem::PemLabel}};
use sha1::Sha1 as Sha1Digest;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{SinkExt, StreamExt};

use crate::config::Config;
use crate::ws::LoxWsClient;

type HmacSha256 = Hmac<Sha256>;
type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenStore {
    pub token: String,
    pub key: String,
    pub valid_until: u64,
}

impl TokenStore {
    pub fn path() -> std::path::PathBuf { Config::dir().join("token.json") }
    pub fn load() -> Option<Self> {
        serde_json::from_str(&std::fs::read_to_string(Self::path()).ok()?).ok()
    }
    pub fn save(&self) -> Result<()> {
        std::fs::write(Self::path(), serde_json::to_string_pretty(self)?)?; Ok(())
    }
    pub fn is_valid(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        self.valid_until > now + 300
    }
}

fn aes_encrypt(key: &[u8], iv: &[u8], plaintext: &[u8]) -> Vec<u8> {
    Aes256CbcEnc::new_from_slices(key, iv).expect("bad key/iv")
        .encrypt_padded_vec_mut::<cbc::cipher::block_padding::Pkcs7>(plaintext)
}

fn recv_text(msg: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>) -> Option<String> {
    match msg? {
        Ok(Message::Text(t)) => Some(t.to_string()),
        _ => None,
    }
}

pub async fn acquire_token(cfg: &Config) -> Result<TokenStore> {
    // 1. Fetch RSA public key via HTTP
    let cfg2 = cfg.clone();
    let pub_key_pem: String = tokio::task::spawn_blocking(move || -> Result<String> {
        let client = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(true).build()?;
        let resp: serde_json::Value = client
            .get(format!("{}/jdev/sys/getPublicKey", cfg2.host))
            .basic_auth(&cfg2.user, Some(&cfg2.pass))
            .send()?.json()?;
        Ok(resp.pointer("/LL/value").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("no public key"))?.to_string())
    }).await??;

    // Loxone returns SubjectPublicKeyInfo mis-labeled as "CERTIFICATE" without line breaks
    // Strip headers, reformat base64 at 64 chars/line, relabel as PUBLIC KEY
    let pub_key: RsaPublicKey = {
        let b64: String = pub_key_pem
            .replace("-----BEGIN CERTIFICATE-----", "")
            .replace("-----END CERTIFICATE-----", "")
            .replace("-----BEGIN PUBLIC KEY-----", "")
            .replace("-----END PUBLIC KEY-----", "")
            .chars().filter(|c| !c.is_whitespace()).collect();
        let mut pem = String::from("-----BEGIN PUBLIC KEY-----\n");
        for chunk in b64.as_bytes().chunks(64) {
            pem.push_str(std::str::from_utf8(chunk).unwrap_or(""));
            pem.push('\n');
        }
        pem.push_str("-----END PUBLIC KEY-----");
        eprintln!("[debug] normalized pem[..80]: {:?}", &pem[..pem.len().min(80)]);
        RsaPublicKey::from_public_key_pem(&pem)
            .map_err(|e| anyhow::anyhow!("RSA parse: {}", e))?
    };

    // 2. Generate AES-256 key + IV
    let mut aes_key = [0u8; 32];
    let mut aes_iv  = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut aes_key);
    rand::thread_rng().fill_bytes(&mut aes_iv);
    // Loxone expects key:iv as HEX strings (not base64)
    let key_info = format!("{}:{}", hex::encode(&aes_key), hex::encode(&aes_iv));

    // 3. RSA-PKCS1v15 encrypt key info
    let encrypted_b64 = {
        let enc = pub_key.encrypt(&mut rand::thread_rng(), Pkcs1v15Encrypt, key_info.as_bytes())?;
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &enc)
    };

    // 4. Compute HMAC for gettoken BEFORE keyexchange (salt is one-time use)
    let cfg3 = cfg.clone();
    let sig = tokio::task::spawn_blocking(move || -> Result<String> {
        let client = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(true).build()?;
        let resp: serde_json::Value = client
            .get(format!("{}/jdev/sys/getkey2", cfg3.host))
            .basic_auth(&cfg3.user, Some(&cfg3.pass))
            .send()?.json()?;
        let val = resp.pointer("/LL/value").ok_or_else(|| anyhow::anyhow!("no key2"))?;
        let key_hex  = val.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let salt_hex = val.get("salt").and_then(|v| v.as_str()).unwrap_or("");
        let key_b = hex::decode(key_hex)?;
        let salt  = String::from_utf8(hex::decode(salt_hex)?)?;

        let mut h1 = Sha1Digest::new();
        h1.update(cfg3.pass.as_bytes());
        let pass_sha1 = format!("{:X}", h1.finalize());
        let visu = format!("{:X}", Sha256::digest(format!("{}:{}", pass_sha1, salt).as_bytes()));

        let mut mac = HmacSha256::new_from_slice(&key_b)?;
        mac.update(format!("{}:{}", cfg3.user, visu).as_bytes());
        Ok(hex::encode(mac.finalize().into_bytes()))
    }).await??;

    // 5. WS connect + key exchange
    let ws_client = LoxWsClient::new(cfg.clone());
    let (mut ws, _) = ws_client.connect_raw().await?;
    ws.send(Message::Text(format!("jdev/sys/keyexchange/{}", encrypted_b64).into())).await?;

    // Read keyexchange response (may get binary header first)
    for _ in 0..5 {
        match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_default();
                let code = v.pointer("/LL/Code").and_then(|c| c.as_str()).unwrap_or("0");
                if code != "200" { bail!("keyexchange failed ({}): {}", code, t); }
                break;
            }
            Ok(Some(Ok(Message::Binary(_)))) => continue,
            Ok(Some(Err(e))) => bail!("WS error: {}", e),
            _ => bail!("WS timeout/closed during keyexchange"),
        }
    }

    // 6. Send encrypted gettoken command
    let client_uuid = uuid::Uuid::new_v4().to_string();
    let cmd = format!("jdev/sys/gettoken/{}/{}/4/{}/lox-cli", sig, cfg.user, client_uuid);
    let enc_cmd_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &aes_encrypt(&aes_key, &aes_iv, cmd.as_bytes()),
    );
    // URL-encode the base64 (+ and / need encoding in WS commands)
    let enc_cmd_url: String = enc_cmd_b64.chars().map(|c| match c {
        '+' => "%2B".to_string(),
        '/' => "%2F".to_string(),
        '=' => "%3D".to_string(),
        c   => c.to_string(),
    }).collect();
    ws.send(Message::Text(format!("jdev/sys/enc/{}", enc_cmd_url).into())).await?;

    // 7. Read token response
    let token_resp: serde_json::Value = 'outer: loop {
        for _ in 0..10 {
            match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
                Ok(Some(Ok(Message::Text(t)))) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                        if v.pointer("/LL/Code").is_some() { break 'outer v; }
                    }
                }
                Ok(Some(Ok(Message::Binary(_)))) => continue,
                Ok(Some(Err(e))) => bail!("WS: {}", e),
                _ => bail!("WS timeout waiting for token"),
            }
        }
        bail!("no token response after 10 messages");
    };

    let code = token_resp.pointer("/LL/Code").and_then(|c| c.as_str()).unwrap_or("0");
    if code != "200" { bail!("gettoken failed ({}): {}", code, token_resp); }

    let val = token_resp.pointer("/LL/value")
        .ok_or_else(|| anyhow::anyhow!("no value in token response"))?;
    let token = val.get("token").and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("no token field"))?;
    let token_key = val.get("key").and_then(|k| k.as_str()).unwrap_or("");
    let valid_until = val.get("validUntil").and_then(|v| v.as_u64()).unwrap_or(0);
    let unix_until = if valid_until > 1_500_000_000 { valid_until }
        else { 1_230_768_000u64.saturating_add(valid_until) };

    let ts = TokenStore {
        token: token.to_string(),
        key: token_key.to_string(),
        valid_until: unix_until,
    };
    ts.save()?;
    Ok(ts)
}
