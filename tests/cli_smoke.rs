use assert_cmd::Command;
use predicates::prelude::*;

fn lox() -> Command {
    Command::cargo_bin("lox").unwrap()
}

// ── Top-level --help and --version ─────────────────────────────────────────

#[test]
fn help_exits_0() {
    lox().arg("--help").assert().success();
}

#[test]
fn version_exits_0() {
    lox().arg("--version").assert().success();
}

// ── Every subcommand --help exits 0 ────────────────────────────────────────
// This catches clap definition bugs (duplicate short flags, missing values,
// conflicting attributes) at the integration-test level.

macro_rules! subcmd_help {
    ($name:ident, $($arg:expr_2021),+) => {
        #[test]
        fn $name() {
            lox()$(.arg($arg))+ .arg("--help").assert().success();
        }
    };
}

// Control commands
subcmd_help!(help_on, "on");
subcmd_help!(help_off, "off");
subcmd_help!(help_blind, "blind");
subcmd_help!(help_gate, "gate");
subcmd_help!(help_thermostat, "thermostat");
subcmd_help!(help_alarm, "alarm");
subcmd_help!(help_door, "door");
subcmd_help!(help_intercom, "intercom");
subcmd_help!(help_charger, "charger");
subcmd_help!(help_music, "music");
subcmd_help!(help_lock, "lock");
subcmd_help!(help_unlock, "unlock");
subcmd_help!(help_run, "run");
subcmd_help!(help_send, "send");

// Input subcommands
subcmd_help!(help_input, "input");
subcmd_help!(help_input_set, "input", "set");
subcmd_help!(help_input_pulse, "input", "pulse");

// Light subcommands
subcmd_help!(help_light, "light");
subcmd_help!(help_light_mood, "light", "mood");
subcmd_help!(help_light_moods, "light", "moods");
subcmd_help!(help_light_dim, "light", "dim");
subcmd_help!(help_light_color, "light", "color");

// Inspect commands
subcmd_help!(help_ls, "ls");
subcmd_help!(help_get, "get");
subcmd_help!(help_info, "info");
subcmd_help!(help_watch, "watch");
subcmd_help!(help_stream, "stream");
subcmd_help!(help_if_cmd, "if");
subcmd_help!(help_rooms, "rooms");
subcmd_help!(help_categories, "categories");
subcmd_help!(help_globals, "globals");
subcmd_help!(help_modes, "modes");
subcmd_help!(help_sensors, "sensors");
subcmd_help!(help_energy, "energy");
subcmd_help!(help_weather, "weather");
subcmd_help!(help_stats, "stats");
subcmd_help!(help_history, "history");

// Autopilot subcommands
subcmd_help!(help_autopilot, "autopilot");
subcmd_help!(help_autopilot_ls, "autopilot", "ls");
subcmd_help!(help_autopilot_state, "autopilot", "state");

// System commands
subcmd_help!(help_status, "status");
subcmd_help!(help_log, "log");
subcmd_help!(help_time, "time");
subcmd_help!(help_discover, "discover");
subcmd_help!(help_extensions, "extensions");
subcmd_help!(help_update, "update");
subcmd_help!(help_update_check, "update", "check");
subcmd_help!(help_update_install, "update", "install");
subcmd_help!(help_reboot, "reboot");

// Files subcommands
subcmd_help!(help_files, "files");
subcmd_help!(help_files_ls, "files", "ls");
subcmd_help!(help_files_get, "files", "get");

// Otel subcommands
subcmd_help!(help_otel, "otel");
subcmd_help!(help_otel_serve, "otel", "serve");
subcmd_help!(help_otel_push, "otel", "push");

// Configuration commands
subcmd_help!(help_setup, "setup");
subcmd_help!(help_setup_set, "setup", "set");
subcmd_help!(help_setup_show, "setup", "show");
subcmd_help!(help_alias, "alias");
subcmd_help!(help_alias_add, "alias", "add");
subcmd_help!(help_alias_remove, "alias", "remove");
subcmd_help!(help_alias_ls, "alias", "ls");
subcmd_help!(help_scene, "scene");
subcmd_help!(help_scene_ls, "scene", "ls");
subcmd_help!(help_scene_show, "scene", "show");
subcmd_help!(help_scene_new, "scene", "new");
subcmd_help!(help_cache, "cache");
subcmd_help!(help_cache_info, "cache", "info");
subcmd_help!(help_cache_clear, "cache", "clear");
subcmd_help!(help_cache_refresh, "cache", "refresh");
subcmd_help!(help_cache_check, "cache", "check");
subcmd_help!(help_token, "token");
subcmd_help!(help_token_fetch, "token", "fetch");
subcmd_help!(help_token_info, "token", "info");
subcmd_help!(help_token_clear, "token", "clear");
subcmd_help!(help_token_check, "token", "check");
subcmd_help!(help_token_refresh, "token", "refresh");
subcmd_help!(help_token_revoke, "token", "revoke");
subcmd_help!(help_config, "config");
subcmd_help!(help_config_download, "config", "download");
subcmd_help!(help_config_ls, "config", "ls");
subcmd_help!(help_config_extract, "config", "extract");
subcmd_help!(help_config_upload, "config", "upload");
subcmd_help!(help_config_users, "config", "users");
subcmd_help!(help_config_devices, "config", "devices");
subcmd_help!(help_config_diff, "config", "diff");
subcmd_help!(help_ctx, "ctx");
subcmd_help!(help_ctx_add, "ctx", "add");
subcmd_help!(help_ctx_use, "ctx", "use");
subcmd_help!(help_ctx_list, "ctx", "list");
subcmd_help!(help_ctx_current, "ctx", "current");
subcmd_help!(help_ctx_remove, "ctx", "remove");
subcmd_help!(help_ctx_rename, "ctx", "rename");
subcmd_help!(help_ctx_init, "ctx", "init");
subcmd_help!(help_ctx_migrate, "ctx", "migrate");
subcmd_help!(help_completions, "completions");
subcmd_help!(help_mcp, "mcp");
subcmd_help!(help_mcp_serve, "mcp", "serve");
subcmd_help!(help_mcp_config, "mcp", "config");
subcmd_help!(help_mcp_tools, "mcp", "tools");

// ── Global flags accepted with subcommands ─────────────────────────────────

#[test]
fn global_json_flag_accepted() {
    lox()
        .args(["--output", "json", "--help"])
        .assert()
        .success();
}

#[test]
fn global_quiet_flag_accepted() {
    lox().args(["-q", "--help"]).assert().success();
}

#[test]
fn global_no_header_flag_accepted() {
    lox().args(["--no-header", "--help"]).assert().success();
}

#[test]
fn help_mentions_loxone() {
    lox()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Loxone"));
}

// ── MCP server over stdio ──────────────────────────────────────────────────

#[test]
fn mcp_serve_handshake_and_tools_list() {
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "\n",
    );
    let out = lox()
        .args(["mcp", "serve", "--read-only"])
        // no Miniserver needed: listing tools never loads the config
        .env("LOX_CONFIG", "/nonexistent/lox-config.yaml")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let lines: Vec<serde_json::Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("stdout carries JSON-RPC only"))
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "one reply per request, none for notifications"
    );
    assert_eq!(lines[0]["result"]["protocolVersion"], "2025-06-18");
    let tools = lines[1]["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|t| t["name"] == "list_controls"));
    assert!(!tools.iter().any(|t| t["name"] == "switch"), "read-only");
}

#[test]
fn mcp_serve_speaks_2026_07_28_discover_lifecycle() {
    let meta = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"smoke","version":"0"}}"#;
    let input = format!(
        "{}\n{}\n",
        format_args!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{{{}}}}}"#,
            meta
        ),
        format_args!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{{}}}}}"#,
            meta
        ),
    );
    let out = lox()
        .args(["mcp", "serve"])
        .env("LOX_CONFIG", "/nonexistent/lox-config.yaml")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let replies: Vec<serde_json::Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("stdout carries JSON-RPC only"))
        .collect();
    let by_id = |id: i64| replies.iter().find(|r| r["id"] == id).expect("reply");
    let discover = &by_id(1)["result"];
    assert!(
        discover["supportedVersions"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("2026-07-28"))
    );
    let list = &by_id(2)["result"];
    assert_eq!(list["resultType"], "complete");
    assert!(list["ttlMs"].as_u64().is_some(), "SEP-2549 cache hint");
    assert!(
        list["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "door")
    );
}

#[test]
fn mcp_tool_call_without_config_reports_error() {
    // 2026-07-28 is stateless: a tools/call carrying its own _meta needs no handshake
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_rooms","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"smoke","version":"0"}}}}"#,
        "\n",
    );
    lox()
        .args(["mcp", "serve"])
        .env("LOX_CONFIG", "/nonexistent/lox-config.yaml")
        .write_stdin(input)
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""isError":true"#))
        .stdout(predicate::str::contains("config_not_found"));
}

#[test]
fn mcp_read_only_conflicts_with_allow_risky() {
    lox()
        .args(["mcp", "serve", "--read-only", "--allow-risky"])
        .assert()
        .failure();
}

#[test]
fn mcp_config_prints_json_snippet() {
    let out = lox()
        .args([
            "--ctx",
            "home",
            "-o",
            "json",
            "mcp",
            "config",
            "--allow-risky",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let args = v["mcpServers"]["loxone"]["args"].as_array().unwrap();
    assert_eq!(
        args,
        &vec![
            serde_json::json!("--ctx"),
            serde_json::json!("home"),
            serde_json::json!("mcp"),
            serde_json::json!("serve"),
            serde_json::json!("--allow-risky"),
        ]
    );
}
