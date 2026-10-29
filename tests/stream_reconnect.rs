//! `lox stream --reconnect` against a fake Miniserver that reboots: the first
//! WebSocket session announces "out of service", the next one comes back with
//! a state that changed in between.

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

/// Throwaway key: the client only encrypts its session key with it.
const PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAta3VACqnlsCCn2ICi7gy
K1qXMW0HQ4xcJCQq5CixkLfmWXm7YgyYrVh/Y2jT0sB5aG0LHWTL1xRZWOOSduWM
VGcNP/CcxMBRqFhv1pKMM4XJL6koXuMUu1fccKZ3MwNf7Gt8cmrWOYGttifU6++C
U9C1W5q6o0816juSUCkD0dx+e5wqPb945DTCYJX39QIxeFrt15FIKxpjTE0lA/jQ
UkGaNBfPhvOh1lh4/CYPDzVwiDhbi5Rivoo1o2u+KYG9QfMN2GJkmff/RNw0Iyah
4fMVA5s6NitY/OSD7ZbX7/zTyjxHVndFf7OVYbz0qYSWr6qjhrx09N+Mpl3V37Vj
uQIDAQAB
-----END PUBLIC KEY-----";

/// State UUIDs as the Miniserver sends them (little-endian fields) and as
/// they appear in the structure.
const ACTIVE: [u8; 16] = [1, 0, 0, 0, 2, 0, 3, 0, 4, 5, 6, 7, 8, 9, 10, 11];
const ACTIVE_UUID: &str = "00000001-0002-0003-0405060708090a0b";
const TEXT: [u8; 16] = [2, 0, 0, 0, 2, 0, 3, 0, 4, 5, 6, 7, 8, 9, 10, 11];
const TEXT_UUID: &str = "00000002-0002-0003-0405060708090a0b";

fn structure() -> String {
    json!({
        "controls": {
            "0000000a-0002-0003-0405060708090a0b": {
                "name": "Lamp",
                "type": "Switch",
                "room": "0000000b-0002-0003-0405060708090a0b",
                "states": { "active": ACTIVE_UUID, "text": TEXT_UUID }
            }
        },
        "rooms": { "0000000b-0002-0003-0405060708090a0b": { "name": "Lab" } }
    })
    .to_string()
}

/// One binary frame: 8-byte header (type, payload length) plus the payload.
fn frame(msg_type: u8, payload: &[u8]) -> Message {
    let mut data = vec![0x03, msg_type, 0, 0];
    data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    data.extend_from_slice(payload);
    Message::Binary(data)
}

fn value_table(value: f64) -> Message {
    let mut p = ACTIVE.to_vec();
    p.extend_from_slice(&value.to_le_bytes());
    frame(2, &p)
}

fn text_table(text: &str) -> Message {
    let mut p = TEXT.to_vec();
    p.extend_from_slice(&[0; 16]);
    p.extend_from_slice(&(text.len() as u32).to_le_bytes());
    p.extend_from_slice(text.as_bytes());
    p.resize(p.len().next_multiple_of(4), 0);
    frame(3, &p)
}

async fn serve(listener: TcpListener) {
    let sessions = Arc::new(AtomicUsize::new(0));
    loop {
        let (sock, _) = listener.accept().await.unwrap();
        let sessions = sessions.clone();
        tokio::spawn(async move { handle(sock, sessions).await });
    }
}

/// HTTP and the WebSocket share the port, as on the Miniserver.
async fn handle(mut sock: TcpStream, sessions: Arc<AtomicUsize>) {
    let mut buf = [0u8; 4096];
    let head = loop {
        let n = sock.peek(&mut buf).await.unwrap();
        let head = String::from_utf8_lossy(&buf[..n]).to_string();
        if n == 0 || head.contains("\r\n\r\n") {
            break head;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    if head.to_ascii_lowercase().contains("upgrade: websocket") {
        let ws = tokio_tungstenite::accept_async(sock).await.unwrap();
        ws_session(ws, sessions.fetch_add(1, Ordering::SeqCst)).await;
        return;
    }
    let _ = sock.read(&mut buf).await;
    let body = match head.split_whitespace().nth(1).unwrap_or("") {
        "/jdev/sys/getPublicKey" => {
            json!({"LL": {"control": "dev/sys/getPublicKey", "value": PUBLIC_KEY, "Code": "200"}})
                .to_string()
        }
        "/data/LoxApp3.json" => structure(),
        _ => String::new(),
    };
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = sock.write_all(resp.as_bytes()).await;
}

async fn ws_session(mut ws: tokio_tungstenite::WebSocketStream<TcpStream>, session: usize) {
    while let Some(Ok(msg)) = ws.next().await {
        let Message::Text(cmd) = msg else { continue };
        let value = if cmd.starts_with("jdev/sys/getkey2/") {
            json!({"key": "41424344", "salt": "abcd", "hashAlg": "SHA256"})
        } else if cmd.starts_with("jdev/sys/gettoken/") {
            json!({"token": "token", "key": "41424344", "validUntil": 4_000_000_000u64})
        } else if cmd.starts_with("authwithtoken/") {
            json!({"validUntil": 4_000_000_000u64})
        } else {
            json!("1")
        };
        let reply = json!({"LL": {"control": cmd, "value": value, "Code": "200"}});
        ws.send(Message::Text(reply.to_string())).await.unwrap();
        if cmd != "jdev/sps/enablebinstatusupdate" {
            continue;
        }
        if session == 0 {
            // dump, one change, then the reboot announcement
            for m in [value_table(0.0), text_table("idle"), value_table(1.0)] {
                ws.send(m).await.unwrap();
            }
            let _ = ws.send(frame(5, &[])).await;
            return;
        }
        // back up: `active` went back to 0 while the stream was down
        for m in [value_table(0.0), text_table("idle"), text_table("busy")] {
            ws.send(m).await.unwrap();
        }
    }
}

/// Runs `lox -o json stream ARGS` against a fresh fake Miniserver until it
/// printed `want` lines (or exited, or 30 s passed). Returns the stdout lines
/// as `state=value` / `event reason`, stderr, and whether it exited by itself
/// with success.
fn stream(args: &[&str], want: usize) -> (Vec<String>, String, Option<bool>) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    rt.spawn(serve(listener));

    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("config.yaml");
    std::fs::write(
        &cfg,
        format!("host: http://127.0.0.1:{}\nuser: test\npass: test\n", port),
    )
    .unwrap();

    let mut child = Command::new(assert_cmd::cargo::cargo_bin("lox"))
        .args(["-o", "json", "stream"])
        .args(args)
        .env("LOX_CONFIG", &cfg)
        .env("HOME", dir.path())
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (tx, rx) = mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let mut lines = Vec::new();
    while lines.len() < want {
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(line) => lines.push(summarize(&serde_json::from_str(&line).unwrap())),
            Err(_) => break,
        }
    }
    // give a stream that should end a moment to do so
    std::thread::sleep(Duration::from_millis(500));
    let exited = child.try_wait().unwrap().map(|status| status.success());
    if exited.is_none() {
        child.kill().unwrap();
        child.wait().unwrap();
    }
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    (lines, stderr, exited)
}

fn summarize(line: &Value) -> String {
    match line["event"].as_str() {
        Some(event) => match line["reason"].as_str() {
            Some(reason) => format!("{} {}", event, reason),
            None => {
                assert!(line["offline_secs"].is_u64(), "{}", line);
                event.to_string()
            }
        },
        None => format!(
            "{}={}",
            line["state"].as_str().unwrap(),
            line["value"].as_str().unwrap()
        ),
    }
}

#[test]
fn stream_reconnects_after_out_of_service() {
    let (lines, stderr, exited) = stream(&["--reconnect"], 5);
    assert_eq!(
        lines,
        [
            // the first dump is the silent baseline: only the change shows
            "active=1",
            "disconnected out_of_service",
            "reconnected",
            // the new dump: only what changed while offline
            "active=0",
            // and live changes again
            "text=busy",
        ],
        "stderr:\n{}",
        stderr
    );
    assert_eq!(exited, None, "still streaming");
    assert!(
        stderr.contains("disconnected: Miniserver going offline"),
        "{}",
        stderr
    );
    assert!(stderr.contains("reconnected after"), "{}", stderr);
}

#[test]
fn stream_reconnect_with_initial_prints_each_snapshot() {
    let (lines, stderr, _) = stream(&["--reconnect", "--initial"], 8);
    assert_eq!(
        lines,
        [
            "active=0",
            "text=idle",
            "active=1",
            "disconnected out_of_service",
            "reconnected",
            "active=0",
            "text=idle",
            "text=busy",
        ],
        "stderr:\n{}",
        stderr
    );
}

#[test]
fn stream_without_reconnect_exits_on_out_of_service() {
    let (lines, stderr, exited) = stream(&[], 2);
    assert_eq!(lines, ["active=1"], "stderr:\n{}", stderr);
    assert_eq!(exited, Some(true), "exits 0; stderr:\n{}", stderr);
    assert!(stderr.contains("--reconnect keeps streaming"), "{}", stderr);
}
