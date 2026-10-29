//! `lox files get` against a mock Miniserver: where the file goes and what is
//! printed.

use assert_cmd::Command;
use httpmock::prelude::*;
use serde_json::Value;

/// Not valid UTF-8 on purpose: stdout must carry the bytes unchanged.
const BODY: &[u8] = b"line 1\nline 2\n\xff\xfe\x00end";

fn setup() -> (MockServer, tempfile::TempDir) {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/dev/fsget//log/def.log");
        then.status(200).body(BODY);
    });
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.yaml"),
        format!("host: {}\nuser: test\npass: test\n", server.base_url()),
    )
    .unwrap();
    (server, dir)
}

fn lox(dir: &tempfile::TempDir) -> Command {
    let mut cmd = Command::cargo_bin("lox").unwrap();
    cmd.env("LOX_CONFIG", dir.path().join("config.yaml"))
        .env("HOME", dir.path())
        .current_dir(dir.path());
    cmd
}

#[test]
fn files_get_save_as_dash_writes_stdout() {
    let (_server, dir) = setup();
    let out = lox(&dir)
        .args(["files", "get", "/log/def.log", "--save-as", "-"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    assert_eq!(out.stdout, BODY);
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!dir.path().join("def.log").exists());
}

#[test]
fn files_get_default_names_the_absolute_path() {
    let (_server, dir) = setup();
    let out = lox(&dir)
        .args(["files", "get", "log/def.log"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    // canonical: the child sees its cwd that way (macOS: /var → /private/var)
    let saved = dir.path().canonicalize().unwrap().join("def.log");
    assert_eq!(std::fs::read(&saved).unwrap(), BODY);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout.trim_end(),
        format!(
            "✓  Downloaded /log/def.log → {} ({} bytes)",
            saved.display(),
            BODY.len()
        )
    );
    // stdout is captured here, as with `> file`: say where the file went
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("--save-as -"), "{}", stderr);
}

#[test]
fn files_get_json_and_quiet() {
    let (_server, dir) = setup();
    let out = lox(&dir)
        .args([
            "files",
            "get",
            "/log/def.log",
            "--save-as",
            "sub.log",
            "-o",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let saved = dir.path().canonicalize().unwrap().join("sub.log");
    assert_eq!(v["ok"], true);
    assert_eq!(v["path"], "/log/def.log");
    assert_eq!(v["saved_as"], saved.to_str().unwrap());
    assert_eq!(v["bytes"], BODY.len());

    let out = lox(&dir)
        .args(["-q", "files", "get", "/log/def.log"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{:?}", out);
    assert_eq!(std::fs::read(dir.path().join("def.log")).unwrap(), BODY);
}
