// `print!`/`println!` for the whole crate: like std's, except that a closed
// stdout (`lox … | head -1`) ends the process quietly instead of panicking
// (#116). Tests keep std's macros so libtest still captures their output.
#[cfg(not(test))]
macro_rules! print {
    ($($arg:tt)*) => { $crate::print_stdout(format_args!($($arg)*)) };
}
#[cfg(not(test))]
macro_rules! println {
    () => { $crate::print_stdout(format_args!("\n")) };
    ($($arg:tt)*) => { $crate::print_stdout(format_args!("{}\n", format_args!($($arg)*))) };
}

mod actions;
mod app_catalogue;
mod commands;
mod config;
mod custom_changes;
mod energy;
mod gitops;
mod logic;
mod loxone_xml;
mod mcp;
mod nfc_learn;
mod otel;
mod scene;
mod snapshot;
#[cfg(feature = "tui")]
mod tui;
mod usermgmt;
mod util;
mod visu;

use anyhow::{Context, Result, bail};
use clap::{ArgAction, Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use dirs::home_dir;
use loxone_client::LoxClient;
use loxone_client::{client, ftp, stream};
use serde_json::Value;
use std::fs;
use util::{json_val_str, now_hms};

/// Load config XML from a .Loxone file or extract from a .zip backup.
pub(crate) fn load_config_xml(path: &str) -> Result<Vec<u8>> {
    let data = fs::read(path).with_context(|| format!("Cannot read {}", path))?;
    if path.ends_with(".zip") {
        Ok(loxone_config::archive::extract_and_decompress(&data)?)
    } else {
        Ok(data)
    }
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
  door, intercom, charger      Door locks, intercoms, EV chargers
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
  user, access                 Users and NFC Code Touch access (admin)
  otel                         Export metrics via OpenTelemetry (OTLP)

Configuration:
  setup, alias, scene          Connection settings, aliases, scenes
  cache, token                 Cache & auth token management
  config                       Loxone Config files (download/inspect/diff)
  logic                        Config as code: Lox Logic modules → .Loxone
  ctx                          Manage multiple Miniserver contexts
  completions                  Generate shell completions
  schema                       Command schema for AI agent discovery
  mcp                          MCP server for AI assistants (Claude, ChatGPT, …)

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
    /// Use a specific context (overrides active context)
    #[arg(long = "ctx", global = true)]
    context: Option<String>,
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
    ///
    /// Actions, and the command each one sends to the Jalousie:
    ///   up, open, full-up        all the way up            FullUp
    ///   down, close, full-down   all the way down          FullDown
    ///   stop                     stop moving               off
    ///   shade, auto              automatic shading         AutomaticDown
    ///   shade <0-100>            slat angle                manualLamella/<n>
    ///   pos <0-100>, <0-100>     to a position             manualPosition/<n>
    ///
    /// up and down are full moves: the blind runs to its end position unless it
    /// is stopped. They are not jog steps; for those send the raw commands, e.g.
    /// `lox send <blind> up` and then `lox send <blind> UpOff`.
    /// Positions: 0 = open, 100 = closed. Slats: 0 = horizontal, 100 = vertical.
    #[command(verbatim_doc_comment)]
    Blind {
        name_or_uuid: String,
        /// up | down | stop | shade | pos | <0-100> (--help lists what each one sends)
        action: String,
        /// Value for `shade` (slat angle) and `pos` (position), 0-100
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
    /// Control thermostat: temp <°C> | mode <auto|manual|auto-heat|…> | override <°C> [minutes]
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
        /// Include initial state snapshot (with --reconnect: again after every reconnect)
        #[arg(long)]
        initial: bool,
        /// Keep streaming across Miniserver reboots and dropped connections
        ///
        /// Waits until the Miniserver answers again (retries back off from 1 s to
        /// 30 s; gives up after 30 min), re-reads the structure and resumes the
        /// same output. The gap is reported on stderr and, with -o json, as
        /// {"event":"disconnected"} / {"event":"reconnected"} lines. States that
        /// changed while offline are printed after the reconnect.
        #[arg(long)]
        reconnect: bool,
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
    /// Energy meter readings, the energy flow, the meters ranked by power
    #[command(args_conflicts_with_subcommands = true)]
    Energy {
        #[arg(long, short = 'r')]
        room: Option<String>,
        /// Rank the meters by current power, highest first (N defaults to 10)
        #[arg(long, value_name = "N", num_args = 0..=1, default_missing_value = "10")]
        top: Option<usize>,
        #[command(subcommand)]
        action: Option<EnergyCmd>,
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
    ///
    /// A rule created or edited in the app can live only in the Miniserver's
    /// custom changes, not in the program file: `lox autopilot ls` and a config
    /// download don't see it, and uploading a config discards it (the Miniserver
    /// logs "Remove old custom changes"). Before an upload, check that
    /// `lox autopilot changed` is older than the download the upload is based on.
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
    /// Interactive terminal UI: rooms, live events, energy, system health
    #[cfg(feature = "tui")]
    #[command(
        long_about = "Interactive terminal UI: browse and switch controls by room, watch live events, \
energy flow, Miniserver health and all configured sites. Press ? inside for keys.\n\n\
Examples:\n  lox tui\n  lox tui --screen events\n  lox tui -r Kitchen\n  lox tui --read-only\n  lox tui --demo    # simulated house, no Miniserver needed"
    )]
    Tui {
        /// Start screen: home, rooms, events, energy, system, sites
        #[arg(long, value_name = "SCREEN")]
        screen: Option<String>,
        /// Open Rooms with this room selected
        #[arg(short = 'r', long)]
        room: Option<String>,
        /// Monitor only: no commands are sent to the Miniserver
        #[arg(long)]
        read_only: bool,
        /// Color theme: night, day, neon, mono
        #[arg(long)]
        theme: Option<String>,
        /// Icon set: plain (default) or nerd (needs a Nerd Font)
        #[arg(long, value_name = "SET")]
        icons: Option<String>,
        /// Do not capture the mouse (keeps the terminal's own text selection)
        #[arg(long)]
        no_mouse: bool,
        /// No animations (also: LOX_NO_MOTION=1)
        #[arg(long)]
        no_motion: bool,
        /// Simulated demo house — no Miniserver needed
        #[arg(long)]
        demo: bool,
    },
    /// Browse Miniserver filesystem
    Files {
        #[command(subcommand)]
        action: FilesCmd,
    },
    /// Users of the Miniserver: list, show (needs the right to manage users)
    User {
        #[command(subcommand)]
        action: UserCmd,
    },
    /// NFC Code Touch access: who may open what, the access log, permissions
    Access {
        #[command(subcommand)]
        action: AccessCmd,
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
    /// Config as code: Lox Logic modules compiled into the .Loxone config (`lox logic help`)
    #[command(alias = "loxl", disable_help_flag = true)]
    Logic {
        /// Lox Logic command and its arguments, passed through as-is: check, fmt,
        /// compile, test, rename, lint, decompile, adopt, diff, drift,
        /// observe, roundtrip
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Manage multiple Miniserver contexts
    Ctx {
        #[command(subcommand)]
        action: CtxCmd,
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
    /// Model Context Protocol server (MCP 2026-07-28) for AI assistants (Claude, ChatGPT, Cursor, …)
    #[command(
        after_help = "Examples:\n  lox mcp serve                 # stdio server (what MCP clients launch)\n  lox mcp config                # print config snippets for Claude Desktop / Claude Code\n  lox mcp tools --allow-risky   # list the tools a client would see\n  lox --ctx home mcp serve --read-only"
    )]
    Mcp {
        #[command(subcommand)]
        action: McpCmd,
    },
}

/// Safety policy of the MCP server, fixed at startup.
#[derive(Args, Clone, Copy)]
pub(crate) struct McpPolicy {
    /// Expose read tools only (no actions)
    #[arg(long, conflicts_with_all = ["allow_risky", "allow_raw"])]
    read_only: bool,
    /// Run high-risk actions (doors, gates, alarm, user changes, the `confirm:` list) without asking the user to confirm
    #[arg(long)]
    allow_risky: bool,
    /// Expose the send_command tool for raw Loxone commands
    #[arg(long)]
    allow_raw: bool,
}

impl McpPolicy {
    pub(crate) fn options(&self, dry_run: bool) -> mcp::ServerOptions {
        mcp::ServerOptions {
            read_only: self.read_only,
            allow_risky: self.allow_risky,
            allow_raw: self.allow_raw,
            dry_run,
        }
    }
}

#[derive(Subcommand)]
pub(crate) enum EnergyCmd {
    /// Energy flow now: production, consumption, grid import/export, battery
    Flow,
}

#[derive(Subcommand)]
pub(crate) enum McpCmd {
    /// Run the MCP server on stdio (launched by the MCP client)
    Serve {
        #[command(flatten)]
        policy: McpPolicy,
    },
    /// Print ready-to-paste MCP client configuration
    Config {
        #[command(flatten)]
        policy: McpPolicy,
    },
    /// List the tools the server exposes under a policy
    #[command(alias = "list")]
    Tools {
        #[command(flatten)]
        policy: McpPolicy,
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
    /// List the rules in the program file: active, and whether their JSON is valid
    ///
    /// Reads the config like `lox config` does (config repo or cached download;
    /// `--at`, `--file`, `--download`). Rules edited in the app since the last
    /// upload may differ: see `lox autopilot changed`.
    #[command(alias = "list")]
    Ls {
        #[command(flatten)]
        src: ConfigSource,
    },
    /// Show when a rule last ran (the Miniserver's rule history)
    State { name_or_uuid: String },
    /// When the rules were last changed on the Miniserver (e.g. edited in the app)
    ///
    /// Reads the live `changed` state of the structure's autopilot entry (unix
    /// seconds; 0 = never). `-o json` gives `{"changed": <unix>, "changed_at":
    /// <RFC 3339 or null>, "uuid": ...}` for scripts.
    ///
    /// The state also moves when the Miniserver writes its program for other
    /// reasons (a password changed in the app, say). `--explain` says what
    /// changed (read-only, FTP): the newest program file in /prog compared with
    /// the one before it (or with `--since FILE`), sorted into rules, users &
    /// access and other changes, plus the rules pending in /sys/custom, which
    /// an upload would delete. JSON adds `program`, `custom_changes` and
    /// `rules_changed` (true when either holds a rule change).
    Changed {
        /// Say what the newest program write and the pending custom changes hold
        #[arg(long)]
        explain: bool,
        /// Compare the newest program file with this config (.Loxone or ZIP,
        /// e.g. the download after the last push) instead of the one before it
        #[arg(long, requires = "explain", value_name = "FILE")]
        since: Option<String>,
    },
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
pub(crate) enum UserCmd {
    /// List the users with state and admin flag
    #[command(alias = "list")]
    Ls,
    /// Show a user: state, validity, groups, NFC tags, number of access codes
    Show {
        /// User name (exact, or a unique part of it) or UUID
        user: String,
    },
    /// List the user groups with their type (normal, full access, …)
    Groups,
    /// Pair NFC tags with a user or remove them
    #[command(subcommand)]
    Nfc(UserNfcCmd),
    /// Set a user's access code for NFC Code Touch keypads
    #[command(subcommand)]
    Code(UserCodeCmd),
}

#[derive(Subcommand)]
pub(crate) enum UserNfcCmd {
    /// Pair an NFC tag with a user (read the ID with `lox access learn`)
    Add {
        /// User name (exact, or a unique part of it) or UUID
        user: String,
        /// Tag ID: hex bytes, with or without spaces ("04 A1 B2 C3 D4 E5 F6")
        id: String,
        /// Name for the tag, e.g. "alice phone"
        name: String,
        /// Skip the confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Remove an NFC tag from a user
    #[command(alias = "remove")]
    Rm {
        /// User name (exact, or a unique part of it) or UUID
        user: String,
        /// The tag: its ID (any form) or its name
        tag: String,
        /// Skip the confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum UserCodeCmd {
    /// Set a user's access code (2-8 digits), replacing the current one
    ///
    /// The code is read from a hidden prompt (asked twice), or from the first
    /// line of stdin when stdin is not a terminal — never from the command
    /// line, where it would end up in the shell history.
    #[command(verbatim_doc_comment)]
    Set {
        /// User name (exact, or a unique part of it) or UUID
        user: String,
        /// Skip the confirmation prompt (required when the code comes from stdin)
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum AccessCmd {
    /// Access log of an NFC Code Touch: who opened when, denied attempts
    Log {
        /// NFC Code Touch: name (or a unique part of it), alias or UUID
        code_touch: String,
        /// Number of entries to show, newest last (0: all)
        #[arg(short = 'n', long, default_value = "50")]
        lines: usize,
    },
    /// Why may (or may not) a user open an NFC Code Touch?
    ///
    /// Joins the live user (state, groups, NFC tags, access codes) with the
    /// access rights in the program file, which is downloaded fresh from the
    /// Miniserver (read-only FTP) unless --file, --at or --cached says
    /// otherwise. Verdicts: granted, no-access-right, no-credential,
    /// user-disabled, unknown-full-access.
    #[command(verbatim_doc_comment)]
    Check {
        /// User name (exact, or a unique part of it) or UUID
        user: String,
        /// NFC Code Touch (default: every Code Touch in the program file)
        code_touch: Option<String>,
        #[command(flatten)]
        src: AccessConfigSource,
    },
    /// Users and groups the Miniserver lists as assigned to a control (live)
    Perms {
        /// Control name, `Name [room]`, alias or UUID
        control: String,
    },
    /// Read an NFC tag's ID at a Code Touch, and optionally pair it with a user
    ///
    /// Starts the Code Touch's learn mode (it shows a cyan running light);
    /// hold the tag, or the unlocked phone, to the reader. Prints the ID and
    /// whether the tag is known already. Learn mode always ends again, also
    /// on timeout and Ctrl-C. With --user (and --name) the tag is then paired
    /// with that user, after a confirmation.
    ///
    /// A secured Code Touch takes learn mode only as a secured command, signed
    /// with the visualization password of the user lox connects with: from
    /// --visu-password-file, LOX_VISU_PASSWORD, `visu_password_file:` in the
    /// config, or a hidden prompt.
    #[command(verbatim_doc_comment)]
    Learn {
        /// NFC Code Touch: name (or a unique part of it), alias or UUID
        code_touch: String,
        /// Pair the tag with this user (name or UUID)
        #[arg(long, requires = "name")]
        user: Option<String>,
        /// Name for the tag when pairing, e.g. "alice phone"
        #[arg(long, requires = "user")]
        name: Option<String>,
        /// How long to wait for a tag (e.g. 60s, 2m; at most 5m)
        #[arg(long, default_value = "60s")]
        timeout: String,
        /// Pair without asking
        #[arg(long, short = 'y')]
        yes: bool,
        /// File holding the visualization password (first line), for a secured Code Touch
        #[arg(long, value_name = "PATH")]
        visu_password_file: Option<std::path::PathBuf>,
    },
}

/// Where `lox access check` reads the access rights from.
#[derive(clap::Args, Debug, Clone, Default)]
pub(crate) struct AccessConfigSource {
    /// A config file (ZIP or .Loxone) instead of a fresh download
    #[arg(long, value_name = "PATH")]
    pub file: Option<String>,
    /// A snapshot of the config repo: commit, version (v273) or date (2026-08-25)
    #[arg(long, value_name = "SNAPSHOT", conflicts_with = "file")]
    pub at: Option<String>,
    /// The config repo's working copy or the cached download (no FTP; may be stale)
    #[arg(long, conflicts_with_all = ["file", "at"])]
    pub cached: bool,
}

#[derive(Subcommand)]
pub(crate) enum FilesCmd {
    /// List files at a path
    Ls {
        /// Path on the Miniserver filesystem
        #[arg(default_value = "/")]
        path: String,
    },
    /// Download a file (into the current directory unless --save-as is given)
    Get {
        /// Path on the Miniserver filesystem
        path: String,
        /// Local output path, `-` for stdout [default: the file's name, in the current directory]
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
pub(crate) enum CtxCmd {
    /// Add a new context
    Add {
        /// Context name
        name: String,
        /// Miniserver host URL
        #[arg(long)]
        host: String,
        /// Username
        #[arg(long)]
        user: String,
        /// Password
        #[arg(long)]
        pass: String,
        /// Miniserver serial number (names its config repo folder, OTel device.id)
        #[arg(long)]
        serial: Option<String>,
    },
    /// Switch the active context
    Use {
        /// Context name to activate
        name: String,
    },
    /// List all contexts (* = active)
    #[command(alias = "ls")]
    List,
    /// Show the current active context
    Current,
    /// Remove a context
    Remove {
        /// Context name to remove
        name: String,
    },
    /// Rename a context
    Rename {
        /// Current name
        old: String,
        /// New name
        new: String,
    },
    /// Initialize a project-local .lox/ directory
    Init {
        /// Miniserver host URL
        #[arg(long)]
        host: Option<String>,
        /// Username
        #[arg(long)]
        user: Option<String>,
        /// Password
        #[arg(long)]
        pass: Option<String>,
        /// Miniserver serial number
        #[arg(long)]
        serial: Option<String>,
    },
    /// Migrate flat config to a 'default' context
    Migrate,
    /// Shortcut: `lox ctx <name>` = `lox ctx use <name>`
    #[command(external_subcommand)]
    External(Vec<String>),
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
        /// File holding the visualization password for secured controls
        /// (e.g. ~/.lox/visu-password; stores the path, not the password; "" removes it)
        #[arg(long, value_name = "PATH")]
        visu_password_file: Option<String>,
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
    /// Compress a .Loxone XML file to sps0.LoxCC (the config inside a config ZIP)
    Compress {
        /// Path to a .Loxone XML file
        file: String,
        /// Write an upload ZIP instead: this downloaded config ZIP with only
        /// sps0.LoxCC replaced, verified to extract byte-identically to FILE
        #[arg(long, value_name = "ZIP")]
        template: Option<String>,
        /// Custom output filename (default: FILE with .LoxCC, or .zip with --template)
        #[arg(long, value_name = "PATH")]
        save_as: Option<String>,
    },
    /// Deploy a Lox Logic project: drift check, compile, stage, upload, program
    /// restart, verify (requires --force to upload)
    Push {
        /// Project directory (holds lox.toml)
        #[arg(default_value = ".")]
        project: String,
        /// Upload the staged config and restart the program, which loads it
        /// within seconds (as Loxone Config's save does)
        #[arg(long)]
        force: bool,
        /// Load the upload by rebooting the Miniserver instead (about a
        /// minute) — for changes the running system only takes on start
        #[arg(long)]
        reboot: bool,
        /// The base is a byte patch of this download (.Loxone); the live
        /// config must still be exactly it
        #[arg(long, value_name = "FILE")]
        patched_from: Option<String>,
        /// Delete managed blocks missing from the module (see `lox logic help`)
        #[arg(long)]
        allow_removals: bool,
        /// Accept a base saved by a new Loxone Config release (its exact version)
        #[arg(long, value_name = "VERSION")]
        accept_version: Option<String>,
        /// Where downloads and push artifacts go (default: PROJECT/backups)
        #[arg(long, value_name = "DIR")]
        backup_dir: Option<String>,
        /// Push even though the Miniserver holds custom changes (made in the
        /// app, the web UI or through the API, not saved by Loxone Config);
        /// loading the upload deletes them
        #[arg(long)]
        discard_custom_changes: bool,
    },
    /// Upload a config to the Miniserver via FTP (dangerous — requires --force)
    ///
    /// The ZIP goes to /prog/sps_new.zip, which the Miniserver loads on its
    /// next start (`lox reboot`). It then deletes its custom changes: what the
    /// app, the web UI or the API changed and Loxone Config has not saved (light
    /// moods, timers, rules). The upload lists them first.
    Upload {
        /// Path to a config ZIP file
        file: String,
        /// Confirm the upload
        #[arg(long)]
        force: bool,
    },
    /// List the users of a config file: NFC tags, access codes, groups and
    /// access rights (live users: `lox user ls`)
    Users {
        /// Path to a .Loxone XML file (from `lox config extract`) or a backup ZIP
        file: String,
    },
    /// List hardware devices from a .Loxone config file
    Devices {
        /// Path to a .Loxone XML file (from `lox config extract`)
        file: String,
    },
    /// What changed in the logic: blocks, parameters and wires, page by page
    ///
    /// OLD and NEW are config files (ZIP or .Loxone) or snapshots of the config
    /// repo: a commit (`abc1234`, `HEAD~1`), a version (`v273`) or a save date
    /// (`2026-08-25`). No arguments: the last change. One: from OLD to the
    /// newest config. `OLD..NEW` works too.
    #[command(verbatim_doc_comment)]
    Diff {
        /// Older config (file or snapshot)
        old: Option<String>,
        /// Newer config (file or snapshot)
        new: Option<String>,
    },
    /// List the logic pages, or show a page as Lox Logic source
    Show {
        /// Page to show as source (exact title or a unique part of it)
        #[arg(long, short)]
        page: Option<String>,
        /// Show every page as source
        #[arg(long, conflicts_with = "page")]
        all: bool,
        #[command(flatten)]
        src: ConfigSource,
    },
    /// What is wired to a block: inputs, outputs and parameters, with
    /// hardware shown as the device and room it lives in
    #[command(verbatim_doc_comment)]
    Wiring {
        /// Block name, `Name [room]` or UUID
        name: String,
        /// Room, page or device to disambiguate the name
        #[arg(short, long)]
        room: Option<String>,
        /// Follow the wires up to the sensors, down to the actuators, or both
        #[arg(long, value_enum)]
        trace: Option<TraceDir>,
        /// How many hops to follow with --trace
        #[arg(long, default_value = "4")]
        depth: usize,
        /// Show parameters that still have their default value too
        #[arg(long)]
        all_params: bool,
        #[command(flatten)]
        src: ConfigSource,
    },
    /// Search the config's blocks by name, type, device, room or page
    Find {
        /// Words to search for (all must match)
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        #[command(flatten)]
        src: ConfigSource,
    },
    /// Check the logic for dead blocks, unused inputs, broken references and
    /// blinds or gates whose motor outputs are wired to nothing
    Lint {
        #[command(flatten)]
        src: ConfigSource,
    },
    /// How a block changed across the snapshots of the config repo
    History {
        /// Block name, `Name [room]` or UUID
        name: String,
        /// Room, page or device to disambiguate the name
        #[arg(short, long)]
        room: Option<String>,
        /// Number of snapshots to look back
        #[arg(short = 'n', long, default_value = "50")]
        count: usize,
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
    ///
    /// Uploaded as /prog/sps_new.zip, which the Miniserver loads on its next
    /// start (`lox reboot`).
    Restore {
        /// Git commit hash to restore from
        commit: String,
        /// Confirm the upload (required — dangerous operation)
        #[arg(long)]
        force: bool,
    },
}

/// Which config a `lox config show/wiring/find/lint` looks at.
#[derive(clap::Args, Debug, Clone, Default)]
pub(crate) struct ConfigSource {
    /// A snapshot of the config repo: commit, version (v273) or date (2026-08-25)
    #[arg(long, value_name = "SNAPSHOT")]
    pub at: Option<String>,
    /// A config file (ZIP or .Loxone)
    #[arg(long, value_name = "PATH", conflicts_with = "at")]
    pub file: Option<String>,
    /// Read the newest backup from the Miniserver via FTP (read-only)
    #[arg(long, conflicts_with_all = ["at", "file"])]
    pub download: bool,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq)]
pub(crate) enum TraceDir {
    Up,
    Down,
    Both,
}

// ── Error envelope ────────────────────────────────────────────────────────────

/// Categorize an anyhow error into a machine-readable error code.
pub(crate) fn categorize_error(e: &anyhow::Error) -> &'static str {
    let msg = format!("{:#}", e);
    let lower = msg.to_lowercase();
    if lower.contains("no control matching") || lower.contains("no nfc code touch matching") {
        "control_not_found"
    } else if lower.contains("no user matching") {
        "user_not_found"
    } else if lower.contains("has no nfc tag") {
        "tag_not_found"
    } else if lower.contains("no nfc tag was read") {
        "learn_timeout"
    } else if lower.contains("already used by another user") {
        "code_in_use"
    } else if lower.starts_with("aborted") {
        "declined_by_user"
    } else if lower.contains("ambiguous user") {
        "ambiguous_user"
    } else if lower.contains("ambiguous") {
        "ambiguous_control"
    } else if lower.contains("config not found") {
        "config_not_found"
    } else if e
        .downcast_ref::<commands::push_users::UserChangesPending>()
        .is_some()
    {
        "user_changes_pending"
    } else if lower.contains("requires --yes") {
        "confirmation_required"
    } else if let Some(http_err) = util::http_status(e) {
        match http_err.status {
            401 => "unauthorized",
            403 => "forbidden",
            404 => "not_found",
            _ => "http_error",
        }
    } else if let Some(ll) = e.downcast_ref::<usermgmt::LlCodeError>() {
        match ll.code.as_str() {
            "401" => "unauthorized",
            "403" => "forbidden",
            "404" => "not_found",
            _ => "http_error",
        }
    } else if lower.contains("connection") || lower.contains("timeout") {
        "connection_error"
    } else {
        "error"
    }
}

/// Backs the crate's `print!`/`println!`: a reader that went away is a
/// normal end of output, not an error.
#[cfg(not(test))]
fn print_stdout(args: std::fmt::Arguments) {
    use std::io::Write;
    if let Err(e) = std::io::stdout().write_fmt(args) {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        panic!("failed printing to stdout: {e}");
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
    // Set context override before any Config::load() calls
    config::set_ctx_override(cli.context.clone());

    client::set_verbose(cli.verbose);
    let ctx = commands::RunContext {
        json: cli.output == OutputFormat::Json,
        quiet: cli.quiet,
        csv: cli.output == OutputFormat::Csv,
        dry_run: cli.dry_run,
        non_interactive: cli.non_interactive,
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
        Cmd::Energy {
            action: Some(EnergyCmd::Flow),
            ..
        } => commands::energy::cmd_flow(&ctx),
        Cmd::Energy { room, top, .. } => commands::energy::cmd_energy(&ctx, room, top),
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
            reconnect,
        } => commands::inspect::cmd_stream(&ctx, r#type, room, control, initial, reconnect),
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
        #[cfg(feature = "tui")]
        Cmd::Tui {
            screen,
            room,
            read_only,
            theme,
            icons,
            no_mouse,
            no_motion,
            demo,
        } => tui::run::run(tui::run::TuiArgs {
            screen,
            room,
            read_only,
            theme,
            icons,
            no_mouse,
            no_motion,
            no_color: cli.no_color,
            demo,
        }),
        Cmd::Files { action } => commands::system::cmd_files(&ctx, action),
        Cmd::User { action } => commands::access::cmd_user(&ctx, action),
        Cmd::Access { action } => commands::access::cmd_access(&ctx, action),
        Cmd::Otel { action } => commands::system::cmd_otel(&ctx, action),

        // ── Configuration commands ────────────────────────────────────
        Cmd::Setup { action } => commands::config_cmd::cmd_setup(&ctx, action),
        Cmd::Alias { action } => commands::config_cmd::cmd_alias(&ctx, action),
        Cmd::Scene { action } => commands::config_cmd::cmd_scene(&ctx, action),
        Cmd::Cache { action } => commands::config_cmd::cmd_cache(&ctx, action),
        Cmd::Token { action } => commands::config_cmd::cmd_token(&ctx, action),
        Cmd::Config { action } => commands::config_cmd::cmd_config(&ctx, action),
        Cmd::Logic { args } => {
            // Lox Logic exits 1 on findings (check errors, a diff, drift) and 2 on
            // usage errors; scripts rely on both.
            use std::process::ExitCode;
            let code = loxone_config::cli::main_with("lox logic", &args);
            if code != ExitCode::SUCCESS {
                use std::io::Write;
                let _ = std::io::stdout().flush();
                std::process::exit(if code == ExitCode::from(2) { 2 } else { 1 });
            }
            Ok(())
        }
        Cmd::Ctx { action } => commands::ctx::cmd_ctx(&ctx, action),
        Cmd::Completions { shell, install } => {
            commands::config_cmd::cmd_completions(&ctx, shell, install)
        }
        Cmd::Schema { command } => commands::config_cmd::cmd_schema(&ctx, command),
        Cmd::Mcp { action } => mcp::cmd_mcp(&ctx, action),
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
            "blind" => Some(vec![
                "up",
                "down",
                "full-up",
                "full-down",
                "stop",
                "shade",
                "shade <0-100>",
                "pos <0-100>",
            ]),
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

#[cfg(test)]
mod tests {
    use super::*;
    use client::is_uuid;

    // ── json_val_str (Gen2 numeric value handling) ─────────────────────────

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

    // ── weather parsing ─────────────────────────────────────────────────────

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
        use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
        use std::io::Cursor;
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
        use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
        use std::io::Cursor;
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
        let entry_size: usize = 4 + 4 + 8; // uuid(4) + ts(4) + 1×f64
        let aligned = header_end.div_ceil(entry_size) * entry_size;
        assert_eq!(aligned, 16);
    }

    #[test]
    fn test_stats_header_alignment_long_name() {
        // 21-byte UTF-8 name, header_end = 33, entry_size = 16 → aligned = 48
        let name = "Zähler Licht Vorraum"; // 21 UTF-8 bytes (ä = 2 bytes)
        assert_eq!(name.len(), 21);
        let header_end = 12 + name.len();
        let entry_size: usize = 4 + 4 + 8;
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

    // ── xml_attr ──────────────────────────────────────────────────────────────

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

    // ── kb_fmt ────────────────────────────────────────────────────────────────

    // ── lox_epoch / lox_timestamp_to_string ───────────────────────────────────

    // ── stats helpers ─────────────────────────────────────────────────────────

    // ── parse_stats_entries ───────────────────────────────────────────────────

    // ── abs_path ──────────────────────────────────────────────────────────────

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
