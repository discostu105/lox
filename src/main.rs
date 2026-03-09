mod client;
mod config;
mod ftp;
mod loxcc;
mod scene;
mod token;
mod ws;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
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

fn xml_attr<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    let key = format!("{}=\"", attr);
    let start = xml.find(&key)? + key.len();
    let end = xml[start..].find('"')? + start;
    Some(&xml[start..end])
}

fn print_resp(resp: &Value, json: bool, name: &str, cmd: &str) {
    if json {
        println!("{}", serde_json::to_string_pretty(resp).unwrap());
    } else {
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

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "lox", about = "Loxone Miniserver CLI", version)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
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
    /// Miniserver health
    Status {
        #[arg(long)]
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
    /// List controls
    Ls {
        #[arg(long)]
        r#type: Option<String>,
        #[arg(long)]
        room: Option<String>,
        #[arg(long)]
        values: bool,
        /// Filter by category name
        #[arg(long)]
        cat: Option<String>,
        /// Show only favorites
        #[arg(long)]
        favorites: bool,
    },
    /// List all rooms in the structure
    Rooms,
    /// List all categories
    Categories,
    /// Show detailed control info (sub-controls, states, moods, flags)
    Info {
        name_or_uuid: String,
        #[arg(long)]
        room: Option<String>,
    },
    /// Show global states (operating mode, sunrise/sunset, wind/rain warnings)
    Globals,
    /// List operating modes
    Modes,
    /// Get full state of a control
    Get {
        name_or_uuid: String,
        #[arg(long)]
        room: Option<String>,
    },
    /// Send raw command
    Send {
        name_or_uuid: String,
        command: String,
        #[arg(long)]
        room: Option<String>,
        /// Send as secured command (requires visualization password hash)
        #[arg(long)]
        secured: Option<String>,
    },
    /// Turn on
    On {
        name_or_uuid: String,
        #[arg(long)]
        room: Option<String>,
    },
    /// Turn off
    Off {
        name_or_uuid: String,
        #[arg(long)]
        room: Option<String>,
    },
    /// Momentary pulse
    Pulse {
        name_or_uuid: String,
        #[arg(long)]
        room: Option<String>,
    },
    /// Control blind: up | down | stop | shade [<0-100>] | pos <0-100>
    Blind {
        name_or_uuid: String,
        action: String,
        #[arg(allow_hyphen_values = true)]
        pos: Option<f64>,
        #[arg(long)]
        room: Option<String>,
    },
    /// Control light moods: plus | minus | off | <mood-id>
    Mood {
        name_or_uuid: String,
        /// Mood action: plus, minus, off, or numeric mood ID
        action: String,
        #[arg(long)]
        room: Option<String>,
    },
    /// Set dimmer level (0-100)
    Dimmer {
        name_or_uuid: String,
        /// Brightness level 0-100
        level: f64,
        #[arg(long)]
        room: Option<String>,
    },
    /// Control gate: open | close | stop
    Gate {
        name_or_uuid: String,
        /// Action: open, close, stop
        action: String,
        #[arg(long)]
        room: Option<String>,
    },
    /// Set color on ColorPickerV2 (hex RGB e.g. #FF0000 or hsv(h,s,v))
    Color {
        name_or_uuid: String,
        /// Color value: hex RGB (#FF0000) or hsv(h,s,v)
        value: String,
        #[arg(long)]
        room: Option<String>,
    },
    /// Show weather data
    Weather {
        /// Show 7-day forecast
        #[arg(long)]
        forecast: bool,
    },
    /// Discover Miniservers on the local network (UDP broadcast)
    Discover {
        /// Timeout in seconds
        #[arg(long, default_value = "3")]
        timeout: u64,
    },
    /// Control thermostat (IRoomControllerV2)
    Thermostat {
        name_or_uuid: String,
        /// Set comfort temperature
        #[arg(long)]
        temp: Option<f64>,
        /// Set operating mode: auto|manual|comfort|eco
        #[arg(long)]
        mode: Option<String>,
        /// Override temperature
        #[arg(long)]
        r#override: Option<f64>,
        /// Override duration in minutes
        #[arg(long, default_value = "60")]
        duration: u64,
        #[arg(long)]
        room: Option<String>,
    },
    /// Control alarm: arm | disarm | quit
    Alarm {
        name_or_uuid: String,
        /// Action: arm, disarm, quit/ack
        action: String,
        /// Arm without motion detection
        #[arg(long)]
        no_motion: bool,
        #[arg(long)]
        room: Option<String>,
    },
    /// Lock a control (admin, firmware v11.3.2.11+)
    Lock {
        name_or_uuid: String,
        /// Reason for locking
        #[arg(long, default_value = "locked via CLI")]
        reason: String,
        #[arg(long)]
        room: Option<String>,
    },
    /// Unlock a control
    Unlock {
        name_or_uuid: String,
        #[arg(long)]
        room: Option<String>,
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
        /// Output as CSV
        #[arg(long)]
        csv: bool,
        #[arg(long)]
        room: Option<String>,
    },
    /// Check for firmware updates
    Update {
        #[command(subcommand)]
        action: UpdateCmd,
    },
    /// Control music server zones
    Music {
        #[command(subcommand)]
        action: MusicCmd,
    },
    /// Reboot the Miniserver
    Reboot {
        /// Skip confirmation
        #[arg(long)]
        confirm: bool,
    },
    /// Browse Miniserver filesystem
    Files {
        #[command(subcommand)]
        action: FilesCmd,
    },
    /// List connected extensions and devices
    Extensions,
    /// Show Miniserver system date/time
    Time,
    /// Poll a control's state and print changes (Ctrl+C to stop)
    Watch {
        name_or_uuid: String,
        #[arg(long, default_value = "2", help = "Poll interval in seconds")]
        interval: u64,
    },
    /// Check state (exit 0=match, 1=no match)
    If {
        name_or_uuid: String,
        op: String,
        value: String,
        /// Room qualifier to disambiguate controls with the same name
        #[arg(long)]
        room: Option<String>,
    },
    /// Run a scene
    Run { scene: String },
    /// Manage scenes
    Scene {
        #[command(subcommand)]
        action: SceneCmd,
    },
    /// Fetch the Miniserver system log (tail)
    Log {
        #[arg(long, default_value = "50", help = "Number of lines to show")]
        lines: usize,
    },
    /// Set analog/virtual input value
    Set {
        /// Control name or UUID
        name_or_uuid: String,
        /// Value to send (numeric or text)
        value: String,
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
    /// List and inspect automatic rules (Automatik-Regel / Autopilot)
    Autopilot {
        #[command(subcommand)]
        action: AutopilotCmd,
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
enum AutopilotCmd {
    /// List all automatic rules
    List,
    /// Show when a rule last fired
    State { name_or_uuid: String },
}

#[derive(Subcommand)]
enum UpdateCmd {
    /// Check for available firmware updates
    Check,
    /// Install firmware update (requires confirmation)
    Install {
        /// Skip confirmation
        #[arg(long)]
        confirm: bool,
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
        #[arg(long)]
        output: Option<String>,
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
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        user: Option<String>,
        /// Password (or set LOX_PASS env var to avoid it appearing in the process table)
        #[arg(long, env = "LOX_PASS")]
        pass: Option<String>,
        #[arg(long)]
        serial: Option<String>,
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
    List,
}

#[derive(Subcommand)]
enum SceneCmd {
    /// List all saved scenes
    List,
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
        #[arg(short, long)]
        output: Option<String>,
        /// Also decompress LoxCC to XML
        #[arg(long)]
        extract: bool,
    },
    /// List available configs on the Miniserver
    List,
    /// Decompress a local config ZIP to .Loxone XML
    Extract {
        /// Path to a config ZIP file
        file: String,
        /// Custom output filename
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Upload a config to the Miniserver via FTP (dangerous — requires --force)
    Upload {
        /// Path to a config ZIP file
        file: String,
        /// Confirm the upload
        #[arg(long)]
        force: bool,
    },
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.cmd {
        Cmd::Setup { action } => match action {
            SetupCmd::Set {
                host,
                user,
                pass,
                serial,
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
                cfg.save()?;
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
                    println!("✓  alias '{}' → {}", name, uuid);
                }
                AliasCmd::Remove { name } => {
                    if cfg.aliases.remove(&name).is_some() {
                        cfg.save()?;
                        println!("✓  removed alias '{}'", name);
                    } else {
                        println!("No alias named '{}'", name);
                    }
                }
                AliasCmd::List => {
                    if cfg.aliases.is_empty() {
                        println!("No aliases. Add with: lox alias add <name> <uuid>");
                    } else {
                        let mut aliases: Vec<_> = cfg.aliases.iter().collect();
                        aliases.sort_by_key(|(k, _)| k.as_str());
                        println!("{:<24} UUID", "ALIAS");
                        println!("{}", "─".repeat(60));
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

            if cli.json {
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
                if cli.json {
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
                if cli.json {
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
                if cli.json {
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
                if cli.json {
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
            if cli.json {
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
                println!(
                    "{:<40} {:<24} {:<22} {:<20} UUID",
                    "NAME", "ROOM", "TYPE", "VALUE"
                );
                println!("{}", "─".repeat(140));
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
                println!("{:<40} {:<24} {:<22} UUID", "NAME", "ROOM", "TYPE");
                println!("{}", "─".repeat(120));
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
                let mut names: Vec<&str> = rooms
                    .values()
                    .filter_map(|r| r.get("name").and_then(|n| n.as_str()))
                    .collect();
                names.sort();
                for n in names {
                    println!("{}", n);
                }
            }
        }

        Cmd::Categories => {
            let mut lox = LoxClient::new(Config::load()?);
            let cats = lox.list_categories()?;
            if cli.json {
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

            if cli.json {
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
            if cli.json {
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
            if cli.json {
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

            if cli.json {
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
            let resp = if let Some(hash) = secured {
                lox.get_json(&format!("/jdev/sps/ios/{}/{}/{}", hash, uuid, command))?
            } else {
                lox.send_cmd(&uuid, &command)?
            };
            print_resp(&resp, cli.json, &name_or_uuid, &command);
        }
        Cmd::On { name_or_uuid, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let resp = lox.send_cmd(&uuid, "on")?;
            print_resp(&resp, cli.json, &name_or_uuid, "on");
        }
        Cmd::Off { name_or_uuid, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let resp = lox.send_cmd(&uuid, "off")?;
            print_resp(&resp, cli.json, &name_or_uuid, "off");
        }
        Cmd::Pulse { name_or_uuid, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let resp = lox.send_cmd(&uuid, "pulse")?;
            print_resp(&resp, cli.json, &name_or_uuid, "pulse");
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
            let resp = lox.send_cmd(&ctrl.uuid, cmd)?;
            print_resp(&resp, cli.json, &ctrl.name, cmd);
            if !cli.json {
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
            let resp = lox.send_cmd(&ctrl.uuid, cmd)?;
            if cli.json {
                print_resp(&resp, true, &ctrl.name, cmd);
            } else {
                println!("✓  {} → mood {}", ctrl.name, action);
                thread::sleep(Duration::from_millis(400));
                let xml = lox.get_all(&ctrl.uuid)?;
                let state = xml_attr(&xml, "value").unwrap_or_default();
                // State encodes active moods as a packed integer
                // 200002700 = off, other values = mood active
                let is_off = state.starts_with("200002700") || state == "0";
                println!(
                    "   State: {}  ({})",
                    state,
                    if is_off { "off" } else { "active" }
                );
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
            let resp = lox.send_cmd(&ctrl.uuid, &format!("{}", level))?;
            print_resp(&resp, cli.json, &ctrl.name, &format!("dim={}", level));
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
            let resp = lox.send_cmd(&ctrl.uuid, cmd)?;
            print_resp(&resp, cli.json, &ctrl.name, cmd);
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
            let resp = lox.send_cmd(&ctrl.uuid, &cmd)?;
            print_resp(&resp, cli.json, &ctrl.name, &cmd);
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

                if cli.json {
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
                if cli.json {
                    found.push(serde_json::json!({
                        "address": addr.to_string(),
                        "response": msg.to_string(),
                    }));
                } else {
                    println!("  Found: {} — {}", addr, msg.trim());
                }
            }
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&found)?);
            } else if found.is_empty() {
                println!("No Miniservers found. (Timeout: {}s)", timeout);
            }
        }

        Cmd::Thermostat {
            name_or_uuid,
            temp,
            mode,
            r#override,
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
            if let Some(t) = temp {
                let resp = lox.send_cmd(&ctrl.uuid, &format!("setComfortTemperature/{}", t))?;
                print_resp(&resp, cli.json, &ctrl.name, &format!("temp={}", t));
            } else if let Some(m) = mode {
                let lower = m.to_lowercase();
                let mode_id = match lower.as_str() {
                    "auto" | "automatic" => "0",
                    "manual" => "1",
                    "comfort" => "2",
                    "eco" | "economy" => "3",
                    "building-protection" | "building" => "4",
                    other => other,
                };
                let resp = lox.send_cmd(&ctrl.uuid, &format!("setOperatingMode/{}", mode_id))?;
                print_resp(&resp, cli.json, &ctrl.name, &format!("mode={}", m));
            } else if let Some(temp_override) = r#override {
                let resp = lox.send_cmd(
                    &ctrl.uuid,
                    &format!("override/{}/{}", temp_override, duration),
                )?;
                print_resp(
                    &resp,
                    cli.json,
                    &ctrl.name,
                    &format!("override={}°/{}min", temp_override, duration),
                );
            } else {
                // Show current thermostat state
                let xml = lox.get_all(&ctrl.uuid)?;
                let val = xml_attr(&xml, "value").unwrap_or("?");
                if cli.json {
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

        Cmd::Alarm {
            name_or_uuid,
            action,
            no_motion,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            if !matches!(ctrl.typ.as_str(), "Alarm") {
                bail!("'{}' is type '{}', not an Alarm", ctrl.name, ctrl.typ);
            }
            let cmd = match action.to_lowercase().as_str() {
                "arm" | "on" => {
                    if no_motion {
                        "delayedon/0"
                    } else {
                        "delayedon/1"
                    }
                }
                "disarm" | "off" => "off",
                "quit" | "ack" | "acknowledge" => "quit",
                other => bail!("Unknown alarm action '{}'. Use: arm, disarm, quit", other),
            };
            let resp = lox.send_cmd(&ctrl.uuid, cmd)?;
            print_resp(&resp, cli.json, &ctrl.name, cmd);
        }

        Cmd::Lock {
            name_or_uuid,
            reason,
            room,
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            let resp = lox.send_cmd(
                &ctrl.uuid,
                &format!("lockcontrol/1/{}", encode_path_value(&reason)),
            )?;
            print_resp(&resp, cli.json, &ctrl.name, "lock");
        }

        Cmd::Unlock { name_or_uuid, room } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve_with_room(&name_or_uuid, room.as_deref())?;
            let ctrl = lox.find_control(&uuid)?;
            let resp = lox.send_cmd(&ctrl.uuid, "unlockcontrol")?;
            print_resp(&resp, cli.json, &ctrl.name, "unlock");
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
            if cli.json {
                let arr: Vec<_> = stats_controls
                    .iter()
                    .map(|(n, u, t, r)| {
                        serde_json::json!({"name": n, "uuid": u, "type": t, "room": r})
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else {
                println!("{:<40} {:<22} {:<24} UUID", "NAME", "TYPE", "ROOM");
                println!("{}", "─".repeat(120));
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
            csv,
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
            let path = format!("/binstatisticdata/{}/{}", ctrl.uuid, period);
            let data = lox.get_bytes(&path)?;

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
                } else if !cli.json {
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
                if data.len() > 12 {
                    let value_count = cursor.read_u32::<LittleEndian>().unwrap_or(0);
                    let _control_type = cursor.read_u32::<LittleEndian>().unwrap_or(0);
                    let name_length = cursor.read_u32::<LittleEndian>().unwrap_or(0);
                    // Mask off version flag bit from valueCount
                    let _vc = value_count & 0x7FFF_FFFF;
                    // Skip name string
                    let skip = name_length as usize;
                    if cursor.position() as usize + skip <= data.len() {
                        cursor.set_position(cursor.position() + skip as u64);
                    }
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
                    } else if cli.json {
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
                if cli.json {
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
                            if cli.json {
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
            UpdateCmd::Install { confirm } => {
                if !confirm {
                    bail!("Firmware update requires --confirm flag. This will reboot your Miniserver!");
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
            };
            let url = format!("{}/zone/{}/{}", music_base, zone, cmd_path);
            match client.get(&url).send() {
                Ok(resp) => {
                    let body = resp.text().unwrap_or_default();
                    if cli.json {
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

        Cmd::Reboot { confirm } => {
            if !confirm {
                bail!("Reboot requires --confirm flag. This will restart your Miniserver!");
            }
            let lox = LoxClient::new(Config::load()?);
            let resp = lox.get_text("/jdev/sys/reboot")?;
            let val = xml_attr(&resp, "value").unwrap_or("ok");
            println!("Reboot initiated: {}", val);
        }

        Cmd::Files { action } => match action {
            FilesCmd::Ls { path } => {
                let lox = LoxClient::new(Config::load()?);
                let listing =
                    lox.get_text(&format!("/dev/fsget/{}", path.trim_start_matches('/')))?;
                println!("{}", listing);
            }
            FilesCmd::Get { path, output } => {
                let lox = LoxClient::new(Config::load()?);
                let data =
                    lox.get_bytes(&format!("/dev/fsget/{}", path.trim_start_matches('/')))?;
                let out_path = output
                    .unwrap_or_else(|| path.rsplit('/').next().unwrap_or("download").to_string());
                fs::write(&out_path, &data)?;
                println!("✓ Downloaded {} bytes → {}", data.len(), out_path);
            }
        },

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

            if cli.json {
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
            if cli.json {
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
            if !cli.json {
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

        Cmd::Run { scene } => {
            let s = Scene::load(&scene)?;
            let mut lox = LoxClient::new(Config::load()?);
            println!("▶  {}", s.name.as_deref().unwrap_or(&scene));
            if let Some(d) = &s.description {
                println!("   {}", d);
            }
            println!();
            for (i, step) in s.steps.iter().enumerate() {
                let uuid = match lox.resolve(&step.control) {
                    Ok(u) => u,
                    Err(e) => {
                        eprintln!("Step {}: {}", i + 1, e);
                        continue;
                    }
                };
                let resp = lox.send_cmd(&uuid, &step.cmd)?;
                print_resp(&resp, cli.json, &step.control, &step.cmd);
                if step.delay_ms > 0 {
                    thread::sleep(Duration::from_millis(step.delay_ms));
                }
            }
        }

        Cmd::Scene { action } => match action {
            SceneCmd::List => {
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
        } => {
            let mut lox = LoxClient::new(Config::load()?);
            let uuid = lox.resolve(&name_or_uuid)?;
            let resp = lox.send_cmd(&uuid, &encode_path_value(&value))?;
            let code = resp
                .pointer("/LL/Code")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let val = resp
                .pointer("/LL/value")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            if code == "200" {
                println!("✓  {} = {}", name_or_uuid, val);
            } else {
                bail!("Error {}: {}", code, val);
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
                if cli.json {
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
                AutopilotCmd::List => {
                    let rules = lox.list_autopilot_rules()?;
                    if rules.is_empty() {
                        println!("No autopilot rules found.");
                    } else if cli.json {
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
                    if cli.json {
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
                    if cli.json {
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
                        .danger_accept_invalid_certs(true)
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
        Cmd::Config { action } => {
            let cfg = Config::load()?;
            match action {
                ConfigCmd::List => {
                    let backups = ftp::list_backups(&cfg)?;
                    if backups.is_empty() {
                        println!("No configs found on the Miniserver.");
                    } else if cli.json {
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
                ConfigCmd::Download { output, extract } => {
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
                    let out_path = output.unwrap_or_else(|| newest.filename.clone());
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
                ConfigCmd::Extract { file, output } => {
                    let zip_data =
                        fs::read(&file).with_context(|| format!("Cannot read {}", file))?;
                    eprintln!("Extracting sps0.LoxCC...");
                    let xml = loxcc::extract_and_decompress(&zip_data)?;
                    let xml_path = output.unwrap_or_else(|| {
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
    }

    Ok(())
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
}
