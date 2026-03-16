mod client;
mod config;
mod ftp;
mod loxcc;
mod loxone_xml;
mod otel;
mod scene;
mod stream;
mod token;
mod ws;

use anyhow::{bail, Context, Result};
use clap::{ArgAction, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use client::LoxClient;
use config::Config;
use reqwest::blocking::Client;
use scene::Scene;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::thread;
use std::time::Duration;

use byteorder::{LittleEndian, ReadBytesExt};

/// Load config XML from a .Loxone file or extract from a .zip backup.
fn load_config_xml(path: &str) -> Result<Vec<u8>> {
    let data = fs::read(path).with_context(|| format!("Cannot read {}", path))?;
    if path.ends_with(".zip") {
        loxcc::extract_and_decompress(&data)
    } else {
        Ok(data)
    }
}

fn xml_attr<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    let key = format!("{}=\"", attr);
    let start = xml.find(&key)? + key.len();
    let end = xml[start..].find('"')? + start;
    Some(&xml[start..end])
}

fn print_resp(resp: &Value, json: bool, quiet: bool, name: &str, cmd: &str) {
    if json {
        println!("{}", serde_json::to_string_pretty(resp).unwrap());
    } else if !quiet {
        let val = resp
            .pointer("/LL/value")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let code = resp
            .pointer("/LL/Code")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        println!(
            "{}  {} → {} = {}",
            if code == "200" { "✓" } else { "✗" },
            name,
            cmd,
            val
        );
    }
}

/// Print a dry-run envelope showing what would be executed.
fn print_dry_run(
    json: bool,
    quiet: bool,
    uuid: &str,
    command: &str,
    control_name: &str,
    room: Option<&str>,
) {
    if json {
        let mut obj = serde_json::json!({
            "ok": true,
            "dry_run": true,
            "would_execute": {
                "uuid": uuid,
                "command": command,
                "control": control_name,
            }
        });
        if let Some(r) = room {
            obj["would_execute"]["room"] = serde_json::Value::String(r.to_string());
        }
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else if !quiet {
        println!("[dry-run] {} → {} (uuid: {})", control_name, command, uuid);
    }
}

/// Execute a command or print dry-run info. Returns the response (or None for dry-run).
#[allow(clippy::too_many_arguments)]
fn send_or_dry_run(
    lox: &LoxClient,
    uuid: &str,
    cmd: &str,
    control_name: &str,
    room: Option<&str>,
    dry_run: bool,
    json: bool,
    quiet: bool,
) -> Result<Option<Value>> {
    if dry_run {
        print_dry_run(json, quiet, uuid, cmd, control_name, room);
        Ok(None)
    } else {
        Ok(Some(lox.send_cmd(uuid, cmd)?))
    }
}

fn bar(v: f64, max: f64) -> String {
    let n = ((v / max * 20.0) as usize).min(20);
    format!(
        "[{}{}] {:.0}%",
        "█".repeat(n),
        "░".repeat(20 - n),
        v / max * 100.0
    )
}

fn kb_fmt(kb: f64) -> String {
    if kb > 1024.0 {
        format!("{:.0} MB", kb / 1024.0)
    } else {
        format!("{:.0} kB", kb)
    }
}

fn now_hms() -> String {
    use chrono::Timelike;
    let now = chrono::Local::now();
    format!("{:02}:{:02}:{:02}", now.hour(), now.minute(), now.second())
}

fn detect_shell() -> Option<Shell> {
    std::env::var("SHELL").ok().and_then(|s| {
        let name = s.rsplit('/').next().unwrap_or(&s);
        match name {
            "bash" => Some(Shell::Bash),
            "zsh" => Some(Shell::Zsh),
            "fish" => Some(Shell::Fish),
            "elvish" => Some(Shell::Elvish),
            "pwsh" | "powershell" => Some(Shell::PowerShell),
            _ => None,
        }
    })
}

fn install_completions(shell: Shell, cmd: &mut clap::Command) -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_default();
    let (path, label) = match shell {
        Shell::Bash => {
            let dir = format!("{}/.local/share/bash-completion/completions", home);
            fs::create_dir_all(&dir)?;
            (format!("{}/lox", dir), "bash")
        }
        Shell::Zsh => {
            // Use ~/.zfunc which is a common user completions dir
            let dir = format!("{}/.zfunc", home);
            fs::create_dir_all(&dir)?;
            (format!("{}/_lox", dir), "zsh")
        }
        Shell::Fish => {
            let dir = format!("{}/.config/fish/completions", home);
            fs::create_dir_all(&dir)?;
            (format!("{}/lox.fish", dir), "fish")
        }
        Shell::Elvish => {
            bail!("Elvish completions must be installed manually — run: lox completions elvish");
        }
        Shell::PowerShell => {
            bail!("PowerShell completions must be installed manually — run: lox completions powershell");
        }
        _ => bail!("Unsupported shell"),
    };
    let mut buf = Vec::new();
    generate(shell, cmd, "lox", &mut buf);
    fs::write(&path, buf)?;

    eprintln!("Installed {} completions to {}", label, path);
    if shell == Shell::Zsh {
        eprintln!("Ensure ~/.zfunc is in your fpath. Add to ~/.zshrc:");
        eprintln!("  fpath=(~/.zfunc $fpath)");
        eprintln!("  autoload -Uz compinit && compinit");
    }
    eprintln!("Restart your shell or open a new tab to activate.");
    Ok(())
}

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "lox",
    about = "Loxone Miniserver CLI",
    version,
    infer_subcommands = true,
    disable_help_subcommand = true,
    help_template = "\
{about-with-newline}
{usage-heading} {usage}

Control:
  on, off                      Turn controls on/off
  input                        Set/pulse analog & virtual inputs
  light                        Moods, dimmer, color
  blind, gate                  Blinds, gates
  thermostat, alarm            Climate, security
  door, intercom, charger     Door locks, intercoms, EV chargers
  music                        Music server zones
  lock, unlock                 Lock/unlock controls (admin)
  run, send                    Run scenes, send raw commands

Inspect:
  ls, get, info, watch, stream  List, query, monitor, stream controls
  if                            Conditional state check
  rooms, categories, modes     Structure metadata
  globals, sensors, energy     Global states, sensor & energy readings
  weather, stats, history      Weather, statistics
  autopilot                    Automatic rules

System:
  status, log, time            Health, logs, clock
  discover, extensions         Find Miniservers, list extensions
  update, reboot               Firmware updates, reboot
  files                        Browse Miniserver filesystem
  otel                         Export metrics via OpenTelemetry (OTLP)

Configuration:
  setup, alias, scene          Connection settings, aliases, scenes
  cache, token                 Cache & auth token management
  config                       Loxone Config files (download/inspect/diff)
  completions                  Generate shell completions
  schema                       Command schema for AI agent discovery

{options}{after-help}"
)]
struct Cli {
    /// Output format: json, csv, or table (default)
    #[arg(long, short = 'o', global = true, value_enum, default_value = "table")]
    output: OutputFormat,
    /// Suppress non-essential output
    #[arg(long, short = 'q', global = true)]
    quiet: bool,
    /// Disable colored output (also respects NO_COLOR env var)
    #[arg(long, global = true)]
    no_color: bool,
    /// Suppress table headers (useful for piping)
    #[arg(long, global = true)]
    no_header: bool,
    /// Verbose output (-v for requests, -vv for request+response bodies)
    #[arg(short = 'v', long, global = true, action = ArgAction::Count)]
    verbose: u8,
    /// Validate and resolve inputs without executing commands
    #[arg(long, global = true)]
    dry_run: bool,
    /// Fail instead of prompting for confirmation (implied by --output json)
    #[arg(long, global = true)]
    non_interactive: bool,
    /// Trace ID for correlating agent actions in logs
    #[arg(long, global = true)]
    trace_id: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    /// Human-readable table (default)
    Table,
    /// JSON output
    Json,
    /// CSV output (where applicable)
    Csv,
}

#[derive(Subcommand)]
enum Cmd {
    // ── Control ──────────────────────────────────────────────────────────────
    /// Control analog/virtual inputs: set | pulse
    Input {
        #[command(subcommand)]
        action: InputCmd,
    },
    /// Turn on
    On {
        name_or_uuid: Option<String>,
        #[arg(long, short = 'r')]
        room: Option<String>,
        /// Apply to all controls in a room
        #[arg(long)]
        all_in_room: Option<String>,
    },
    /// Turn off
    Off {
        name_or_uuid: Option<String>,
        #[arg(long, short = 'r')]
        room: Option<String>,
        /// Apply to all controls in a room
        #[arg(long)]
        all_in_room: Option<String>,
    },
    /// Set analog/virtual input value (use `lox input set` instead)
    #[command(hide = true)]
    Set {
        /// Control name or UUID
        name_or_uuid: String,
        /// Value to send (numeric or text)
        value: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Momentary pulse (use `lox input pulse` instead)
    #[command(hide = true)]
    Pulse {
        name_or_uuid: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Control blind: up | down | stop | shade [<0-100>] | pos <0-100>
    Blind {
        name_or_uuid: String,
        action: String,
        #[arg(allow_hyphen_values = true)]
        pos: Option<f64>,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Control lights: mood | dim | color
    Light {
        #[command(subcommand)]
        action: LightCmd,
    },
    /// Control light moods: plus | minus | off | <mood-id>
    #[command(hide = true)]
    Mood {
        name_or_uuid: String,
        /// Mood action: plus, minus, off, or numeric mood ID
        action: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Set dimmer level (0-100)
    #[command(hide = true)]
    Dimmer {
        name_or_uuid: String,
        /// Brightness level 0-100
        level: f64,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Control gate: open | close | stop
    Gate {
        name_or_uuid: String,
        /// Action: open, close, stop
        action: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Set color on ColorPickerV2 (hex RGB e.g. #FF0000 or hsv(h,s,v))
    #[command(hide = true)]
    Color {
        name_or_uuid: String,
        /// Color value: hex RGB (#FF0000) or hsv(h,s,v)
        value: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Control thermostat: temp <°C> | mode <auto|eco|comfort|manual> | override <°C> [minutes]
    Thermostat {
        name_or_uuid: String,
        /// Action: temp, mode, override (omit to show current state)
        action: Option<String>,
        /// Value for the action (temperature or mode name)
        value: Option<String>,
        /// Duration in minutes (for override, default: 60)
        #[arg(allow_hyphen_values = true)]
        duration: Option<u64>,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Control alarm: arm | arm-home | disarm | quit
    Alarm {
        name_or_uuid: String,
        /// Action: arm, arm-home, disarm, quit/ack
        action: String,
        /// Arm without motion detection
        #[arg(long)]
        no_motion: bool,
        /// PIN code for arm/disarm
        #[arg(long)]
        code: Option<String>,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Control door lock: lock | unlock | open
    #[command(alias = "doorlock")]
    Door {
        name_or_uuid: String,
        /// Action: lock, unlock, open
        action: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Control intercom: answer | hangup | open
    Intercom {
        name_or_uuid: String,
        /// Action: answer, hangup, open
        action: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Control EV charger: start | stop | pause
    Charger {
        name_or_uuid: String,
        /// Action: start, stop, pause
        action: String,
        /// Maximum charge limit in kWh
        #[arg(long)]
        limit: Option<f64>,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Control music server zones
    Music {
        #[command(subcommand)]
        action: MusicCmd,
    },
    /// Lock a control (admin, firmware v11.3.2.11+)
    Lock {
        name_or_uuid: String,
        /// Reason for locking
        #[arg(long, default_value = "locked via CLI")]
        reason: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Unlock a control
    Unlock {
        name_or_uuid: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Run a scene
    Run {
        scene: String,
        /// Preview what would be executed without sending commands
        #[arg(long)]
        dry_run: bool,
    },
    /// Send raw command
    Send {
        name_or_uuid: String,
        command: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
        /// Send as secured command (requires visualization password hash)
        #[arg(long)]
        secured: Option<String>,
    },

    // ── Inspect ──────────────────────────────────────────────────────────────
    /// List controls
    Ls {
        #[arg(long, short = 't')]
        r#type: Option<String>,
        #[arg(long, short = 'r')]
        room: Option<String>,
        #[arg(long)]
        values: bool,
        /// Filter by category name
        #[arg(long, short = 'c')]
        cat: Option<String>,
        /// Show only favorites
        #[arg(long, short = 'f')]
        favorites: bool,
    },
    /// Get full state of a control
    Get {
        name_or_uuid: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Show detailed control info (sub-controls, states, moods, flags)
    Info {
        name_or_uuid: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Poll a control's state and print changes (Ctrl+C to stop)
    Watch {
        name_or_uuid: String,
        #[arg(
            long,
            short = 'i',
            default_value = "2",
            help = "Poll interval in seconds"
        )]
        interval: u64,
    },
    /// Stream real-time state changes via WebSocket (Ctrl+C to stop)
    Stream {
        /// Filter by control type
        #[arg(long, short = 't')]
        r#type: Option<String>,
        /// Filter by room
        #[arg(long, short = 'r')]
        room: Option<String>,
        /// Filter by control name (fuzzy match)
        #[arg(long, short = 'c')]
        control: Option<String>,
        /// Include initial state snapshot
        #[arg(long)]
        initial: bool,
    },
    /// Check state (exit 0=match, 1=no match)
    If {
        name_or_uuid: String,
        op: String,
        value: String,
        /// Room qualifier to disambiguate controls with the same name
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// List all rooms in the structure
    Rooms,
    /// List all categories
    Categories,
    /// Show global states (operating mode, sunrise/sunset, wind/rain warnings)
    Globals,
    /// List operating modes
    Modes,
    /// List sensor readings (temperature, door/window, motion, smoke)
    Sensors {
        /// Filter by sensor type: temperature, door-window, motion, smoke, all
        #[arg(long, default_value = "all")]
        r#type: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Show energy meter readings
    Energy {
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Show weather data
    Weather {
        /// Show 7-day forecast
        #[arg(long)]
        forecast: bool,
    },
    /// List controls that have statistics enabled
    Stats,
    /// Fetch historical statistics data
    History {
        name_or_uuid: String,
        /// Fetch monthly data (YYYY-MM)
        #[arg(long)]
        month: Option<String>,
        /// Fetch daily data (YYYY-MM-DD)
        #[arg(long)]
        day: Option<String>,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// List and inspect automatic rules (Automatik-Regel / Autopilot)
    Autopilot {
        #[command(subcommand)]
        action: AutopilotCmd,
    },

    // ── System ───────────────────────────────────────────────────────────────
    /// Miniserver health
    Status {
        /// Show energy meters (deprecated: use `lox energy` instead)
        #[arg(long, hide = true)]
        energy: bool,
        /// Show CPU, tasks, context switches, SD card health
        #[arg(long)]
        diag: bool,
        /// Show network configuration (IP, MAC, DNS, DHCP, NTP)
        #[arg(long)]
        net: bool,
        /// Show CAN bus statistics
        #[arg(long)]
        bus: bool,
        /// Show LAN packet statistics
        #[arg(long)]
        lan: bool,
        /// Show all diagnostic sections
        #[arg(long)]
        all: bool,
    },
    /// Fetch the Miniserver system log (tail)
    Log {
        #[arg(
            long,
            short = 'n',
            default_value = "50",
            help = "Number of lines to show"
        )]
        lines: usize,
    },
    /// Show Miniserver system date/time
    Time,
    /// Discover Miniservers on the local network (UDP broadcast)
    Discover {
        /// Timeout in seconds
        #[arg(long, default_value = "3")]
        timeout: u64,
    },
    /// List connected extensions and devices
    Extensions,
    /// Check for firmware updates
    Update {
        #[command(subcommand)]
        action: UpdateCmd,
    },
    /// Reboot the Miniserver
    Reboot {
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Browse Miniserver filesystem
    Files {
        #[command(subcommand)]
        action: FilesCmd,
    },

    /// Export metrics, logs & traces to OpenTelemetry backends
    Otel {
        #[command(subcommand)]
        action: OtelCmd,
    },

    // ── Configuration ────────────────────────────────────────────────────────
    /// Configure connection settings
    Setup {
        #[command(subcommand)]
        action: SetupCmd,
    },
    /// Manage control name aliases
    Alias {
        #[command(subcommand)]
        action: AliasCmd,
    },
    /// Manage scenes
    Scene {
        #[command(subcommand)]
        action: SceneCmd,
    },
    /// Manage local cache
    Cache {
        #[command(subcommand)]
        action: CacheCmd,
    },
    /// Manage auth token (more secure than Basic Auth)
    Token {
        #[command(subcommand)]
        action: TokenCmd,
    },
    /// Download, inspect, and manage Loxone Config files
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
    /// Generate or install shell completions
    Completions {
        /// Shell to generate completions for (auto-detected if omitted)
        shell: Option<Shell>,

        /// Install completions to the standard location for your shell
        #[arg(long)]
        install: bool,
    },
    /// Show command schema for AI agent discovery
    Schema {
        /// Show schema for a specific command (e.g. "blind", "light")
        command: Option<String>,
    },
}

#[derive(Subcommand)]
enum TokenCmd {
    /// Fetch and save a new token (valid 20 days)
    Fetch,
    /// Show current token status
    Info,
    /// Delete saved token (revert to Basic Auth)
    Clear,
    /// Check if token is still valid on the Miniserver
    Check,
    /// Refresh token to extend validity
    Refresh,
    /// Revoke token on the Miniserver
    Revoke,
}

#[derive(Subcommand)]
enum LightCmd {
    /// Set light mood: plus | minus | off | <mood-id>
    Mood {
        name_or_uuid: String,
        /// Mood action: plus, minus, off, or numeric mood ID
        action: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Set dimmer level (0-100)
    #[command(alias = "dimmer")]
    Dim {
        name_or_uuid: String,
        /// Brightness level 0-100
        level: f64,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Set color (hex RGB e.g. #FF0000 or hsv(h,s,v))
    Color {
        name_or_uuid: String,
        /// Color value: hex RGB (#FF0000) or hsv(h,s,v)
        value: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
}

#[derive(Subcommand)]
enum InputCmd {
    /// Set analog/virtual input value
    Set {
        /// Control name or UUID
        name_or_uuid: String,
        /// Value to send (numeric or text)
        value: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
    /// Momentary pulse
    Pulse {
        name_or_uuid: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
}

#[derive(Subcommand)]
enum AutopilotCmd {
    /// List all automatic rules
    #[command(alias = "list")]
    Ls,
    /// Show when a rule last fired
    State { name_or_uuid: String },
}

#[derive(Subcommand)]
enum UpdateCmd {
    /// Check for available firmware updates
    Check,
    /// Install firmware update (requires --yes to confirm)
    Install {
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum MusicCmd {
    /// Play in a zone
    Play {
        /// Zone number
        #[arg(default_value = "1")]
        zone: u32,
    },
    /// Pause in a zone
    Pause {
        /// Zone number
        #[arg(default_value = "1")]
        zone: u32,
    },
    /// Stop in a zone
    Stop {
        /// Zone number
        #[arg(default_value = "1")]
        zone: u32,
    },
    /// Set volume in a zone
    Volume {
        /// Volume level 0-100
        level: u32,
        /// Zone number
        #[arg(default_value = "1")]
        zone: u32,
    },
    /// Skip to next track
    Next {
        /// Zone number
        #[arg(default_value = "1")]
        zone: u32,
    },
    /// Go to previous track
    Prev {
        /// Zone number
        #[arg(default_value = "1")]
        zone: u32,
    },
    /// Mute a zone
    Mute {
        /// Zone number
        #[arg(default_value = "1")]
        zone: u32,
    },
    /// Unmute a zone
    Unmute {
        /// Zone number
        #[arg(default_value = "1")]
        zone: u32,
    },
}

#[derive(Subcommand)]
enum FilesCmd {
    /// List files at a path
    Ls {
        /// Path on the Miniserver filesystem
        #[arg(default_value = "/")]
        path: String,
    },
    /// Download a file
    Get {
        /// Path on the Miniserver filesystem
        path: String,
        /// Local output path (defaults to filename)
        #[arg(long, value_name = "PATH")]
        save_as: Option<String>,
    },
}

#[derive(Subcommand)]
enum OtelCmd {
    /// Continuously push metrics, logs & traces to an OTLP endpoint
    Serve {
        /// OTLP HTTP endpoint URL (e.g. http://localhost:4318)
        #[arg(long, default_value = "http://localhost:4318")]
        endpoint: String,
        /// Push interval (e.g. 30s, 1m, 5m)
        #[arg(long, short = 'i', default_value = "30s")]
        interval: String,
        /// Filter by control type
        #[arg(long, short = 't')]
        r#type: Option<String>,
        /// Filter by room
        #[arg(long, short = 'r')]
        room: Option<String>,
        /// Additional header for auth (e.g. "Authorization=Bearer xxx")
        #[arg(long)]
        header: Vec<String>,
        /// Use delta temporality (required by Dynatrace and some backends)
        #[arg(long)]
        delta: bool,
        /// Disable log export (metrics and traces only)
        #[arg(long)]
        no_logs: bool,
        /// Disable trace export (metrics and logs only)
        #[arg(long)]
        no_traces: bool,
    },
    /// One-shot: push current state and exit
    Push {
        /// OTLP HTTP endpoint URL
        #[arg(long, default_value = "http://localhost:4318")]
        endpoint: String,
        /// Filter by control type
        #[arg(long, short = 't')]
        r#type: Option<String>,
        /// Filter by room
        #[arg(long, short = 'r')]
        room: Option<String>,
        /// Additional header for auth
        #[arg(long)]
        header: Vec<String>,
        /// Use delta temporality (required by Dynatrace and some backends)
        #[arg(long)]
        delta: bool,
        /// Disable log export (metrics only)
        #[arg(long)]
        no_logs: bool,
    },
}

#[derive(Subcommand)]
enum CacheCmd {
    /// Show cache info
    Info,
    /// Clear structure cache (forces fresh fetch)
    Clear,
    /// Refresh structure cache now
    Refresh,
    /// Check if cache is current (without full download)
    Check,
}

#[derive(Subcommand)]
enum SetupCmd {
    /// Set one or more config fields (omitted fields are preserved)
    Set {
        #[arg(long, env = "LOX_HOST")]
        host: Option<String>,
        #[arg(long, env = "LOX_USER")]
        user: Option<String>,
        /// Password (or set LOX_PASS env var to avoid it appearing in the process table)
        #[arg(long, env = "LOX_PASS")]
        pass: Option<String>,
        #[arg(long, env = "LOX_SERIAL")]
        serial: Option<String>,
        /// Enable SSL certificate verification (for valid certs)
        #[arg(long)]
        verify_ssl: bool,
        /// Disable SSL certificate verification (default, for self-signed Miniserver certs)
        #[arg(long, conflicts_with = "verify_ssl")]
        no_verify_ssl: bool,
    },
    /// Show current config (password redacted)
    Show,
}

#[derive(Subcommand)]
enum AliasCmd {
    /// Add or update an alias
    Add { name: String, uuid: String },
    /// Remove an alias
    Remove { name: String },
    /// List all aliases
    #[command(alias = "list")]
    Ls,
}

#[derive(Subcommand)]
enum SceneCmd {
    /// List all saved scenes
    #[command(alias = "list")]
    Ls,
    /// Print a scene's YAML definition
    Show { name: String },
    /// Create a new empty scene file
    New { name: String },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Download the latest Loxone Config from the Miniserver via FTP
    Download {
        /// Custom output filename
        #[arg(long, value_name = "PATH")]
        save_as: Option<String>,
        /// Also decompress LoxCC to XML
        #[arg(long)]
        extract: bool,
    },
    /// List available configs on the Miniserver
    #[command(alias = "list")]
    Ls,
    /// Decompress a local config ZIP to .Loxone XML
    Extract {
        /// Path to a config ZIP file
        file: String,
        /// Custom output filename
        #[arg(long, value_name = "PATH")]
        save_as: Option<String>,
    },
    /// Upload a config to the Miniserver via FTP (dangerous — requires --force)
    Upload {
        /// Path to a config ZIP file
        file: String,
        /// Confirm the upload
        #[arg(long)]
        force: bool,
    },
    /// List user accounts from a .Loxone config file
    Users {
        /// Path to a .Loxone XML file (from `lox config extract`)
        file: String,
    },
    /// List hardware devices from a .Loxone config file
    Devices {
        /// Path to a .Loxone XML file (from `lox config extract`)
        file: String,
    },
    /// Compare two config files (ZIP or .Loxone)
    Diff {
        /// First config file (older)
        file1: String,
        /// Second config file (newer)
        file2: String,
    },
}

// ── Error envelope ────────────────────────────────────────────────────────────

/// Categorize an anyhow error into a machine-readable error code.
fn categorize_error(e: &anyhow::Error) -> &'static str {
    let msg = format!("{:#}", e);
    let lower = msg.to_lowercase();
    if lower.contains("no control matching") {
        "control_not_found"
    } else if lower.contains("ambiguous") {
        "ambiguous_control"
    } else if lower.contains("config not found") {
        "config_not_found"
    } else if lower.contains("requires --yes") {
        "confirmation_required"
    } else if let Some(http_err) = e.downcast_ref::<client::HttpStatusError>() {
        match http_err.status {
            401 => "unauthorized",
            403 => "forbidden",
            404 => "not_found",
            _ => "http_error",
        }
    } else if lower.contains("connection") || lower.contains("timeout") {
        "connection_error"
    } else {
        "error"
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let json = cli.output == OutputFormat::Json;
    if let Err(e) = run(cli) {
        if json {
            let envelope = serde_json::json!({
                "ok": false,
                "error": categorize_error(&e),
                "message": format!("{:#}", e),
            });
            println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
        } else {
            eprintln!("Error: {:#}", e);
        }
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    client::set_verbose(cli.verbose);
    let quiet = cli.quiet;
    let no_header = cli.no_header;
    let json = cli.output == OutputFormat::Json;
    let csv = cli.output == OutputFormat::Csv;
    let dry_run = cli.dry_run;
    // In JSON mode, always be non-interactive
    let _non_interactive = cli.non_interactive || json;
    let trace_id = cli.trace_id.clone();

    if let Some(tid) = &trace_id {
        if cli.verbose > 0 {
            eprintln!("trace-id: {}", tid);
        }
    }

    // Respect NO_COLOR env var (clig.dev standard) and --no-color flag
    if cli.no_color || std::env::var("NO_COLOR").is_ok() {
        // No ANSI colors are used currently, but this signals to any future
        // colored output that colors should be suppressed.
        std::env::set_var("NO_COLOR", "1");
    }

    match cli.cmd {
        Cmd::Setup { action } => match action {
            SetupCmd::Set {
                host,
                user,
                pass,
                serial,
                verify_ssl,
                no_verify_ssl,
            } => {
                let mut cfg = Config::load().unwrap_or_default();
                if let Some(h) = host {
                    cfg.host = if h.starts_with("http://") || h.starts_with("https://") {
                        h
                    } else {
                        format!("https://{}", h)
                    };
                }
                if let Some(u) = user {
                    cfg.user = u;
                }
                if let Some(p) = pass {
                    cfg.pass = p;
                }
                if let Some(s) = serial {
                    cfg.serial = s;
                }
                if verify_ssl {
                    cfg.verify_ssl = Some(true);
                } else if no_verify_ssl {
                    cfg.verify_ssl = Some(false);
                }
                let path = cfg.save()?;
                if !quiet {
                    println!("✓  Config saved to {:?}", path);
                }
            }
            SetupCmd::Show => {
                let cfg = Config::load()?;
                println!("host:   {}", cfg.host);
                println!("user:   {}", cfg.user);
                println!("pass:   {}", "*".repeat(cfg.pass.len()));
                if !cfg.serial.is_empty() {
                    println!("serial: {}", cfg.serial);
                }
                if !cfg.aliases.is_empty() {
                    println!("aliases:");
                    let mut aliases: Vec<_> = cfg.aliases.iter().collect();
                    aliases.sort_by_key(|(k, _)| k.as_str());
                    for (name, uuid) in aliases {
                        println!("  {}: {}", name, uuid);
                    }
                }
            }
        },

        Cmd::Alias { action } => {
            let mut cfg = Config::load()?;
            match action {
                AliasCmd::Add { name, uuid } => {
                    cfg.aliases.insert(name.clone(), uuid.clone());
                    cfg.save()?;
                    if !quiet {
                        println!("✓  alias '{}' → {}", name, uuid);
                    }
                }
                AliasCmd::Remove { name } => {
                    if cfg.aliases.remove(&name).is_some() {
                        cfg.save()?;
                        if !quiet {
                            println!("✓  removed alias '{}'", name);
                        }
                    } else {
                        println!("No alias named '{}'", name);
                    }
                }
                AliasCmd::Ls => {
                    if cfg.aliases.is_empty() {
                        println!("No aliases. Add with: lox alias add <name> <uuid>");
                    } else {
                        let mut aliases: Vec<_> = cfg.aliases.iter().collect();
                        aliases.sort_by_key(|(k, _)| k.as_str());
                        if !no_header {
                            println!("{:<24} UUID", "ALIAS");
                            println!("{}", "─".repeat(60));
                        }
                        for (name, uuid) in aliases {
                            println!("{:<24} {}", name, uuid);
                        }
                    }
                }
            }
        }

        Cmd::Status {
            energy,
            diag,
            net,
            bus,
            lan,
            all,
        } => {
            let show_diag = diag || all;
            let show_net = net || all;
            let show_bus = bus || all;
            let show_lan = lan || all;
            let lox = LoxClient::new(Config::load()?);
            let version = lox.get_text("/dev/cfg/version")?;
            let heap = lox.get_text("/dev/sys/heap")?;
            let sps = lox.get_text("/dev/sps/state")?;
            let check = lox.get_text("/dev/sys/check")?;
            let status = lox.get_text("/data/status")?;

            let ver = xml_attr(&version, "value").unwrap_or("?");
            let heap_val = xml_attr(&heap, "value").unwrap_or("?");
            let sps_num = xml_attr(&sps, "value").unwrap_or("?");
            let conn = xml_attr(&check, "value").unwrap_or("?");

            let sps_label = match sps_num {
                "5" => "✓ Running",
                "3" => "Started",
                "7" => "⚠ Error",
                "1" => "Booting",
                "8" => "Updating",
                n => n,
            };
            let heap_display = if let Some((used, total)) = heap_val.split_once('/') {
                let t_str = total.trim_end_matches("kB");
                let u: f64 = used.parse().unwrap_or(0.0);
                let t: f64 = t_str.parse().unwrap_or(1.0);
                format!("{} / {}  {}", kb_fmt(u), kb_fmt(t), bar(u, t))
            } else {
                heap_val.to_string()
            };

            let ms_name = xml_attr(&status, "Name").unwrap_or("Loxone");
            let ms_ip = xml_attr(&status, "IP").unwrap_or("?");
            let online = if xml_attr(&status, "Offline").unwrap_or("false") == "false" {
                "✓ Online"
            } else {
                "✗ Offline"
            };

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "name": ms_name, "ip": ms_ip, "version": ver,
                        "plc": sps_label, "heap": heap_val, "connections": conn,
                    })
                );
            } else {
                println!("┌─ Loxone Miniserver ─────────────────────────────────");
                println!("│  Name:        {} ({} {})", ms_name, ms_ip, online);
                println!("│  Firmware:    {}", ver);
                println!("│  PLC:         {}", sps_label);
                println!("│  Memory:      {}", heap_display);
                println!("│  Connections: {}", conn);
                println!("└─────────────────────────────────────────────────────");
            }
            if energy {
                let mut lox_mut = LoxClient::new(Config::load()?);
                let meters = lox_mut
                    .list_controls(Some("meter"), None)
                    .unwrap_or_default()
                    .into_iter()
                    .chain(
                        lox_mut
                            .list_controls(Some("energymanager"), None)
                            .unwrap_or_default(),
                    )
                    .collect::<Vec<_>>();
                if meters.is_empty() {
                    println!(
                        "No energy meters found in structure (type 'Meter' or 'EnergyManager')."
                    );
                } else {
                    println!("┌─ Energy Meters ─────────────────────────────────────");
                    for ctrl in &meters {
                        let val = lox_mut
                            .get_all(&ctrl.uuid)
                            .ok()
                            .and_then(|xml| xml_attr(&xml, "value").map(|s| s.to_string()))
                            .unwrap_or_else(|| "-".to_string());
                        let v: f64 = val.parse().unwrap_or(0.0);
                        println!(
                            "│  {:<24} {:>8.3} kW  [{}]",
                            ctrl.name,
                            v,
                            ctrl.room.as_deref().unwrap_or("─")
                        );
                    }
                    println!("└─────────────────────────────────────────────────────");
                }
            }
            if show_diag {
                let cpu = lox.get_text("/jdev/sys/lastcpu").unwrap_or_default();
                let tasks = lox.get_text("/jdev/sys/numtasks").unwrap_or_default();
                let ctx = lox
                    .get_text("/jdev/sys/contextswitches")
                    .unwrap_or_default();
                let sd = lox.get_text("/jdev/sys/sdtest").unwrap_or_default();
                let cpu_val = xml_attr(&cpu, "value").unwrap_or("?");
                let tasks_val = xml_attr(&tasks, "value").unwrap_or("?");
                let ctx_val = xml_attr(&ctx, "value").unwrap_or("?");
                let sd_val = xml_attr(&sd, "value").unwrap_or("?");
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "cpu": cpu_val, "tasks": tasks_val,
                            "context_switches": ctx_val, "sd_health": sd_val,
                        })
                    );
                } else {
                    println!("┌─ Diagnostics ───────────────────────────────────────");
                    println!("│  CPU:              {}", cpu_val);
                    println!("│  Tasks:            {}", tasks_val);
                    println!("│  Context switches: {}", ctx_val);
                    println!("│  SD card:          {}", sd_val);
                    println!("└─────────────────────────────────────────────────────");
                }
            }
            if show_net {
                let ip = lox.get_text("/jdev/cfg/ip").unwrap_or_default();
                let mac = lox.get_text("/jdev/cfg/mac").unwrap_or_default();
                let mask = lox.get_text("/jdev/cfg/mask").unwrap_or_default();
                let gw = lox.get_text("/jdev/cfg/gateway").unwrap_or_default();
                let dns1 = lox.get_text("/jdev/cfg/dns1").unwrap_or_default();
                let dhcp = lox.get_text("/jdev/cfg/dhcp").unwrap_or_default();
                let ntp = lox.get_text("/jdev/cfg/ntp").unwrap_or_default();
                let ip_val = xml_attr(&ip, "value").unwrap_or("?");
                let mac_val = xml_attr(&mac, "value").unwrap_or("?");
                let mask_val = xml_attr(&mask, "value").unwrap_or("?");
                let gw_val = xml_attr(&gw, "value").unwrap_or("?");
                let dns1_val = xml_attr(&dns1, "value").unwrap_or("?");
                let dhcp_val = xml_attr(&dhcp, "value").unwrap_or("?");
                let ntp_val = xml_attr(&ntp, "value").unwrap_or("?");
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ip": ip_val, "mac": mac_val, "mask": mask_val,
                            "gateway": gw_val, "dns": dns1_val,
                            "dhcp": dhcp_val, "ntp": ntp_val,
                        })
                    );
                } else {
                    println!("┌─ Network ───────────────────────────────────────────");
                    println!("│  IP:      {}", ip_val);
                    println!("│  MAC:     {}", mac_val);
                    println!("│  Mask:    {}", mask_val);
                    println!("│  Gateway: {}", gw_val);
                    println!("│  DNS:     {}", dns1_val);
                    println!("│  DHCP:    {}", dhcp_val);
                    println!("│  NTP:     {}", ntp_val);
                    println!("└─────────────────────────────────────────────────────");
                }
            }
            if show_bus {
                let sent = lox.get_text("/jdev/bus/packetssent").unwrap_or_default();
                let recv = lox
                    .get_text("/jdev/bus/packetsreceived")
                    .unwrap_or_default();
                let rerr = lox.get_text("/jdev/bus/receiveerrors").unwrap_or_default();
                let ferr = lox.get_text("/jdev/bus/frameerrors").unwrap_or_default();
                let over = lox.get_text("/jdev/bus/overruns").unwrap_or_default();
                let sent_val = xml_attr(&sent, "value").unwrap_or("?");
                let recv_val = xml_attr(&recv, "value").unwrap_or("?");
                let rerr_val = xml_attr(&rerr, "value").unwrap_or("?");
                let ferr_val = xml_attr(&ferr, "value").unwrap_or("?");
                let over_val = xml_attr(&over, "value").unwrap_or("?");
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "packets_sent": sent_val, "packets_received": recv_val,
                            "receive_errors": rerr_val, "frame_errors": ferr_val,
                            "overruns": over_val,
                        })
                    );
                } else {
                    println!("┌─ CAN Bus ───────────────────────────────────────────");
                    println!("│  Packets sent:     {}", sent_val);
                    println!("│  Packets received: {}", recv_val);
                    println!("│  Receive errors:   {}", rerr_val);
                    println!("│  Frame errors:     {}", ferr_val);
                    println!("│  Overruns:         {}", over_val);
                    println!("└─────────────────────────────────────────────────────");
                }
            }
            if show_lan {
                let txp = lox.get_text("/jdev/lan/txp").unwrap_or_default();
                let txe = lox.get_text("/jdev/lan/txe").unwrap_or_default();
                let txc = lox.get_text("/jdev/lan/txc").unwrap_or_default();
                let rxp = lox.get_text("/jdev/lan/rxp").unwrap_or_default();
                let rxo = lox.get_text("/jdev/lan/rxo").unwrap_or_default();
                let eof = lox.get_text("/jdev/lan/eof").unwrap_or_default();
                let txp_val = xml_attr(&txp, "value").unwrap_or("?");
                let txe_val = xml_attr(&txe, "value").unwrap_or("?");
                let txc_val = xml_attr(&txc, "value").unwrap_or("?");
                let rxp_val = xml_attr(&rxp, "value").unwrap_or("?");
                let rxo_val = xml_attr(&rxo, "value").unwrap_or("?");
                let eof_val = xml_attr(&eof, "value").unwrap_or("?");
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "tx_packets": txp_val, "tx_errors": txe_val, "tx_collisions": txc_val,
                            "rx_packets": rxp_val, "rx_overflow": rxo_val, "eof_errors": eof_val,
                        })
                    );
                } else {
                    println!("┌─ LAN Statistics ────────────────────────────────────");
                    println!("│  TX packets:    {}", txp_val);
                    println!("│  TX errors:     {}", txe_val);
                    println!("│  TX collisions: {}", txc_val);
                    println!("│  RX packets:    {}", rxp_val);
                    println!("│  RX overflow:   {}", rxo_val);
                    println!("│  EOF errors:    {}", eof_val);
                    println!("└─────────────────────────────────────────────────────");
                }
            }
        }

        Cmd::Ls {
            r#type,
            room,
            values,
            cat,
            favorites,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let controls = lox.list_controls_ext(
                r#type.as_deref(),
                room.as_deref(),
                cat.as_deref(),
                favorites,
            )?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &controls
                            .iter()
                            .map(|c| serde_json::json!({
                                "name": c.name, "uuid": c.uuid, "type": c.typ, "room": c.room
                            }))
                            .collect::<Vec<_>>()
                    )?
                );
            } else if values {
                if !no_header {
                    println!(
                        "{:<40} {:<24} {:<22} {:<20} UUID",
                        "NAME", "ROOM", "TYPE", "VALUE"
                    );
                    println!("{}", "─".repeat(140));
                }
                for c in &controls {
                    let val = lox
                        .get_all(&c.uuid)
                        .ok()
                        .and_then(|xml| xml_attr(&xml, "value").map(|s| s.to_string()))
                        .unwrap_or_else(|| "-".to_string());
                    println!(
                        "{:<40} {:<24} {:<22} {:<20} {}",
                        c.name,
                        c.room.as_deref().unwrap_or("─"),
                        c.typ,
                        val,
                        c.uuid
                    );
                }
                println!("\n{} controls", controls.len());
            } else {
                if !no_header {
                    println!("{:<40} {:<24} {:<22} UUID", "NAME", "ROOM", "TYPE");
                    println!("{}", "─".repeat(120));
                }
                for c in &controls {
                    println!(
                        "{:<40} {:<24} {:<22} {}",
                        c.name,
                        c.room.as_deref().unwrap_or("─"),
                        c.typ,
                        c.uuid
                    );
                }
                println!("\n{} controls", controls.len());
            }
        }

        Cmd::Rooms => {
            let mut lox = LoxClient::new(Config::load()?);
            let structure = lox.get_structure()?;
            if let Some(rooms) = structure.get("rooms").and_then(|r| r.as_object()) {
                let mut entries: Vec<_> = rooms
                    .iter()
                    .filter_map(|(uuid, r)| {
                        r.get("name")
                            .and_then(|n| n.as_str())
                            .map(|name| (uuid.as_str(), name))
                    })
                    .collect();
                entries.sort_by_key(|(_, name)| *name);
                if json {
                    let arr: Vec<_> = entries
                        .iter()
                        .map(|(uuid, name)| serde_json::json!({"uuid": uuid, "name": name}))
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&arr)?);
                } else {
                    for (_, name) in entries {
                        println!("{}", name);
                    }
                }
            }
        }

        Cmd::Categories => {
            let mut lox = LoxClient::new(Config::load()?);
            let cats = lox.list_categories()?;
            if json {
                let arr: Vec<_> = cats
                    .iter()
                    .map(|(uuid, name)| serde_json::json!({"uuid": uuid, "name": name}))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else {
                println!("{:<36} NAME", "UUID");
                println!("{}", "─".repeat(60));
                for (uuid, name) in &cats {
                    println!("{:<36} {}", uuid, name);
                }
                println!("\n{} categories", cats.len());
            }
        }

        Cmd::Info { name_or_uuid, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid_resolved = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid_resolved)?;
            let ctrl_json = lox.get_control_json(&ctrl.uuid)?;
            let xml = lox.get_all(&ctrl.uuid).unwrap_or_default();

            if json {
                println!("{}", serde_json::to_string_pretty(&ctrl_json)?);
            } else {
                println!("Control:    {} ({})", ctrl.name, ctrl.uuid);
                println!(
                    "Type:       {}   Room: {}   Cat: {}",
                    ctrl.typ,
                    ctrl.room.as_deref().unwrap_or("─"),
                    ctrl.cat.as_deref().unwrap_or("─"),
                );
                println!(
                    "Favorite:   {}   Secured: {}",
                    if ctrl.is_favorite { "yes" } else { "no" },
                    if ctrl.is_secured { "yes" } else { "no" },
                );
                let val = xml_attr(&xml, "value").unwrap_or("?");
                println!("Value:      {}", val);

                // Sub-controls
                if let Some(subs) = ctrl_json.get("subControls").and_then(|s| s.as_object()) {
                    println!("\nSub-controls:");
                    for (sub_uuid, sub) in subs {
                        let sub_name = sub.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                        let sub_type = sub.get("type").and_then(|t| t.as_str()).unwrap_or("?");
                        println!("  {:<30} {:<20} {}", sub_name, sub_type, sub_uuid);
                    }
                }

                // States
                if let Some(states) = ctrl_json.get("states").and_then(|s| s.as_object()) {
                    println!("\nStates:");
                    let mut state_list: Vec<_> = states.iter().collect();
                    state_list.sort_by_key(|(k, _)| k.as_str());
                    for (state_name, state_uuid) in &state_list {
                        let uuid_str = state_uuid.as_str().unwrap_or("?");
                        println!("  {:<30} {}", state_name, uuid_str);
                    }
                }

                // Details (moods for LightControllerV2)
                if let Some(details) = ctrl_json.get("details").and_then(|d| d.as_object()) {
                    if let Some(moods) = details.get("moods").and_then(|m| m.as_array()) {
                        println!("\nMoods:");
                        for mood in moods {
                            let id = mood.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
                            let name = mood.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                            println!("  {:<6} {}", id, name);
                        }
                    }
                }

                // Statistics config
                if let Some(stat) = ctrl_json.get("statistic") {
                    if !stat.is_null() {
                        println!("\nStatistics: enabled");
                        if let Some(outputs) = stat.get("outputs").and_then(|o| o.as_object()) {
                            for (k, v) in outputs {
                                let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                                println!("  {:<30} {}", name, k);
                            }
                        }
                    }
                }
            }
        }

        Cmd::Globals => {
            let mut lox = LoxClient::new(Config::load()?);
            let globals = lox.get_global_states()?;
            if json {
                let mut map = serde_json::Map::new();
                for (name, uuid) in &globals {
                    let val = lox
                        .get_all(uuid)
                        .ok()
                        .and_then(|xml| xml_attr(&xml, "value").map(|s| s.to_string()))
                        .unwrap_or_else(|| "?".to_string());
                    map.insert(
                        name.clone(),
                        serde_json::json!({"uuid": uuid, "value": val}),
                    );
                }
                println!("{}", serde_json::to_string_pretty(&Value::Object(map))?);
            } else {
                println!("{:<30} {:<36} VALUE", "STATE", "UUID");
                println!("{}", "─".repeat(90));
                for (name, uuid) in &globals {
                    let val = lox
                        .get_all(uuid)
                        .ok()
                        .and_then(|xml| xml_attr(&xml, "value").map(|s| s.to_string()))
                        .unwrap_or_else(|| "?".to_string());
                    println!("{:<30} {:<36} {}", name, uuid, val);
                }
            }
        }

        Cmd::Modes => {
            let mut lox = LoxClient::new(Config::load()?);
            let modes = lox.get_operating_modes()?;
            // Try to get the current operating mode
            let globals = lox.get_global_states().unwrap_or_default();
            let current_mode = globals
                .iter()
                .find(|(name, _)| name == "operatingMode")
                .and_then(|(_, uuid)| {
                    lox.get_all(uuid)
                        .ok()
                        .and_then(|xml| xml_attr(&xml, "value").map(|s| s.to_string()))
                });
            if json {
                let arr: Vec<_> = modes
                    .iter()
                    .map(|(id, name)| {
                        let active = current_mode.as_deref() == Some(id.as_str());
                        serde_json::json!({"id": id, "name": name, "active": active})
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else {
                println!("{:<6} {:<30} STATUS", "ID", "MODE");
                println!("{}", "─".repeat(50));
                for (id, name) in &modes {
                    let active = current_mode.as_deref() == Some(id.as_str());
                    println!(
                        "{:<6} {:<30} {}",
                        id,
                        name,
                        if active { "← active" } else { "" }
                    );
                }
            }
        }

        Cmd::Get { name_or_uuid, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid_resolved = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid_resolved)?;
            let xml = lox.get_all(&ctrl.uuid)?;
            let val = xml_attr(&xml, "value").unwrap_or("?");
            let code = xml_attr(&xml, "Code").unwrap_or("?");

            if json {
                let mut result = serde_json::json!({
                    "name": ctrl.name, "uuid": ctrl.uuid,
                    "type": ctrl.typ, "room": ctrl.room, "value": val,
                });
                for attr in &[
                    "StateUp",
                    "StateDown",
                    "StatePos",
                    "StateShade",
                    "StateAutoShade",
                    "StateSafety",
                ] {
                    if let Some(v) = xml_attr(&xml, attr) {
                        result[attr] = Value::String(v.to_string());
                    }
                }
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Control:  {} ({})", ctrl.name, ctrl.uuid);
                println!(
                    "Type:     {}   Room: {}",
                    ctrl.typ,
                    ctrl.room.as_deref().unwrap_or("─")
                );
                println!("Value:    {}  [Code {}]", val, code);
                for attr in &[
                    "StateUp",
                    "StateDown",
                    "StatePos",
                    "StateShade",
                    "StateAutoShade",
                ] {
                    if let Some(v) = xml_attr(&xml, attr) {
                        println!("  {:<12} {}", attr.trim_start_matches("State"), v);
                    }
                }
                let mut i = 1;
                loop {
                    let Some(n) = xml_attr(&xml, &format!("n{}", i)) else {
                        break;
                    };
                    let v = xml_attr(&xml, &format!("v{}", i)).unwrap_or("?");
                    if !n.is_empty() {
                        println!("  {:<30} = {}", n, v);
                    }
                    i += 1;
                }
            }
        }

        Cmd::Send {
            name_or_uuid,
            command,
            room,
            secured,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            if dry_run {
                let ctrl = lox.find_control(&uuid).ok();
                let name = ctrl
                    .as_ref()
                    .map(|c| c.name.as_str())
                    .unwrap_or(&name_or_uuid);
                let room = ctrl.as_ref().and_then(|c| c.room.as_deref());
                print_dry_run(json, quiet, &uuid, &command, name, room);
            } else {
                let resp = if let Some(hash) = secured {
                    lox.get_json(&format!("/jdev/sps/ios/{}/{}/{}", hash, uuid, command))?
                } else {
                    lox.send_cmd(&uuid, &command)?
                };
                print_resp(&resp, json, quiet, &name_or_uuid, &command);
            }
        }
        Cmd::On {
            name_or_uuid,
            room,
            all_in_room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            if let Some(room_name) = all_in_room {
                let controls = lox.resolve_all_in_room(&room_name, None)?;
                for ctrl in &controls {
                    if dry_run {
                        print_dry_run(
                            json,
                            quiet,
                            &ctrl.uuid,
                            "on",
                            &ctrl.name,
                            ctrl.room.as_deref(),
                        );
                    } else {
                        match lox.send_cmd(&ctrl.uuid, "on") {
                            Ok(resp) => print_resp(&resp, json, quiet, &ctrl.name, "on"),
                            Err(e) => eprintln!("  {} — {}", ctrl.name, e),
                        }
                    }
                }
            } else {
                let name = name_or_uuid
                    .ok_or_else(|| anyhow::anyhow!("Provide a control name or --all-in-room"))?;
                let uuid = lox.resolve_with_room(&name, room.as_deref())?;
                if dry_run {
                    let ctrl = lox.find_control(&uuid).ok();
                    let room = ctrl.as_ref().and_then(|c| c.room.as_deref());
                    print_dry_run(json, quiet, &uuid, "on", &name, room);
                } else {
                    let resp = lox.send_cmd(&uuid, "on")?;
                    print_resp(&resp, json, quiet, &name, "on");
                }
            }
        }
        Cmd::Off {
            name_or_uuid,
            room,
            all_in_room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            if let Some(room_name) = all_in_room {
                let controls = lox.resolve_all_in_room(&room_name, None)?;
                for ctrl in &controls {
                    if dry_run {
                        print_dry_run(
                            json,
                            quiet,
                            &ctrl.uuid,
                            "off",
                            &ctrl.name,
                            ctrl.room.as_deref(),
                        );
                    } else {
                        match lox.send_cmd(&ctrl.uuid, "off") {
                            Ok(resp) => print_resp(&resp, json, quiet, &ctrl.name, "off"),
                            Err(e) => eprintln!("  {} — {}", ctrl.name, e),
                        }
                    }
                }
            } else {
                let name = name_or_uuid
                    .ok_or_else(|| anyhow::anyhow!("Provide a control name or --all-in-room"))?;
                let uuid = lox.resolve_with_room(&name, room.as_deref())?;
                if dry_run {
                    let ctrl = lox.find_control(&uuid).ok();
                    let room = ctrl.as_ref().and_then(|c| c.room.as_deref());
                    print_dry_run(json, quiet, &uuid, "off", &name, room);
                } else {
                    let resp = lox.send_cmd(&uuid, "off")?;
                    print_resp(&resp, json, quiet, &name, "off");
                }
            }
        }
        Cmd::Pulse { name_or_uuid, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            if dry_run {
                let ctrl = lox.find_control(&uuid).ok();
                let room = ctrl.as_ref().and_then(|c| c.room.as_deref());
                print_dry_run(json, quiet, &uuid, "pulse", &name_or_uuid, room);
            } else {
                let resp = lox.send_cmd(&uuid, "pulse")?;
                print_resp(&resp, json, quiet, &name_or_uuid, "pulse");
            }
        }

        Cmd::Blind {
            name_or_uuid,
            action,
            pos,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !matches!(ctrl.typ.as_str(), "Jalousie" | "CentralJalousie") {
                bail!("'{}' is type '{}', not a Jalousie", ctrl.name, ctrl.typ);
            }
            let cmd_owned: String;
            let cmd: &str = match action.to_lowercase().as_str() {
                "up" | "open" => "FullUp",
                "down" | "close" => "FullDown",
                "stop" => "off",
                "shade" | "auto" => {
                    if let Some(pct) = pos {
                        if !(0.0..=100.0).contains(&pct) {
                            bail!("Position must be 0-100");
                        }
                        cmd_owned = format!("manualLamella/{:.4}", pct);
                        &cmd_owned
                    } else {
                        "AutomaticDown"
                    }
                }
                "pos" | "position" => {
                    let pct = pos.ok_or_else(|| anyhow::anyhow!("pos requires a value 0-100"))?;
                    if !(0.0..=100.0).contains(&pct) {
                        bail!("Position must be 0-100");
                    }
                    cmd_owned = format!("manualPosition/{:.4}", pct);
                    &cmd_owned
                }
                other => {
                    // Try numeric mood-style: pos 50
                    if let Ok(pct) = other.parse::<f64>() {
                        if (0.0..=100.0).contains(&pct) {
                            cmd_owned = format!("manualPosition/{:.4}", pct);
                            &cmd_owned
                        } else {
                            bail!("Position must be 0-100");
                        }
                    } else {
                        bail!(
                            "Unknown action '{}'. Use: up down stop shade [<0-100>] pos <0-100>",
                            other
                        )
                    }
                }
            };
            if dry_run {
                print_dry_run(
                    json,
                    quiet,
                    &ctrl.uuid,
                    cmd,
                    &ctrl.name,
                    ctrl.room.as_deref(),
                );
                return Ok(());
            }
            let resp = lox.send_cmd(&ctrl.uuid, cmd)?;
            print_resp(&resp, json, quiet, &ctrl.name, cmd);
            if !json {
                if cmd.starts_with("manualLamella") {
                    // Slat tilt doesn't change StatePos; just read once after a short settle.
                    thread::sleep(Duration::from_millis(800));
                    let xml = lox.get_all(&ctrl.uuid)?;
                    if let Some(p) = xml_attr(&xml, "StatePos").and_then(|v| v.parse::<f64>().ok())
                    {
                        println!("   Position: {:.0}%  {}", p * 100.0, bar(p, 1.0));
                    }
                    if let Some(s) =
                        xml_attr(&xml, "StateShade").and_then(|v| v.parse::<f64>().ok())
                    {
                        println!("   Shade:    {:.0}%  {}", s * 100.0, bar(s, 1.0));
                    }
                } else {
                    // manualPosition moves don't set StateUp/StateDown, so we can't use those
                    // as a motion indicator. Instead, poll until StatePos stabilizes (two
                    // consecutive reads agree) or we time out after 30s.
                    let deadline = std::time::Instant::now() + Duration::from_secs(30);
                    let mut prev_pos: Option<f64> = None;
                    loop {
                        thread::sleep(Duration::from_millis(500));
                        let xml = lox.get_all(&ctrl.uuid)?;
                        let cur_pos =
                            xml_attr(&xml, "StatePos").and_then(|v| v.parse::<f64>().ok());
                        let timed_out = std::time::Instant::now() >= deadline;
                        let stable = matches!((prev_pos, cur_pos), (Some(a), Some(b)) if (a - b).abs() < 0.005);
                        if stable || timed_out {
                            if let Some(p) = cur_pos {
                                let suffix = if timed_out && !stable {
                                    "  (moving…)"
                                } else {
                                    ""
                                };
                                println!(
                                    "   Position: {:.0}%  {}{}",
                                    p * 100.0,
                                    bar(p, 1.0),
                                    suffix
                                );
                            }
                            break;
                        }
                        prev_pos = cur_pos;
                    }
                }
            }
        }

        Cmd::Light { action } => match action {
            LightCmd::Mood {
                name_or_uuid,
                action,
                room,
            } => {
                let mut lox = LoxClient::new(Config::load()?);
                let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
                let ctrl = lox.find_control(&uuid)?;
                if !matches!(ctrl.typ.as_str(), "LightControllerV2" | "LightController") {
                    bail!(
                        "'{}' is type '{}', not a LightController",
                        ctrl.name,
                        ctrl.typ
                    );
                }
                let cmd_owned: String;
                let cmd: &str = match action.to_lowercase().as_str() {
                    "plus" | "next" | "+" => "plus",
                    "minus" | "prev" | "-" => "minus",
                    "off" => "setMood/778",
                    other => {
                        if let Ok(id) = other.parse::<u32>() {
                            cmd_owned = format!("setMood/{}", id);
                            &cmd_owned
                        } else {
                            bail!(
                                "Unknown mood action '{}'. Use: plus, minus, off, or a numeric mood ID",
                                other
                            )
                        }
                    }
                };
                if let Some(resp) = send_or_dry_run(
                    &lox,
                    &ctrl.uuid,
                    cmd,
                    &ctrl.name,
                    ctrl.room.as_deref(),
                    dry_run,
                    json,
                    quiet,
                )? {
                    if json {
                        print_resp(&resp, true, quiet, &ctrl.name, cmd);
                    } else if !quiet {
                        println!("✓  {} → mood {}", ctrl.name, action);
                    }
                }
            }
            LightCmd::Dim {
                name_or_uuid,
                level,
                room,
            } => {
                let mut lox = LoxClient::new(Config::load()?);
                let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
                let ctrl = lox.find_control(&uuid)?;
                if !(0.0..=100.0).contains(&level) {
                    bail!("Dimmer level must be 0-100");
                }
                let dim_cmd = format!("{}", level);
                if let Some(resp) = send_or_dry_run(
                    &lox,
                    &ctrl.uuid,
                    &dim_cmd,
                    &ctrl.name,
                    ctrl.room.as_deref(),
                    dry_run,
                    json,
                    quiet,
                )? {
                    print_resp(&resp, json, quiet, &ctrl.name, &format!("dim={}", level));
                }
            }
            LightCmd::Color {
                name_or_uuid,
                value,
                room,
            } => {
                let mut lox = LoxClient::new(Config::load()?);
                let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
                let ctrl = lox.find_control(&uuid)?;
                if !matches!(ctrl.typ.as_str(), "ColorPickerV2" | "ColorPicker") {
                    bail!("'{}' is type '{}', not a ColorPicker", ctrl.name, ctrl.typ);
                }
                let cmd = if value.starts_with('#') {
                    let hex = value.trim_start_matches('#');
                    if hex.len() != 6 {
                        bail!("Hex color must be 6 digits: #RRGGBB");
                    }
                    let r = u8::from_str_radix(&hex[0..2], 16)?;
                    let g = u8::from_str_radix(&hex[2..4], 16)?;
                    let b = u8::from_str_radix(&hex[4..6], 16)?;
                    let (h, s, v) = rgb_to_hsv(r, g, b);
                    format!("hsv({},{},{})", h, s, v)
                } else {
                    value
                };
                if let Some(resp) = send_or_dry_run(
                    &lox,
                    &ctrl.uuid,
                    &cmd,
                    &ctrl.name,
                    ctrl.room.as_deref(),
                    dry_run,
                    json,
                    quiet,
                )? {
                    print_resp(&resp, json, quiet, &ctrl.name, &cmd);
                }
            }
        },

        Cmd::Input { action } => match action {
            InputCmd::Set {
                name_or_uuid,
                value,
                room,
            } => {
                let mut lox = LoxClient::new(Config::load()?);
                let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
                let encoded = encode_path_value(&value);
                if dry_run {
                    print_dry_run(json, quiet, &uuid, &encoded, &name_or_uuid, None);
                } else {
                    let resp = lox.send_cmd(&uuid, &encoded)?;
                    let code = resp
                        .pointer("/LL/Code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let val = resp
                        .pointer("/LL/value")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    if code == "200" {
                        if !quiet {
                            println!("✓  {} = {}", name_or_uuid, val);
                        }
                    } else {
                        bail!("Error {}: {}", code, val);
                    }
                }
            }
            InputCmd::Pulse { name_or_uuid, room } => {
                let mut lox = LoxClient::new(Config::load()?);
                let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
                if let Some(resp) = send_or_dry_run(
                    &lox,
                    &uuid,
                    "pulse",
                    &name_or_uuid,
                    None,
                    dry_run,
                    json,
                    quiet,
                )? {
                    print_resp(&resp, json, quiet, &name_or_uuid, "pulse");
                }
            }
        },

        // Legacy aliases — hidden but still functional
        Cmd::Mood {
            name_or_uuid,
            action,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !matches!(ctrl.typ.as_str(), "LightControllerV2" | "LightController") {
                bail!(
                    "'{}' is type '{}', not a LightController",
                    ctrl.name,
                    ctrl.typ
                );
            }
            let cmd_owned: String;
            let cmd: &str = match action.to_lowercase().as_str() {
                "plus" | "next" | "+" => "plus",
                "minus" | "prev" | "-" => "minus",
                "off" => "setMood/778",
                other => {
                    if let Ok(id) = other.parse::<u32>() {
                        cmd_owned = format!("setMood/{}", id);
                        &cmd_owned
                    } else {
                        bail!(
                            "Unknown mood action '{}'. Use: plus, minus, off, or a numeric mood ID",
                            other
                        )
                    }
                }
            };
            if let Some(resp) = send_or_dry_run(
                &lox,
                &ctrl.uuid,
                cmd,
                &ctrl.name,
                ctrl.room.as_deref(),
                dry_run,
                json,
                quiet,
            )? {
                if json {
                    print_resp(&resp, true, quiet, &ctrl.name, cmd);
                } else {
                    println!("✓  {} → mood {}", ctrl.name, action);
                    thread::sleep(Duration::from_millis(400));
                    let xml = lox.get_all(&ctrl.uuid)?;
                    let state = xml_attr(&xml, "value").unwrap_or_default();
                    let is_off = state.starts_with("200002700") || state == "0";
                    println!(
                        "   State: {}  ({})",
                        state,
                        if is_off { "off" } else { "active" }
                    );
                }
            }
        }

        Cmd::Dimmer {
            name_or_uuid,
            level,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !(0.0..=100.0).contains(&level) {
                bail!("Dimmer level must be 0-100");
            }
            let dim_cmd = format!("{}", level);
            if let Some(resp) = send_or_dry_run(
                &lox,
                &ctrl.uuid,
                &dim_cmd,
                &ctrl.name,
                ctrl.room.as_deref(),
                dry_run,
                json,
                quiet,
            )? {
                print_resp(&resp, json, quiet, &ctrl.name, &format!("dim={}", level));
            }
        }

        Cmd::Gate {
            name_or_uuid,
            action,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !matches!(ctrl.typ.as_str(), "Gate" | "CentralGate") {
                bail!("'{}' is type '{}', not a Gate", ctrl.name, ctrl.typ);
            }
            let cmd = match action.to_lowercase().as_str() {
                "open" => "open",
                "close" => "close",
                "stop" => "stop",
                other => bail!("Unknown gate action '{}'. Use: open, close, stop", other),
            };
            if let Some(resp) = send_or_dry_run(
                &lox,
                &ctrl.uuid,
                cmd,
                &ctrl.name,
                ctrl.room.as_deref(),
                dry_run,
                json,
                quiet,
            )? {
                print_resp(&resp, json, quiet, &ctrl.name, cmd);
            }
        }

        Cmd::Color {
            name_or_uuid,
            value,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !matches!(ctrl.typ.as_str(), "ColorPickerV2" | "ColorPicker") {
                bail!("'{}' is type '{}', not a ColorPicker", ctrl.name, ctrl.typ);
            }
            // Parse color: #RRGGBB → "hsv(h,s,v)" or pass through hsv() format
            let cmd = if value.starts_with('#') {
                // Convert hex RGB to Loxone-compatible format
                let hex = value.trim_start_matches('#');
                if hex.len() != 6 {
                    bail!("Hex color must be 6 digits: #RRGGBB");
                }
                let r = u8::from_str_radix(&hex[0..2], 16).context("Invalid red component")?;
                let g = u8::from_str_radix(&hex[2..4], 16).context("Invalid green component")?;
                let b = u8::from_str_radix(&hex[4..6], 16).context("Invalid blue component")?;
                // Convert RGB to HSV
                let (h, s, v) = rgb_to_hsv(r, g, b);
                format!("hsv({},{},{})", h, s, v)
            } else {
                value.clone()
            };
            if let Some(resp) = send_or_dry_run(
                &lox,
                &ctrl.uuid,
                &cmd,
                &ctrl.name,
                ctrl.room.as_deref(),
                dry_run,
                json,
                quiet,
            )? {
                print_resp(&resp, json, quiet, &ctrl.name, &cmd);
            }
        }

        Cmd::Weather { forecast } => {
            let lox = LoxClient::new(Config::load()?);
            let data = lox.get_bytes("/data/weatheru.bin")?;
            if data.is_empty() {
                println!("No weather data available on the Miniserver.");
            } else {
                // Parse 108-byte weather entries
                let entry_size = 108;
                let num_entries = data.len() / entry_size;
                let lox_epoch = chrono::NaiveDate::from_ymd_opt(2009, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_local_timezone(chrono::Local)
                    .unwrap();

                let max_display = if forecast {
                    num_entries
                } else {
                    1.min(num_entries)
                };

                if json {
                    let mut arr = Vec::new();
                    for i in 0..max_display {
                        let offset = i * entry_size;
                        let mut cursor = Cursor::new(&data[offset..offset + entry_size]);
                        if let Some(entry) = parse_weather_entry(&mut cursor, &lox_epoch) {
                            arr.push(entry);
                        }
                    }
                    println!("{}", serde_json::to_string_pretty(&arr)?);
                } else {
                    println!(
                        "{:<20} {:>8} {:>8} {:>8} {:>8} {:>10}",
                        "TIME", "TEMP°C", "HUM%", "WIND", "RAIN", "CLOUDS"
                    );
                    println!("{}", "─".repeat(72));
                    for i in 0..max_display {
                        let offset = i * entry_size;
                        let mut cursor = Cursor::new(&data[offset..offset + entry_size]);
                        if let Some(entry) = parse_weather_entry(&mut cursor, &lox_epoch) {
                            println!(
                                "{:<20} {:>8.1} {:>8.0} {:>8.1} {:>8.1} {:>10.0}",
                                entry["timestamp"].as_str().unwrap_or("?"),
                                entry["temperature"].as_f64().unwrap_or(0.0),
                                entry["humidity"].as_f64().unwrap_or(0.0),
                                entry["wind_speed"].as_f64().unwrap_or(0.0),
                                entry["rain"].as_f64().unwrap_or(0.0),
                                entry["clouds"].as_f64().unwrap_or(0.0),
                            );
                        }
                    }
                }
            }
        }

        Cmd::Discover { timeout } => {
            use std::net::UdpSocket;
            println!("Scanning for Loxone Miniservers...");
            let socket = UdpSocket::bind("0.0.0.0:0")?;
            socket.set_broadcast(true)?;
            socket.set_read_timeout(Some(Duration::from_secs(timeout)))?;
            // Send discovery packet (single null byte) to port 7070
            socket.send_to(&[0x00], "255.255.255.255:7070")?;
            let mut buf = [0u8; 1024];
            let mut found = Vec::new();
            while let Ok((len, addr)) = socket.recv_from(&mut buf) {
                let msg = String::from_utf8_lossy(&buf[..len]);
                if json {
                    found.push(serde_json::json!({
                        "address": addr.to_string(),
                        "response": msg.to_string(),
                    }));
                } else {
                    println!("  Found: {} — {}", addr, msg.trim());
                }
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&found)?);
            } else if found.is_empty() {
                println!("No Miniservers found. (Timeout: {}s)", timeout);
            }
        }

        Cmd::Thermostat {
            name_or_uuid,
            action,
            value,
            duration,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !matches!(
                ctrl.typ.as_str(),
                "IRoomControllerV2" | "IRoomController" | "Fronius"
            ) {
                bail!(
                    "'{}' is type '{}', not a room controller",
                    ctrl.name,
                    ctrl.typ
                );
            }
            if let Some(act) = action {
                match act.to_lowercase().as_str() {
                    "temp" | "temperature" => {
                        let t: f64 = value
                            .as_deref()
                            .ok_or_else(|| {
                                anyhow::anyhow!("Usage: lox thermostat <name> temp <°C>")
                            })?
                            .parse()
                            .context("Temperature must be a number")?;
                        let temp_cmd = format!("setComfortTemperature/{}", t);
                        if let Some(resp) = send_or_dry_run(
                            &lox,
                            &ctrl.uuid,
                            &temp_cmd,
                            &ctrl.name,
                            ctrl.room.as_deref(),
                            dry_run,
                            json,
                            quiet,
                        )? {
                            print_resp(&resp, json, quiet, &ctrl.name, &format!("temp={}", t));
                        }
                    }
                    "mode" => {
                        let m = value.as_deref().ok_or_else(|| {
                            anyhow::anyhow!(
                                "Usage: lox thermostat <name> mode <auto|manual|comfort|eco>"
                            )
                        })?;
                        let lower = m.to_lowercase();
                        let mode_id = match lower.as_str() {
                            "auto" | "automatic" => "0",
                            "manual" => "1",
                            "comfort" => "2",
                            "eco" | "economy" => "3",
                            "building-protection" | "building" => "4",
                            other => other,
                        };
                        let mode_cmd = format!("setOperatingMode/{}", mode_id);
                        if let Some(resp) = send_or_dry_run(
                            &lox,
                            &ctrl.uuid,
                            &mode_cmd,
                            &ctrl.name,
                            ctrl.room.as_deref(),
                            dry_run,
                            json,
                            quiet,
                        )? {
                            print_resp(&resp, json, quiet, &ctrl.name, &format!("mode={}", m));
                        }
                    }
                    "override" => {
                        let temp_override: f64 = value
                            .as_deref()
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "Usage: lox thermostat <name> override <°C> [minutes]"
                                )
                            })?
                            .parse()
                            .context("Override temperature must be a number")?;
                        let dur = duration.unwrap_or(60);
                        let override_cmd = format!("override/{}/{}", temp_override, dur);
                        if let Some(resp) = send_or_dry_run(
                            &lox,
                            &ctrl.uuid,
                            &override_cmd,
                            &ctrl.name,
                            ctrl.room.as_deref(),
                            dry_run,
                            json,
                            quiet,
                        )? {
                            print_resp(
                                &resp,
                                json,
                                quiet,
                                &ctrl.name,
                                &format!("override={}°/{}min", temp_override, dur),
                            );
                        }
                    }
                    other => bail!(
                        "Unknown thermostat action '{}'. Use: temp, mode, override",
                        other
                    ),
                }
            } else {
                // Show current thermostat state
                let xml = lox.get_all(&ctrl.uuid)?;
                let val = xml_attr(&xml, "value").unwrap_or("?");
                if json {
                    let mut result = serde_json::json!({
                        "name": ctrl.name, "uuid": ctrl.uuid,
                        "type": ctrl.typ, "value": val,
                    });
                    let mut i = 1;
                    loop {
                        let Some(n) = xml_attr(&xml, &format!("n{}", i)) else {
                            break;
                        };
                        let v = xml_attr(&xml, &format!("v{}", i)).unwrap_or("?");
                        if !n.is_empty() {
                            result[n] = Value::String(v.to_string());
                        }
                        i += 1;
                    }
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!("Thermostat: {} ({})", ctrl.name, ctrl.uuid);
                    println!("Room:       {}", ctrl.room.as_deref().unwrap_or("─"));
                    let mut i = 1;
                    loop {
                        let Some(n) = xml_attr(&xml, &format!("n{}", i)) else {
                            break;
                        };
                        let v = xml_attr(&xml, &format!("v{}", i)).unwrap_or("?");
                        if !n.is_empty() {
                            println!("  {:<30} = {}", n, v);
                        }
                        i += 1;
                    }
                }
            }
        }

        Cmd::Door {
            name_or_uuid,
            action,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !ctrl.typ.contains("DoorLock") && !ctrl.typ.contains("Lock") {
                bail!("'{}' is type '{}', not a DoorLock", ctrl.name, ctrl.typ);
            }
            let cmd = match action.to_lowercase().as_str() {
                "lock" => "on",
                "unlock" => "off",
                "open" => "open",
                other => bail!(
                    "Unknown doorlock action '{}'. Use: lock, unlock, open",
                    other
                ),
            };
            if let Some(resp) = send_or_dry_run(
                &lox,
                &ctrl.uuid,
                cmd,
                &ctrl.name,
                ctrl.room.as_deref(),
                dry_run,
                json,
                quiet,
            )? {
                print_resp(&resp, json, quiet, &ctrl.name, &action);
            }
        }

        Cmd::Intercom {
            name_or_uuid,
            action,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !ctrl.typ.contains("Intercom") {
                bail!("'{}' is type '{}', not an Intercom", ctrl.name, ctrl.typ);
            }
            let cmd = match action.to_lowercase().as_str() {
                "answer" => "answer",
                "hangup" | "decline" => "hangup",
                "open" => "open",
                other => bail!(
                    "Unknown intercom action '{}'. Use: answer, hangup, open",
                    other
                ),
            };
            if let Some(resp) = send_or_dry_run(
                &lox,
                &ctrl.uuid,
                cmd,
                &ctrl.name,
                ctrl.room.as_deref(),
                dry_run,
                json,
                quiet,
            )? {
                print_resp(&resp, json, quiet, &ctrl.name, &action);
            }
        }

        Cmd::Charger {
            name_or_uuid,
            action,
            limit,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !ctrl.typ.contains("Charger") && !ctrl.typ.contains("EV") {
                bail!("'{}' is type '{}', not a Charger", ctrl.name, ctrl.typ);
            }
            let cmd_owned: String;
            let cmd: &str = match action.to_lowercase().as_str() {
                "start" => {
                    if let Some(kwh) = limit {
                        cmd_owned = format!("start/{:.1}", kwh);
                        &cmd_owned
                    } else {
                        "start"
                    }
                }
                "stop" => "stop",
                "pause" => "pause",
                other => bail!(
                    "Unknown charger action '{}'. Use: start, stop, pause",
                    other
                ),
            };
            if let Some(resp) = send_or_dry_run(
                &lox,
                &ctrl.uuid,
                cmd,
                &ctrl.name,
                ctrl.room.as_deref(),
                dry_run,
                json,
                quiet,
            )? {
                print_resp(&resp, json, quiet, &ctrl.name, &action);
            }
        }

        Cmd::Alarm {
            name_or_uuid,
            action,
            no_motion,
            code,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !matches!(ctrl.typ.as_str(), "Alarm") {
                bail!("'{}' is type '{}', not an Alarm", ctrl.name, ctrl.typ);
            }
            let cmd_owned: String;
            let cmd: &str = match action.to_lowercase().as_str() {
                "arm" | "on" => {
                    let base = if no_motion {
                        "delayedon/0"
                    } else {
                        "delayedon/1"
                    };
                    if let Some(ref pin) = code {
                        cmd_owned = format!("{}/{}", base, pin);
                        &cmd_owned
                    } else {
                        base
                    }
                }
                "arm-home" | "home" => {
                    if let Some(ref pin) = code {
                        cmd_owned = format!("delayedon/0/{}", pin);
                        &cmd_owned
                    } else {
                        "delayedon/0"
                    }
                }
                "disarm" | "off" => {
                    if let Some(ref pin) = code {
                        cmd_owned = format!("off/{}", pin);
                        &cmd_owned
                    } else {
                        "off"
                    }
                }
                "quit" | "ack" | "acknowledge" => "quit",
                other => bail!(
                    "Unknown alarm action '{}'. Use: arm, arm-home, disarm, quit",
                    other
                ),
            };
            if let Some(resp) = send_or_dry_run(
                &lox,
                &ctrl.uuid,
                cmd,
                &ctrl.name,
                ctrl.room.as_deref(),
                dry_run,
                json,
                quiet,
            )? {
                print_resp(&resp, json, quiet, &ctrl.name, cmd);
            }
        }

        Cmd::Lock {
            name_or_uuid,
            reason,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            let lock_cmd = format!("lockcontrol/1/{}", encode_path_value(&reason));
            if let Some(resp) = send_or_dry_run(
                &lox,
                &ctrl.uuid,
                &lock_cmd,
                &ctrl.name,
                ctrl.room.as_deref(),
                dry_run,
                json,
                quiet,
            )? {
                print_resp(&resp, json, quiet, &ctrl.name, "lock");
            }
        }

        Cmd::Unlock { name_or_uuid, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if let Some(resp) = send_or_dry_run(
                &lox,
                &ctrl.uuid,
                "unlockcontrol",
                &ctrl.name,
                ctrl.room.as_deref(),
                dry_run,
                json,
                quiet,
            )? {
                print_resp(&resp, json, quiet, &ctrl.name, "unlock");
            }
        }

        Cmd::Stats => {
            let mut lox = LoxClient::new(Config::load()?);
            let structure = lox.get_structure()?.clone();
            let mut rooms: HashMap<String, String> = HashMap::new();
            if let Some(map) = structure.get("rooms").and_then(|r| r.as_object()) {
                for (uuid, room) in map {
                    if let Some(name) = room.get("name").and_then(|n| n.as_str()) {
                        rooms.insert(uuid.clone(), name.to_string());
                    }
                }
            }
            let mut stats_controls = Vec::new();
            if let Some(ctrl_map) = structure.get("controls").and_then(|c| c.as_object()) {
                for (uuid, ctrl) in ctrl_map {
                    if let Some(stat) = ctrl.get("statistic") {
                        if !stat.is_null() {
                            let name = ctrl.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                            let typ = ctrl.get("type").and_then(|t| t.as_str()).unwrap_or("?");
                            let room_uuid = ctrl.get("room").and_then(|r| r.as_str()).unwrap_or("");
                            let room = rooms.get(room_uuid).cloned();
                            stats_controls.push((
                                name.to_string(),
                                uuid.clone(),
                                typ.to_string(),
                                room,
                            ));
                        }
                    }
                }
            }
            stats_controls.sort_by(|a, b| a.0.cmp(&b.0));
            if json {
                let arr: Vec<_> = stats_controls
                    .iter()
                    .map(|(n, u, t, r)| {
                        serde_json::json!({"name": n, "uuid": u, "type": t, "room": r})
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else {
                if !no_header {
                    println!("{:<40} {:<22} {:<24} UUID", "NAME", "TYPE", "ROOM");
                    println!("{}", "─".repeat(120));
                }
                for (name, uuid, typ, room) in &stats_controls {
                    println!(
                        "{:<40} {:<22} {:<24} {}",
                        name,
                        typ,
                        room.as_deref().unwrap_or("─"),
                        uuid
                    );
                }
                println!("\n{} controls with statistics", stats_controls.len());
            }
        }

        Cmd::History {
            name_or_uuid,
            month,
            day,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;

            // Determine output column names from statistics config
            let ctrl_json = lox.get_control_json(&ctrl.uuid).ok();
            let output_names: Vec<String> = ctrl_json
                .as_ref()
                .and_then(|cj| cj.get("statistic"))
                .and_then(|s| s.get("outputs"))
                .and_then(|o| o.as_object())
                .map(|outputs| {
                    let mut entries: Vec<_> = outputs.iter().collect();
                    entries.sort_by_key(|(k, _)| k.as_str().to_string());
                    entries
                        .iter()
                        .map(|(_, v)| {
                            v.get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("value")
                                .to_string()
                        })
                        .collect()
                })
                .unwrap_or_else(|| vec!["value".to_string()]);
            let num_outputs = output_names.len();

            // Build the URL path
            let period = if let Some(d) = &day {
                d.replace('-', "")
            } else if let Some(m) = &month {
                m.replace('-', "")
            } else {
                // Default to current month
                chrono::Local::now().format("%Y%m").to_string()
            };
            // Stats files may be named {uuid}.{period} or {uuid}_{N}.{period}
            // for controls with multiple outputs.  Try bare name first, then
            // fall back to listing the /stats directory for suffixed files.
            let bare = format!("/dev/fsget//stats/{}.{}", ctrl.uuid, period);
            let data = match lox.get_bytes(&bare) {
                Ok(d) => d,
                Err(_) => {
                    let listing = lox.get_text("/dev/fslist//stats")?;
                    let prefix = ctrl.uuid.as_str();
                    let suffix = format!(".{}", period);
                    let mut files: Vec<&str> = listing
                        .lines()
                        .filter_map(|line| line.rsplit_once(' ').map(|(_, name)| name))
                        .filter(|name| name.starts_with(prefix) && name.ends_with(suffix.as_str()))
                        .collect();
                    files.sort();
                    if files.is_empty() {
                        anyhow::bail!(
                            "No statistics files found for {} in period {}",
                            ctrl.uuid,
                            period
                        );
                    }
                    // Use the first matching file (primary output)
                    let path = format!("/dev/fsget//stats/{}", files[0]);
                    lox.get_bytes(&path)?
                }
            };

            if data.is_empty() {
                println!("No statistics data available for this period.");
            } else {
                // Parse binary statistics format:
                // Header: u32 valueCount | u32 controlType | u32 nameLength
                // Then: nameLength bytes of name string
                // Entries: 4 bytes UUID prefix + u32 timestamp + N × f64 values
                let entry_size = 4 + 4 + num_outputs * 8; // UUID(4) + ts(4) + values
                let lox_epoch = chrono::NaiveDate::from_ymd_opt(2009, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_local_timezone(chrono::Local)
                    .unwrap();

                if csv {
                    print!("timestamp");
                    for name in &output_names {
                        print!(",{}", name);
                    }
                    println!();
                } else if !json {
                    print!("{:<20}", "TIMESTAMP");
                    for name in &output_names {
                        print!(" {:>15}", name);
                    }
                    println!();
                    println!("{}", "─".repeat(20 + num_outputs * 16));
                }

                let mut json_arr = Vec::new();
                let mut cursor = Cursor::new(data.as_slice());

                // Skip binary statistics header if present:
                // u32 valueCount | u32 controlType | u32 nameLength | name bytes
                // After the header, entries are aligned to entry_size boundaries.
                if data.len() > 12 {
                    let value_count = cursor.read_u32::<LittleEndian>().unwrap_or(0);
                    let _control_type = cursor.read_u32::<LittleEndian>().unwrap_or(0);
                    let name_length = cursor.read_u32::<LittleEndian>().unwrap_or(0);
                    // Mask off version flag bit from valueCount
                    let _vc = value_count & 0x7FFF_FFFF;
                    // Skip name string
                    let header_end = 12 + name_length as usize;
                    // Align to next entry_size boundary
                    let aligned = header_end.div_ceil(entry_size) * entry_size;
                    cursor.set_position(aligned as u64);
                }

                while cursor.position() as usize + entry_size <= data.len() {
                    // Each entry: 4 bytes UUID prefix + u32 timestamp + N × f64
                    let _uuid_part = cursor.read_u32::<LittleEndian>().unwrap_or(0);
                    let ts = cursor.read_u32::<LittleEndian>().unwrap_or(0);
                    let dt = lox_epoch + chrono::Duration::seconds(ts as i64);
                    let mut values = Vec::new();
                    for _ in 0..num_outputs {
                        values.push(cursor.read_f64::<LittleEndian>().unwrap_or(0.0));
                    }
                    let dt_str = dt.format("%Y-%m-%d %H:%M:%S").to_string();
                    if csv {
                        print!("{}", dt_str);
                        for v in &values {
                            print!(",{:.4}", v);
                        }
                        println!();
                    } else if json {
                        let mut entry = serde_json::json!({"timestamp": dt_str});
                        for (i, name) in output_names.iter().enumerate() {
                            entry[name] = serde_json::json!(values[i]);
                        }
                        json_arr.push(entry);
                    } else {
                        print!("{:<20}", dt_str);
                        for v in &values {
                            print!(" {:>15.4}", v);
                        }
                        println!();
                    }
                }
                if json {
                    println!("{}", serde_json::to_string_pretty(&json_arr)?);
                }
            }
        }

        Cmd::Update { action } => match action {
            UpdateCmd::Check => {
                let lox = LoxClient::new(Config::load()?);
                let version = lox.get_text("/dev/cfg/version")?;
                let ver = xml_attr(&version, "value").unwrap_or("?");
                println!("Current firmware: {}", ver);
                let cfg = Config::load()?;
                if !cfg.serial.is_empty() {
                    let url = format!(
                        "https://update.loxone.com/updatecheck.xml?serial={}&version={}",
                        cfg.serial, ver
                    );
                    match reqwest::blocking::get(&url) {
                        Ok(resp) => {
                            let body = resp.text().unwrap_or_default();
                            // Try multiple XML attributes for the version
                            let new_ver = xml_attr(&body, "Firmware")
                                .or_else(|| xml_attr(&body, "version"))
                                .or_else(|| {
                                    // Only use "Version" if it looks like a version string (contains dots)
                                    xml_attr(&body, "Version").filter(|v| v.contains('.'))
                                });
                            let update_available = xml_attr(&body, "Version")
                                .or_else(|| xml_attr(&body, "update"))
                                .map(|v| v != "0" && v != ver)
                                .unwrap_or(false);
                            let is_update_available =
                                update_available || new_ver.map(|v| v != ver).unwrap_or(false);
                            if json {
                                println!(
                                    "{}",
                                    serde_json::json!({
                                        "current": ver,
                                        "available": new_ver.unwrap_or(if is_update_available { "yes" } else { ver }),
                                        "update_available": is_update_available,
                                    })
                                );
                            } else if let Some(nv) = new_ver {
                                if nv != ver {
                                    println!("Update available: {}", nv);
                                } else {
                                    println!("✓ Firmware is up to date");
                                }
                            } else if is_update_available {
                                println!("Update available (check Loxone Config for details)");
                            } else {
                                println!("✓ Firmware is up to date");
                            }
                        }
                        Err(e) => {
                            eprintln!("Could not check for updates: {}", e);
                            println!("Current firmware: {}", ver);
                        }
                    }
                } else {
                    println!(
                        "Set serial number for update checks: lox config set --serial <serial>"
                    );
                }
            }
            UpdateCmd::Install { yes } => {
                if !yes {
                    bail!("Firmware update requires --yes flag. This will reboot your Miniserver!");
                }
                let lox = LoxClient::new(Config::load()?);
                let resp = lox.get_text("/jdev/sys/updatetolatestrelease")?;
                let val = xml_attr(&resp, "value").unwrap_or("?");
                println!("Update triggered: {}", val);
                println!("The Miniserver will reboot during the update process.");
            }
        },

        Cmd::Music { action } => {
            let cfg = Config::load()?;
            let music_base = format!(
                "{}:7091",
                cfg.host
                    .trim_end_matches('/')
                    .replace("https://", "http://")
            );
            let client = Client::builder()
                .user_agent(client::USER_AGENT)
                .danger_accept_invalid_certs(true)
                .timeout(Duration::from_secs(10))
                .build()?;
            let (zone, cmd_path) = match action {
                MusicCmd::Play { zone } => (zone, "play".to_string()),
                MusicCmd::Pause { zone } => (zone, "pause".to_string()),
                MusicCmd::Stop { zone } => (zone, "stop".to_string()),
                MusicCmd::Volume { level, zone } => {
                    if level > 100 {
                        bail!("Volume must be 0-100");
                    }
                    (zone, format!("volume/{}", level))
                }
                MusicCmd::Next { zone } => (zone, "queueplus".to_string()),
                MusicCmd::Prev { zone } => (zone, "queueminus".to_string()),
                MusicCmd::Mute { zone } => (zone, "mute".to_string()),
                MusicCmd::Unmute { zone } => (zone, "unmute".to_string()),
            };
            let url = format!("{}/zone/{}/{}", music_base, zone, cmd_path);
            match client.get(&url).send() {
                Ok(resp) => {
                    let body = resp.text().unwrap_or_default();
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({"zone": zone, "command": cmd_path, "response": body})
                        );
                    } else {
                        println!("✓ Zone {} → {}", zone, cmd_path);
                    }
                }
                Err(e) => bail!("Music server error: {}", e),
            }
        }

        Cmd::Reboot { yes } => {
            if !yes {
                bail!("Reboot requires --yes flag. This will restart your Miniserver!");
            }
            let lox = LoxClient::new(Config::load()?);
            let resp = lox.get_text("/jdev/sys/reboot")?;
            let val = xml_attr(&resp, "value").unwrap_or("ok");
            println!("Reboot initiated: {}", val);
        }

        Cmd::Files { action } => {
            let lox = LoxClient::new(Config::load()?);
            // Loxone fsget/fslist expect absolute paths — the double slash
            // in e.g. /dev/fsget//temp/file.img is intentional (API prefix +
            // filesystem root).
            let abs = |p: &str| {
                if p.starts_with('/') {
                    p.to_string()
                } else {
                    format!("/{p}")
                }
            };
            match action {
                FilesCmd::Ls { path } => {
                    let listing = lox.get_text(&format!("/dev/fslist/{}", abs(&path)))?;
                    println!("{}", listing);
                }
                FilesCmd::Get { path, save_as } => {
                    let data = lox.get_bytes(&format!("/dev/fsget/{}", abs(&path)))?;
                    let out_path = save_as.unwrap_or_else(|| {
                        path.rsplit('/').next().unwrap_or("download").to_string()
                    });
                    fs::write(&out_path, &data)?;
                    println!("✓ Downloaded {} bytes → {}", data.len(), out_path);
                }
            }
        }

        Cmd::Extensions => {
            let mut lox = LoxClient::new(Config::load()?);
            let structure = lox.get_structure()?.clone();

            // Collect extension devices from the structure file
            let mut ext_list: Vec<serde_json::Value> = Vec::new();

            // Check for dedicated "extensions" key in structure
            if let Some(exts) = structure.get("extensions").and_then(|e| e.as_object()) {
                for (uuid, ext) in exts {
                    let name = ext.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                    let typ = ext.get("type").and_then(|t| t.as_str()).unwrap_or("?");
                    let serial = ext.get("serial").and_then(|s| s.as_str()).unwrap_or("");
                    let online = ext.get("online").and_then(|o| o.as_bool());
                    ext_list.push(serde_json::json!({
                        "uuid": uuid, "name": name, "type": typ,
                        "serial": serial, "online": online,
                    }));
                }
            }

            // Also check controls with extension-like types
            let all_controls = lox.list_controls(None, None)?;
            let ext_controls: Vec<_> = all_controls
                .iter()
                .filter(|c| {
                    c.typ.contains("Extension")
                        || c.typ.contains("TreeDevice")
                        || c.typ.contains("AirDevice")
                })
                .collect();

            if json {
                let mut all = ext_list.clone();
                for c in &ext_controls {
                    all.push(serde_json::json!({
                        "name": c.name, "uuid": c.uuid, "type": c.typ,
                        "room": c.room,
                    }));
                }
                println!("{}", serde_json::to_string_pretty(&all)?);
            } else if ext_list.is_empty() && ext_controls.is_empty() {
                println!("No extensions found in structure.");
            } else {
                if !ext_list.is_empty() {
                    println!("{:<40} {:<22} {:<16} UUID", "NAME", "TYPE", "SERIAL");
                    println!("{}", "─".repeat(100));
                    for ext in &ext_list {
                        println!(
                            "{:<40} {:<22} {:<16} {}",
                            ext["name"].as_str().unwrap_or("?"),
                            ext["type"].as_str().unwrap_or("?"),
                            ext["serial"].as_str().unwrap_or(""),
                            ext["uuid"].as_str().unwrap_or("?"),
                        );
                    }
                    println!("\n{} extensions", ext_list.len());
                }
                if !ext_controls.is_empty() {
                    if !ext_list.is_empty() {
                        println!();
                    }
                    println!("{:<40} {:<22} UUID", "NAME", "TYPE");
                    println!("{}", "─".repeat(100));
                    for c in &ext_controls {
                        println!("{:<40} {:<22} {}", c.name, c.typ, c.uuid);
                    }
                }
            }
        }

        Cmd::Time => {
            let lox = LoxClient::new(Config::load()?);
            // /jdev/sys/date and /jdev/sys/time require admin rights.
            // /data/status is accessible to all users and contains the Miniserver
            // timestamp in the "Modified" XML attribute (e.g. "2026-03-08 23:32:30").
            let status_xml = lox.get_text("/data/status")?;
            let datetime_val = xml_attr(&status_xml, "Modified").unwrap_or("?");
            let (date_val, time_val) = if datetime_val.contains(' ') {
                let mut parts = datetime_val.splitn(2, ' ');
                (parts.next().unwrap_or("?"), parts.next().unwrap_or("?"))
            } else {
                (datetime_val, "?")
            };
            if json {
                println!(
                    "{}",
                    serde_json::json!({"date": date_val, "time": time_val, "datetime": datetime_val})
                );
            } else {
                println!("Miniserver date: {}", date_val);
                println!("Miniserver time: {}", time_val);
            }
        }

        Cmd::Watch {
            name_or_uuid,
            interval,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let ctrl = lox.find_control(&name_or_uuid)?;
            println!(
                "Watching '{}' every {}s  (Ctrl+C to stop)",
                ctrl.name, interval
            );
            let mut last = String::new();
            loop {
                if let Ok(xml) = lox.get_all(&ctrl.uuid) {
                    let val = xml_attr(&xml, "value").unwrap_or("?").to_string();
                    if val != last {
                        println!("[{}]  {} = {}", now_hms(), ctrl.name, val);
                        last = val;
                    }
                }
                thread::sleep(Duration::from_secs(interval));
            }
        }

        Cmd::Stream {
            r#type,
            room,
            control,
            initial,
        } => {
            let cfg = Config::load()?;
            let mut lox = LoxClient::new(cfg.clone());
            let structure = lox.get_structure()?.clone();
            let state_map = stream::build_state_uuid_map(&structure);
            if !quiet {
                eprintln!("Streaming state changes (Ctrl+C to stop)...");
            }
            if csv && !no_header {
                println!("timestamp,control,state,value,room,type,uuid");
            }

            let type_filter = r#type;
            let room_filter = room;
            let control_filter = control;
            let mut initial_done = false;

            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(stream::stream_events(&cfg, |events| {
                for event in &events {
                    match event {
                        stream::StateEvent::ValueState { uuid, value } => {
                            if !initial && !initial_done {
                                continue;
                            }
                            if let Some(info) = state_map.get(uuid) {
                                if !matches_filters(
                                    info,
                                    type_filter.as_deref(),
                                    room_filter.as_deref(),
                                    control_filter.as_deref(),
                                ) {
                                    continue;
                                }
                                print_stream_event(
                                    json,
                                    csv,
                                    &info.control_name,
                                    &info.state_name,
                                    &format!("{}", value),
                                    info.room.as_deref(),
                                    &info.control_type,
                                    uuid,
                                );
                            }
                        }
                        stream::StateEvent::TextState { uuid, text, .. } => {
                            if !initial && !initial_done {
                                continue;
                            }
                            if let Some(info) = state_map.get(uuid) {
                                if !matches_filters(
                                    info,
                                    type_filter.as_deref(),
                                    room_filter.as_deref(),
                                    control_filter.as_deref(),
                                ) {
                                    continue;
                                }
                                print_stream_event(
                                    json,
                                    csv,
                                    &info.control_name,
                                    &info.state_name,
                                    text,
                                    info.room.as_deref(),
                                    &info.control_type,
                                    uuid,
                                );
                            }
                        }
                        stream::StateEvent::OutOfService => {
                            if !quiet {
                                eprintln!("Miniserver going offline (firmware update?). Exiting.");
                            }
                            std::process::exit(0);
                        }
                        _ => {} // Keepalive, Daytimer, Weather — skip for now
                    }
                }
                if !initial_done {
                    initial_done = true;
                }
                Ok(())
            }))?;
        }

        Cmd::If {
            name_or_uuid,
            op,
            value,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid_resolved = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid_resolved)?;
            let xml = lox.get_all(&ctrl.uuid)?;
            let current = xml_attr(&xml, "value").unwrap_or("").to_string();
            let matches = eval_op(&current, &op, &value)?;
            if !json {
                println!(
                    "{} = {}  →  {} {} {}  →  {}",
                    ctrl.name,
                    current,
                    current,
                    op,
                    value,
                    if matches { "✓ true" } else { "✗ false" }
                );
            } else {
                println!(
                    "{}",
                    serde_json::json!({
                        "control": ctrl.name, "current": current,
                        "op": op, "target": value, "result": matches
                    })
                );
            }
            std::process::exit(if matches { 0 } else { 1 });
        }

        Cmd::Run {
            scene,
            dry_run: scene_dry_run,
        } => {
            let s = Scene::load(&scene)?;
            let mut lox = LoxClient::new(Config::load()?);
            if dry_run || scene_dry_run {
                println!("▶  {} (dry run)", s.name.as_deref().unwrap_or(&scene));
                if let Some(d) = &s.description {
                    println!("   {}", d);
                }
                println!();
                for (i, step) in s.steps.iter().enumerate() {
                    let resolved = match lox.resolve(&step.control) {
                        Ok(u) => u,
                        Err(e) => {
                            eprintln!("Step {}: {} (resolve failed: {})", i + 1, step.control, e);
                            continue;
                        }
                    };
                    println!(
                        "  {}. {} → {}{}",
                        i + 1,
                        step.control,
                        step.cmd,
                        if step.delay_ms > 0 {
                            format!(" (delay {}ms)", step.delay_ms)
                        } else {
                            String::new()
                        }
                    );
                    let _ = resolved; // used for validation
                }
            } else {
                if !quiet {
                    println!("▶  {}", s.name.as_deref().unwrap_or(&scene));
                    if let Some(d) = &s.description {
                        println!("   {}", d);
                    }
                    println!();
                }
                for (i, step) in s.steps.iter().enumerate() {
                    let uuid = match lox.resolve(&step.control) {
                        Ok(u) => u,
                        Err(e) => {
                            eprintln!("Step {}: {}", i + 1, e);
                            continue;
                        }
                    };
                    if let Some(resp) = send_or_dry_run(
                        &lox,
                        &uuid,
                        &step.cmd,
                        &step.control,
                        None,
                        dry_run,
                        json,
                        quiet,
                    )? {
                        print_resp(&resp, json, quiet, &step.control, &step.cmd);
                    }
                    if step.delay_ms > 0 && !dry_run {
                        thread::sleep(Duration::from_millis(step.delay_ms));
                    }
                }
            }
        }

        Cmd::Scene { action } => match action {
            SceneCmd::Ls => {
                let names = Scene::list()?;
                if names.is_empty() {
                    println!("No scenes. Create: lox scene new <name>");
                } else {
                    for name in &names {
                        let desc = Scene::load(name)
                            .ok()
                            .and_then(|s| s.description)
                            .unwrap_or_default();
                        println!("  {:<20}  {}", name, desc);
                    }
                }
            }
            SceneCmd::Show { name } => {
                println!(
                    "{}",
                    fs::read_to_string(Scene::scenes_dir().join(format!("{}.yaml", name)))
                        .with_context(|| format!("Scene '{}' not found", name))?
                );
            }
            SceneCmd::New { name } => {
                let dir = Scene::scenes_dir();
                fs::create_dir_all(&dir)?;
                let path = dir.join(format!("{}.yaml", name));
                if path.exists() {
                    bail!("Already exists");
                }
                fs::write(&path, format!(
                    "name: \"{name}\"\ndescription: \"\"\nsteps:\n  - control: \"\"\n    cmd: \"on\"\n"))?;
                println!("✓  {:?}", path);
            }
        },

        Cmd::Set {
            name_or_uuid,
            value,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let encoded = encode_path_value(&value);
            if dry_run {
                print_dry_run(json, quiet, &uuid, &encoded, &name_or_uuid, None);
            } else {
                let resp = lox.send_cmd(&uuid, &encoded)?;
                let code = resp
                    .pointer("/LL/Code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let val = resp
                    .pointer("/LL/value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                if code == "200" {
                    if !quiet {
                        println!("✓  {} = {}", name_or_uuid, val);
                    }
                } else {
                    bail!("Error {}: {}", code, val);
                }
            }
        }
        Cmd::Token { action } => match action {
            TokenCmd::Fetch => {
                let cfg = Config::load()?;
                println!("Fetching token from Miniserver...");
                let rt = tokio::runtime::Runtime::new()?;
                match rt.block_on(token::acquire_token(&cfg)) {
                    Ok(ts) => {
                        let _exp =
                            std::time::UNIX_EPOCH + std::time::Duration::from_secs(ts.valid_until);
                        let days = ts.valid_until.saturating_sub(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                        ) / 86400;
                        println!("✓ Token saved to {:?}", token::TokenStore::path());
                        println!("  Valid for: {} days", days);
                    }
                    Err(e) => bail!("Token fetch failed: {}", e),
                }
            }
            TokenCmd::Info => match token::TokenStore::load() {
                Some(ts) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    let days_left = ts.valid_until.saturating_sub(now) / 86400;
                    println!(
                        "Token: {}...{}",
                        &ts.token[..8],
                        &ts.token[ts.token.len() - 4..]
                    );
                    if ts.is_valid() {
                        println!("Status: ✓ valid ({} days remaining)", days_left);
                    } else {
                        println!("Status: ⚠ expired — run: lox token fetch");
                    }
                }
                None => println!("No token saved. Using Basic Auth. Run: lox token fetch"),
            },
            TokenCmd::Clear => {
                let path = token::TokenStore::path();
                if path.exists() {
                    fs::remove_file(&path)?;
                    println!("✓ Token cleared (reverting to Basic Auth)");
                } else {
                    println!("No token to clear");
                }
            }
            TokenCmd::Check => {
                let ts = token::TokenStore::load()
                    .ok_or_else(|| anyhow::anyhow!("No token saved. Run: lox token fetch"))?;
                let cfg = Config::load()?;
                let lox = LoxClient::new(cfg.clone());
                // Hash the token with the key for the check endpoint
                let hash = token::hash_token(&ts.token, &ts.key);
                let resp = lox.get_json(&format!("/jdev/sys/checktoken/{}/{}", hash, cfg.user))?;
                let code = resp
                    .pointer("/LL/Code")
                    .and_then(|c| c.as_str())
                    .unwrap_or("?");
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "valid": code == "200",
                            "code": code,
                        })
                    );
                } else if code == "200" {
                    println!("✓ Token is valid on the Miniserver");
                } else {
                    println!("✗ Token is not valid (code {})", code);
                }
            }
            TokenCmd::Refresh => {
                let ts = token::TokenStore::load()
                    .ok_or_else(|| anyhow::anyhow!("No token saved. Run: lox token fetch"))?;
                let cfg = Config::load()?;
                let lox = LoxClient::new(cfg.clone());
                let hash = token::hash_token(&ts.token, &ts.key);
                let resp =
                    lox.get_json(&format!("/jdev/sys/refreshtoken/{}/{}", hash, cfg.user))?;
                let code = resp
                    .pointer("/LL/Code")
                    .and_then(|c| c.as_str())
                    .unwrap_or("?");
                if code == "200" {
                    // Update the valid_until in our local store
                    let valid_until = resp
                        .pointer("/LL/value")
                        .and_then(|v| v.get("validUntil"))
                        .and_then(|v| v.as_u64());
                    if let Some(vu) = valid_until {
                        let unix_until = if vu > 1_500_000_000 {
                            vu
                        } else {
                            1_230_768_000u64.saturating_add(vu)
                        };
                        let new_ts = token::TokenStore {
                            token: ts.token,
                            key: ts.key,
                            valid_until: unix_until,
                        };
                        new_ts.save()?;
                        let days = unix_until.saturating_sub(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                        ) / 86400;
                        println!("✓ Token refreshed ({} days remaining)", days);
                    } else {
                        println!("✓ Token refreshed");
                    }
                } else {
                    bail!("Token refresh failed (code {})", code);
                }
            }
            TokenCmd::Revoke => {
                let ts = token::TokenStore::load()
                    .ok_or_else(|| anyhow::anyhow!("No token saved. Run: lox token fetch"))?;
                let cfg = Config::load()?;
                let lox = LoxClient::new(cfg.clone());
                let hash = token::hash_token(&ts.token, &ts.key);
                let resp = lox.get_json(&format!("/jdev/sys/killtoken/{}/{}", hash, cfg.user))?;
                let code = resp
                    .pointer("/LL/Code")
                    .and_then(|c| c.as_str())
                    .unwrap_or("?");
                if code == "200" {
                    // Remove local token
                    let path = token::TokenStore::path();
                    if path.exists() {
                        fs::remove_file(&path)?;
                    }
                    println!("✓ Token revoked and cleared");
                } else {
                    bail!("Token revoke failed (code {})", code);
                }
            }
        },
        Cmd::Autopilot { action } => {
            let mut lox = LoxClient::new(Config::load()?);
            match action {
                AutopilotCmd::Ls => {
                    let rules = lox.list_autopilot_rules()?;
                    if rules.is_empty() {
                        println!("No autopilot rules found.");
                    } else if json {
                        let arr: Vec<serde_json::Value> = rules
                            .iter()
                            .map(|r| {
                                serde_json::json!({
                                    "name": r.name,
                                    "uuid": r.uuid,
                                    "state_changed": r.state_changed,
                                    "state_history": r.state_history,
                                })
                            })
                            .collect();
                        println!("{}", serde_json::to_string_pretty(&arr)?);
                    } else {
                        let name_w = rules.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
                        println!("{:<width$}  UUID", "NAME", width = name_w);
                        for r in &rules {
                            println!("{:<width$}  {}", r.name, r.uuid, width = name_w);
                        }
                    }
                }
                AutopilotCmd::State { name_or_uuid } => {
                    let rule = lox.find_autopilot_rule(&name_or_uuid)?;
                    let resp = lox.get_json(&format!("/jdev/sps/io/{}/state", rule.state_changed));
                    if json {
                        match resp {
                            Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
                            Err(e) => bail!("{}", e),
                        }
                    } else {
                        let last_run = match resp {
                            Ok(v) => {
                                let raw = v
                                    .get("LL")
                                    .and_then(|ll| ll.get("value"))
                                    .and_then(|val| val.as_str())
                                    .unwrap_or("");
                                if raw.is_empty() || raw == "0" {
                                    "never".to_string()
                                } else if let Ok(secs) = raw.parse::<i64>() {
                                    let lox_epoch = chrono::NaiveDate::from_ymd_opt(2009, 1, 1)
                                        .unwrap()
                                        .and_hms_opt(0, 0, 0)
                                        .unwrap()
                                        .and_local_timezone(chrono::Local)
                                        .unwrap();
                                    let dt = lox_epoch + chrono::Duration::seconds(secs);
                                    dt.format("%Y-%m-%d %H:%M:%S").to_string()
                                } else {
                                    raw.to_string()
                                }
                            }
                            Err(_) => "unavailable".to_string(),
                        };
                        println!("Rule:     {}", rule.name);
                        println!("Last run: {}", last_run);
                    }
                }
            }
        }
        Cmd::Cache { action } => {
            let cfg = Config::load()?;
            let cache = LoxClient::cache_path(&cfg);
            match action {
                CacheCmd::Info => {
                    if cache.exists() {
                        let meta = cache.metadata()?;
                        let age = std::time::SystemTime::now()
                            .duration_since(meta.modified()?)
                            .unwrap_or_default();
                        let size = meta.len();
                        println!("Cache: {:?}", cache);
                        println!("Size:  {:.1} KB", size as f64 / 1024.0);
                        println!("Age:   {}m {}s", age.as_secs() / 60, age.as_secs() % 60);
                        if age.as_secs() < 86400 {
                            println!("Status: ✓ valid ({} until refresh)", {
                                let remaining = 86400u64.saturating_sub(age.as_secs());
                                format!("{}h {}m", remaining / 3600, (remaining % 3600) / 60)
                            });
                        } else {
                            println!("Status: ⚠ stale (will refresh on next command)");
                        }
                    } else {
                        println!("No cache. Will be created on first command.");
                    }
                }
                CacheCmd::Clear => {
                    if cache.exists() {
                        fs::remove_file(&cache)?;
                        println!("✓ Cache cleared");
                    } else {
                        println!("No cache to clear");
                    }
                }
                CacheCmd::Check => {
                    let lox = LoxClient::new(cfg);
                    let resp = lox.get_json("/jdev/sps/LoxAPPversion3")?;
                    let remote_ver = resp
                        .pointer("/LL/value")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "remote_version": remote_ver,
                                "cache_exists": cache.exists(),
                            })
                        );
                    } else {
                        println!("Remote structure version: {}", remote_ver);
                        if cache.exists() {
                            println!("Cache: exists at {:?}", cache);
                        } else {
                            println!("Cache: not present");
                        }
                    }
                }
                CacheCmd::Refresh => {
                    let client = Client::builder()
                        .user_agent(client::USER_AGENT)
                        .danger_accept_invalid_certs(true)
                        .redirect(LoxClient::same_origin_redirect_policy(&cfg.host))
                        .timeout(Duration::from_secs(15))
                        .build()?;
                    if cache.exists() {
                        let _ = fs::remove_file(&cache);
                    }
                    LoxClient::load_or_fetch_structure(&cfg, &client)?;
                    println!("✓ Structure cache refreshed ({:?})", cache);
                }
            }
        }
        Cmd::Config { action } => match action {
            ConfigCmd::Ls => {
                let cfg = Config::load()?;
                let backups = ftp::list_backups(&cfg)?;
                if backups.is_empty() {
                    println!("No configs found on the Miniserver.");
                } else if json {
                    let arr: Vec<serde_json::Value> = backups
                        .iter()
                        .map(|b| {
                            serde_json::json!({
                                "filename": b.filename,
                                "version": b.version,
                                "date": b.formatted_date(),
                                "size": b.size,
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&arr)?);
                } else {
                    println!("  {:<4} {:<8} {:<22} Size", "#", "Version", "Date");
                    for (i, b) in backups.iter().enumerate() {
                        println!(
                            "  {:<4} {:<8} {:<22} {} KB{}",
                            i + 1,
                            b.version,
                            b.formatted_date(),
                            b.size / 1024,
                            if i == 0 { "  (latest)" } else { "" }
                        );
                    }
                }
            }
            ConfigCmd::Download { save_as, extract } => {
                let cfg = Config::load()?;
                let backups = ftp::list_backups(&cfg)?;
                if backups.is_empty() {
                    bail!("No configs found on the Miniserver.");
                }
                let newest = &backups[0];
                eprintln!(
                    "Downloading {} ({} KB)...",
                    newest.filename,
                    newest.size / 1024
                );
                let data = ftp::download_backup(&cfg, &newest.filename)?;
                let out_path = save_as.unwrap_or_else(|| newest.filename.clone());
                fs::write(&out_path, &data)?;
                println!("Saved to {}", out_path);

                if extract {
                    eprintln!("Extracting sps0.LoxCC...");
                    let xml = loxcc::extract_and_decompress(&data)?;
                    let xml_path = out_path
                        .strip_suffix(".zip")
                        .unwrap_or(&out_path)
                        .to_string()
                        + ".Loxone";
                    fs::write(&xml_path, &xml)?;
                    println!(
                        "Decompressed {} KB → {} KB → {}",
                        data.len() / 1024,
                        xml.len() / 1024,
                        xml_path
                    );
                }
            }
            ConfigCmd::Extract { file, save_as } => {
                let zip_data = fs::read(&file).with_context(|| format!("Cannot read {}", file))?;
                eprintln!("Extracting sps0.LoxCC...");
                let xml = loxcc::extract_and_decompress(&zip_data)?;
                let xml_path = save_as.unwrap_or_else(|| {
                    file.strip_suffix(".zip").unwrap_or(&file).to_string() + ".Loxone"
                });
                fs::write(&xml_path, &xml)?;
                println!(
                    "Decompressed {} KB → {} KB → {}",
                    zip_data.len() / 1024,
                    xml.len() / 1024,
                    xml_path
                );
            }
            ConfigCmd::Upload { file, force } => {
                let cfg = Config::load()?;
                if !force {
                    eprintln!(
                        "⚠  WARNING: Uploading a config will replace the current Miniserver\n\
                             \x20  programming. A bad configuration can require physical SD card\n\
                             \x20  access to recover.\n\
                             \n\
                             \x20  Config file: {}\n\
                             \n\
                             \x20  Use --force to proceed.",
                        file
                    );
                    std::process::exit(1);
                }
                let data = fs::read(&file).with_context(|| format!("Cannot read {}", file))?;
                let filename = std::path::Path::new(&file)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&file);
                eprintln!("Uploading {} ({} KB)...", filename, data.len() / 1024);
                ftp::upload_backup(&cfg, filename, &data)?;
                println!("Upload complete.");
                println!("Reboot the Miniserver to apply: lox reboot");
            }
            ConfigCmd::Users { file } => {
                if file.ends_with(".zip") {
                    bail!(
                        "Expected a .Loxone XML file. Run `lox config extract {}` first.",
                        file
                    );
                }
                let xml = fs::read(&file).with_context(|| format!("Cannot read {}", file))?;
                let users = loxone_xml::parse_users(&xml)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&users)?);
                } else {
                    let nfc_count = users.iter().filter(|u| u.nfc).count();
                    println!("  {:<26} {:<6} Description", "Name", "NFC");
                    for u in &users {
                        println!(
                            "  {:<26} {:<6} {}",
                            u.name,
                            if u.nfc { "yes" } else { "-" },
                            u.description,
                        );
                    }
                    println!("\n{} users ({} with NFC)", users.len(), nfc_count);
                }
            }
            ConfigCmd::Devices { file } => {
                if file.ends_with(".zip") {
                    bail!(
                        "Expected a .Loxone XML file. Run `lox config extract {}` first.",
                        file
                    );
                }
                let xml = fs::read(&file).with_context(|| format!("Cannot read {}", file))?;
                let devices = loxone_xml::parse_devices(&xml)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&devices)?);
                } else {
                    let tree: Vec<_> = devices
                        .iter()
                        .filter(|d| d.bus == loxone_xml::DeviceBus::Tree)
                        .collect();
                    let air: Vec<_> = devices
                        .iter()
                        .filter(|d| d.bus == loxone_xml::DeviceBus::Air)
                        .collect();
                    let net: Vec<_> = devices
                        .iter()
                        .filter(|d| d.bus == loxone_xml::DeviceBus::Network)
                        .collect();

                    if !tree.is_empty() {
                        println!("  Tree devices ({})", tree.len());
                        println!("  {:<30} {:<12} Type", "Name", "Serial");
                        for d in &tree {
                            println!(
                                "  {:<30} {:<12} {}",
                                d.name,
                                d.serial.as_deref().unwrap_or("-"),
                                d.type_label
                            );
                        }
                    }
                    if !air.is_empty() {
                        if !tree.is_empty() {
                            println!();
                        }
                        println!("  LoxAIR devices ({})", air.len());
                        println!("  {:<30} Type", "Name");
                        for d in &air {
                            println!("  {:<30} {}", d.name, d.type_label);
                        }
                    }
                    if !net.is_empty() {
                        if !tree.is_empty() || !air.is_empty() {
                            println!();
                        }
                        println!("  Network devices ({})", net.len());
                        println!("  {:<30} {:<18} MAC", "Name", "Address");
                        for d in &net {
                            println!(
                                "  {:<30} {:<18} {}",
                                d.name,
                                d.address.as_deref().unwrap_or("-"),
                                d.mac.as_deref().unwrap_or("-")
                            );
                        }
                    }
                    println!("\n{} devices total", devices.len());
                }
            }
            ConfigCmd::Diff { file1, file2 } => {
                let xml1 = load_config_xml(&file1)?;
                let xml2 = load_config_xml(&file2)?;
                let s1 = loxone_xml::parse_config_summary(&xml1)?;
                let s2 = loxone_xml::parse_config_summary(&xml2)?;
                let diff = loxone_xml::diff_configs(&s1, &s2);

                if json {
                    println!("{}", serde_json::to_string_pretty(&diff)?);
                } else {
                    println!(
                        "Config version: {} → {}",
                        diff.version_old, diff.version_new
                    );
                    println!("Modified: {} → {}", diff.date_old, diff.date_new);

                    if !diff.controls_added.is_empty()
                        || !diff.controls_removed.is_empty()
                        || !diff.controls_changed.is_empty()
                    {
                        println!("\nControls:");
                        for c in &diff.controls_added {
                            println!("  + Added: \"{}\" ({})", c.name, c.control_type);
                        }
                        for c in &diff.controls_changed {
                            println!(
                                "  ~ Changed: \"{}\" — {} \"{}\" → \"{}\"",
                                c.name, c.field, c.old_value, c.new_value
                            );
                        }
                        for c in &diff.controls_removed {
                            println!("  - Removed: \"{}\" ({})", c.name, c.control_type);
                        }
                    }

                    if !diff.rooms_added.is_empty()
                        || !diff.rooms_removed.is_empty()
                        || !diff.rooms_renamed.is_empty()
                    {
                        println!("\nRooms:");
                        for r in &diff.rooms_added {
                            println!("  + Added: \"{}\"", r);
                        }
                        for r in &diff.rooms_renamed {
                            println!("  ~ Renamed: \"{}\" → \"{}\"", r.old, r.new);
                        }
                        for r in &diff.rooms_removed {
                            println!("  - Removed: \"{}\"", r);
                        }
                    }

                    if !diff.categories_added.is_empty()
                        || !diff.categories_removed.is_empty()
                        || !diff.categories_renamed.is_empty()
                    {
                        println!("\nCategories:");
                        for c in &diff.categories_added {
                            println!("  + Added: \"{}\"", c);
                        }
                        for c in &diff.categories_renamed {
                            println!("  ~ Renamed: \"{}\" → \"{}\"", c.old, c.new);
                        }
                        for c in &diff.categories_removed {
                            println!("  - Removed: \"{}\"", c);
                        }
                    }

                    if !diff.users_added.is_empty() || !diff.users_removed.is_empty() {
                        println!("\nUsers:");
                        for u in &diff.users_added {
                            println!("  + Added: \"{}\"", u);
                        }
                        for u in &diff.users_removed {
                            println!("  - Removed: \"{}\"", u);
                        }
                    }

                    let total = diff.controls_added.len()
                        + diff.controls_removed.len()
                        + diff.controls_changed.len()
                        + diff.rooms_added.len()
                        + diff.rooms_removed.len()
                        + diff.rooms_renamed.len()
                        + diff.categories_added.len()
                        + diff.categories_removed.len()
                        + diff.categories_renamed.len()
                        + diff.users_added.len()
                        + diff.users_removed.len();

                    if !diff.has_changes() {
                        println!("\nNo structural changes.");
                    } else {
                        println!("\n{} changes total", total);
                    }
                }
            }
        },
        Cmd::Sensors { r#type, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let type_lower = r#type.to_lowercase();
            let type_filter: Option<&str> = match type_lower.as_str() {
                "temperature" | "temp" => Some("InfoOnlyAnalog"),
                "door-window" | "doorwindow" => Some("InfoOnlyDigital"),
                "motion" => None, // We'll filter manually
                "smoke" => Some("SmokeAlarm"),
                _ => None,
            };
            let controls = lox.list_controls(type_filter, room.as_deref())?;
            let filtered: Vec<_> = controls
                .iter()
                .filter(|c| match type_lower.as_str() {
                    "temperature" | "temp" => c.typ == "InfoOnlyAnalog",
                    "door-window" | "doorwindow" => c.typ == "InfoOnlyDigital",
                    "motion" => {
                        matches!(c.typ.as_str(), "PresenceDetector" | "MotionSensor")
                    }
                    "smoke" => c.typ == "SmokeAlarm",
                    _ => matches!(
                        c.typ.as_str(),
                        "InfoOnlyAnalog"
                            | "InfoOnlyDigital"
                            | "PresenceDetector"
                            | "MotionSensor"
                            | "SmokeAlarm"
                            | "Meter"
                    ),
                })
                .collect();
            if json {
                let mut arr = Vec::new();
                for c in &filtered {
                    let xml = lox.get_all(&c.uuid).unwrap_or_default();
                    let val = xml_attr(&xml, "value").unwrap_or("?").to_string();
                    arr.push(serde_json::json!({
                        "name": c.name, "uuid": c.uuid, "type": c.typ,
                        "room": c.room, "value": val,
                    }));
                }
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else if filtered.is_empty() {
                println!("No sensors found.");
            } else {
                if !no_header {
                    println!("{:<36} {:<20} {:<20} VALUE", "NAME", "TYPE", "ROOM");
                    println!("{}", "─".repeat(96));
                }
                for c in &filtered {
                    let xml = lox.get_all(&c.uuid).unwrap_or_default();
                    let val = xml_attr(&xml, "value").unwrap_or("?");
                    println!(
                        "{:<36} {:<20} {:<20} {}",
                        c.name,
                        c.typ,
                        c.room.as_deref().unwrap_or("─"),
                        val
                    );
                }
                println!("\n{} sensors", filtered.len());
            }
        }

        Cmd::Energy { room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let controls = lox.list_controls(None, room.as_deref())?;
            let energy: Vec<_> = controls
                .iter()
                .filter(|c| {
                    matches!(
                        c.typ.as_str(),
                        "Meter" | "EnergyManager" | "EnergyMonitor" | "EnergyFlowMonitor"
                    ) || c.typ.contains("Energy")
                })
                .collect();
            if json {
                let mut arr = Vec::new();
                for c in &energy {
                    let xml = lox.get_all(&c.uuid).unwrap_or_default();
                    let val = xml_attr(&xml, "value").unwrap_or("?").to_string();
                    arr.push(serde_json::json!({
                        "name": c.name, "uuid": c.uuid, "type": c.typ,
                        "room": c.room, "value": val,
                    }));
                }
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else if energy.is_empty() {
                println!("No energy meters found.");
            } else {
                if !no_header {
                    println!("{:<36} {:<20} {:<20} VALUE", "NAME", "TYPE", "ROOM");
                    println!("{}", "─".repeat(96));
                }
                for c in &energy {
                    let xml = lox.get_all(&c.uuid).unwrap_or_default();
                    let val = xml_attr(&xml, "value").unwrap_or("?");
                    println!(
                        "{:<36} {:<20} {:<20} {}",
                        c.name,
                        c.typ,
                        c.room.as_deref().unwrap_or("─"),
                        val
                    );
                }
                println!("\n{} energy meters", energy.len());
            }
        }

        Cmd::Completions { shell, install } => {
            let detected = shell.or_else(detect_shell);
            let Some(sh) = detected else {
                bail!("Could not detect shell. Specify one: lox completions bash|zsh|fish");
            };
            let mut cmd = Cli::command();
            if install {
                install_completions(sh, &mut cmd)?;
            } else {
                generate(sh, &mut cmd, "lox", &mut std::io::stdout());
            }
        }

        Cmd::Log { lines } => {
            let lox = LoxClient::new(Config::load()?);
            let log = lox.get_text("/dev/fsget/log/def.log")?;
            if log.contains("<errorcode>403</errorcode>")
                || log.contains("<errorcode>401</errorcode>")
            {
                bail!("Access denied. The Miniserver log requires admin privileges.");
            }
            if log.trim_start().starts_with('<') && log.contains("<errorcode>") {
                let code = xml_attr(&log, "errorcode").unwrap_or("?");
                bail!("Miniserver returned error code {}", code);
            }
            let all: Vec<&str> = log.lines().collect();
            for line in &all[all.len().saturating_sub(lines)..] {
                println!("{}", line);
            }
        }

        Cmd::Otel { action } => {
            let cfg = Config::load()?;
            match action {
                OtelCmd::Serve {
                    endpoint,
                    interval,
                    r#type,
                    room,
                    header,
                    delta,
                    no_logs,
                    no_traces,
                } => {
                    let interval = otel::parse_interval(&interval)?;
                    otel::serve(
                        &cfg,
                        &endpoint,
                        interval,
                        r#type.as_deref(),
                        room.as_deref(),
                        &header,
                        quiet,
                        delta,
                        no_logs,
                        no_traces,
                    )?;
                }
                OtelCmd::Push {
                    endpoint,
                    r#type,
                    room,
                    header,
                    delta,
                    no_logs,
                } => {
                    otel::push(
                        &cfg,
                        &endpoint,
                        r#type.as_deref(),
                        room.as_deref(),
                        &header,
                        quiet,
                        delta,
                        no_logs,
                    )?;
                }
            }
        }

        Cmd::Schema { command } => {
            let schema = build_schema(command.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&schema)?);
        }
    }

    Ok(())
}

// ── Schema introspection ─────────────────────────────────────────────────────

fn build_schema(filter: Option<&str>) -> Result<Value> {
    let cmd = Cli::command();

    if let Some(name) = filter {
        let sub = cmd.get_subcommands().find(|s| {
            s.get_name().eq_ignore_ascii_case(name)
                || s.get_all_aliases().any(|a| a.eq_ignore_ascii_case(name))
        });
        let Some(sub) = sub else {
            bail!(
                "Unknown command '{}'. Run `lox schema` for all commands.",
                name
            );
        };
        return Ok(describe_command(sub, true));
    }

    let commands: Vec<Value> = cmd
        .get_subcommands()
        .filter(|s| !s.is_hide_set())
        .map(|s| describe_command(s, false))
        .collect();

    Ok(serde_json::json!({
        "name": "lox",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Loxone Miniserver CLI",
        "global_flags": [
            {"name": "--output", "short": "-o", "type": "enum", "values": ["table", "json", "csv"], "default": "table"},
            {"name": "--quiet", "short": "-q", "type": "bool", "description": "Suppress non-essential output"},
            {"name": "--dry-run", "type": "bool", "description": "Validate and resolve inputs without executing commands"},
            {"name": "--non-interactive", "type": "bool", "description": "Fail instead of prompting for confirmation"},
            {"name": "--trace-id", "type": "string", "description": "Trace ID for correlating agent actions"},
            {"name": "--verbose", "short": "-v", "type": "count", "description": "Verbose output (-v requests, -vv bodies)"},
        ],
        "commands": commands,
    }))
}

fn describe_command(cmd: &clap::Command, detailed: bool) -> Value {
    let mut obj = serde_json::json!({
        "name": cmd.get_name(),
        "description": cmd.get_about().map(|s| s.to_string()).unwrap_or_default(),
    });

    // Collect positional and flag arguments
    let args: Vec<Value> = cmd
        .get_arguments()
        .filter(|a| a.get_id() != "help" && a.get_id() != "version")
        .map(|a| {
            let mut arg = serde_json::json!({
                "name": a.get_id().to_string(),
            });
            if let Some(help) = a.get_help() {
                arg["description"] = Value::String(help.to_string());
            }
            if let Some(short) = a.get_short() {
                arg["short"] = Value::String(format!("-{}", short));
            }
            if a.get_long().is_some() {
                arg["long"] = Value::String(format!("--{}", a.get_id()));
            }
            if a.get_num_args().is_some_and(|r| r.max_values() == 0) {
                arg["type"] = Value::String("bool".to_string());
            }
            if let Some(vals) = a.get_default_values().first() {
                arg["default"] = Value::String(vals.to_string_lossy().to_string());
            }
            arg
        })
        .collect();

    if !args.is_empty() {
        obj["args"] = Value::Array(args);
    }

    // Subcommands
    let subs: Vec<Value> = cmd
        .get_subcommands()
        .filter(|s| !s.is_hide_set())
        .map(|s| describe_command(s, detailed))
        .collect();

    if !subs.is_empty() {
        obj["subcommands"] = Value::Array(subs);
    }

    if detailed {
        // Include control type hints for common commands
        let type_hints = match cmd.get_name() {
            "blind" => Some("Jalousie, CentralJalousie"),
            "gate" => Some("Gate, CentralGate"),
            "alarm" => Some("Alarm"),
            "thermostat" => Some("IRoomControllerV2, IRoomController"),
            "door" => Some("DoorLock"),
            "intercom" => Some("Intercom"),
            "charger" => Some("Charger, EV"),
            "light" => Some("LightControllerV2, LightController, ColorPickerV2"),
            _ => None,
        };
        if let Some(types) = type_hints {
            obj["control_types"] = Value::String(types.to_string());
        }

        // Include valid actions
        let actions = match cmd.get_name() {
            "blind" => Some(vec!["up", "down", "stop", "shade", "pos <0-100>"]),
            "gate" => Some(vec!["open", "close", "stop"]),
            "alarm" => Some(vec!["arm", "arm-home", "disarm", "quit"]),
            "door" => Some(vec!["lock", "unlock", "open"]),
            "intercom" => Some(vec!["answer", "hangup", "open"]),
            "charger" => Some(vec!["start", "stop", "pause"]),
            _ => None,
        };
        if let Some(acts) = actions {
            obj["valid_actions"] = Value::Array(
                acts.into_iter()
                    .map(|a| Value::String(a.to_string()))
                    .collect(),
            );
        }
    }

    obj
}

// ── Stream helpers ────────────────────────────────────────────────────────────

fn matches_filters(
    info: &stream::StateUuidInfo,
    type_filter: Option<&str>,
    room_filter: Option<&str>,
    control_filter: Option<&str>,
) -> bool {
    if let Some(tf) = type_filter {
        if !info
            .control_type
            .to_lowercase()
            .contains(&tf.to_lowercase())
        {
            return false;
        }
    }
    if let Some(rf) = room_filter {
        if !info
            .room
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(&rf.to_lowercase())
        {
            return false;
        }
    }
    if let Some(cf) = control_filter {
        if !info
            .control_name
            .to_lowercase()
            .contains(&cf.to_lowercase())
        {
            return false;
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn print_stream_event(
    json: bool,
    csv: bool,
    control: &str,
    state: &str,
    value: &str,
    room: Option<&str>,
    control_type: &str,
    uuid: &str,
) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "timestamp": chrono::Local::now().to_rfc3339(),
                "control": control,
                "state": state,
                "value": value,
                "room": room,
                "type": control_type,
                "uuid": uuid,
            })
        );
    } else if csv {
        println!(
            "{},{},{},{},{},{},{}",
            chrono::Local::now().to_rfc3339(),
            control,
            state,
            value,
            room.unwrap_or(""),
            control_type,
            uuid,
        );
    } else {
        println!(
            "[{}]  {} ({}) = {}  [{}]",
            now_hms(),
            control,
            state,
            value,
            room.unwrap_or("-"),
        );
    }
}

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (u16, u16, u16) {
    let rf = r as f64 / 255.0;
    let gf = g as f64 / 255.0;
    let bf = b as f64 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;
    let h = if delta == 0.0 {
        0.0
    } else if max == rf {
        60.0 * (((gf - bf) / delta) % 6.0)
    } else if max == gf {
        60.0 * (((bf - rf) / delta) + 2.0)
    } else {
        60.0 * (((rf - gf) / delta) + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    let s = if max == 0.0 { 0.0 } else { delta / max * 100.0 };
    let v = max * 100.0;
    (h.round() as u16, s.round() as u16, v.round() as u16)
}

fn parse_weather_entry(
    cursor: &mut Cursor<&[u8]>,
    lox_epoch: &chrono::DateTime<chrono::Local>,
) -> Option<serde_json::Value> {
    // Weather entry: 108 bytes total (mixed i32 + f64 fields)
    // Offset  0: u32 timestamp (seconds since Loxone epoch 2009-01-01)
    let ts = cursor.read_u32::<LittleEndian>().ok()?;
    // Offset  4: i32 weather type (picto-code)
    let _weather_type = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset  8: i32 wind direction (degrees)
    let wind_dir = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 12: i32 solar radiation (UV index)
    let _radiation = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 16: i32 relative humidity (%)
    let humidity = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 20: f64 temperature (°C)
    let temperature = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 28: f64 felt temperature (°C)
    let _felt_temp = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 36: f64 dewpoint (°C)
    let _dewpoint = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 44: f64 precipitation (mm)
    let rain = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 52: f64 wind speed (m/s)
    let wind_speed = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 60: f64 barometric pressure (hPa)
    let pressure = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 68: i32 low clouds (%)
    let low_clouds = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 72: i32 medium clouds (%)
    let med_clouds = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 76: i32 high clouds (%)
    let high_clouds = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 80: i32 precipitation probability
    let _precip_prob = cursor.read_i32::<LittleEndian>().ok()?;
    // Offset 84: f64 absolute radiation
    let _abs_radiation = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 92: f64 snow fraction
    let _snow_fraction = cursor.read_f64::<LittleEndian>().ok()?;
    // Offset 100: f64 CAPE
    let _cape = cursor.read_f64::<LittleEndian>().ok()?;

    let clouds = ((low_clouds + med_clouds + high_clouds) as f64 / 3.0).round();
    let dt = *lox_epoch + chrono::Duration::seconds(ts as i64);
    Some(serde_json::json!({
        "timestamp": dt.format("%Y-%m-%d %H:%M").to_string(),
        "temperature": temperature,
        "humidity": humidity as f64,
        "wind_speed": wind_speed,
        "wind_direction": wind_dir as f64,
        "rain": rain,
        "pressure": pressure,
        "clouds": clouds,
    }))
}

fn eval_op(current: &str, op: &str, target: &str) -> Result<bool> {
    Ok(match op {
        "eq" | "==" => {
            // Try numeric comparison first; fall back to string
            match (current.parse::<f64>(), target.parse::<f64>()) {
                (Ok(a), Ok(b)) => (a - b).abs() < f64::EPSILON * a.abs().max(b.abs()).max(1.0),
                _ => current == target,
            }
        }
        "ne" | "!=" => match (current.parse::<f64>(), target.parse::<f64>()) {
            (Ok(a), Ok(b)) => (a - b).abs() >= f64::EPSILON * a.abs().max(b.abs()).max(1.0),
            _ => current != target,
        },
        "contains" => current.contains(target),
        "gt" | ">" => parse_f(current)? > parse_f(target)?,
        "lt" | "<" => parse_f(current)? < parse_f(target)?,
        "ge" | ">=" => parse_f(current)? >= parse_f(target)?,
        "le" | "<=" => parse_f(current)? <= parse_f(target)?,
        _ => bail!("Unknown op '{}'", op),
    })
}

fn parse_f(s: &str) -> Result<f64> {
    // Try direct parse first, then strip non-numeric suffix (e.g. "21.5°", "100%")
    s.parse::<f64>().or_else(|_| {
        let stripped = s.trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-');
        stripped
            .parse::<f64>()
            .with_context(|| format!("Not a number: '{}'", s))
    })
}

/// Percent-encode characters that would corrupt an HTTP path segment.
/// Does NOT encode '/' so that Loxone command separators (e.g. "manualPosition/50") pass through.
fn encode_path_value(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '%' => vec!['%', '2', '5'],
            ' ' => vec!['%', '2', '0'],
            '#' => vec!['%', '2', '3'],
            '?' => vec!['%', '3', 'F'],
            '+' => vec!['%', '2', 'B'],
            c => vec![c],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use client::is_uuid;

    // ── eval_op ──────────────────────────────────────────────────────────────

    #[test]
    fn test_eval_op_eq() {
        assert!(eval_op("1", "eq", "1").unwrap());
        assert!(!eval_op("1", "eq", "2").unwrap());
        assert!(eval_op("hello", "==", "hello").unwrap());
        // Numeric string formatting differences (Loxone API returns trailing zeros)
        assert!(eval_op("200002700", "==", "200002700.000").unwrap());
        assert!(eval_op("21.5", "==", "21.500000").unwrap());
        assert!(!eval_op("21.5", "==", "21.6").unwrap());
    }

    #[test]
    fn test_eval_op_ne() {
        assert!(eval_op("1", "ne", "2").unwrap());
        assert!(!eval_op("1", "!=", "1").unwrap());
    }

    #[test]
    fn test_eval_op_numeric_comparisons() {
        assert!(eval_op("10", "gt", "5").unwrap());
        assert!(eval_op("5", "lt", "10").unwrap());
        assert!(eval_op("5", "ge", "5").unwrap());
        assert!(eval_op("5", "le", "5").unwrap());
        assert!(!eval_op("4", ">", "5").unwrap());
    }

    #[test]
    fn test_eval_op_contains() {
        assert!(eval_op("hello world", "contains", "world").unwrap());
        assert!(!eval_op("hello", "contains", "world").unwrap());
    }

    #[test]
    fn test_eval_op_unknown_returns_err() {
        assert!(eval_op("1", "bogus", "1").is_err());
    }

    // ── encode_path_value ────────────────────────────────────────────────────

    #[test]
    fn test_encode_path_value_plain() {
        assert_eq!(encode_path_value("on"), "on");
        assert_eq!(encode_path_value("manualPosition/50"), "manualPosition/50");
    }

    #[test]
    fn test_encode_path_value_space() {
        assert_eq!(encode_path_value("hello world"), "hello%20world");
    }

    #[test]
    fn test_encode_path_value_percent() {
        assert_eq!(encode_path_value("50%"), "50%25");
    }

    #[test]
    fn test_encode_path_value_plus() {
        assert_eq!(encode_path_value("a+b"), "a%2Bb");
    }

    // ── parse_f (unit suffix stripping) ─────────────────────────────────────

    #[test]
    fn test_parse_f_plain() {
        assert_eq!(parse_f("21.5").unwrap(), 21.5);
        assert_eq!(parse_f("-3.0").unwrap(), -3.0);
    }

    #[test]
    fn test_parse_f_with_unit_suffix() {
        assert_eq!(parse_f("21.5°").unwrap(), 21.5);
        assert_eq!(parse_f("100%").unwrap(), 100.0);
        assert_eq!(parse_f("15.3°C").unwrap(), 15.3);
    }

    // ── rgb_to_hsv ────────────────────────────────────────────────────────────

    #[test]
    fn test_rgb_to_hsv_red() {
        let (h, s, v) = rgb_to_hsv(255, 0, 0);
        assert_eq!(h, 0);
        assert_eq!(s, 100);
        assert_eq!(v, 100);
    }

    #[test]
    fn test_rgb_to_hsv_green() {
        let (h, s, v) = rgb_to_hsv(0, 255, 0);
        assert_eq!(h, 120);
        assert_eq!(s, 100);
        assert_eq!(v, 100);
    }

    #[test]
    fn test_rgb_to_hsv_blue() {
        let (h, s, v) = rgb_to_hsv(0, 0, 255);
        assert_eq!(h, 240);
        assert_eq!(s, 100);
        assert_eq!(v, 100);
    }

    #[test]
    fn test_rgb_to_hsv_white() {
        let (h, s, v) = rgb_to_hsv(255, 255, 255);
        assert_eq!(h, 0);
        assert_eq!(s, 0);
        assert_eq!(v, 100);
    }

    #[test]
    fn test_rgb_to_hsv_black() {
        let (h, s, v) = rgb_to_hsv(0, 0, 0);
        assert_eq!(h, 0);
        assert_eq!(s, 0);
        assert_eq!(v, 0);
    }

    // ── weather parsing ─────────────────────────────────────────────────────

    #[test]
    fn test_parse_weather_entry() {
        use byteorder::{LittleEndian, WriteBytesExt};
        let lox_epoch = chrono::NaiveDate::from_ymd_opt(2009, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .unwrap();
        let mut data = Vec::new();
        // 108-byte weather entry: u32 ts + 4×i32 + 6×f64 + 4×i32 + 3×f64
        data.write_u32::<LittleEndian>(3600).unwrap(); // timestamp
        data.write_i32::<LittleEndian>(1).unwrap(); // weather type
        data.write_i32::<LittleEndian>(180).unwrap(); // wind direction
        data.write_i32::<LittleEndian>(5).unwrap(); // radiation
        data.write_i32::<LittleEndian>(65).unwrap(); // humidity
        data.write_f64::<LittleEndian>(21.5).unwrap(); // temperature
        data.write_f64::<LittleEndian>(19.0).unwrap(); // felt temp
        data.write_f64::<LittleEndian>(15.0).unwrap(); // dewpoint
        data.write_f64::<LittleEndian>(0.5).unwrap(); // precipitation
        data.write_f64::<LittleEndian>(10.0).unwrap(); // wind speed
        data.write_f64::<LittleEndian>(1013.0).unwrap(); // pressure
        data.write_i32::<LittleEndian>(30).unwrap(); // low clouds
        data.write_i32::<LittleEndian>(60).unwrap(); // med clouds
        data.write_i32::<LittleEndian>(90).unwrap(); // high clouds
        data.write_i32::<LittleEndian>(40).unwrap(); // precip probability
        data.write_f64::<LittleEndian>(500.0).unwrap(); // abs radiation
        data.write_f64::<LittleEndian>(0.0).unwrap(); // snow fraction
        data.write_f64::<LittleEndian>(100.0).unwrap(); // CAPE
        assert_eq!(data.len(), 108);
        let mut cursor = Cursor::new(data.as_slice());
        let entry = parse_weather_entry(&mut cursor, &lox_epoch).unwrap();
        assert_eq!(entry["temperature"].as_f64().unwrap(), 21.5);
        assert_eq!(entry["humidity"].as_f64().unwrap(), 65.0);
        assert_eq!(entry["wind_speed"].as_f64().unwrap(), 10.0);
        assert_eq!(entry["wind_direction"].as_f64().unwrap(), 180.0);
        assert_eq!(entry["rain"].as_f64().unwrap(), 0.5);
        assert_eq!(entry["pressure"].as_f64().unwrap(), 1013.0);
        assert_eq!(entry["clouds"].as_f64().unwrap(), 60.0); // avg(30,60,90)
    }

    // ── is_uuid ──────────────────────────────────────────────────────────────

    #[test]
    fn test_is_uuid() {
        assert!(is_uuid("1fbc668c-005c-7471-ffffed57184a04d2"));
        assert!(!is_uuid("Licht Wohnzimmer"));
        assert!(!is_uuid("short-str"));
    }

    // ── binary stats parsing ────────────────────────────────────────────────

    #[test]
    fn test_parse_binary_stats_data() {
        use byteorder::{LittleEndian, WriteBytesExt};
        // Build a small binary stats payload: 2 entries, 1 output each
        let mut data = Vec::new();
        // Entry 1: ts=1000, value=23.5
        data.write_u32::<LittleEndian>(1000).unwrap();
        data.write_f64::<LittleEndian>(23.5).unwrap();
        // Entry 2: ts=2000, value=24.1
        data.write_u32::<LittleEndian>(2000).unwrap();
        data.write_f64::<LittleEndian>(24.1).unwrap();

        let num_outputs = 1;
        let entry_size = 4 + num_outputs * 8;
        let mut cursor = Cursor::new(&data);
        let mut entries = Vec::new();
        while cursor.position() as usize + entry_size <= data.len() {
            let ts = cursor.read_u32::<LittleEndian>().unwrap();
            let val = cursor.read_f64::<LittleEndian>().unwrap();
            entries.push((ts, val));
        }
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], (1000, 23.5));
        assert_eq!(entries[1], (2000, 24.1));
    }

    #[test]
    fn test_parse_binary_stats_multi_output() {
        use byteorder::{LittleEndian, WriteBytesExt};
        // 1 entry with 3 outputs
        let mut data = Vec::new();
        data.write_u32::<LittleEndian>(5000).unwrap();
        data.write_f64::<LittleEndian>(100.0).unwrap();
        data.write_f64::<LittleEndian>(200.0).unwrap();
        data.write_f64::<LittleEndian>(300.0).unwrap();

        let num_outputs = 3;
        let entry_size = 4 + num_outputs * 8;
        let mut cursor = Cursor::new(&data);
        assert!(cursor.position() as usize + entry_size <= data.len());
        let ts = cursor.read_u32::<LittleEndian>().unwrap();
        let v1 = cursor.read_f64::<LittleEndian>().unwrap();
        let v2 = cursor.read_f64::<LittleEndian>().unwrap();
        let v3 = cursor.read_f64::<LittleEndian>().unwrap();
        assert_eq!(ts, 5000);
        assert_eq!(v1, 100.0);
        assert_eq!(v2, 200.0);
        assert_eq!(v3, 300.0);
    }

    // ── CLI definition (flag conflicts, etc.) ─────────────────────────────

    /// Clap's debug_assert runs all internal consistency checks: duplicate
    /// short flags, missing values, conflicting attributes, etc.  This is
    /// the same check that caused the `-v` panic at runtime — catching it
    /// here means CI will fail before any user ever sees it.
    #[test]
    fn test_cli_debug_assert() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    // ── binary stats header alignment ─────────────────────────────────────

    /// Stats files have a header (12 bytes + variable-length name) followed
    /// by entries aligned to entry_size boundaries.  Verify the alignment
    /// calculation for short and long names.
    #[test]
    fn test_stats_header_alignment_short_name() {
        // "CO2" → 3 bytes, header_end = 15, entry_size = 16 → aligned = 16
        let name = b"CO2";
        let header_end = 12 + name.len();
        let entry_size: usize = 4 + 4 + 1 * 8; // uuid(4) + ts(4) + 1×f64
        let aligned = header_end.div_ceil(entry_size) * entry_size;
        assert_eq!(aligned, 16);
    }

    #[test]
    fn test_stats_header_alignment_long_name() {
        // 21-byte UTF-8 name, header_end = 33, entry_size = 16 → aligned = 48
        let name = "Zähler Licht Vorraum"; // 21 UTF-8 bytes (ä = 2 bytes)
        assert_eq!(name.len(), 21);
        let header_end = 12 + name.len();
        let entry_size: usize = 4 + 4 + 1 * 8;
        let aligned = header_end.div_ceil(entry_size) * entry_size;
        assert_eq!(aligned, 48);
    }

    #[test]
    fn test_stats_header_alignment_exact_boundary() {
        // Name exactly fills to a boundary: header_end = 32, entry_size = 16 → 32
        let header_end = 32usize;
        let entry_size: usize = 16;
        let aligned = header_end.div_ceil(entry_size) * entry_size;
        assert_eq!(aligned, 32);
    }

    /// Full round-trip: build a binary stats file with header + aligned
    /// entries and verify we parse the correct timestamps and values.
    #[test]
    fn test_stats_file_parse_with_header() {
        use byteorder::{LittleEndian, WriteBytesExt};
        let mut data = Vec::new();
        // Header: valueCount=1 (with version flag), controlType=0, nameLength=3
        data.write_u32::<LittleEndian>(0x8000_0001).unwrap();
        data.write_u32::<LittleEndian>(0).unwrap();
        data.write_u32::<LittleEndian>(3).unwrap(); // "CO2"
        data.extend_from_slice(b"CO2");
        // Pad to 16 bytes (entry_size alignment)
        data.push(0); // null terminator / padding to reach 16
        assert_eq!(data.len(), 16);
        // Entry 1: uuid_prefix(4) + ts(4) + value(8) = 16 bytes
        data.write_u32::<LittleEndian>(0x03b498a5).unwrap(); // uuid prefix
        data.write_u32::<LittleEndian>(541_293_056).unwrap(); // ~2026-03-01
        data.write_f64::<LittleEndian>(985.2).unwrap();
        // Entry 2
        data.write_u32::<LittleEndian>(0x03b498a5).unwrap();
        data.write_u32::<LittleEndian>(541_296_656).unwrap(); // +1 hour
        data.write_f64::<LittleEndian>(1215.1).unwrap();

        let num_outputs = 1;
        let entry_size = 4 + 4 + num_outputs * 8;
        let mut cursor = Cursor::new(data.as_slice());

        // Skip header (same logic as the history command)
        let value_count = cursor.read_u32::<LittleEndian>().unwrap();
        let _control_type = cursor.read_u32::<LittleEndian>().unwrap();
        let name_length = cursor.read_u32::<LittleEndian>().unwrap();
        assert_eq!(value_count & 0x7FFF_FFFF, 1);
        assert_eq!(name_length, 3);
        let header_end = 12 + name_length as usize;
        let aligned = header_end.div_ceil(entry_size) * entry_size;
        cursor.set_position(aligned as u64);
        assert_eq!(cursor.position(), 16);

        // Parse entries
        let mut entries = Vec::new();
        while cursor.position() as usize + entry_size <= data.len() {
            let _uuid_part = cursor.read_u32::<LittleEndian>().unwrap();
            let ts = cursor.read_u32::<LittleEndian>().unwrap();
            let val = cursor.read_f64::<LittleEndian>().unwrap();
            entries.push((ts, val));
        }
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, 541_293_056);
        assert_eq!(entries[0].1, 985.2);
        assert_eq!(entries[1].0, 541_296_656);
        assert_eq!(entries[1].1, 1215.1);
    }
}
