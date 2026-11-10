//! FTP client wrapper for Loxone Miniserver backup access

use crate::error::{Context, Error, Result, bail};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
use suppaftp::{FtpError, FtpStream, Status};

use crate::connection::Connection;

const RETRY_DELAYS_MS: [u64; 3] = [200, 400, 800];

/// Where and how patiently to connect.
struct Server {
    host: String,
    port: u16,
    /// Limit for opening the control and the data connection.
    connect_timeout: Duration,
    /// Limit for one read or write: a Miniserver that stops answering in the
    /// middle of a transfer ends the command instead of blocking it forever.
    io_timeout: Duration,
}

impl Server {
    fn of(conn: &Connection) -> Self {
        Server {
            host: ftp_host(conn),
            port: 21,
            connect_timeout: Duration::from_secs(10),
            io_timeout: Duration::from_secs(30),
        }
    }

    /// Connect and log in.
    fn open(&self, conn: &Connection) -> Result<FtpStream> {
        // A line break would start a second FTP command.
        if conn.user.contains(['\r', '\n']) || conn.pass.contains(['\r', '\n']) {
            bail!("The user name and password must not contain line breaks");
        }
        let unreachable = || {
            format!(
                "Could not connect to {}:{}. Check that FTP is enabled on the Miniserver.",
                self.host, self.port
            )
        };
        let addrs = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .with_context(unreachable)?;
        // The timeouts are set before the first read, so a server that
        // accepts and then says nothing cannot block the welcome either.
        let mut last = None;
        let mut ftp = None;
        for addr in addrs {
            let connected = TcpStream::connect_timeout(&addr, self.connect_timeout)
                .and_then(|stream| set_timeouts(&stream, self.io_timeout).map(|()| stream))
                .map_err(FtpError::ConnectionError)
                .and_then(FtpStream::connect_with_stream);
            match connected {
                Ok(stream) => {
                    ftp = Some(stream);
                    break;
                }
                Err(e) => last = Some(e),
            }
        }
        let ftp = match (ftp, last) {
            (Some(ftp), _) => ftp,
            (None, Some(e)) => return Err(e).with_context(unreachable),
            (None, None) => bail!("{}", unreachable()),
        };
        let (connect_timeout, io_timeout) = (self.connect_timeout, self.io_timeout);
        let mut ftp = ftp.passive_stream_builder(move |addr| {
            let data = TcpStream::connect_timeout(&addr, connect_timeout)
                .map_err(FtpError::ConnectionError)?;
            set_timeouts(&data, io_timeout).map_err(FtpError::ConnectionError)?;
            Ok(data)
        });
        ftp.login(conn.user.as_str(), conn.pass.as_str())
            .context("FTP login failed. Check your admin credentials.")?;
        Ok(ftp)
    }
}

fn set_timeouts(stream: &TcpStream, timeout: Duration) -> std::io::Result<()> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))
}

/// The FTP answer under any context, if `e` is one.
fn ftp_status(e: &Error) -> Option<Status> {
    match e {
        Error::Ftp(FtpError::UnexpectedResponse(r)) => Some(r.status),
        Error::Context { source, .. } => ftp_status(source),
        _ => None,
    }
}

/// Retry an FTP operation up to 3 times with exponential backoff. A rejected
/// login is not retried: the same credentials fail again, and repeated
/// failures can lock the user out.
fn ftp_with_retry<T, F>(f: F) -> Result<T>
where
    F: Fn() -> Result<T>,
{
    let mut last_err = None;
    for (attempt, delay) in std::iter::once(&0u64)
        .chain(RETRY_DELAYS_MS.iter())
        .enumerate()
    {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(*delay));
        }
        match f() {
            Ok(val) => return Ok(val),
            Err(e) if ftp_status(&e) == Some(Status::NotLoggedIn) => return Err(e),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.expect("retry loop must execute at least once"))
}

/// A file name the Miniserver gave or the user typed, as one path segment.
fn check_file_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
        || name == "."
        || name == ".."
    {
        bail!("'{}' is not a file name", name.escape_debug());
    }
    Ok(())
}

/// Parsed backup filename: `sps_<version>_<timestamp>.zip`
#[derive(Debug, Clone)]
pub struct BackupInfo {
    pub filename: String,
    pub version: u32,
    pub timestamp: String, // "20260308182256"
    pub size: u64,
}

impl BackupInfo {
    /// Format the timestamp as "YYYY-MM-DD HH:MM:SS"
    pub fn formatted_date(&self) -> String {
        if self.timestamp.len() >= 14 {
            format!(
                "{}-{}-{} {}:{}:{}",
                &self.timestamp[0..4],
                &self.timestamp[4..6],
                &self.timestamp[6..8],
                &self.timestamp[8..10],
                &self.timestamp[10..12],
                &self.timestamp[12..14],
            )
        } else {
            self.timestamp.clone()
        }
    }
}

/// Parse a backup filename like "sps_194_20260308182256.zip"
pub fn parse_backup_name(name: &str, size: u64) -> Option<BackupInfo> {
    let stem = name.strip_suffix(".zip")?;
    let parts: Vec<&str> = stem.splitn(3, '_').collect();
    if parts.len() != 3 || parts[0] != "sps" {
        return None;
    }
    let version = parts[1].parse::<u32>().ok()?;
    let timestamp = parts[2].to_string();
    if timestamp.len() < 14 || !timestamp.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(BackupInfo {
        filename: name.to_string(),
        version,
        timestamp,
        size,
    })
}

/// Connect to the Miniserver FTP and list backup ZIPs in /prog/.
pub fn list_backups(conn: &Connection) -> Result<Vec<BackupInfo>> {
    list_backups_on(&Server::of(conn), conn)
}

fn list_backups_on(server: &Server, conn: &Connection) -> Result<Vec<BackupInfo>> {
    let listing = ftp_with_retry(|| {
        let mut ftp = server.open(conn)?;
        // A directory that does not exist (550) holds no backups; any other
        // failure must not read as "no backups".
        let listing = match ftp.list(Some("/prog")) {
            Ok(listing) => listing,
            Err(FtpError::UnexpectedResponse(r)) if r.status == Status::FileUnavailable => {
                Vec::new()
            }
            Err(e) => return Err(e).context("Failed to list /prog"),
        };
        ftp.quit().ok();
        Ok(listing)
    })?;

    let mut backups = Vec::new();
    for line in &listing {
        // FTP LIST output: parse last token as filename, size from fields
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        let name = fields.last().unwrap();
        if !name.ends_with(".zip") || !name.starts_with("sps_") {
            continue;
        }
        // Size is typically field index 4 in Unix-style listing
        let size = fields
            .get(4)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        if let Some(info) = parse_backup_name(name, size) {
            backups.push(info);
        }
    }
    // Sort by timestamp descending (newest first)
    backups.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(backups)
}

/// Download a specific backup file from the Miniserver.
pub fn download_backup(conn: &Connection, filename: &str) -> Result<Vec<u8>> {
    download_backup_on(&Server::of(conn), conn, filename)
}

fn download_backup_on(server: &Server, conn: &Connection, filename: &str) -> Result<Vec<u8>> {
    check_file_name(filename)?;
    let path = format!("/prog/{}", filename);
    ftp_with_retry(|| {
        let mut ftp = server.open(conn)?;
        ftp.transfer_type(suppaftp::types::FileType::Binary)
            .context("Failed to set binary transfer mode")?;
        let cursor = ftp
            .retr_as_buffer(&path)
            .with_context(|| format!("Failed to download {}", path))?;
        ftp.quit().ok();
        Ok(cursor.into_inner())
    })
}

/// The file name the Miniserver loads as its new config on start (it logs
/// `Loading sps_new.zip`). Loxone Connection uploads under this name too; a
/// backup re-uploaded under its own name is not loaded.
pub const CONFIG_UPLOAD_NAME: &str = "sps_new.zip";

/// Upload a config ZIP as `/prog/sps_new.zip`, the config the Miniserver
/// starts from after its next reboot.
pub fn upload_config(conn: &Connection, data: &[u8]) -> Result<()> {
    let server = Server::of(conn);
    let path = format!("/prog/{}", CONFIG_UPLOAD_NAME);
    ftp_with_retry(|| {
        let mut ftp = server.open(conn)?;
        ftp.transfer_type(suppaftp::types::FileType::Binary)
            .context("Failed to set binary transfer mode")?;
        let mut reader = std::io::Cursor::new(data);
        ftp.put_file(&path, &mut reader)
            .with_context(|| format!("Failed to upload {}", path))?;
        ftp.quit().ok();
        Ok(())
    })
}

/// Where the Miniserver keeps its custom changes, one `<UUID>.LoxCC` per
/// object (see `custom_changes`).
pub const CUSTOM_CHANGES_DIR: &str = "/sys/custom";

/// Download every `*.LoxCC` in /sys/custom: file name and bytes. A directory
/// that does not exist (FTP 550) holds none.
pub fn download_custom_changes(conn: &Connection) -> Result<Vec<(String, Vec<u8>)>> {
    let server = Server::of(conn);
    ftp_with_retry(|| {
        let mut ftp = server.open(conn)?;
        let names = match ftp.nlst(Some(CUSTOM_CHANGES_DIR)) {
            Ok(names) => names,
            Err(FtpError::UnexpectedResponse(r)) if r.status == Status::FileUnavailable => {
                Vec::new()
            }
            Err(e) => {
                return Err(e).with_context(|| format!("Failed to list {}", CUSTOM_CHANGES_DIR));
            }
        };
        ftp.transfer_type(suppaftp::types::FileType::Binary)
            .context("Failed to set binary transfer mode")?;
        let mut files = Vec::new();
        for name in names {
            let name = name.rsplit('/').next().unwrap_or(&name).to_string();
            if !name.to_ascii_lowercase().ends_with(".loxcc") || check_file_name(&name).is_err() {
                continue;
            }
            let path = format!("{}/{}", CUSTOM_CHANGES_DIR, name);
            let data = ftp
                .retr_as_buffer(&path)
                .with_context(|| format!("Failed to download {}", path))?
                .into_inner();
            files.push((name, data));
        }
        ftp.quit().ok();
        Ok(files)
    })
}

/// Extract the FTP hostname from config (strip scheme, port, path).
fn ftp_host(conn: &Connection) -> String {
    let rest = conn
        .host
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let authority = rest.split('/').next().unwrap_or(rest);
    // An IPv6 literal: `[fe80::1]:8443`
    if let Some(inner) = authority.strip_prefix('[')
        && let Some((address, _)) = inner.split_once(']')
    {
        return address.to_string();
    }
    authority.split(':').next().unwrap_or(authority).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_backup_name_valid() {
        let info = parse_backup_name("sps_194_20260308182256.zip", 252416).unwrap();
        assert_eq!(info.version, 194);
        assert_eq!(info.timestamp, "20260308182256");
        assert_eq!(info.size, 252416);
        assert_eq!(info.formatted_date(), "2026-03-08 18:22:56");
    }

    #[test]
    fn test_parse_backup_name_invalid() {
        assert!(parse_backup_name("random.zip", 0).is_none());
        assert!(parse_backup_name("sps_abc_20260308.zip", 0).is_none());
        assert!(parse_backup_name("sps_194_short.zip", 0).is_none());
        assert!(parse_backup_name("sps_194_20260308182256.tar", 0).is_none());
    }

    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone, Copy)]
    enum Fake {
        /// Lists two backups and serves `sps_2_20260102000000.zip`.
        Serves,
        RejectsLogin,
        /// Logs in, then answers LIST with an error.
        CannotList,
        /// Accepts the connection and never says a word.
        Silent,
    }

    const LISTING: &str = "-rw-r--r-- 1 0 0 1111 Jan 01 00:00 sps_1_20260101000000.zip\r\n\
                           -rw-r--r-- 1 0 0 2222 Jan 02 00:00 sps_2_20260102000000.zip\r\n\
                           -rw-r--r-- 1 0 0 9 Jan 02 00:00 notes.txt\r\n";

    /// A small FTP server on localhost; returns its port and a login counter.
    fn fake_ftp(fake: Fake) -> (Server, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let logins = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&logins);
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for control in listener.incoming().flatten() {
                if matches!(fake, Fake::Silent) {
                    held.push(control);
                    continue;
                }
                let _ = serve_session(control, fake, &counter);
            }
        });
        let server = Server {
            host: "127.0.0.1".into(),
            port,
            connect_timeout: Duration::from_secs(5),
            io_timeout: Duration::from_millis(300),
        };
        (server, logins)
    }

    fn serve_session(
        mut control: TcpStream,
        fake: Fake,
        logins: &AtomicUsize,
    ) -> std::io::Result<()> {
        let mut lines = BufReader::new(control.try_clone()?).lines();
        control.write_all(b"220 ready\r\n")?;
        let mut data: Option<TcpListener> = None;
        while let Some(Ok(line)) = lines.next() {
            let (verb, _) = line.split_once(' ').unwrap_or((line.as_str(), ""));
            match verb {
                "USER" => control.write_all(b"331 password please\r\n")?,
                "PASS" => {
                    logins.fetch_add(1, Ordering::SeqCst);
                    if matches!(fake, Fake::RejectsLogin) {
                        control.write_all(b"530 Login incorrect\r\n")?;
                    } else {
                        control.write_all(b"230 logged in\r\n")?;
                    }
                }
                "TYPE" => control.write_all(b"200 ok\r\n")?,
                "PASV" => {
                    let listener = TcpListener::bind("127.0.0.1:0")?;
                    let p = listener.local_addr()?.port();
                    data = Some(listener);
                    let reply = format!(
                        "227 Entering Passive Mode (127,0,0,1,{},{})\r\n",
                        p >> 8,
                        p & 255
                    );
                    control.write_all(reply.as_bytes())?;
                }
                "LIST" | "RETR" => {
                    if matches!(fake, Fake::CannotList) {
                        control.write_all(b"451 local error\r\n")?;
                        continue;
                    }
                    control.write_all(b"150 here it comes\r\n")?;
                    let (mut stream, _) = data.take().expect("PASV first").accept()?;
                    stream.write_all(if verb == "LIST" {
                        LISTING.as_bytes()
                    } else {
                        b"zip-bytes"
                    })?;
                    drop(stream);
                    control.write_all(b"226 done\r\n")?;
                }
                "QUIT" => {
                    control.write_all(b"221 bye\r\n")?;
                    break;
                }
                _ => control.write_all(b"502 not implemented\r\n")?,
            }
        }
        Ok(())
    }

    fn user() -> Connection {
        Connection::new("http://127.0.0.1", "admin", "secret")
    }

    #[test]
    fn lists_the_backups_newest_first_and_downloads_one() {
        let (server, logins) = fake_ftp(Fake::Serves);
        let backups = list_backups_on(&server, &user()).unwrap();
        let names: Vec<_> = backups.iter().map(|b| b.filename.as_str()).collect();
        assert_eq!(
            names,
            ["sps_2_20260102000000.zip", "sps_1_20260101000000.zip"]
        );
        assert_eq!(backups[0].size, 2222);
        let zip = download_backup_on(&server, &user(), names[0]).unwrap();
        assert_eq!(zip, b"zip-bytes");
        assert_eq!(logins.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_rejected_login_is_not_tried_again() {
        let (server, logins) = fake_ftp(Fake::RejectsLogin);
        let err = list_backups_on(&server, &user()).unwrap_err();
        assert_eq!(ftp_status(&err), Some(Status::NotLoggedIn));
        assert!(err.to_string().contains("FTP login failed"), "{err}");
        assert_eq!(logins.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_failed_listing_is_an_error_not_an_empty_list() {
        let (server, logins) = fake_ftp(Fake::CannotList);
        let err = list_backups_on(&server, &user()).unwrap_err();
        assert!(err.to_string().contains("Failed to list /prog"), "{err}");
        // Other failures are still retried.
        assert_eq!(logins.load(Ordering::SeqCst), 1 + RETRY_DELAYS_MS.len());
    }

    #[test]
    fn a_server_that_never_answers_does_not_block() {
        let (server, _) = fake_ftp(Fake::Silent);
        let started = std::time::Instant::now();
        assert!(server.open(&user()).is_err());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn names_and_credentials_cannot_carry_a_second_command() {
        let (server, logins) = fake_ftp(Fake::Serves);
        for name in [
            "x.zip\r\nDELE /prog/sps_1.zip",
            "../sys/x",
            "a/b.zip",
            "",
            "..",
        ] {
            assert!(
                download_backup_on(&server, &user(), name).is_err(),
                "{name:?}"
            );
        }
        let sneaky = Connection::new("http://127.0.0.1", "admin\r\nDELE x", "secret");
        assert!(server.open(&sneaky).is_err());
        assert_eq!(logins.load(Ordering::SeqCst), 0, "nothing was sent");
    }

    #[test]
    fn test_ftp_host_reads_ipv6_and_paths() {
        let host = |h: &str| {
            ftp_host(&Connection {
                host: h.into(),
                ..Default::default()
            })
        };
        assert_eq!(host("https://[fe80::1]:8443"), "fe80::1");
        assert_eq!(host("http://[::1]"), "::1");
        assert_eq!(host("https://miniserver.local/"), "miniserver.local");
        assert_eq!(host("miniserver.local:80/path"), "miniserver.local");
    }

    #[test]
    fn test_ftp_host_strips_scheme() {
        let conn = Connection {
            host: "https://192.168.1.77".into(),
            ..Default::default()
        };
        assert_eq!(ftp_host(&conn), "192.168.1.77");
    }

    #[test]
    fn test_ftp_host_strips_port() {
        let conn = Connection {
            host: "https://192.168.1.77:8443".into(),
            ..Default::default()
        };
        assert_eq!(ftp_host(&conn), "192.168.1.77");
    }

    #[test]
    fn test_formatted_date() {
        let info = BackupInfo {
            filename: "sps_1_20260101120000.zip".into(),
            version: 1,
            timestamp: "20260101120000".into(),
            size: 0,
        };
        assert_eq!(info.formatted_date(), "2026-01-01 12:00:00");
    }
}
