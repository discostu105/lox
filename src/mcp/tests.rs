//! Protocol and tool tests against a mocked Miniserver.

use super::*;
use httpmock::prelude::*;
use serde_json::{Value, json};

const LIGHT: &str = "0f1e2d3c-0001-1111-ffff000000000001";
const BLIND: &str = "0f1e2d3c-0002-1111-ffff000000000002";
const DOOR: &str = "0f1e2d3c-0003-1111-ffff000000000003";
const TEMP: &str = "0f1e2d3c-0004-1111-ffff000000000004";
const SWITCH: &str = "0f1e2d3c-0005-1111-ffff000000000005";
const ALARM: &str = "0f1e2d3c-0006-1111-ffff000000000006";

fn structure() -> Value {
    json!({
        "rooms": {
            "r1": { "name": "Wohnzimmer" },
            "r2": { "name": "Küche" },
        },
        "cats": { "c1": { "name": "Beleuchtung" } },
        "controls": {
            LIGHT: { "name": "Licht Wohnzimmer", "type": "LightControllerV2", "room": "r1", "cat": "c1" },
            BLIND: { "name": "Beschattung Süd", "type": "Jalousie", "room": "r1" },
            DOOR: { "name": "Haustür", "type": "DoorLock", "room": "r1" },
            TEMP: { "name": "Temperatur", "type": "InfoOnlyAnalog", "room": "r2" },
            SWITCH: { "name": "Licht Küche", "type": "Switch", "room": "r2", "cat": "c1" },
            ALARM: { "name": "Alarmanlage", "type": "Alarm", "room": "r1" },
        }
    })
}

fn mock_miniserver() -> MockServer {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/data/LoxApp3.json");
        then.status(200).json_body(structure());
    });
    server
}

fn config(server: &MockServer, data_dir: Option<&std::path::Path>) -> Config {
    Config {
        host: server.base_url(),
        user: "test".into(),
        pass: "test".into(),
        verify_ssl: Some(false),
        data_dir: data_dir.map(|d| d.to_path_buf()).unwrap_or_default(),
        ..Default::default()
    }
}

fn server_for(ms: &MockServer, opts: ServerOptions) -> McpServer {
    McpServer::with_config(opts, config(ms, None))
}

/// Send one request, return the parsed reply.
fn rpc(server: &mut McpServer, method: &str, params: Value) -> Value {
    let req = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let reply = server
        .handle_line(&req.to_string())
        .expect("requests get a reply");
    serde_json::from_str(&reply).unwrap()
}

/// Call a tool, return (structuredContent, isError).
fn call(server: &mut McpServer, tool: &str, args: Value) -> (Value, bool) {
    let r = rpc(
        server,
        "tools/call",
        json!({ "name": tool, "arguments": args }),
    );
    let result = &r["result"];
    assert!(result.is_object(), "expected a tool result, got {}", r);
    // text content always mirrors the structured content
    let text = result["content"][0]["text"].as_str().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(text).unwrap(),
        result["structuredContent"]
    );
    (
        result["structuredContent"].clone(),
        result["isError"].as_bool().unwrap(),
    )
}

fn tool_names(server: &mut McpServer) -> Vec<String> {
    let r = rpc(server, "tools/list", json!({}));
    r["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

// ── Protocol ──────────────────────────────────────────────────────────────────

#[test]
fn initialize_echoes_known_protocol_version() {
    let mut s = McpServer::new(ServerOptions::default());
    let r = rpc(
        &mut s,
        "initialize",
        json!({ "protocolVersion": "2025-03-26" }),
    );
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(r["result"]["serverInfo"]["name"], "lox");
    assert!(r["result"]["capabilities"]["tools"].is_object());
    assert!(
        r["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("list_controls")
    );
}

#[test]
fn initialize_falls_back_to_newest_version() {
    let mut s = McpServer::new(ServerOptions::default());
    let r = rpc(
        &mut s,
        "initialize",
        json!({ "protocolVersion": "1999-01-01" }),
    );
    assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
}

#[test]
fn notifications_get_no_reply() {
    let mut s = McpServer::new(ServerOptions::default());
    let n = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    assert!(s.handle_line(&n.to_string()).is_none());
}

#[test]
fn ping_returns_empty_result() {
    let mut s = McpServer::new(ServerOptions::default());
    let r = rpc(&mut s, "ping", Value::Null);
    assert_eq!(r["result"], json!({}));
}

#[test]
fn unknown_method_is_method_not_found() {
    let mut s = McpServer::new(ServerOptions::default());
    let r = rpc(&mut s, "resources/list", json!({}));
    assert_eq!(r["error"]["code"], -32601);
}

#[test]
fn malformed_json_is_parse_error() {
    let mut s = McpServer::new(ServerOptions::default());
    let r: Value = serde_json::from_str(&s.handle_line("{not json").unwrap()).unwrap();
    assert_eq!(r["error"]["code"], -32700);
    assert_eq!(r["id"], Value::Null);
}

#[test]
fn batch_replies_only_to_requests() {
    let mut s = McpServer::new(ServerOptions::default());
    let batch = json!([
        { "jsonrpc": "2.0", "id": 7, "method": "ping" },
        { "jsonrpc": "2.0", "method": "notifications/initialized" },
        { "jsonrpc": "2.0", "id": 8, "method": "ping" },
    ]);
    let r: Value = serde_json::from_str(&s.handle_line(&batch.to_string()).unwrap()).unwrap();
    let ids: Vec<i64> = r
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, vec![7, 8]);
}

#[test]
fn string_ids_are_preserved() {
    let mut s = McpServer::new(ServerOptions::default());
    let req = json!({ "jsonrpc": "2.0", "id": "abc", "method": "ping" });
    let r: Value = serde_json::from_str(&s.handle_line(&req.to_string()).unwrap()).unwrap();
    assert_eq!(r["id"], "abc");
}

#[test]
fn unknown_tool_is_invalid_params() {
    let mut s = McpServer::new(ServerOptions::default());
    let r = rpc(
        &mut s,
        "tools/call",
        json!({ "name": "self_destruct", "arguments": {} }),
    );
    assert_eq!(r["error"]["code"], -32602);
}

// ── Tool listing per policy ───────────────────────────────────────────────────

#[test]
fn default_policy_lists_read_and_action_tools_without_raw() {
    let mut s = McpServer::new(ServerOptions::default());
    let names = tool_names(&mut s);
    for t in [
        "list_controls",
        "get_control",
        "switch",
        "blind",
        "light",
        "door",
        "run_scene",
    ] {
        assert!(names.contains(&t.to_string()), "missing {}", t);
    }
    assert!(!names.contains(&"send_command".to_string()));
}

#[test]
fn read_only_hides_action_tools() {
    let opts = ServerOptions {
        read_only: true,
        ..Default::default()
    };
    let mut s = McpServer::new(opts);
    let names = tool_names(&mut s);
    assert!(names.contains(&"get_control".to_string()));
    assert!(
        !names
            .iter()
            .any(|n| n == "switch" || n == "blind" || n == "run_scene")
    );
    // hidden tools cannot be called either
    let r = rpc(
        &mut s,
        "tools/call",
        json!({ "name": "switch", "arguments": {} }),
    );
    assert_eq!(r["error"]["code"], -32602);
}

#[test]
fn allow_raw_adds_send_command() {
    let opts = ServerOptions {
        allow_raw: true,
        ..Default::default()
    };
    assert!(tool_names(&mut McpServer::new(opts)).contains(&"send_command".to_string()));
}

#[test]
fn tool_definitions_are_well_formed() {
    let opts = ServerOptions {
        allow_raw: true,
        ..Default::default()
    };
    for t in tool_defs(&opts) {
        let name = t["name"].as_str().unwrap();
        assert!(!t["description"].as_str().unwrap().is_empty(), "{}", name);
        assert_eq!(t["inputSchema"]["type"], "object", "{}", name);
        assert!(t["annotations"]["readOnlyHint"].is_boolean(), "{}", name);
        if let Some(req) = t["inputSchema"]["required"].as_array() {
            for r in req {
                let key = r.as_str().unwrap();
                assert!(
                    t["inputSchema"]["properties"][key].is_object(),
                    "{}.{}",
                    name,
                    key
                );
            }
        }
    }
}

#[test]
fn risky_tools_are_marked_destructive() {
    let defs = tool_defs(&ServerOptions::default());
    let ann = |n: &str| {
        defs.iter().find(|t| t["name"] == n).unwrap()["annotations"]["destructiveHint"].clone()
    };
    assert_eq!(ann("door"), true);
    assert_eq!(ann("gate"), true);
    assert_eq!(ann("blind"), false);
}

// ── Read tools ────────────────────────────────────────────────────────────────

#[test]
fn list_rooms_counts_controls() {
    let ms = mock_miniserver();
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(&mut s, "list_rooms", json!({}));
    assert!(!err, "{}", out);
    assert_eq!(out["count"], 2);
    let wz = out["rooms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "Wohnzimmer")
        .unwrap();
    assert_eq!(wz["controls"], 4);
}

#[test]
fn list_controls_filters_and_names_tools() {
    let ms = mock_miniserver();
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, _) = call(
        &mut s,
        "list_controls",
        json!({ "room": "wohn", "type": "Jalousie" }),
    );
    assert_eq!(out["count"], 1);
    let c = &out["controls"][0];
    assert_eq!(c["name"], "Beschattung Süd");
    assert_eq!(c["tool"], "blind");
    assert_eq!(c["uuid"], BLIND);

    let (out, _) = call(&mut s, "list_controls", json!({ "name": "licht" }));
    assert_eq!(out["count"], 2);
    let (out, _) = call(&mut s, "list_controls", json!({ "category": "beleucht" }));
    assert_eq!(out["count"], 2);
}

#[test]
fn get_control_returns_live_state() {
    let ms = mock_miniserver();
    ms.mock(|when, then| {
        when.method(GET).path(format!("/dev/sps/io/{}/all", BLIND));
        then.status(200).body(format!(
            r#"<LL control="dev/sps/io/{}/all" value="0.3" Code="200" StatePos="0.3" StateShade="0"/>"#,
            BLIND
        ));
    });
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(&mut s, "get_control", json!({ "name": "Beschattung" }));
    assert!(!err, "{}", out);
    assert_eq!(out["value"], 0.3);
    assert_eq!(out["states"]["StatePos"], 0.3);
    assert_eq!(out["room"], "Wohnzimmer");
}

#[test]
fn ambiguous_name_is_a_tool_error_with_code() {
    let ms = mock_miniserver();
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(&mut s, "get_control", json!({ "name": "Licht" }));
    assert!(err);
    assert_eq!(out["ok"], false);
    assert_eq!(out["error"], "ambiguous_control");
    assert!(out["message"].as_str().unwrap().contains("Licht Küche"));
}

#[test]
fn room_disambiguates() {
    let ms = mock_miniserver();
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(
        &mut s,
        "switch",
        json!({ "name": "Licht", "room": "Küche", "state": "on", "dry_run": true }),
    );
    assert!(!err, "{}", out);
    assert_eq!(out["control"]["uuid"], SWITCH);
}

#[test]
fn unknown_control_is_control_not_found() {
    let ms = mock_miniserver();
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(&mut s, "get_control", json!({ "name": "Sauna" }));
    assert!(err);
    assert_eq!(out["error"], "control_not_found");
}

#[test]
fn list_sensors_reads_values() {
    let ms = mock_miniserver();
    ms.mock(|when, then| {
        when.method(GET).path(format!("/dev/sps/io/{}/all", TEMP));
        then.status(200).body(r#"<LL value="21.5" Code="200"/>"#);
    });
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(&mut s, "list_sensors", json!({ "kind": "temperature" }));
    assert!(!err, "{}", out);
    assert_eq!(out["count"], 1);
    assert_eq!(out["sensors"][0]["value"], 21.5);

    let (out, err) = call(&mut s, "list_sensors", json!({ "kind": "humidity" }));
    assert!(err);
    assert_eq!(out["error"], "invalid_arguments");
}

#[test]
fn system_status_parses_values() {
    let ms = mock_miniserver();
    for (path, value) in [
        ("/dev/cfg/version", "15.2.10.14"),
        ("/dev/sps/state", "5"),
        ("/dev/sys/heap", "12000/32000kB"),
    ] {
        ms.mock(|when, then| {
            when.method(GET).path(path);
            then.status(200).body(format!(
                r#"<LL control="{}" value="{}" Code="200"/>"#,
                path, value
            ));
        });
    }
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(&mut s, "system_status", json!({}));
    assert!(!err, "{}", out);
    assert_eq!(out["firmware"], "15.2.10.14");
    assert_eq!(out["plc_running"], true);
    assert_eq!(out["heap_total_kb"], 32000.0);
}

// ── Action tools ──────────────────────────────────────────────────────────────

#[test]
fn switch_sends_command() {
    let ms = mock_miniserver();
    let m = ms.mock(|when, then| {
        when.method(GET).path(format!("/jdev/sps/io/{}/on", SWITCH));
        then.status(200)
            .json_body(json!({ "LL": { "control": "on", "value": "1", "Code": "200" } }));
    });
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(
        &mut s,
        "switch",
        json!({ "name": "Licht Küche", "state": "on" }),
    );
    assert!(!err, "{}", out);
    m.assert();
    assert_eq!(out["ok"], true);
    assert_eq!(out["code"], 200);
    assert_eq!(out["value"], 1);
    assert_eq!(out["cli"], "lox on \"Licht Küche\" -r \"Küche\"");
}

#[test]
fn blind_position_dry_run_sends_nothing() {
    let ms = mock_miniserver();
    let m = ms.mock(|when, then| {
        when.method(GET).path_contains("/jdev/sps/io/");
        then.status(200)
            .json_body(json!({ "LL": { "Code": "200" } }));
    });
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(
        &mut s,
        "blind",
        json!({ "name": "Beschattung", "action": "position", "value": 40, "dry_run": true }),
    );
    assert!(!err, "{}", out);
    assert_eq!(out["dry_run"], true);
    assert_eq!(out["commands"], json!(["manualPosition/40.0000"]));
    m.assert_hits(0);
}

#[test]
fn global_dry_run_forces_dry_run() {
    let ms = mock_miniserver();
    let opts = ServerOptions {
        dry_run: true,
        ..Default::default()
    };
    let mut s = server_for(&ms, opts);
    let (out, _) = call(
        &mut s,
        "switch",
        json!({ "name": "Licht Küche", "state": "off" }),
    );
    assert_eq!(out["dry_run"], true);
}

#[test]
fn blind_validates_arguments() {
    let ms = mock_miniserver();
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(
        &mut s,
        "blind",
        json!({ "name": "Beschattung", "action": "position" }),
    );
    assert!(err);
    assert_eq!(out["error"], "invalid_arguments");
    let (out, err) = call(
        &mut s,
        "blind",
        json!({ "name": "Beschattung", "action": "position", "value": 140 }),
    );
    assert!(err);
    assert!(out["message"].as_str().unwrap().contains("0-100"));
}

#[test]
fn missing_required_argument_is_invalid_arguments() {
    let ms = mock_miniserver();
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(&mut s, "switch", json!({ "state": "on" }));
    assert!(err);
    assert_eq!(out["error"], "invalid_arguments");
    assert!(out["message"].as_str().unwrap().contains("'name'"));
}

#[test]
fn action_on_wrong_type_is_rejected() {
    let ms = mock_miniserver();
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(
        &mut s,
        "blind",
        json!({ "name": "Licht Küche", "action": "up" }),
    );
    assert!(err);
    assert!(out["message"].as_str().unwrap().contains("not a Jalousie"));
}

#[test]
fn light_mood_and_dim() {
    let ms = mock_miniserver();
    let m = ms.mock(|when, then| {
        when.method(GET)
            .path(format!("/jdev/sps/io/{}/changeTo/778", LIGHT));
        then.status(200)
            .json_body(json!({ "LL": { "value": "778", "Code": "200" } }));
    });
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(
        &mut s,
        "light",
        json!({ "name": "Licht Wohnzimmer", "action": "mood", "value": "off" }),
    );
    assert!(!err, "{}", out);
    m.assert();

    let (out, err) = call(
        &mut s,
        "light",
        json!({ "name": "Licht Küche", "action": "dim", "value": 30, "dry_run": true }),
    );
    assert!(!err, "{}", out);
    assert_eq!(out["commands"], json!(["30"]));
}

#[test]
fn risky_action_is_refused_by_default() {
    let ms = mock_miniserver();
    let m = ms.mock(|when, then| {
        when.method(GET).path_contains("/jdev/sps/io/");
        then.status(200)
            .json_body(json!({ "LL": { "Code": "200" } }));
    });
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(
        &mut s,
        "door",
        json!({ "name": "Haustür", "action": "open" }),
    );
    assert!(err);
    assert_eq!(out["error"], "action_not_allowed");
    assert!(out["message"].as_str().unwrap().contains("--allow-risky"));
    m.assert_hits(0);

    // a dry run is fine and reports that the real call would be refused
    let (out, err) = call(
        &mut s,
        "door",
        json!({ "name": "Haustür", "action": "open", "dry_run": true }),
    );
    assert!(!err);
    assert_eq!(out["allowed"], false);
    m.assert_hits(0);
}

#[test]
fn risky_action_runs_with_allow_risky() {
    let ms = mock_miniserver();
    let m = ms.mock(|when, then| {
        when.method(GET).path(format!("/jdev/sps/io/{}/open", DOOR));
        then.status(200)
            .json_body(json!({ "LL": { "value": "1", "Code": "200" } }));
    });
    let opts = ServerOptions {
        allow_risky: true,
        ..Default::default()
    };
    let mut s = server_for(&ms, opts);
    let (out, err) = call(
        &mut s,
        "door",
        json!({ "name": "Haustür", "action": "open" }),
    );
    assert!(!err, "{}", out);
    m.assert();
}

#[test]
fn alarm_quit_is_not_risky_and_pin_is_hidden() {
    let ms = mock_miniserver();
    let m = ms.mock(|when, then| {
        when.method(GET)
            .path(format!("/jdev/sps/io/{}/quit", ALARM));
        then.status(200)
            .json_body(json!({ "LL": { "Code": "200" } }));
    });
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(
        &mut s,
        "alarm",
        json!({ "name": "Alarm", "action": "quit" }),
    );
    assert!(!err, "{}", out);
    m.assert();

    let (out, _) = call(
        &mut s,
        "alarm",
        json!({ "name": "Alarm", "action": "disarm", "code": "4711", "dry_run": true }),
    );
    assert!(!out.to_string().contains("4711"), "{}", out);
}

#[test]
fn miniserver_error_code_is_a_tool_error() {
    let ms = mock_miniserver();
    ms.mock(|when, then| {
        when.method(GET)
            .path(format!("/jdev/sps/io/{}/pulse", SWITCH));
        then.status(200)
            .json_body(json!({ "LL": { "value": "0", "Code": "500" } }));
    });
    let mut s = server_for(&ms, ServerOptions::default());
    let (out, err) = call(
        &mut s,
        "switch",
        json!({ "name": "Licht Küche", "state": "pulse" }),
    );
    assert!(err);
    assert!(out["message"].as_str().unwrap().contains("code 500"));
}

#[test]
fn send_command_passes_raw_command() {
    let ms = mock_miniserver();
    let m = ms.mock(|when, then| {
        when.method(GET)
            .path(format!("/jdev/sps/io/{}/FullUp", BLIND));
        then.status(200)
            .json_body(json!({ "LL": { "Code": "200" } }));
    });
    let opts = ServerOptions {
        allow_raw: true,
        ..Default::default()
    };
    let mut s = server_for(&ms, opts);
    let (out, err) = call(
        &mut s,
        "send_command",
        json!({ "name": "Beschattung", "command": "FullUp" }),
    );
    assert!(!err, "{}", out);
    m.assert();
}

// ── Scenes ────────────────────────────────────────────────────────────────────

#[test]
fn scenes_are_listed_and_run() {
    let ms = mock_miniserver();
    let on = ms.mock(|when, then| {
        when.method(GET).path(format!("/jdev/sps/io/{}/on", SWITCH));
        then.status(200)
            .json_body(json!({ "LL": { "Code": "200" } }));
    });
    let down = ms.mock(|when, then| {
        when.method(GET)
            .path(format!("/jdev/sps/io/{}/FullDown", BLIND));
        then.status(200)
            .json_body(json!({ "LL": { "Code": "200" } }));
    });
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("scenes/abend.yaml"),
        "name: Abend\ndescription: Evening\nsteps:\n  - control: Licht Küche\n    cmd: \"on\"\n  - control: Beschattung\n    cmd: FullDown\n  - control: Sauna\n    cmd: \"on\"\n",
    )
    .unwrap();
    let mut s = McpServer::with_config(ServerOptions::default(), config(&ms, Some(dir.path())));

    let (out, err) = call(&mut s, "list_scenes", json!({}));
    assert!(!err, "{}", out);
    assert_eq!(out["scenes"][0]["scene"], "abend");
    assert_eq!(out["scenes"][0]["steps"], 3);

    let (out, err) = call(&mut s, "run_scene", json!({ "scene": "abend" }));
    assert!(!err, "{}", out);
    on.assert();
    down.assert();
    assert_eq!(out["ok"], false, "the unknown control step fails");
    assert_eq!(out["steps"][2]["ok"], false);

    let (out, err) = call(&mut s, "run_scene", json!({ "scene": "nope" }));
    assert!(err, "{}", out);
}

#[test]
fn tools_list_needs_no_config() {
    // Config is loaded lazily on the first tool call, so a client can list
    // tools before `lox setup` has run.
    let mut s = McpServer::new(ServerOptions::default());
    assert!(!tool_names(&mut s).is_empty());
}
