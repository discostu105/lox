//! The crate's error type.

/// An HTTP error status from the Miniserver, with the request path (any
/// credentials in it redacted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpStatusError {
    pub status: u16,
    pub path: String,
}

impl std::fmt::Display for HttpStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}: {}", self.status, self.path)
    }
}

impl std::error::Error for HttpStatusError {}

/// Everything that can go wrong talking to a Miniserver.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The HTTP request failed (connect, TLS, body).
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    /// The Miniserver answered with an HTTP error status (401, 404, …).
    #[error(transparent)]
    Status(#[from] HttpStatusError),
    /// The WebSocket failed (connect, handshake, closed).
    #[error(transparent)]
    WebSocket(Box<tokio_tungstenite::tungstenite::Error>),
    /// FTP (backups, program upload) failed.
    #[error(transparent)]
    Ftp(#[from] suppaftp::FtpError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// A name or UUID matched no control, or several.
    #[error("{0}")]
    Control(String),
    /// The Miniserver answered with something unexpected, or refused (a
    /// rejected token, a malformed binary message).
    #[error("{0}")]
    Protocol(String),
    /// No answer in time.
    #[error("{0}")]
    Timeout(String),
    /// An error returned by the caller's event handler.
    #[error(transparent)]
    Handler(Box<dyn std::error::Error + Send + Sync>),
    /// An error with what was being done when it happened. Shows the
    /// message; the cause is its [`source`](std::error::Error::source).
    #[error("{message}")]
    Context {
        message: String,
        #[source]
        source: Box<Error>,
    },
}

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Error::WebSocket(Box::new(e))
    }
}

impl Error {
    /// The HTTP status error under any context, if this is one.
    pub fn http_status(&self) -> Option<&HttpStatusError> {
        match self {
            Error::Status(e) => Some(e),
            Error::Context { source, .. } => source.http_status(),
            _ => None,
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// `anyhow`-style context for the crate's own results.
pub(crate) trait Context<T> {
    fn context(self, message: impl Into<String>) -> Result<T>;
    fn with_context<M: Into<String>>(self, message: impl FnOnce() -> M) -> Result<T>;
}

impl<T, E: Into<Error>> Context<T> for std::result::Result<T, E> {
    fn context(self, message: impl Into<String>) -> Result<T> {
        self.map_err(|e| Error::Context {
            message: message.into(),
            source: Box::new(e.into()),
        })
    }
    fn with_context<M: Into<String>>(self, message: impl FnOnce() -> M) -> Result<T> {
        self.map_err(|e| Error::Context {
            message: message().into(),
            source: Box::new(e.into()),
        })
    }
}

impl<T> Context<T> for Option<T> {
    fn context(self, message: impl Into<String>) -> Result<T> {
        self.ok_or_else(|| Error::Protocol(message.into()))
    }
    fn with_context<M: Into<String>>(self, message: impl FnOnce() -> M) -> Result<T> {
        self.ok_or_else(|| Error::Protocol(message().into()))
    }
}

/// Return early with an [`Error::Protocol`].
macro_rules! bail {
    ($($arg:tt)*) => {
        return Err($crate::Error::Protocol(format!($($arg)*)))
    };
}
pub(crate) use bail;

/// An [`Error::Protocol`] from a format string.
macro_rules! err {
    ($($arg:tt)*) => {
        $crate::Error::Protocol(format!($($arg)*))
    };
}
pub(crate) use err;

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    fn status(code: u16) -> Error {
        HttpStatusError {
            status: code,
            path: "/jdev/sps/io/x".into(),
        }
        .into()
    }

    #[test]
    fn http_status_is_found_under_context() {
        let e = Err::<(), _>(status(404))
            .context("loading the structure")
            .context("listing controls")
            .unwrap_err();
        assert_eq!(e.http_status().map(|s| s.status), Some(404));
        assert_eq!(e.to_string(), "listing controls");
        assert_eq!(e.source().unwrap().to_string(), "loading the structure");
        assert!(err!("x").http_status().is_none());
    }

    #[test]
    fn option_context_is_a_protocol_error() {
        let e = None::<u8>
            .with_context(|| format!("no {}", "token"))
            .unwrap_err();
        assert!(matches!(e, Error::Protocol(ref m) if m == "no token"));
        assert_eq!(Some(1).context("unused").unwrap(), 1);
    }

    #[test]
    fn bail_returns_a_protocol_error() {
        fn f(n: u8) -> Result<()> {
            bail!("bad {}", n)
        }
        assert_eq!(f(3).unwrap_err().to_string(), "bad 3");
    }

    #[test]
    fn handler_errors_show_through() {
        let inner: Box<dyn std::error::Error + Send + Sync> = "stop here".into();
        assert_eq!(Error::Handler(inner).to_string(), "stop here");
    }

    #[test]
    fn websocket_errors_convert() {
        let e: Error = tokio_tungstenite::tungstenite::Error::ConnectionClosed.into();
        assert!(matches!(e, Error::WebSocket(_)));
        assert_eq!(status(401).to_string(), "HTTP 401: /jdev/sps/io/x");
    }
}
