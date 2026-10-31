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
mod inventory;
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

use anyhow::{Context, Result};
use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use commands::access::{AccessCmd, UserCmd};
use commands::config_cmd::{AliasCmd, CacheCmd, ConfigCmd, SceneCmd, SetupCmd, TokenCmd};
use commands::control::{InputCmd, LightCmd, MusicCmd};
use commands::ctx::CtxCmd;
use commands::energy::EnergyCmd;
use commands::inspect::AutopilotCmd;
use commands::system::{FilesCmd, OtelCmd, UpdateCmd};
use loxone_client::{client, ftp};
use mcp::McpCmd;
use std::fs;

/// Load config XML from a .Loxone file or extract from a .zip backup.
pub(crate) fn load_config_xml(path: &str) -> Result<Vec<u8>> {
    let data = fs::read(path).with_context(|| format!("Cannot read {}", path))?;
    if path.ends_with(".zip") {
        Ok(loxone_config::archive::extract_and_decompress(&data)?)
    } else {
        Ok(data)
    }
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
            commands::completions::cmd_completions(&ctx, shell, install)
        }
        Cmd::Schema { command } => commands::schema::cmd_schema(&ctx, command),
        Cmd::Mcp { action } => mcp::cmd_mcp(&ctx, action),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use client::is_uuid;

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
}
