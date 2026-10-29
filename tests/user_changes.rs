//! `lox user nfc add/rm` and `lox user code set` against a mock Miniserver:
//! confirmation, dry run, and an access code that never shows up in output
//! or the -vv log.

use assert_cmd::Command;
use httpmock::prelude::*;
use serde_json::{Value, json};

const ALICE: &str = "1a2b3c4d-0001-0001-ffff000000000001";
const BOB: &str = "1a2b3c4d-0001-0002-ffff000000000002";
const CODE: &str = "73519";

fn ll(control: &str, value: &Value, code: &str) -> Value {
    json!({ "LL": { "control": control, "value": value.to_string(), "Code": code } })
}

/// alice has one NFC tag, bob an access code.
fn setup() -> (MockServer, tempfile::TempDir) {
    let server = MockServer::start();
    let users = json!([
        { "name": "alice", "uuid": ALICE, "isAdmin": false, "userState": 0 },
        { "name": "bob", "uuid": BOB, "isAdmin": false, "userState": 0 },
    ]);
    server.mock(|when, then| {
        when.method(GET).path("/jdev/sps/getuserlist2");
        then.status(200)
            .json_body(ll("dev/sps/getuserlist2", &users, "200"));
    });
    let alice = json!({ "name": "alice", "uuid": ALICE, "userState": 0, "usergroups": [],
        "nfcTags": [{ "name": "alice phone", "id": "01 02 03 04 05 06 07 08 EC" }], "keycodes": [] });
    let bob = json!({ "name": "bob", "uuid": BOB, "userState": 0, "usergroups": [],
        "nfcTags": [], "keycodes": [{}] });
    for (uuid, user) in [(ALICE, alice), (BOB, bob)] {
        let control = format!("dev/sps/getuser/{}", uuid);
        server.mock(|when, then| {
            when.method(GET).path(format!("/jdev/sps/getuser/{}", uuid));
            then.status(200).json_body(ll(&control, &user, "200"));
        });
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.yaml"),
        format!("host: {}\nuser: test\npass: test\n", server.base_url()),
    )
    .unwrap();
    (server, dir)
}

/// The Miniserver's answer to setting bob's code, echoing the path as it does.
fn code_answer<'a>(server: &'a MockServer, code: &str) -> httpmock::Mock<'a> {
    let control = format!("dev/sps/updateuseraccesscode/{}/{}", BOB, CODE);
    let body = ll(&control, &json!(""), code);
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/jdev/sps/updateuseraccesscode/{}/{}", BOB, CODE));
        then.status(200).json_body(body);
    })
}

fn lox(dir: &tempfile::TempDir) -> Command {
    let mut cmd = Command::cargo_bin("lox").unwrap();
    cmd.env("LOX_CONFIG", dir.path().join("config.yaml"))
        .env("HOME", dir.path())
        .current_dir(dir.path());
    cmd
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn code_set_reads_stdin_and_keeps_the_code_out_of_the_log() {
    let (server, dir) = setup();
    let m = code_answer(&server, "200");
    let out = lox(&dir)
        .args(["-vv", "user", "code", "set", "bob", "--yes"])
        .write_stdin(format!("{}\n", CODE))
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    m.assert_hits(1);
    let (stdout, stderr) = (text(&out.stdout), text(&out.stderr));
    assert!(
        stdout.contains("Set a new access code for bob"),
        "{}",
        stdout
    );
    assert!(stderr.contains("updateuseraccesscode"), "-vv logs the call");
    assert!(stderr.contains("note:"), "{}", stderr);
    assert!(
        !stdout.contains(CODE) && !stderr.contains(CODE),
        "{}\n{}",
        stdout,
        stderr
    );
}

#[test]
fn code_set_without_yes_and_without_a_terminal_changes_nothing() {
    let (server, dir) = setup();
    let m = code_answer(&server, "200");
    let out = lox(&dir)
        .args(["-o", "json", "user", "code", "set", "bob"])
        .write_stdin(format!("{}\n", CODE))
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"], "confirmation_required", "{}", v);
    m.assert_hits(0);
}

#[test]
fn a_code_in_use_is_named_and_not_shown() {
    let (server, dir) = setup();
    let m = code_answer(&server, "409");
    let out = lox(&dir)
        .args(["-o", "json", "user", "code", "set", "bob", "--yes"])
        .write_stdin(CODE)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"], "code_in_use", "{}", v);
    assert!(!v.to_string().contains(CODE), "{}", v);
    m.assert_hits(1);
    // a malformed code is refused before anything is sent
    let out = lox(&dir)
        .args(["user", "code", "set", "bob", "--yes"])
        .write_stdin("12a4\n")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("2 to 8 digits"));
    m.assert_hits(1);
}

#[test]
fn nfc_add_dry_run_shows_the_request_and_sends_nothing() {
    let (server, dir) = setup();
    let m = server.mock(|when, then| {
        when.method(GET).path_contains("addusernfc");
        then.status(200);
    });
    let out = lox(&dir)
        .args([
            "-o",
            "json",
            "--dry-run",
            "user",
            "nfc",
            "add",
            "bob",
            "aa:bb:cc:dd",
            "bob card",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["tag"]["id"], "AA BB CC DD");
    assert_eq!(
        v["requests"][0],
        format!(
            "GET /jdev/sps/addusernfc/{}/AA%20BB%20CC%20DD/bob%20card",
            BOB
        )
    );
    m.assert_hits(0);
}

#[test]
fn nfc_rm_by_name_sends_the_tag_id() {
    let (server, dir) = setup();
    let m = server.mock(|when, then| {
        when.method(GET).path(format!(
            "/jdev/sps/removeusernfc/{}/01%2002%2003%2004%2005%2006%2007%2008%20EC",
            ALICE
        ));
        then.status(200)
            .json_body(ll("dev/sps/removeusernfc", &json!(""), "200"));
    });
    let out = lox(&dir)
        .args(["user", "nfc", "rm", "alice", "Alice Phone", "--yes"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    assert!(text(&out.stdout).contains("Removed the NFC tag 'alice phone'"));
    m.assert_hits(1);
    // unknown tag: the tags she has
    let out = lox(&dir)
        .args(["-o", "json", "user", "nfc", "rm", "alice", "spare", "--yes"])
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"], "tag_not_found");
    assert!(v["message"].as_str().unwrap().contains("alice phone"));
    m.assert_hits(1);
}
