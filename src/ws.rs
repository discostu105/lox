//! Loxone WebSocket client + binary protocol parser

use anyhow::{bail, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use futures_util::{SinkExt, StreamExt};
use rustls::{crypto::ring, ClientConfig};
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::Connector;
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message};

use rand::RngCore as _;

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
    let tail: String = buf[pos..pos + 8]
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
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
    if buf.len() < 8 || buf[0] != 0x03 {
        return None;
    }
    let typ = match buf[1] {
        0x00 => MsgType::Text,
        0x01 => MsgType::BinaryFile,
        0x02 => MsgType::ValueEventTable,
        0x03 => MsgType::TextEventTable,
        0x06 => MsgType::Keepalive,
        n => MsgType::Other(n),
    };
    let len = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    Some((typ, len))
}

fn parse_value_table(buf: &[u8]) -> Vec<StateEvent> {
    let mut events = Vec::new();
    let mut i = 0;
    while i + 24 <= buf.len() {
        let uuid = parse_lox_uuid(&buf[i..i + 16]);
        let value = f64::from_le_bytes(buf[i + 16..i + 24].try_into().unwrap_or([0; 8]));
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
    cfg.dangerous()
        .set_certificate_verifier(Arc::new(NoCertVerifier));
    cfg.enable_early_data = true;
    // Allow servers that don't send close_notify (Loxone Miniserver)
    Arc::new(cfg)
}

#[derive(Debug)]
struct NoCertVerifier;
impl rustls::client::danger::ServerCertVerifier for NoCertVerifier {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer,
        _: &[rustls::pki_types::CertificateDer],
        _: &rustls::pki_types::ServerName,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

// ── WS Client ────────────────────────────────────────────────────────────────

pub struct LoxWsClient {
    cfg: Config,
}

impl LoxWsClient {
    pub fn new(cfg: Config) -> Self {
        Self { cfg }
    }

    fn ws_url(&self) -> String {
        self.cfg
            .host
            .replace("https://", "wss://")
            .replace("http://", "ws://")
            .trim_end_matches('/')
            .to_string()
            + "/ws/rfc6455"
    }

    pub async fn connect_raw(
        &self,
    ) -> Result<(
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tokio_tungstenite::tungstenite::http::Response<Option<Vec<u8>>>,
    )> {
        let url = self.ws_url();
        let tls_cfg = make_tls_config();
        let basic = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            format!("{}:{}", self.cfg.user, self.cfg.pass),
        );
        let req = tokio_tungstenite::tungstenite::http::Request::builder()
            .uri(&url)
            .header("Authorization", format!("Basic {}", basic))
            .header(
                "Host",
                url.split("wss://")
                    .nth(1)
                    .unwrap_or("")
                    .split('/')
                    .next()
                    .unwrap_or(""),
            )
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_ws_key())
            .body(())?;
        connect_async_tls_with_config(req, None, false, Some(Connector::Rustls(tls_cfg)))
            .await
            .map_err(|e| anyhow::anyhow!("WS connect: {}", e))
    }

    pub async fn run(
        &self,
        on_events: impl Fn(Vec<StateEvent>) + Send + Sync + 'static,
    ) -> Result<()> {
        let url = self.ws_url();
        let tls_cfg = make_tls_config();

        // Build request with Basic Auth header
        let basic = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            format!("{}:{}", self.cfg.user, self.cfg.pass),
        );
        let req = Request::builder()
            .uri(&url)
            .header("Authorization", format!("Basic {}", basic))
            .header(
                "Host",
                url.split("wss://")
                    .nth(1)
                    .unwrap_or("")
                    .split('/')
                    .next()
                    .unwrap_or(""),
            )
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_ws_key())
            .body(())?;

        let connector = Connector::Rustls(tls_cfg);
        let (mut ws, resp) = connect_async_tls_with_config(req, None, false, Some(connector))
            .await
            .map_err(|e| anyhow::anyhow!("WS connect failed: {}", e))?;

        println!("  ✓ Connected (HTTP {})", resp.status());

        // Basic auth done via HTTP Upgrade header — WS is already authenticated
        println!("  ✓ Authenticated (Basic Auth)");

        // Subscribe to all state updates
        ws.send(Message::Text("jdev/sps/enablestatusupdate".into()))
            .await?;
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
                                if pending_type.is_some() {
                                    eprintln!("ws: header frame displaced pending payload — dropping");
                                }
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
}

fn generate_ws_key() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_lox_uuid_known_value() {
        // UUID bytes for "1fbc668c-005c-7471-ffffed57184a04d2" in Loxone binary format:
        // u32le 0x8c66bc1f, u16le 0x5c00, u16le 0x7174, then 8 raw bytes ff ff ed 57 18 4a 04 d2
        let mut buf = vec![0u8; 16];
        buf[0..4].copy_from_slice(&0x1fbc668cu32.to_le_bytes());
        buf[4..6].copy_from_slice(&0x005cu16.to_le_bytes());
        buf[6..8].copy_from_slice(&0x7471u16.to_le_bytes());
        buf[8..16].copy_from_slice(&[0xff, 0xff, 0xed, 0x57, 0x18, 0x4a, 0x04, 0xd2]);
        let uuid = parse_lox_uuid(&buf);
        assert_eq!(uuid, "1fbc668c-005c-7471-ffffed57184a04d2");
    }

    #[test]
    fn test_parse_header_valid() {
        let mut buf = [0u8; 8];
        buf[0] = 0x03;
        buf[1] = 0x02; // ValueEventTable
        buf[4..8].copy_from_slice(&48u32.to_le_bytes());
        let result = parse_header(&buf);
        assert!(result.is_some());
        let (typ, len) = result.unwrap();
        assert_eq!(typ, MsgType::ValueEventTable);
        assert_eq!(len, 48);
    }

    #[test]
    fn test_parse_header_wrong_magic() {
        let buf = [0u8; 8];
        assert!(parse_header(&buf).is_none());
    }

    #[test]
    fn test_parse_header_too_short() {
        let buf = [0x03u8; 4];
        assert!(parse_header(&buf).is_none());
    }

    #[test]
    fn test_parse_value_table_empty() {
        let events = parse_value_table(&[]);
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_value_table_one_record() {
        let mut buf = vec![0u8; 24];
        // UUID: all zeros → "00000000-0000-0000-0000000000000000"
        // Value: 1.5 as f64le
        buf[16..24].copy_from_slice(&1.5f64.to_le_bytes());
        let events = parse_value_table(&buf);
        assert_eq!(events.len(), 1);
        assert!((events[0].value - 1.5).abs() < 1e-12);
    }

    #[test]
    fn test_parse_value_table_truncated_ignored() {
        // 23 bytes — not a complete record, should produce no events
        let buf = vec![0u8; 23];
        let events = parse_value_table(&buf);
        assert!(events.is_empty());
    }

    #[test]
    fn test_generate_ws_key_length() {
        // Base64 of 16 bytes = 24 chars (with padding)
        let key = generate_ws_key();
        assert_eq!(key.len(), 24);
    }

    #[test]
    fn test_generate_ws_key_unique() {
        let k1 = generate_ws_key();
        let k2 = generate_ws_key();
        assert_ne!(k1, k2);
    }
}
