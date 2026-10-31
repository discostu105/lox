//! Loxone WebSocket client — used for token auth key exchange

use crate::error::{Context, Result};
use rustls::{ClientConfig, crypto::ring};
use std::sync::Arc;
use tokio_tungstenite::Connector;
use tokio_tungstenite::connect_async_tls_with_config;
use tokio_tungstenite::tungstenite::http::Request;

use rand::RngCore as _;

use crate::connection::Connection;

// ── TLS (accept all) ─────────────────────────────────────────────────────────

fn make_tls_config() -> Arc<ClientConfig> {
    let _ = ring::default_provider().install_default();
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut tls = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    tls.dangerous()
        .set_certificate_verifier(Arc::new(NoCertVerifier));
    tls.enable_early_data = true;
    Arc::new(tls)
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
    conn: Connection,
}

impl LoxWsClient {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    fn ws_url(&self) -> String {
        self.conn
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
            format!("{}:{}", self.conn.user, self.conn.pass),
        );
        let req = Request::builder()
            .uri(&url)
            .header("Authorization", format!("Basic {}", basic))
            .header(
                "Host",
                url.split("://")
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
            .body(())
            .map_err(|e| crate::error::err!("WS request: {}", e))?;
        connect_async_tls_with_config(req, None, false, Some(Connector::Rustls(tls_cfg)))
            .await
            .context("WS connect")
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
    fn test_generate_ws_key_length() {
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
