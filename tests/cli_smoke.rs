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
    ($name:ident, $($arg:expr),+) => {
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
subcmd_help!(help_completions, "completions");

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
