//! What the transport needs to talk to one Miniserver: where it is, who
//! connects, and where its state (token, structure cache) is kept. The HTTP
//! client, the WebSocket session, token auth and FTP take a [`Connection`];
//! lox's own settings (contexts, aliases, scenes, confirm lists) stay in
//! `Config`, which builds one with `Config::connection`.

use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Connection {
    /// Base URL of the Miniserver, e.g. `https://192.168.1.77` or
    /// `http://miniserver.local`.
    pub host: String,
    pub user: String,
    pub pass: String,
    /// Verify the TLS certificate of HTTPS requests. Miniservers use
    /// self-signed certificates, so this is off by default; the WebSocket
    /// connection never verifies.
    pub verify_ssl: bool,
    /// Where token auth keeps its token (`token.json`, written mode 0600);
    /// `None`: a token is requested for every WebSocket session.
    pub token_path: Option<PathBuf>,
    /// Where the structure file (`LoxAPP3.json`) is cached for 24 hours;
    /// `None`: fetched every time.
    pub structure_cache: Option<PathBuf>,
}

impl Connection {
    /// A connection without TLS verification, token or structure cache.
    #[allow(dead_code)] // lox builds its connections from Config; tests use this
    pub fn new(host: impl Into<String>, user: impl Into<String>, pass: impl Into<String>) -> Self {
        Connection {
            host: host.into(),
            user: user.into(),
            pass: pass.into(),
            ..Default::default()
        }
    }

    /// The URL of `path` on the Miniserver.
    pub fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.host.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }
}

/// The password never prints.
impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("host", &self.host)
            .field("user", &self.user)
            .field("pass", &"***")
            .field("verify_ssl", &self.verify_ssl)
            .field("token_path", &self.token_path)
            .field("structure_cache", &self.structure_cache)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_password() {
        let c = Connection::new("http://ms", "admin", "s3cret");
        let shown = format!("{c:?}");
        assert!(!shown.contains("s3cret"), "{shown}");
        assert!(shown.contains("admin"), "{shown}");
    }

    #[test]
    fn url_joins_host_and_path() {
        let c = Connection::new("http://ms/", "u", "p");
        assert_eq!(c.url("/jdev/cfg/version"), "http://ms/jdev/cfg/version");
        assert_eq!(c.url("data/LoxApp3.json"), "http://ms/data/LoxApp3.json");
    }
}
