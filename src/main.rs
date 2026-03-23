mod client;
mod commands;
mod config;
mod ftp;
mod gitops;
mod loxcc;
mod loxone_xml;
mod otel;
mod scene;
mod stream;
mod token;
mod ws;

use anyhow::{Context, Result, bail};
use clap::{ArgAction, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use client::LoxClient;
use dirs::home_dir;
use serde_json::Value;
use std::fs;
use std::io::Cursor;

use byteorder::{LittleEndian, ReadBytesExt};

/// Load config XML from a .Loxone file or extract from a .zip backup.
pub(crate) fn load_config_xml(path: &str) -> Result<Vec<u8>> {
    let data = fs::read(path).with_context(|| format!("Cannot read {}", path))?;
    if path.ends_with(".zip") {
        loxcc::extract_and_decompress(&data)
    } else {
        Ok(data)
    }
}

pub(crate) fn xml_attr<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    // XML format: attr="value"
    let xml_key = format!("{}=\"", attr);
    let mut search_from = 0;
    while let Some(pos) = xml[search_from..].find(&xml_key) {
        let abs_pos = search_from + pos;
        if abs_pos == 0 || matches!(xml.as_bytes()[abs_pos - 1], b' ' | b'<' | b'\t' | b'\n') {
            let start = abs_pos + xml_key.len();
            let end = xml[start..].find('"')? + start;
            return Some(&xml[start..end]);
        }
        search_from = abs_pos + 1;
    }

    // JSON format: "attr": value  (handles /jdev/ responses)
    let json_key = format!("\"{}\": ", attr);
    if let Some(pos) = xml.find(&json_key) {
        let start = pos + json_key.len();
        let rest = &xml[start..];
        if let Some(stripped) = rest.strip_prefix('"') {
            // String value: extract between quotes
            let end = stripped.find('"')?;
            return Some(&stripped[..end]);
        }
        // Numeric/other value: read until delimiter
        let end = rest.find([',', '}', ']']).unwrap_or(rest.len());
        let val = rest[..end].trim();
        if !val.is_empty() {
            return Some(val);
        }
    }

    None
}

/// Extract a JSON value as a display string, handling both string and numeric types.
/// Gen2 Miniservers return numeric values (e.g. `"Code": 200`) instead of strings
/// (`"Code": "200"`), so `.as_str()` alone is insufficient.
pub(crate) fn json_val_str(v: &Value) -> Option<String> {
    v.as_str()
        .map(|s| s.to_string())
        .or_else(|| v.as_i64().map(|n| n.to_string()))
        .or_else(|| v.as_f64().map(|n| n.to_string()))
}

pub(crate) fn print_resp(resp: &Value, json: bool, quiet: bool, name: &str, cmd: &str) {
    if json {
        println!("{}", serde_json::to_string_pretty(resp).unwrap());
    } else if !quiet {
        let val = resp
            .pointer("/LL/value")
            .and_then(json_val_str)
            .unwrap_or_else(|| "?".to_string());
        let code = resp
            .pointer("/LL/Code")
            .and_then(json_val_str)
            .unwrap_or_else(|| "?".to_string());
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
pub(crate) fn print_dry_run(
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
pub(crate) fn send_or_dry_run(
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

pub(crate) fn bar(v: f64, max: f64) -> String {
    let n = ((v / max * 20.0) as usize).min(20);
    format!(
        "[{}{}] {:.0}%",
        "█".repeat(n),
        "░".repeat(20 - n),
        v / max * 100.0
    )
}

pub(crate) fn kb_fmt(kb: f64) -> String {
    if kb > 1024.0 {
        format!("{:.0} MB", kb / 1024.0)
    } else {
        format!("{:.0} kB", kb)
    }
}

/// Loxone epoch: 2009-01-01 00:00:00 local time.
/// All Loxone timestamps are seconds since this epoch.
pub(crate) fn lox_epoch() -> chrono::DateTime<chrono::Local> {
    chrono::NaiveDate::from_ymd_opt(2009, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
}

/// Convert a Loxone timestamp (seconds since 2009-01-01) to a formatted string.
pub(crate) fn lox_timestamp_to_string(ts: i64) -> String {
    let dt = lox_epoch() + chrono::Duration::seconds(ts);
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// A single parsed statistics entry.
pub(crate) struct StatsEntry {
    pub(crate) timestamp: String,
    pub(crate) values: Vec<f64>,
}

/// Parse binary statistics data (after header has been skipped).
/// `data` is the raw bytes starting from the first entry.
/// `num_outputs` is the number of f64 values per entry.
pub(crate) fn parse_stats_entries(data: &[u8], num_outputs: usize) -> Vec<StatsEntry> {
    let entry_size = 4 + 4 + num_outputs * 8; // UUID(4) + ts(4) + values
    let mut cursor = Cursor::new(data);
    let mut entries = Vec::new();
    while cursor.position() as usize + entry_size <= data.len() {
        let _uuid_part = cursor.read_u32::<LittleEndian>().unwrap_or(0);
        let ts = cursor.read_u32::<LittleEndian>().unwrap_or(0);
        let mut values = Vec::new();
        for _ in 0..num_outputs {
            values.push(cursor.read_f64::<LittleEndian>().unwrap_or(0.0));
        }
        entries.push(StatsEntry {
            timestamp: lox_timestamp_to_string(ts as i64),
            values,
        });
    }
    entries
}

/// Skip the binary stats file header and return the offset where entries begin.
/// Header format: u32 valueCount | u32 controlType | u32 nameLength | name bytes
/// Returns None if data is too short.
pub(crate) fn stats_data_offset(data: &[u8], entry_size: usize) -> Option<usize> {
    if data.len() <= 12 {
        return None;
    }
    let mut cursor = Cursor::new(data);
    let _value_count = cursor.read_u32::<LittleEndian>().ok()?;
    let _control_type = cursor.read_u32::<LittleEndian>().ok()?;
    let name_length = cursor.read_u32::<LittleEndian>().ok()?;
    let header_end = 12 + name_length as usize;
    Some(header_end.div_ceil(entry_size) * entry_size)
}

/// Determine the stats period string from day/month flags.
pub(crate) fn stats_period(day: Option<&str>, month: Option<&str>) -> String {
    if let Some(d) = day {
        d.replace('-', "")
    } else if let Some(m) = month {
        m.replace('-', "")
    } else {
        chrono::Local::now().format("%Y%m").to_string()
    }
}

/// Build the bare fsget path for a stats file.
pub(crate) fn stats_file_path(uuid: &str, period: &str) -> String {
    format!("/dev/fsget//stats/{}.{}", uuid, period)
}

/// Parse an fslist output to find matching stats files for a UUID and period.
pub(crate) fn find_stats_files<'a>(listing: &'a str, uuid: &str, period: &str) -> Vec<&'a str> {
    let suffix = format!(".{}", period);
    let mut files: Vec<&str> = listing
        .lines()
        .filter_map(|line| line.rsplit_once(' ').map(|(_, name)| name))
        .filter(|name| name.starts_with(uuid) && name.ends_with(suffix.as_str()))
        .collect();
    files.sort();
    files
}

/// Normalize a filesystem path for Loxone fsget/fslist API.
/// Ensures the path starts with '/' so the double-slash URL
/// (e.g. /dev/fsget//path) is correct.
pub(crate) fn abs_path(p: &str) -> String {
    if p.starts_with('/') {
        p.to_string()
    } else {
        format!("/{p}")
    }
}

pub(crate) fn now_hms() -> String {
    use chrono::Timelike;
    let now = chrono::Local::now();
    format!("{:02}:{:02}:{:02}", now.hour(), now.minute(), now.second())
}

pub(crate) fn detect_shell() -> Option<Shell> {
    // On Unix, $SHELL is the login shell
    if let Some(shell) = std::env::var("SHELL").ok().and_then(|s| {
        let name = s.rsplit('/').next().unwrap_or(&s);
        match name {
            "bash" => Some(Shell::Bash),
            "zsh" => Some(Shell::Zsh),
            "fish" => Some(Shell::Fish),
            "elvish" => Some(Shell::Elvish),
            "pwsh" | "powershell" => Some(Shell::PowerShell),
            _ => None,
        }
    }) {
        return Some(shell);
    }
    // On Windows, $SHELL is not set; check for PowerShell via PSModulePath
    if std::env::var("PSModulePath").is_ok() {
        return Some(Shell::PowerShell);
    }
    None
}

pub(crate) fn install_completions(shell: Shell, cmd: &mut clap::Command) -> Result<()> {
    // Check LOX_HOME first (for testing), then HOME, then dirs::home_dir()
    let home = std::env::var("LOX_HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(home_dir)
        .unwrap_or_default();
    let (path, label) = match shell {
        Shell::Bash => {
            let dir = home.join(".local/share/bash-completion/completions");
            fs::create_dir_all(&dir)?;
            (dir.join("lox"), "bash")
        }
        Shell::Zsh => {
            let dir = home.join(".zfunc");
            fs::create_dir_all(&dir)?;
            (dir.join("_lox"), "zsh")
        }
        Shell::Fish => {
            let dir = home.join(".config/fish/completions");
            fs::create_dir_all(&dir)?;
            (dir.join("lox.fish"), "fish")
        }
        Shell::PowerShell => {
            // Install as a PowerShell profile script
            let documents = if cfg!(windows) {
                home.join("Documents")
            } else {
                home.join(".config")
            };
            let dir = documents.join("PowerShell");
            fs::create_dir_all(&dir)?;
            (dir.join("lox_completions.ps1"), "powershell")
        }
        Shell::Elvish => {
            bail!("Elvish completions must be installed manually — run: lox completions elvish");
        }
        _ => bail!("Unsupported shell"),
    };
    let mut buf = Vec::new();
    generate(shell, cmd, "lox", &mut buf);
    fs::write(&path, &buf)?;

    eprintln!("Installed {} completions to {}", label, path.display());
    if shell == Shell::Zsh {
        eprintln!("Ensure ~/.zfunc is in your fpath. Add to ~/.zshrc:");
        eprintln!("  fpath=(~/.zfunc $fpath)");
        eprintln!("  autoload -Uz compinit && compinit");
    }
    if shell == Shell::PowerShell {
        eprintln!("Add this line to your PowerShell $PROFILE to load completions:");
        eprintln!("  . \"{}\"", path.display());
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
  health                       Device health dashboard
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
pub(crate) struct Cli {
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
pub(crate) enum OutputFormat {
    /// Human-readable table (default)
    Table,
    /// JSON output
    Json,
    /// CSV output (where applicable)
    Csv,
}

#[derive(Subcommand)]
pub(crate) enum Cmd {
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
        /// Show CPU, tasks, context switches, interrupts, SD card health
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
    /// Device health dashboard — battery, signal, offline, bus errors
    Health {
        /// Filter by device type: tree, air
        #[arg(long = "type", short = 't')]
        device_type: Option<String>,
        /// Show only devices with problems (warnings + offline)
        #[arg(long)]
        problems: bool,
    },
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
pub(crate) enum TokenCmd {
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
pub(crate) enum LightCmd {
    /// List available moods for a LightControllerV2
    #[command(alias = "list-moods")]
    Moods {
        name_or_uuid: String,
        #[arg(long, short = 'r')]
        room: Option<String>,
    },
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
pub(crate) enum InputCmd {
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
pub(crate) enum AutopilotCmd {
    /// List all automatic rules
    #[command(alias = "list")]
    Ls,
    /// Show when a rule last fired
    State { name_or_uuid: String },
}

#[derive(Subcommand)]
pub(crate) enum UpdateCmd {
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
pub(crate) enum MusicCmd {
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
pub(crate) enum FilesCmd {
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
pub(crate) enum OtelCmd {
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
pub(crate) enum CacheCmd {
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
pub(crate) enum SetupCmd {
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
pub(crate) enum AliasCmd {
    /// Add or update an alias
    Add { name: String, uuid: String },
    /// Remove an alias
    Remove { name: String },
    /// List all aliases
    #[command(alias = "list")]
    Ls,
}

#[derive(Subcommand)]
pub(crate) enum SceneCmd {
    /// List all saved scenes
    #[command(alias = "list")]
    Ls,
    /// Print a scene's YAML definition
    Show { name: String },
    /// Create a new empty scene file
    New { name: String },
}

#[derive(Subcommand)]
pub(crate) enum ConfigCmd {
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
    /// Initialize a git repository for config version tracking
    Init {
        /// Path for the config git repository
        path: String,
    },
    /// Download the latest config from the Miniserver and commit to git
    Pull {
        /// Suppress output (for cron usage)
        #[arg(long)]
        quiet: bool,
    },
    /// Show config change history from the git repository
    Log {
        /// Number of entries to show
        #[arg(short, long, default_value = "20")]
        count: usize,
    },
    /// Restore a previous config version from git and upload to Miniserver
    Restore {
        /// Git commit hash to restore from
        commit: String,
        /// Confirm the upload (required — dangerous operation)
        #[arg(long)]
        force: bool,
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
    let ctx = commands::RunContext {
        json: cli.output == OutputFormat::Json,
        quiet: cli.quiet,
        csv: cli.output == OutputFormat::Csv,
        dry_run: cli.dry_run,
        no_header: cli.no_header,
        trace_id: cli.trace_id.clone(),
    };

    if let Some(tid) = &ctx.trace_id
        && cli.verbose > 0
    {
        eprintln!("trace-id: {}", tid);
    }

    // Respect NO_COLOR env var (clig.dev standard) and --no-color flag
    if cli.no_color || std::env::var("NO_COLOR").is_ok() {
        // SAFETY: Called during single-threaded startup before any concurrent work.
        unsafe { std::env::set_var("NO_COLOR", "1") };
    }

    match cli.cmd {
        // ── Control commands ──────────────────────────────────────────
        Cmd::On {
            name_or_uuid,
            room,
            all_in_room,
        } => commands::control::cmd_on(&ctx, name_or_uuid, room, all_in_room),
        Cmd::Off {
            name_or_uuid,
            room,
            all_in_room,
        } => commands::control::cmd_off(&ctx, name_or_uuid, room, all_in_room),
        Cmd::Pulse { name_or_uuid, room } => commands::control::cmd_pulse(&ctx, name_or_uuid, room),
        Cmd::Blind {
            name_or_uuid,
            action,
            pos,
            room,
        } => commands::control::cmd_blind(&ctx, name_or_uuid, action, pos, room),
        Cmd::Light { action } => commands::control::cmd_light(&ctx, action),
        Cmd::Input { action } => commands::control::cmd_input(&ctx, action),
        Cmd::Mood {
            name_or_uuid,
            action,
            room,
        } => commands::control::cmd_mood(&ctx, name_or_uuid, action, room),
        Cmd::Dimmer {
            name_or_uuid,
            level,
            room,
        } => commands::control::cmd_dimmer(&ctx, name_or_uuid, level, room),
        Cmd::Gate {
            name_or_uuid,
            action,
            room,
        } => commands::control::cmd_gate(&ctx, name_or_uuid, action, room),
        Cmd::Color {
            name_or_uuid,
            value,
            room,
        } => commands::control::cmd_color(&ctx, name_or_uuid, value, room),
        Cmd::Thermostat {
            name_or_uuid,
            action,
            value,
            duration,
            room,
        } => commands::control::cmd_thermostat(&ctx, name_or_uuid, action, value, duration, room),
        Cmd::Door {
            name_or_uuid,
            action,
            room,
        } => commands::control::cmd_door(&ctx, name_or_uuid, action, room),
        Cmd::Intercom {
            name_or_uuid,
            action,
            room,
        } => commands::control::cmd_intercom(&ctx, name_or_uuid, action, room),
        Cmd::Charger {
            name_or_uuid,
            action,
            limit,
            room,
        } => commands::control::cmd_charger(&ctx, name_or_uuid, action, limit, room),
        Cmd::Alarm {
            name_or_uuid,
            action,
            no_motion,
            code,
            room,
        } => commands::control::cmd_alarm(&ctx, name_or_uuid, action, no_motion, code, room),
        Cmd::Lock {
            name_or_uuid,
            reason,
            room,
        } => commands::control::cmd_lock(&ctx, name_or_uuid, reason, room),
        Cmd::Unlock { name_or_uuid, room } => {
            commands::control::cmd_unlock(&ctx, name_or_uuid, room)
        }
        Cmd::Send {
            name_or_uuid,
            command,
            room,
            secured,
        } => commands::control::cmd_send(&ctx, name_or_uuid, command, room, secured),
        Cmd::Set {
            name_or_uuid,
            value,
            room,
        } => commands::control::cmd_set(&ctx, name_or_uuid, value, room),
        Cmd::Run { scene, dry_run } => commands::control::cmd_run(&ctx, scene, dry_run),
        Cmd::Music { action } => commands::control::cmd_music(&ctx, action),

        // ── Inspect commands ──────────────────────────────────────────
        Cmd::Ls {
            r#type,
            room,
            values,
            cat,
            favorites,
        } => commands::inspect::cmd_ls(&ctx, r#type, room, values, cat, favorites),
        Cmd::Get { name_or_uuid, room } => commands::inspect::cmd_get(&ctx, name_or_uuid, room),
        Cmd::Info { name_or_uuid, room } => commands::inspect::cmd_info(&ctx, name_or_uuid, room),
        Cmd::Rooms => commands::inspect::cmd_rooms(&ctx),
        Cmd::Categories => commands::inspect::cmd_categories(&ctx),
        Cmd::Globals => commands::inspect::cmd_globals(&ctx),
        Cmd::Modes => commands::inspect::cmd_modes(&ctx),
        Cmd::Sensors { r#type, room } => commands::inspect::cmd_sensors(&ctx, r#type, room),
        Cmd::Energy { room } => commands::inspect::cmd_energy(&ctx, room),
        Cmd::Weather { forecast } => commands::inspect::cmd_weather(&ctx, forecast),
        Cmd::Stats => commands::inspect::cmd_stats(&ctx),
        Cmd::History {
            name_or_uuid,
            month,
            day,
            room,
        } => commands::inspect::cmd_history(&ctx, name_or_uuid, month, day, room),
        Cmd::Autopilot { action } => commands::inspect::cmd_autopilot(&ctx, action),
        Cmd::Watch {
            name_or_uuid,
            interval,
        } => commands::inspect::cmd_watch(&ctx, name_or_uuid, interval),
        Cmd::Stream {
            r#type,
            room,
            control,
            initial,
        } => commands::inspect::cmd_stream(&ctx, r#type, room, control, initial),
        Cmd::If {
            name_or_uuid,
            op,
            value,
            room,
        } => commands::inspect::cmd_if(&ctx, name_or_uuid, op, value, room),

        // ── System commands ───────────────────────────────────────────
        Cmd::Status {
            energy,
            diag,
            net,
            bus,
            lan,
            all,
        } => commands::system::cmd_status(&ctx, energy, diag, net, bus, lan, all),
        Cmd::Log { lines } => commands::system::cmd_log(&ctx, lines),
        Cmd::Time => commands::system::cmd_time(&ctx),
        Cmd::Discover { timeout } => commands::system::cmd_discover(&ctx, timeout),
        Cmd::Extensions => commands::system::cmd_extensions(&ctx),
        Cmd::Health {
            device_type,
            problems,
        } => commands::system::cmd_health(&ctx, device_type, problems),
        Cmd::Update { action } => commands::system::cmd_update(&ctx, action),
        Cmd::Reboot { yes } => commands::system::cmd_reboot(&ctx, yes),
        Cmd::Files { action } => commands::system::cmd_files(&ctx, action),
        Cmd::Otel { action } => commands::system::cmd_otel(&ctx, action),

        // ── Configuration commands ────────────────────────────────────
        Cmd::Setup { action } => commands::config_cmd::cmd_setup(&ctx, action),
        Cmd::Alias { action } => commands::config_cmd::cmd_alias(&ctx, action),
        Cmd::Scene { action } => commands::config_cmd::cmd_scene(&ctx, action),
        Cmd::Cache { action } => commands::config_cmd::cmd_cache(&ctx, action),
        Cmd::Token { action } => commands::config_cmd::cmd_token(&ctx, action),
        Cmd::Config { action } => commands::config_cmd::cmd_config(&ctx, action),
        Cmd::Completions { shell, install } => {
            commands::config_cmd::cmd_completions(&ctx, shell, install)
        }
        Cmd::Schema { command } => commands::config_cmd::cmd_schema(&ctx, command),
    }
}

// ── Schema introspection ─────────────────────────────────────────────────────

pub(crate) fn build_schema(filter: Option<&str>) -> Result<Value> {
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

pub(crate) fn describe_command(cmd: &clap::Command, detailed: bool) -> Value {
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

pub(crate) fn matches_filters(
    info: &stream::StateUuidInfo,
    type_filter: Option<&str>,
    room_filter: Option<&str>,
    control_filter: Option<&str>,
) -> bool {
    if let Some(tf) = type_filter
        && !info
            .control_type
            .to_lowercase()
            .contains(&tf.to_lowercase())
    {
        return false;
    }
    if let Some(rf) = room_filter
        && !info
            .room
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(&rf.to_lowercase())
    {
        return false;
    }
    if let Some(cf) = control_filter
        && !info
            .control_name
            .to_lowercase()
            .contains(&cf.to_lowercase())
    {
        return false;
    }
    true
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn print_stream_event(
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

pub(crate) fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (u16, u16, u16) {
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

pub(crate) fn parse_weather_entry(
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

pub(crate) fn eval_op(current: &str, op: &str, target: &str) -> Result<bool> {
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

pub(crate) fn parse_f(s: &str) -> Result<f64> {
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
pub(crate) fn encode_path_value(s: &str) -> String {
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

    // ── json_val_str (Gen2 numeric value handling) ─────────────────────────

    #[test]
    fn test_json_val_str_string() {
        let v = serde_json::json!("200");
        assert_eq!(json_val_str(&v), Some("200".to_string()));
    }

    #[test]
    fn test_json_val_str_integer() {
        let v = serde_json::json!(200);
        assert_eq!(json_val_str(&v), Some("200".to_string()));
    }

    #[test]
    fn test_json_val_str_float() {
        let v = serde_json::json!(21.5);
        assert_eq!(json_val_str(&v), Some("21.5".to_string()));
    }

    #[test]
    fn test_json_val_str_null() {
        let v = serde_json::json!(null);
        assert_eq!(json_val_str(&v), None);
    }

    #[test]
    fn test_print_resp_numeric_code() {
        // Gen2 Miniservers return numeric Code values
        let resp = serde_json::json!({"LL": {"Code": 200, "value": 1}});
        // Should not panic and should show ✓ (code 200)
        print_resp(&resp, false, false, "test", "on");
    }

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
        let epoch = lox_epoch();
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
        let entry = parse_weather_entry(&mut cursor, &epoch).unwrap();
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
    /// entries and verify we parse the correct timestamps and values
    /// using the extracted helper functions.
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
        let offset = stats_data_offset(&data, entry_size).unwrap();
        assert_eq!(offset, 16);
        let entries = parse_stats_entries(&data[offset..], num_outputs);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].values[0], 985.2);
        assert_eq!(entries[1].values[0], 1215.1);
    }

    // ── xml_attr ──────────────────────────────────────────────────────────────

    #[test]
    fn test_xml_attr_basic() {
        let xml = r#"<LL control="test" value="42" Code="200"/>"#;
        assert_eq!(xml_attr(xml, "value"), Some("42"));
        assert_eq!(xml_attr(xml, "Code"), Some("200"));
        assert_eq!(xml_attr(xml, "control"), Some("test"));
    }

    #[test]
    fn test_xml_attr_missing() {
        let xml = r#"<LL control="test" value="42"/>"#;
        assert_eq!(xml_attr(xml, "missing"), None);
    }

    #[test]
    fn test_xml_attr_empty_value() {
        let xml = r#"<LL value=""/>"#;
        assert_eq!(xml_attr(xml, "value"), Some(""));
    }

    #[test]
    fn test_xml_attr_spaces_in_value() {
        let xml = r#"<LL value="hello world"/>"#;
        assert_eq!(xml_attr(xml, "value"), Some("hello world"));
    }

    #[test]
    fn test_xml_attr_no_partial_name_match() {
        // "value" must NOT match "value2" — word boundary check
        let xml = r#"<LL value2="wrong" value="right"/>"#;
        assert_eq!(xml_attr(xml, "value"), Some("right"));
    }

    #[test]
    fn test_xml_attr_partial_name_only_match() {
        // When only value2 exists, looking for "value" returns None
        let xml = r#"<LL value2="wrong"/>"#;
        assert_eq!(xml_attr(xml, "value"), None);
    }

    #[test]
    fn test_xml_attr_json_format() {
        // /jdev/ endpoints return JSON instead of XML
        let json =
            r#"{"LL": { "control": "dev/sys/lastcpu", "value": "CPU:12 SPS:26", "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("CPU:12 SPS:26"));
        assert_eq!(xml_attr(json, "Code"), Some("200"));
        assert_eq!(xml_attr(json, "control"), Some("dev/sys/lastcpu"));
    }

    #[test]
    fn test_xml_attr_json_empty_value() {
        let json = r#"{"LL": { "control": "dev/sys/ints", "value": "", "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some(""));
    }

    #[test]
    fn test_xml_attr_json_numeric_value() {
        // Gen2 Miniservers return numeric values without quotes for counters
        let json =
            r#"{"LL": { "control": "dev/sys/contextswitches", "value": 123456789, "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("123456789"));
        assert_eq!(xml_attr(json, "Code"), Some("200"));
    }

    #[test]
    fn test_xml_attr_json_numeric_last_field() {
        // Numeric value as the last field before closing brace
        let json = r#"{"LL": { "value": 42}}"#;
        assert_eq!(xml_attr(json, "value"), Some("42"));
    }

    #[test]
    fn test_xml_attr_json_float_value() {
        let json = r#"{"LL": { "value": 3.14, "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("3.14"));
    }

    #[test]
    fn test_xml_attr_json_negative_value() {
        let json = r#"{"LL": { "value": -1, "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("-1"));
    }

    #[test]
    fn test_xml_attr_json_zero_value() {
        let json = r#"{"LL": { "value": 0, "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("0"));
    }

    #[test]
    fn test_xml_attr_json_bool_value() {
        // Unlikely from Miniserver but should not panic
        let json = r#"{"LL": { "value": true, "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("true"));
    }

    #[test]
    fn test_xml_attr_json_null_value() {
        let json = r#"{"LL": { "value": null, "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("null"));
    }

    /// Exercises xml_attr against every response format the Miniserver uses
    /// for the diagnostic endpoints exposed by `lox status --diag/--net/--bus/--lan`.
    /// This is a regression test: Gen2 Miniservers return numeric JSON values
    /// for counters, which the original string-only parser missed.
    #[test]
    fn test_xml_attr_all_diagnostic_response_formats() {
        // Gen1 XML format (most endpoints)
        let xml = r#"<LL control="dev/sys/heap" value="352880/1016404kB" Code="200"/>"#;
        assert_eq!(xml_attr(xml, "value"), Some("352880/1016404kB"));

        // Gen2 JSON with string value (lastcpu, numtasks, sdtest)
        let json = r#"{"LL": { "control": "dev/sys/lastcpu", "value": "CPU:11 SPS:24 Cycles:99", "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("CPU:11 SPS:24 Cycles:99"));

        // Gen2 JSON with numeric value (ints, comints, contextswitches, contextswitchesi)
        for endpoint in [
            "dev/sys/contextswitches",
            "dev/sys/ints",
            "dev/sys/comints",
            "dev/sys/contextswitchesi",
        ] {
            let json = format!(
                r#"{{"LL": {{ "control": "{}", "value": 987654, "Code": "200"}}}}"#,
                endpoint
            );
            assert_eq!(
                xml_attr(&json, "value"),
                Some("987654"),
                "failed for endpoint {}",
                endpoint
            );
        }

        // Gen2 JSON with numeric value for bus/lan counters
        for endpoint in [
            "bus/packetssent",
            "bus/packetsreceived",
            "bus/parityerrors",
            "lan/txp",
            "lan/txu",
            "lan/exh",
            "lan/nob",
        ] {
            let json = format!(
                r#"{{"LL": {{ "control": "{}", "value": 42, "Code": "200"}}}}"#,
                endpoint
            );
            assert_eq!(
                xml_attr(&json, "value"),
                Some("42"),
                "failed for endpoint {}",
                endpoint
            );
        }

        // Gen2 JSON with string value for network config
        let json = r#"{"LL": { "control": "cfg/dns2", "value": "8.8.4.4", "Code": "200"}}"#;
        assert_eq!(xml_attr(json, "value"), Some("8.8.4.4"));
    }

    // ── matches_filters ───────────────────────────────────────────────────────

    #[test]
    fn test_matches_filters_no_filters() {
        let info = stream::StateUuidInfo {
            control_name: "Light".to_string(),
            control_uuid: "uuid".to_string(),
            state_name: "active".to_string(),
            control_type: "Switch".to_string(),
            room: Some("Kitchen".to_string()),
            category: None,
            unit: None,
        };
        assert!(matches_filters(&info, None, None, None));
    }

    #[test]
    fn test_matches_filters_type_filter() {
        let info = stream::StateUuidInfo {
            control_name: "Light".to_string(),
            control_uuid: "uuid".to_string(),
            state_name: "active".to_string(),
            control_type: "Switch".to_string(),
            room: Some("Kitchen".to_string()),
            category: None,
            unit: None,
        };
        assert!(matches_filters(&info, Some("switch"), None, None));
        assert!(!matches_filters(&info, Some("dimmer"), None, None));
    }

    #[test]
    fn test_matches_filters_room_filter() {
        let info = stream::StateUuidInfo {
            control_name: "Light".to_string(),
            control_uuid: "uuid".to_string(),
            state_name: "active".to_string(),
            control_type: "Switch".to_string(),
            room: Some("Kitchen".to_string()),
            category: None,
            unit: None,
        };
        assert!(matches_filters(&info, None, Some("kitchen"), None));
        assert!(!matches_filters(&info, None, Some("bedroom"), None));
    }

    #[test]
    fn test_matches_filters_control_filter() {
        let info = stream::StateUuidInfo {
            control_name: "Main Light".to_string(),
            control_uuid: "uuid".to_string(),
            state_name: "active".to_string(),
            control_type: "Switch".to_string(),
            room: Some("Kitchen".to_string()),
            category: None,
            unit: None,
        };
        assert!(matches_filters(&info, None, None, Some("main")));
        assert!(!matches_filters(&info, None, None, Some("side")));
    }

    #[test]
    fn test_matches_filters_missing_room() {
        let info = stream::StateUuidInfo {
            control_name: "Light".to_string(),
            control_uuid: "uuid".to_string(),
            state_name: "active".to_string(),
            control_type: "Switch".to_string(),
            room: None,
            category: None,
            unit: None,
        };
        // Room filter should not match when room is None
        assert!(!matches_filters(&info, None, Some("kitchen"), None));
        // But no room filter should still pass
        assert!(matches_filters(&info, None, None, None));
    }

    // ── bar ───────────────────────────────────────────────────────────────────

    #[test]
    fn test_bar_zero() {
        let s = bar(0.0, 100.0);
        assert!(s.starts_with("[░"));
        assert!(s.ends_with("0%"));
    }

    #[test]
    fn test_bar_full() {
        let s = bar(100.0, 100.0);
        assert!(s.starts_with("[████████████████████]"));
        assert!(s.ends_with("100%"));
    }

    #[test]
    fn test_bar_half() {
        let s = bar(50.0, 100.0);
        assert!(s.contains("50%"));
    }

    // ── kb_fmt ────────────────────────────────────────────────────────────────

    #[test]
    fn test_kb_fmt_kb() {
        assert_eq!(kb_fmt(512.0), "512 kB");
    }

    #[test]
    fn test_kb_fmt_mb() {
        assert_eq!(kb_fmt(2048.0), "2 MB");
    }

    #[test]
    fn test_kb_fmt_boundary() {
        // At exactly 1024, stays in kB
        assert_eq!(kb_fmt(1024.0), "1024 kB");
    }

    // ── lox_epoch / lox_timestamp_to_string ───────────────────────────────────

    #[test]
    fn test_lox_epoch_is_2009() {
        let e = lox_epoch();
        assert_eq!(e.format("%Y-%m-%d").to_string(), "2009-01-01");
    }

    #[test]
    fn test_lox_timestamp_to_string() {
        // 0 seconds = epoch itself
        let s = lox_timestamp_to_string(0);
        assert!(s.starts_with("2009-01-01"));
    }

    // ── stats helpers ─────────────────────────────────────────────────────────

    #[test]
    fn test_stats_period_day() {
        assert_eq!(stats_period(Some("2026-03-15"), None), "20260315");
    }

    #[test]
    fn test_stats_period_month() {
        assert_eq!(stats_period(None, Some("2026-03")), "202603");
    }

    #[test]
    fn test_stats_period_default() {
        // Default returns current month in YYYYMM format
        let p = stats_period(None, None);
        assert_eq!(p.len(), 6);
        assert!(p.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_stats_file_path() {
        assert_eq!(
            stats_file_path("abc-123", "202603"),
            "/dev/fsget//stats/abc-123.202603"
        );
    }

    #[test]
    fn test_find_stats_files_basic() {
        let listing = "123456 abc-123.202603\n789012 abc-123_1.202603\n345678 other.202603";
        let files = find_stats_files(listing, "abc-123", "202603");
        assert_eq!(files, vec!["abc-123.202603", "abc-123_1.202603"]);
    }

    #[test]
    fn test_find_stats_files_no_match() {
        let listing = "123456 other.202603";
        let files = find_stats_files(listing, "abc-123", "202603");
        assert!(files.is_empty());
    }

    #[test]
    fn test_stats_data_offset_short_name() {
        use byteorder::{LittleEndian, WriteBytesExt};
        let mut data = Vec::new();
        data.write_u32::<LittleEndian>(0x8000_0001).unwrap();
        data.write_u32::<LittleEndian>(0).unwrap();
        data.write_u32::<LittleEndian>(3).unwrap(); // name_length=3
        data.extend_from_slice(b"CO2\0"); // pad to 16
        let entry_size = 4 + 4 + 1 * 8;
        assert_eq!(stats_data_offset(&data, entry_size), Some(16));
    }

    #[test]
    fn test_stats_data_offset_too_short() {
        let data = vec![0u8; 10]; // Less than 12 bytes
        assert_eq!(stats_data_offset(&data, 16), None);
    }

    // ── parse_stats_entries ───────────────────────────────────────────────────

    #[test]
    fn test_parse_stats_entries_empty() {
        let entries = parse_stats_entries(&[], 1);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_stats_entries_single_output() {
        use byteorder::{LittleEndian, WriteBytesExt};
        let mut data = Vec::new();
        data.write_u32::<LittleEndian>(0xAABBCCDD).unwrap(); // uuid prefix
        data.write_u32::<LittleEndian>(1000).unwrap();
        data.write_f64::<LittleEndian>(23.5).unwrap();
        let entries = parse_stats_entries(&data, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].values, vec![23.5]);
    }

    #[test]
    fn test_parse_stats_entries_multi_output() {
        use byteorder::{LittleEndian, WriteBytesExt};
        let mut data = Vec::new();
        data.write_u32::<LittleEndian>(0xAABBCCDD).unwrap();
        data.write_u32::<LittleEndian>(5000).unwrap();
        data.write_f64::<LittleEndian>(100.0).unwrap();
        data.write_f64::<LittleEndian>(200.0).unwrap();
        data.write_f64::<LittleEndian>(300.0).unwrap();
        let entries = parse_stats_entries(&data, 3);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].values, vec![100.0, 200.0, 300.0]);
    }

    // ── abs_path ──────────────────────────────────────────────────────────────

    #[test]
    fn test_abs_path_with_leading_slash() {
        assert_eq!(abs_path("/temp/file.img"), "/temp/file.img");
    }

    #[test]
    fn test_abs_path_without_leading_slash() {
        assert_eq!(abs_path("temp/file.img"), "/temp/file.img");
    }

    #[test]
    fn test_abs_path_root() {
        assert_eq!(abs_path("/"), "/");
    }

    // ── completions ─────────────────────────────────────────────────────────

    #[test]
    fn test_completions_bash_generates_output() {
        let mut cmd = Cli::command();
        let mut buf = Vec::new();
        generate(Shell::Bash, &mut cmd, "lox", &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("_lox"),
            "bash completions should define _lox function"
        );
        assert!(
            output.contains("COMPREPLY"),
            "bash completions should use COMPREPLY"
        );
    }

    #[test]
    fn test_completions_zsh_generates_output() {
        let mut cmd = Cli::command();
        let mut buf = Vec::new();
        generate(Shell::Zsh, &mut cmd, "lox", &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("compdef"),
            "zsh completions should contain compdef"
        );
    }

    #[test]
    fn test_completions_fish_generates_output() {
        let mut cmd = Cli::command();
        let mut buf = Vec::new();
        generate(Shell::Fish, &mut cmd, "lox", &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("complete -c lox"),
            "fish completions should define completions for lox"
        );
    }

    #[test]
    fn test_detect_shell_from_env() {
        // detect_shell reads $SHELL — just verify it returns Some for known shells
        // or None for unknown ones (depends on test environment)
        let result = detect_shell();
        // In CI $SHELL may or may not be set; just ensure no panic
        let _ = result;
    }

    #[test]
    fn test_install_completions_creates_file() {
        let tmp = std::env::temp_dir().join("lox_test_completions");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Use LOX_HOME to override home directory for the test (works on all platforms)
        // SAFETY: Test runs serially; no other threads access this env var here.
        unsafe { std::env::set_var("LOX_HOME", tmp.to_str().unwrap()) };

        let mut cmd = Cli::command();

        if cfg!(windows) {
            let result = install_completions(Shell::PowerShell, &mut cmd);
            assert!(
                result.is_ok(),
                "install_completions should succeed for powershell"
            );
            let ps_file = tmp.join("Documents/PowerShell/lox_completions.ps1");
            assert!(
                ps_file.exists(),
                "powershell completion file should be created"
            );
        } else {
            let result = install_completions(Shell::Bash, &mut cmd);
            assert!(
                result.is_ok(),
                "install_completions should succeed for bash"
            );
            let bash_file = tmp.join(".local/share/bash-completion/completions/lox");
            assert!(bash_file.exists(), "bash completion file should be created");
            let content = fs::read_to_string(&bash_file).unwrap();
            assert!(
                content.contains("_lox"),
                "installed file should contain bash completions"
            );
        }

        unsafe { std::env::remove_var("LOX_HOME") };
        let _ = fs::remove_dir_all(&tmp);
    }
}
