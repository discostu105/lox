//! Loxone WebSocket client + binary protocol parser

use anyhow::{bail, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use futures_util::{SinkExt, StreamExt};
use std::io::Cursor;
use std::time::Duration;
use tokio::time;
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message};
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::Connector;
use rustls::{ClientConfig, crypto::ring};
use std::sync::Arc;

use crate::config::Config;

// ── State event ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StateEvent {
    pub uuid: String,
    pub value: f64,
}

// ── UUID binary decoding ──────────────────────────────────────────────────────

fn parse_lox_uuid(buf: &[u8]) -> String {
    let mut c = Cursor::new(buf);
    let a = c.read_u32::<LittleEndian>().unwrap_or(0);
    let b = c.read_u16::<LittleEndian>().unwrap_or(0);
    let d = c.read_u16::<LittleEndian>().unwrap_or(0);
    let pos = c.position() as usize;
    let tail: String = buf[pos..pos+8].iter().map(|b| format!("{:02x}", b)).collect();
    format!("{:08x}-{:04x}-{:04x}-{}", a, b, d, tail)
}

// ── Binary protocol ───────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum MsgType {
    Text,
    BinaryFile,
    ValueEventTable,
    TextEventTable,
    Keepalive,
    Other(u8),
}

fn parse_header(buf: &[u8]) -> Option<(MsgType, u32)> {
    if buf.len() < 8 || buf[0] != 0x03 { return None; }
    let typ = match buf[1] {
        0x00 => MsgType::Text,
        0x01 => MsgType::BinaryFile,
        0x02 => MsgType::ValueEventTable,
        0x03 => MsgType::TextEventTable,
        0x06 => MsgType::Keepalive,
        n    => MsgType::Other(n),
    };
    let len = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    Some((typ, len))
}

fn parse_value_table(buf: &[u8]) -> Vec<StateEvent> {
    let mut events = Vec::new();
    let mut i = 0;
    while i + 24 <= buf.len() {
        let uuid = parse_lox_uuid(&buf[i..i+16]);
        let value = f64::from_le_bytes(buf[i+16..i+24].try_into().unwrap_or([0;8]));
        events.push(StateEvent { uuid, value });
        i += 24;
    }
    events
}

// ── TLS (accept all) ─────────────────────────────────────────────────────────

fn make_tls_config() -> Arc<ClientConfig> {
    let _ = ring::default_provider().install_default();
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut cfg = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    cfg.dangerous().set_certificate_verifier(Arc::new(NoCertVerifier));
    cfg.enable_early_data = true;
    // Allow servers that don't send close_notify (Loxone Miniserver)
    Arc::new(cfg)
}

#[derive(Debug)]
struct NoCertVerifier;
impl rustls::client::danger::ServerCertVerifier for NoCertVerifier {
    fn verify_server_cert(&self, _: &rustls::pki_types::CertificateDer,
        _: &[rustls::pki_types::CertificateDer], _: &rustls::pki_types::ServerName,
        _: &[u8], _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, _: &[u8], _: &rustls::pki_types::CertificateDer,
        _: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(&self, _: &[u8], _: &rustls::pki_types::CertificateDer,
        _: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        ring::default_provider().signature_verification_algorithms.supported_schemes()
    }
}

// ── WS Client ────────────────────────────────────────────────────────────────

pub struct LoxWsClient {
    cfg: Config,
}

impl LoxWsClient {
    pub fn new(cfg: Config) -> Self { Self { cfg } }

    fn ws_url(&self) -> String {
        self.cfg.host
            .replace("https://", "wss://")
            .replace("http://", "ws://")
            .trim_end_matches('/')
            .to_string() + "/ws/rfc6455"
    }

    pub async fn connect_raw(&self) -> Result<(tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, tokio_tungstenite::tungstenite::http::Response<Option<Vec<u8>>>)> {
        let url = self.ws_url();
        let tls_cfg = make_tls_config();
        let basic = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            format!("{}:{}", self.cfg.user, self.cfg.pass)
        );
        let req = tokio_tungstenite::tungstenite::http::Request::builder()
            .uri(&url)
            .header("Authorization", format!("Basic {}", basic))
            .header("Host", url.split("wss://").nth(1).unwrap_or("").split('/').next().unwrap_or(""))
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_ws_key())
            .body(())?;
        connect_async_tls_with_config(req, None, false, Some(Connector::Rustls(tls_cfg)))
            .await.map_err(|e| anyhow::anyhow!("WS connect: {}", e))
    }

    pub async fn run(&self, on_events: impl Fn(Vec<StateEvent>) + Send + Sync + 'static) -> Result<()> {
        let url = self.ws_url();
        let tls_cfg = make_tls_config();

        // Build request with Basic Auth header
        let basic = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            format!("{}:{}", self.cfg.user, self.cfg.pass)
        );
        let req = Request::builder()
            .uri(&url)
            .header("Authorization", format!("Basic {}", basic))
            .header("Host", url.split("wss://").nth(1).unwrap_or("").split('/').next().unwrap_or(""))
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_ws_key())
            .body(())?;

        let connector = Connector::Rustls(tls_cfg);
        let (mut ws, resp) = connect_async_tls_with_config(req, None, false, Some(connector))
            .await.map_err(|e| anyhow::anyhow!("WS connect failed: {}", e))?;

        println!("  ✓ Connected (HTTP {})", resp.status());

        // Basic auth done via HTTP Upgrade header — WS is already authenticated
        println!("  ✓ Authenticated (Basic Auth)");

        // Subscribe to all state updates
        ws.send(Message::Text("jdev/sps/enablestatusupdate".into())).await?;
        println!("  ✓ Subscribed to state updates\n");

        // Main event loop
        let mut pending_type: Option<MsgType> = None;
        let mut ka_interval = time::interval(Duration::from_secs(25));
        ka_interval.tick().await;

        loop {
            tokio::select! {
                msg = ws.next() => {
                    match msg {
                        None => bail!("WS closed"),
                        Some(Err(e)) => bail!("WS error: {}", e),
                        Some(Ok(Message::Binary(bin))) => {
                            let buf = bin.as_ref();
                            if let Some((typ, _)) = parse_header(buf) {
                                pending_type = Some(typ);
                            } else if let Some(MsgType::ValueEventTable) = &pending_type {
                                let events = parse_value_table(buf);
                                if !events.is_empty() { on_events(events); }
                                pending_type = None;
                            } else {
                                pending_type = None;
                            }
                        }
                        Some(Ok(Message::Text(_))) => { pending_type = None; }
                        Some(Ok(Message::Ping(d))) => { ws.send(Message::Pong(d)).await?; }
                        Some(Ok(_)) => {}
                    }
                }
                _ = ka_interval.tick() => {
                    ws.send(Message::Text("keepalive".into())).await?;
                }
            }
        }
    }

    async fn hmac_auth(&self) -> Result<String> {
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256};
        type HmacSha256 = Hmac<Sha256>;

        let cfg = self.cfg.clone();
        tokio::task::spawn_blocking(move || {
            let client = reqwest::blocking::Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(Duration::from_secs(10))
                .build()?;
            let resp: serde_json::Value = client
                .get(format!("{}/jdev/sys/getkey2", cfg.host))
                .basic_auth(&cfg.user, Some(&cfg.pass))
                .send()?.json()?;

            let val = resp.pointer("/LL/value").ok_or_else(|| anyhow::anyhow!("no value"))?;
            let key_hex = val.get("key").and_then(|v| v.as_str()).unwrap_or("");
            let salt_hex = val.get("salt").and_then(|v| v.as_str()).unwrap_or("");

            let key = hex::decode(key_hex)?;
            let salt = String::from_utf8(hex::decode(salt_hex)?)?;

            let mut sha1 = sha1::Sha1::new();
            sha1.update(cfg.pass.as_bytes());
            let pass_sha1 = format!("{:X}", sha1.finalize());

            let visu = {
                let mut h = Sha256::new();
                h.update(format!("{}:{}", pass_sha1, salt).as_bytes());
                format!("{:X}", h.finalize())
            };

            let mut mac = HmacSha256::new_from_slice(&key)?;
            mac.update(format!("{}:{}", cfg.user, visu).as_bytes());
            Ok(hex::encode(mac.finalize().into_bytes()))
        }).await?
    }
}

fn generate_ws_key() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos();
    base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        format!("{:016x}", t as u64 ^ 0xdeadbeef_cafebabe_u64)
    )
}
