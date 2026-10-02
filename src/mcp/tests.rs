//! Protocol tests: a real rmcp client talks to `LoxMcp` over an in-memory
//! duplex, against an `httpmock` Miniserver. Every behavior that depends on the
//! lifecycle runs under both MCP 2026-07-28 (`server/discover`, multi-round-trip
//! confirmation) and 2025-11-25 (`initialize`, server-to-client elicitation).

use super::*;
use httpmock::prelude::*;
use rmcp::{
    ClientHandler, ErrorData, RoleClient,
    model::{
        CallToolRequestParams, ClientCapabilities, ClientConfig, ElicitRequestParams, ElicitResult,
        ElicitationAction, Implementation, NumberOrString, ProgressNotificationParam,
        ProgressToken, ProtocolVersion, RequestMetaObject,
    },
    service::{
        ClientLifecycleMode, ClientServiceExt, NotificationContext, RequestContext, RunningService,
    },
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::config::Config;

const LIGHT: &str = "0f1e2d3c-0001-1111-ffff000000000001";
const BLIND: &str = "0f1e2d3c-0002-1111-ffff000000000002";
const DOOR: &str = "0f1e2d3c-0003-1111-ffff000000000003";
const TEMP: &str = "0f1e2d3c-0004-1111-ffff000000000004";
const SWITCH: &str = "0f1e2d3c-0005-1111-ffff000000000005";
const ALARM: &str = "0f1e2d3c-0006-1111-ffff000000000006";
const THERMO: &str = "0f1e2d3c-0007-1111-ffff000000000007";
const OPENER: &str = "0f1e2d3c-0008-1111-ffff000000000008";

fn structure() -> Value {
    json!({
        "rooms": {
            "r1": { "name": "Wohnzimmer" },
            "r2": { "name": "Küche" },
            "r3": { "name": "Zentral" },
        },
        "cats": {
            "c1": { "name": "Beleuchtung", "image": "IconsFilled/lightbulb-3.svg" },
            "c2": { "name": "Zutritt", "image": "IconsFilled/door-open.svg" },
        },
        "controls": {
            LIGHT: { "name": "Licht Wohnzimmer", "type": "LightControllerV2", "room": "r1", "cat": "c1" },
            BLIND: { "name": "Beschattung Süd", "type": "Jalousie", "room": "r1" },
            DOOR: { "name": "Haustür", "type": "DoorLock", "room": "r1" },
            TEMP: { "name": "Temperatur", "type": "InfoOnlyAnalog", "room": "r2", "details": { "format": "%.1f°" } },
            "0f1e2d3c-0009-1111-ffff000000000009": { "name": "CO2", "type": "InfoOnlyAnalog", "room": "r2", "details": { "format": "%.0fppm" } },
            SWITCH: { "name": "Licht Küche", "type": "Switch", "room": "r2", "cat": "c1" },
            ALARM: { "name": "Alarmanlage", "type": "Alarm", "room": "r1" },
            // a real installation: the thermostat is named like its room, and the
            // front door opener is a plain push-button in an access category
            THERMO: { "name": "Zentral", "type": "IRoomControllerV2", "room": "r3" },
            "0f1e2d3c-000a-1111-ffff00000000000a": { "name": "Jalousie Zentral", "type": "Jalousie", "room": "r3" },
            "0f1e2d3c-000b-1111-ffff00000000000b": { "name": "Spots Küche", "type": "Dimmer", "room": "r2" },
            OPENER: { "name": "Tür öffnen", "type": "Pushbutton", "room": "r3", "cat": "c2" },
        }
    })
}

async fn miniserver() -> MockServer {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/data/LoxApp3.json");
            then.status(200).json_body(structure());
        })
        .await;
    server
}

/// Mock one command endpoint answering Code 200.
async fn command<'a>(ms: &'a MockServer, uuid: &str, cmd: &str) -> httpmock::Mock<'a> {
    let path = format!("/jdev/sps/io/{}/{}", uuid, cmd);
    let cmd = cmd.to_string();
    ms.mock_async(|when, then| {
        when.method(GET).path(path);
        then.status(200)
            .json_body(json!({ "LL": { "control": cmd, "value": "1", "Code": "200" } }));
    })
    .await
}

/// Mock every command endpoint (to prove nothing was sent).
async fn any_command(ms: &MockServer) -> httpmock::Mock<'_> {
    ms.mock_async(|when, then| {
        when.method(GET).path_contains("/jdev/sps/io/");
        then.status(200)
            .json_body(json!({ "LL": { "Code": "200" } }));
    })
    .await
}

fn config(ms: &MockServer, data_dir: Option<&std::path::Path>) -> Config {
    Config {
        host: ms.base_url(),
        user: "test".into(),
        pass: "test".into(),
        verify_ssl: Some(false),
        data_dir: data_dir.map(|d| d.to_path_buf()).unwrap_or_default(),
        ..Default::default()
    }
}

// ── Test client ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Lifecycle {
    /// MCP 2026-07-28: `server/discover`, per-request `_meta`, MRTR
    Modern,
    /// MCP 2025-11-25: `initialize` handshake, server-to-client requests
    Legacy,
}

const BOTH: [Lifecycle; 2] = [Lifecycle::Modern, Lifecycle::Legacy];

#[derive(Clone, Copy, PartialEq)]
enum UserAnswer {
    /// The client has no elicitation capability.
    CannotAsk,
    Accept,
    Decline,
}

#[derive(Clone)]
struct TestClient {
    lifecycle: Lifecycle,
    answer: UserAnswer,
    questions: Arc<Mutex<Vec<String>>>,
    progress: Arc<AtomicUsize>,
}

impl TestClient {
    fn new(lifecycle: Lifecycle, answer: UserAnswer) -> Self {
        Self {
            lifecycle,
            answer,
            questions: Arc::default(),
            progress: Arc::default(),
        }
    }
}

impl ClientHandler for TestClient {
    fn get_info(&self) -> ClientConfig {
        let caps = if self.answer == UserAnswer::CannotAsk {
            ClientCapabilities::default()
        } else {
            ClientCapabilities::builder().enable_elicitation().build()
        };
        let mut info = ClientConfig::new(caps, Implementation::new("lox-test", "0"));
        if self.lifecycle == Lifecycle::Legacy {
            info.protocol_version = ProtocolVersion::V_2025_11_25;
        }
        info
    }

    async fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, ErrorData> {
        if let ElicitRequestParams::FormElicitationParams { message, .. } = &request {
            self.questions.lock().unwrap().push(message.clone());
        }
        Ok(match self.answer {
            UserAnswer::Accept => ElicitResult::new(ElicitationAction::Accept)
                .with_content(json!({ "confirm": true })),
            _ => ElicitResult::new(ElicitationAction::Decline),
        })
    }

    async fn on_progress(
        &self,
        _params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        self.progress.fetch_add(1, Ordering::SeqCst);
    }
}

type Client = RunningService<RoleClient, TestClient>;

async fn connect(server: LoxMcp, client: TestClient) -> Client {
    let (server_io, client_io) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    let lifecycle = match client.lifecycle {
        Lifecycle::Modern => ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        },
        Lifecycle::Legacy => ClientLifecycleMode::Initialize,
    };
    client
        .serve_with_lifecycle(client_io, lifecycle)
        .await
        .expect("client connects")
}

async fn session(ms: &MockServer, opts: ServerOptions, lifecycle: Lifecycle) -> Client {
    connect(
        LoxMcp::with_config(opts, config(ms, None)),
        TestClient::new(lifecycle, UserAnswer::CannotAsk),
    )
    .await
}

/// Call a tool; return (structured content or parsed error envelope, isError).
async fn call(client: &Client, tool: &str, args: Value) -> (Value, bool) {
    call_with(client, CallToolRequestParams::new(tool.to_string()), args).await
}

async fn call_with(client: &Client, params: CallToolRequestParams, args: Value) -> (Value, bool) {
    let params = params.with_arguments(args.as_object().cloned().unwrap_or_default());
    let result = client.call_tool(params).await.expect("tools/call");
    let is_error = result.is_error.unwrap_or(false);
    let value = match &result.structured_content {
        Some(v) => v.clone(),
        None => {
            let text = result
                .content
                .first()
                .and_then(|c| c.as_text())
                .map(|t| t.text.clone())
                .unwrap_or_default();
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        }
    };
    (value, is_error)
}

async fn tool_names(client: &Client) -> Vec<String> {
    client
        .list_all_tools()
        .await
        .expect("tools/list")
        .into_iter()
        .map(|t| t.name.to_string())
        .collect()
}

// ── Lifecycle and listing ─────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn both_lifecycles_connect_and_identify_the_server() {
    for lifecycle in BOTH {
        let ms = miniserver().await;
        let client = session(&ms, ServerOptions::default(), lifecycle).await;
        let info = client.peer_info().expect("server info");
        assert_eq!(
            info.server_info.as_ref().map(|s| s.name.as_str()),
            Some("lox"),
            "{:?}",
            lifecycle
        );
        let expected = match lifecycle {
            Lifecycle::Modern => ProtocolVersion::V_2026_07_28,
            Lifecycle::Legacy => ProtocolVersion::V_2025_11_25,
        };
        assert_eq!(info.protocol_version, expected);
        assert!(
            info.instructions
                .as_deref()
                .is_some_and(|i| i.contains("list_controls"))
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn default_policy_lists_read_and_action_tools_without_raw() {
    let ms = miniserver().await;
    let client = session(&ms, ServerOptions::default(), Lifecycle::Modern).await;
    let names = tool_names(&client).await;
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
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "deterministic tool order (2026-07-28)");
}

#[tokio::test(flavor = "multi_thread")]
async fn tools_list_carries_cache_hints_only_for_2026_07_28() {
    let ms = miniserver().await;
    for (lifecycle, expect) in [(Lifecycle::Modern, true), (Lifecycle::Legacy, false)] {
        let client = session(&ms, ServerOptions::default(), lifecycle).await;
        let list = client.list_tools(None).await.expect("tools/list");
        assert_eq!(list.ttl_ms.is_some(), expect, "{:?}", lifecycle);
        assert_eq!(list.cache_scope.is_some(), expect, "{:?}", lifecycle);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn read_only_hides_action_tools() {
    let ms = miniserver().await;
    let opts = ServerOptions {
        read_only: true,
        ..Default::default()
    };
    let client = session(&ms, opts, Lifecycle::Modern).await;
    let names = tool_names(&client).await;
    assert!(names.contains(&"get_control".to_string()));
    assert!(
        !names
            .iter()
            .any(|n| n == "switch" || n == "blind" || n == "run_scene")
    );
    let result = client
        .call_tool(CallToolRequestParams::new("switch").with_arguments(Default::default()))
        .await;
    let refused = match result {
        Err(_) => true,
        Ok(r) => r.is_error == Some(true),
    };
    assert!(refused, "hidden tools cannot be called");
}

#[test]
fn allow_raw_adds_send_command() {
    let opts = ServerOptions {
        allow_raw: true,
        ..Default::default()
    };
    let names: Vec<String> = LoxMcp::new(opts)
        .tools()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(names.contains(&"send_command".to_string()));
}

#[test]
fn every_tool_has_schemas_annotations_and_a_title() {
    let opts = ServerOptions {
        allow_raw: true,
        ..Default::default()
    };
    for t in LoxMcp::new(opts).tools() {
        let v = serde_json::to_value(&t).unwrap();
        let name = t.name.to_string();
        assert!(
            !v["description"].as_str().unwrap_or("").is_empty(),
            "{}",
            name
        );
        assert!(v["title"].is_string(), "{}", name);
        assert_eq!(v["inputSchema"]["type"], "object", "{}", name);
        assert_eq!(v["outputSchema"]["type"], "object", "{}", name);
        assert!(v["annotations"]["readOnlyHint"].is_boolean(), "{}", name);
        let schema = v["inputSchema"].to_string();
        assert!(
            !schema.contains("$ref"),
            "{}: input schema should be inline",
            name
        );
    }
}

#[test]
fn output_schemas_do_not_require_fields_that_may_be_omitted() {
    // Clients validate structuredContent against outputSchema; a field that is
    // skipped when empty/false must not be listed as required.
    let optional = [
        "states",
        "outputs",
        "secured",
        "dry_run",
        "needs_confirmation",
        "confirmed",
        "cancelled",
        "room",
        "category",
        "tool",
        "code",
        "error",
    ];
    fn walk(v: &Value, optional: &[&str], tool: &str) {
        if let Some(req) = v.get("required").and_then(|r| r.as_array()) {
            for r in req {
                let name = r.as_str().unwrap_or("");
                assert!(
                    !optional.contains(&name),
                    "{}: '{}' is required in the output schema but may be omitted",
                    tool,
                    name
                );
            }
        }
        match v {
            Value::Object(m) => m.values().for_each(|x| walk(x, optional, tool)),
            Value::Array(a) => a.iter().for_each(|x| walk(x, optional, tool)),
            _ => {}
        }
    }
    let opts = ServerOptions {
        allow_raw: true,
        ..Default::default()
    };
    for t in LoxMcp::new(opts).tools() {
        let v = serde_json::to_value(&t).unwrap();
        walk(&v["outputSchema"], &optional, &t.name);
    }
}

#[test]
fn risky_tools_are_marked_destructive() {
    let tools = LoxMcp::new(ServerOptions::default()).tools();
    let destructive = |n: &str| {
        let t = tools.iter().find(|t| t.name == n).unwrap();
        serde_json::to_value(t).unwrap()["annotations"]["destructiveHint"].clone()
    };
    assert_eq!(destructive("door"), true);
    assert_eq!(destructive("gate"), true);
    assert_eq!(destructive("alarm"), true);
    assert_eq!(destructive("blind"), false);
}

// ── Read tools ────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn list_rooms_counts_controls() {
    let ms = miniserver().await;
    let client = session(&ms, ServerOptions::default(), Lifecycle::Modern).await;
    let (out, err) = call(&client, "list_rooms", json!({})).await;
    assert!(!err, "{}", out);
    assert_eq!(out["count"], 3);
    let wz = out["rooms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "Wohnzimmer")
        .unwrap();
    assert_eq!(wz["controls"], 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_controls_filters_and_names_tools() {
    let ms = miniserver().await;
    let client = session(&ms, ServerOptions::default(), Lifecycle::Legacy).await;
    let (out, _) = call(
        &client,
        "list_controls",
        json!({ "room": "wohn", "type": "Jalousie" }),
    )
    .await;
    assert_eq!(out["count"], 1);
    let c = &out["controls"][0];
    assert_eq!(c["name"], "Beschattung Süd");
    assert_eq!(c["tool"], "blind");
    assert_eq!(c["uuid"], BLIND);

    let (out, _) = call(&client, "list_controls", json!({ "name": "licht" })).await;
    assert_eq!(out["count"], 2);
    let (out, _) = call(&client, "list_controls", json!({ "category": "beleucht" })).await;
    assert_eq!(out["count"], 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_control_returns_live_state() {
    let ms = miniserver().await;
    ms.mock_async(|when, then| {
        when.method(GET).path(format!("/dev/sps/io/{}/all", BLIND));
        then.status(200).body(format!(
            r#"<LL control="dev/sps/io/{}/all" value="0.3" Code="200" StatePos="0.3" StateShade="0"/>"#,
            BLIND
        ));
    })
    .await;
    let client = session(&ms, ServerOptions::default(), Lifecycle::Modern).await;
    let (out, err) = call(&client, "get_control", json!({ "name": "Beschattung" })).await;
    assert!(!err, "{}", out);
    assert_eq!(out["value"], 0.3);
    assert_eq!(out["states"]["StatePos"], 0.3);
    assert_eq!(out["room"], "Wohnzimmer");
}

#[tokio::test(flavor = "multi_thread")]
async fn ambiguous_and_unknown_names_are_tool_errors_with_codes() {
    let ms = miniserver().await;
    let client = session(&ms, ServerOptions::default(), Lifecycle::Modern).await;
    let (out, err) = call(&client, "get_control", json!({ "name": "Licht" })).await;
    assert!(err);
    assert_eq!(out["error"], "ambiguous_control");
    let message = out["message"].as_str().unwrap();
    assert!(message.contains("Licht Küche"));
    assert!(message.contains("Pass `room`"), "{message}");
    assert!(!message.contains("--room"), "CLI wording: {message}");

    let (out, err) = call(&client, "get_control", json!({ "name": "Sauna" })).await;
    assert!(err);
    assert_eq!(out["error"], "control_not_found");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_exact_name_wins_over_names_containing_it() {
    // "Zentral" is also inside "Jalousie Zentral"; the thermostat is still addressable
    let ms = miniserver().await;
    let client = session(&ms, ServerOptions::default(), Lifecycle::Modern).await;
    for args in [
        json!({ "name": "Zentral", "action": "temp", "value": 21.5, "dry_run": true }),
        json!({ "name": "zentral", "room": "Zentral", "action": "temp", "value": 21.5, "dry_run": true }),
    ] {
        let (out, err) = call(&client, "thermostat", args).await;
        assert!(!err, "{}", out);
        assert_eq!(out["control"]["uuid"], THERMO);
    }
    // a partial name is still ambiguous
    let (out, err) = call(&client, "get_control", json!({ "name": "Zentr" })).await;
    assert!(err, "{}", out);
    assert_eq!(out["error"], "ambiguous_control");
}

#[tokio::test(flavor = "multi_thread")]
async fn dim_on_a_lighting_controller_is_refused() {
    // a LightControllerV2 ignores a bare level but answers 200: don't report success
    let ms = miniserver().await;
    let m = any_command(&ms).await;
    let client = session(&ms, ServerOptions::default(), Lifecycle::Modern).await;
    let (out, err) = call(
        &client,
        "light",
        json!({ "name": "Licht Wohnzimmer", "action": "dim", "value": 40 }),
    )
    .await;
    assert!(err, "{}", out);
    assert_eq!(out["error"], "invalid_arguments");
    assert!(out["message"].as_str().unwrap().contains("mood"), "{}", out);
    m.assert_hits_async(0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn list_sensors_reads_values() {
    let ms = miniserver().await;
    ms.mock_async(|when, then| {
        when.method(GET).path(format!("/dev/sps/io/{}/all", TEMP));
        then.status(200).body(r#"<LL value="21.5" Code="200"/>"#);
    })
    .await;
    let client = session(&ms, ServerOptions::default(), Lifecycle::Modern).await;
    let (out, err) = call(&client, "list_sensors", json!({ "kind": "temperature" })).await;
    assert!(!err, "{}", out);
    assert_eq!(out["count"], 1);
    assert_eq!(out["sensors"][0]["value"], 21.5);

    // an invalid enum value is a tool error the model can read, not a protocol error
    let (out, err) = call(&client, "list_sensors", json!({ "kind": "humidity" })).await;
    assert!(err, "{}", out);
}

#[tokio::test(flavor = "multi_thread")]
async fn system_status_parses_values() {
    let ms = miniserver().await;
    for (path, value) in [
        ("/dev/cfg/version", "15.2.10.14"),
        ("/dev/sps/state", "5"),
        ("/dev/sys/heap", "12000/32000kB"),
    ] {
        ms.mock_async(|when, then| {
            when.method(GET).path(path);
            then.status(200).body(format!(
                r#"<LL control="{}" value="{}" Code="200"/>"#,
                path, value
            ));
        })
        .await;
    }
    let client = session(&ms, ServerOptions::default(), Lifecycle::Modern).await;
    let (out, err) = call(&client, "system_status", json!({})).await;
    assert!(!err, "{}", out);
    assert_eq!(out["firmware"], "15.2.10.14");
    assert_eq!(out["plc_running"], true);
    assert_eq!(out["heap_total_kb"], 32000.0);
}

#[tokio::test(flavor = "multi_thread")]
async fn unreachable_miniserver_is_a_tool_error() {
    // tools/list needs no Miniserver; a tool call reports the problem as a tool error
    let cfg = Config {
        host: "http://127.0.0.1:9".into(),
        ..Default::default()
    };
    let client = connect(
        LoxMcp::with_config(ServerOptions::default(), cfg),
        TestClient::new(Lifecycle::Modern, UserAnswer::CannotAsk),
    )
    .await;
    assert!(!tool_names(&client).await.is_empty());
    let (out, err) = call(&client, "list_rooms", json!({})).await;
    assert!(err, "{}", out);
    assert_eq!(out["ok"], false);
}

// ── Action tools ──────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn switch_sends_command() {
    for lifecycle in BOTH {
        let ms = miniserver().await;
        let m = command(&ms, SWITCH, "on").await;
        let client = session(&ms, ServerOptions::default(), lifecycle).await;
        let (out, err) = call(
            &client,
            "switch",
            json!({ "name": "Licht Küche", "state": "on" }),
        )
        .await;
        assert!(!err, "{:?}: {}", lifecycle, out);
        m.assert_async().await;
        assert_eq!(out["ok"], true);
        assert_eq!(out["code"], 200);
        assert_eq!(out["cli"], "lox on \"Licht Küche\" -r \"Küche\"");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn room_disambiguates() {
    let ms = miniserver().await;
    let client = session(&ms, ServerOptions::default(), Lifecycle::Modern).await;
    let (out, err) = call(
        &client,
        "switch",
        json!({ "name": "Licht", "room": "Küche", "state": "on", "dry_run": true }),
    )
    .await;
    assert!(!err, "{}", out);
    assert_eq!(out["control"]["uuid"], SWITCH);
}

#[tokio::test(flavor = "multi_thread")]
async fn blind_position_dry_run_sends_nothing() {
    let ms = miniserver().await;
    let m = any_command(&ms).await;
    let client = session(&ms, ServerOptions::default(), Lifecycle::Modern).await;
    let (out, err) = call(
        &client,
        "blind",
        json!({ "name": "Beschattung", "action": "position", "value": 40, "dry_run": true }),
    )
    .await;
    assert!(!err, "{}", out);
    assert_eq!(out["dry_run"], true);
    assert_eq!(out["commands"], json!(["manualPosition/40.0000"]));
    m.assert_hits_async(0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn global_dry_run_forces_dry_run() {
    let ms = miniserver().await;
    let m = any_command(&ms).await;
    let opts = ServerOptions {
        dry_run: true,
        ..Default::default()
    };
    let client = session(&ms, opts, Lifecycle::Modern).await;
    let (out, _) = call(
        &client,
        "switch",
        json!({ "name": "Licht Küche", "state": "off" }),
    )
    .await;
    assert_eq!(out["dry_run"], true);
    m.assert_hits_async(0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_arguments_are_tool_errors() {
    let ms = miniserver().await;
    let client = session(&ms, ServerOptions::default(), Lifecycle::Modern).await;
    let (out, err) = call(
        &client,
        "blind",
        json!({ "name": "Beschattung", "action": "position" }),
    )
    .await;
    assert!(err);
    assert_eq!(out["error"], "invalid_arguments");

    let (out, err) = call(
        &client,
        "blind",
        json!({ "name": "Beschattung", "action": "position", "value": 140 }),
    )
    .await;
    assert!(err);
    assert!(out["message"].as_str().unwrap().contains("0-100"));

    let (_, err) = call(&client, "switch", json!({ "state": "on" })).await;
    assert!(err, "missing name");

    let (out, err) = call(
        &client,
        "blind",
        json!({ "name": "Licht Küche", "action": "up" }),
    )
    .await;
    assert!(err);
    assert!(out["message"].as_str().unwrap().contains("not a Jalousie"));
}

#[tokio::test(flavor = "multi_thread")]
async fn light_mood_and_dim() {
    let ms = miniserver().await;
    let m = command(&ms, LIGHT, "changeTo/778").await;
    let client = session(&ms, ServerOptions::default(), Lifecycle::Modern).await;
    let (out, err) = call(
        &client,
        "light",
        json!({ "name": "Licht Wohnzimmer", "action": "mood", "value": "off" }),
    )
    .await;
    assert!(!err, "{}", out);
    m.assert_async().await;

    let (out, err) = call(
        &client,
        "light",
        json!({ "name": "Spots Küche", "action": "dim", "value": 30, "dry_run": true }),
    )
    .await;
    assert!(!err, "{}", out);
    assert_eq!(out["commands"], json!(["30"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn miniserver_error_code_is_a_tool_error() {
    let ms = miniserver().await;
    ms.mock_async(|when, then| {
        when.method(GET)
            .path(format!("/jdev/sps/io/{}/pulse", SWITCH));
        then.status(200)
            .json_body(json!({ "LL": { "value": "0", "Code": "500" } }));
    })
    .await;
    let client = session(&ms, ServerOptions::default(), Lifecycle::Modern).await;
    let (out, err) = call(
        &client,
        "switch",
        json!({ "name": "Licht Küche", "state": "pulse" }),
    )
    .await;
    assert!(err);
    assert!(out["message"].as_str().unwrap().contains("code 500"));
}

#[tokio::test(flavor = "multi_thread")]
async fn send_command_passes_raw_command() {
    let ms = miniserver().await;
    let m = command(&ms, BLIND, "FullUp").await;
    let opts = ServerOptions {
        allow_raw: true,
        ..Default::default()
    };
    let client = session(&ms, opts, Lifecycle::Modern).await;
    let (out, err) = call(
        &client,
        "send_command",
        json!({ "name": "Beschattung", "command": "FullUp" }),
    )
    .await;
    assert!(!err, "{}", out);
    m.assert_async().await;
}

// ── Confirmation of risky actions ─────────────────────────────────────────────

async fn confirm_session(
    ms: &MockServer,
    lifecycle: Lifecycle,
    answer: UserAnswer,
) -> (Client, TestClient) {
    let tc = TestClient::new(lifecycle, answer);
    let client = connect(
        LoxMcp::with_config(ServerOptions::default(), config(ms, None)),
        tc.clone(),
    )
    .await;
    (client, tc)
}

#[tokio::test(flavor = "multi_thread")]
async fn risky_action_runs_after_the_user_confirms() {
    for lifecycle in BOTH {
        let ms = miniserver().await;
        let m = command(&ms, DOOR, "open").await;
        let (client, tc) = confirm_session(&ms, lifecycle, UserAnswer::Accept).await;
        let (out, err) = call(
            &client,
            "door",
            json!({ "name": "Haustür", "action": "open" }),
        )
        .await;
        assert!(!err, "{:?}: {}", lifecycle, out);
        assert_eq!(out["confirmed"], true);
        m.assert_async().await;
        let questions = tc.questions.lock().unwrap().clone();
        assert_eq!(questions.len(), 1, "asked exactly once");
        assert!(questions[0].contains("Haustür"), "{}", questions[0]);
        assert!(questions[0].contains("lox door"), "{}", questions[0]);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn declined_risky_action_sends_nothing() {
    for lifecycle in BOTH {
        let ms = miniserver().await;
        let m = any_command(&ms).await;
        let (client, _) = confirm_session(&ms, lifecycle, UserAnswer::Decline).await;
        let (out, err) = call(
            &client,
            "alarm",
            json!({ "name": "Alarm", "action": "disarm", "code": "4711" }),
        )
        .await;
        assert!(err, "{:?}: {}", lifecycle, out);
        assert_eq!(out["error"], "declined_by_user");
        m.assert_hits_async(0).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn risky_action_is_refused_when_the_client_cannot_ask() {
    for lifecycle in BOTH {
        let ms = miniserver().await;
        let m = any_command(&ms).await;
        let (client, _) = confirm_session(&ms, lifecycle, UserAnswer::CannotAsk).await;
        let (out, err) = call(
            &client,
            "door",
            json!({ "name": "Haustür", "action": "unlock" }),
        )
        .await;
        assert!(err, "{:?}: {}", lifecycle, out);
        assert_eq!(out["error"], "action_not_allowed");
        assert!(out["message"].as_str().unwrap().contains("--allow-risky"));
        m.assert_hits_async(0).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn generic_actions_on_risky_controls_need_confirmation_too() {
    // `switch off` on a door lock would unlock it: same gate as the door tool
    let ms = miniserver().await;
    let m = any_command(&ms).await;
    let (client, _) = confirm_session(&ms, Lifecycle::Modern, UserAnswer::CannotAsk).await;
    let (out, err) = call(
        &client,
        "switch",
        json!({ "name": "Haustür", "state": "off" }),
    )
    .await;
    assert!(err, "{}", out);
    assert_eq!(out["error"], "action_not_allowed");
    m.assert_hits_async(0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn door_opener_push_buttons_need_confirmation() {
    // "Tür öffnen" is a Pushbutton, but its category icon says door
    for lifecycle in BOTH {
        let ms = miniserver().await;
        let m = any_command(&ms).await;
        let (client, _) = confirm_session(&ms, lifecycle, UserAnswer::CannotAsk).await;
        let (out, err) = call(
            &client,
            "switch",
            json!({ "name": "Tür öffnen", "state": "pulse" }),
        )
        .await;
        assert!(err, "{:?}: {}", lifecycle, out);
        assert_eq!(out["error"], "action_not_allowed");
        m.assert_hits_async(0).await;
    }
    // an ordinary switch is not gated
    let ms = miniserver().await;
    let m = command(&ms, SWITCH, "pulse").await;
    let (client, _) = confirm_session(&ms, Lifecycle::Modern, UserAnswer::CannotAsk).await;
    let (out, err) = call(
        &client,
        "switch",
        json!({ "name": "Licht Küche", "state": "pulse" }),
    )
    .await;
    assert!(!err, "{}", out);
    m.assert_hits_async(1).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn allow_risky_skips_the_confirmation() {
    let ms = miniserver().await;
    let m = command(&ms, DOOR, "open").await;
    let tc = TestClient::new(Lifecycle::Modern, UserAnswer::Decline);
    let opts = ServerOptions {
        allow_risky: true,
        ..Default::default()
    };
    let client = connect(LoxMcp::with_config(opts, config(&ms, None)), tc.clone()).await;
    let (out, err) = call(
        &client,
        "door",
        json!({ "name": "Haustür", "action": "open" }),
    )
    .await;
    assert!(!err, "{}", out);
    m.assert_async().await;
    assert!(tc.questions.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn dry_run_of_a_risky_action_reports_the_confirmation() {
    let ms = miniserver().await;
    let m = any_command(&ms).await;
    let (client, tc) = confirm_session(&ms, Lifecycle::Modern, UserAnswer::Accept).await;
    let (out, err) = call(
        &client,
        "door",
        json!({ "name": "Haustür", "action": "open", "dry_run": true }),
    )
    .await;
    assert!(!err, "{}", out);
    assert_eq!(out["needs_confirmation"], true);
    assert!(
        tc.questions.lock().unwrap().is_empty(),
        "dry runs never ask"
    );
    m.assert_hits_async(0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn alarm_quit_is_not_risky_and_pins_are_masked() {
    let ms = miniserver().await;
    let m = command(&ms, ALARM, "quit").await;
    let (client, tc) = confirm_session(&ms, Lifecycle::Modern, UserAnswer::Accept).await;
    let (out, err) = call(
        &client,
        "alarm",
        json!({ "name": "Alarm", "action": "quit" }),
    )
    .await;
    assert!(!err, "{}", out);
    m.assert_async().await;
    assert!(tc.questions.lock().unwrap().is_empty());

    let (out, _) = call(
        &client,
        "alarm",
        json!({ "name": "Alarm", "action": "disarm", "code": "4711", "dry_run": true }),
    )
    .await;
    assert!(!out.to_string().contains("4711"), "{}", out);
}

#[tokio::test(flavor = "multi_thread")]
async fn forged_request_state_is_rejected() {
    // A retry with a requestState the server never issued must not run the action.
    let ms = miniserver().await;
    let m = any_command(&ms).await;
    let (client, _) = confirm_session(&ms, Lifecycle::Modern, UserAnswer::Accept).await;
    let mut responses = rmcp::model::InputResponses::new();
    responses.insert(
        "confirm".to_string(),
        json!({ "action": "accept", "content": { "confirm": true } }),
    );
    let params = CallToolRequestParams::new("door")
        .with_input_responses(responses)
        .with_request_state("not-issued-by-the-server");
    let (out, err) = call_with(
        &client,
        params,
        json!({ "name": "Haustür", "action": "open" }),
    )
    .await;
    assert!(err, "{}", out);
    assert_eq!(out["error"], "confirmation_expired");
    m.assert_hits_async(0).await;
}

// ── Scenes: progress and cancellation ─────────────────────────────────────────

fn write_scene(dir: &std::path::Path, name: &str, yaml: &str) {
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::write(dir.join(format!("scenes/{}.yaml", name)), yaml).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn scenes_are_listed_and_run_with_progress() {
    let ms = miniserver().await;
    let on = command(&ms, SWITCH, "on").await;
    let down = command(&ms, BLIND, "FullDown").await;
    let dir = tempfile::tempdir().unwrap();
    write_scene(
        dir.path(),
        "abend",
        "name: Abend\ndescription: Evening\nsteps:\n  - control: Licht Küche\n    cmd: \"on\"\n  - control: Beschattung\n    cmd: FullDown\n  - control: Sauna\n    cmd: \"on\"\n",
    );
    let tc = TestClient::new(Lifecycle::Modern, UserAnswer::CannotAsk);
    let client = connect(
        LoxMcp::with_config(ServerOptions::default(), config(&ms, Some(dir.path()))),
        tc.clone(),
    )
    .await;

    let (out, err) = call(&client, "list_scenes", json!({})).await;
    assert!(!err, "{}", out);
    assert_eq!(out["scenes"][0]["scene"], "abend");
    assert_eq!(out["scenes"][0]["steps"], 3);

    let mut params = CallToolRequestParams::new("run_scene");
    params.meta = Some(RequestMetaObject::with_progress_token(ProgressToken(
        NumberOrString::Number(7),
    )));
    let (out, err) = call_with(&client, params, json!({ "scene": "abend" })).await;
    assert!(!err, "{}", out);
    on.assert_async().await;
    down.assert_async().await;
    assert_eq!(out["ok"], false, "the unknown control step fails");
    assert_eq!(out["steps"][2]["ok"], false);
    // notifications may trail the response slightly
    for _ in 0..100 {
        if tc.progress.load(Ordering::SeqCst) == 3 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(
        tc.progress.load(Ordering::SeqCst),
        3,
        "one progress notification per step"
    );

    let (_, err) = call(&client, "run_scene", json!({ "scene": "nope" })).await;
    assert!(err);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_scene_stops_the_remaining_steps() {
    let ms = miniserver().await;
    let on = command(&ms, SWITCH, "on").await;
    let down = command(&ms, BLIND, "FullDown").await;
    let dir = tempfile::tempdir().unwrap();
    write_scene(
        dir.path(),
        "slow",
        "steps:\n  - control: Licht Küche\n    cmd: \"on\"\n    delay_ms: 3000\n  - control: Beschattung\n    cmd: FullDown\n",
    );
    let client = connect(
        LoxMcp::with_config(ServerOptions::default(), config(&ms, Some(dir.path()))),
        TestClient::new(Lifecycle::Modern, UserAnswer::CannotAsk),
    )
    .await;
    let request = rmcp::model::ClientRequest::CallToolRequest(rmcp::model::CallToolRequest::new(
        CallToolRequestParams::new("run_scene")
            .with_arguments(json!({ "scene": "slow" }).as_object().cloned().unwrap()),
    ));
    let handle = client
        .peer()
        .send_cancellable_request(request, Default::default())
        .await
        .expect("request sent");
    // wait until the first step went out, then cancel during the 3 s delay
    for _ in 0..100 {
        if on.hits_async().await == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    on.assert_async().await;
    handle
        .cancel(Some("user stopped it".into()))
        .await
        .expect("cancel");
    // without the cancellation the second step would go out after 3 s
    tokio::time::sleep(std::time::Duration::from_millis(3500)).await;
    down.assert_hits_async(0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn modern_confirmation_is_a_multi_round_trip_bound_to_the_action() {
    let ms = miniserver().await;
    let m = any_command(&ms).await;
    let (client, tc) = confirm_session(&ms, Lifecycle::Modern, UserAnswer::Accept).await;
    let args = |action: &str| {
        json!({ "name": "Haustür", "action": action })
            .as_object()
            .cloned()
            .unwrap()
    };

    // round 1: the server answers input_required with an elicitation and a requestState
    let first = client
        .call_tool_once(CallToolRequestParams::new("door").with_arguments(args("open")))
        .await
        .expect("tools/call");
    let rmcp::model::CallToolResponse::InputRequired(input) = first else {
        panic!("expected input_required, got {:?}", first);
    };
    let state = input.request_state.clone().expect("requestState");
    assert!(
        input
            .input_requests
            .as_ref()
            .is_some_and(|r| r.contains_key("confirm"))
    );
    assert!(
        tc.questions.lock().unwrap().is_empty(),
        "no server-to-client request"
    );

    // round 2 with the answer, but for a different action: rejected, nothing sent
    let mut responses = rmcp::model::InputResponses::new();
    responses.insert(
        "confirm".to_string(),
        json!({ "action": "accept", "content": { "confirm": true } }),
    );
    let retry = CallToolRequestParams::new("door")
        .with_arguments(args("unlock"))
        .with_input_responses(responses.clone())
        .with_request_state(state.clone());
    let (out, err) = call_with(
        &client,
        retry,
        json!({ "name": "Haustür", "action": "unlock" }),
    )
    .await;
    assert!(err, "{}", out);
    assert_eq!(out["error"], "confirmation_mismatch");

    // the requestState is single-use: replaying it for the original action fails too
    let replay = CallToolRequestParams::new("door")
        .with_input_responses(responses)
        .with_request_state(state);
    let (out, err) = call_with(
        &client,
        replay,
        json!({ "name": "Haustür", "action": "open" }),
    )
    .await;
    assert!(err, "{}", out);
    assert_eq!(out["error"], "confirmation_expired");
    m.assert_hits_async(0).await;
}
