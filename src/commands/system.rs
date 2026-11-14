use anyhow::{Context, Result, bail};
use clap::Subcommand;
use loxone_client::sdtest::{SdTestResult, parse_sdtest};
use loxone_client::system::PlcState;
use std::fs;
use std::io::{IsTerminal, Write};
use std::net::UdpSocket;
use std::path::PathBuf;
use std::time::Duration;

use crate::commands::RunContext;
use crate::config::Config;
use crate::util::{abs_path, bar, kb_fmt, xml_attr};

pub fn cmd_status(
    ctx: &RunContext,
    energy: bool,
    diag: bool,
    net: bool,
    bus: bool,
    lan: bool,
    all: bool,
) -> Result<()> {
    let show_diag = diag || all;
    let show_net = net || all;
    let show_bus = bus || all;
    let show_lan = lan || all;
    let lox = Config::load()?.client()?;
    let sys = lox.system_status()?;
    let check = lox.get_text("/dev/sys/check")?;
    let status = lox.get_text("/data/status")?;

    let ver = sys.firmware.as_str();
    let heap_val = sys.heap_raw.as_str();
    let conn = xml_attr(&check, "value").unwrap_or("?");

    let sps_label = match &sys.plc {
        PlcState::Running => "✓ Running",
        PlcState::Started => "Started",
        PlcState::Error => "⚠ Error",
        PlcState::Booting => "Booting",
        PlcState::Updating => "Updating",
        PlcState::Other(code) => code.as_str(),
    };
    let heap_display = match sys.heap {
        Some(h) => format!(
            "{} / {}  {}",
            kb_fmt(h.used_kb),
            kb_fmt(h.total_kb),
            bar(h.used_kb, h.total_kb.max(1.0))
        ),
        None => heap_val.to_string(),
    };

    let ms_name = xml_attr(&status, "Name").unwrap_or("Loxone");
    let ms_ip = xml_attr(&status, "IP").unwrap_or("?");
    let online = if xml_attr(&status, "Offline").unwrap_or("false") == "false" {
        "✓ Online"
    } else {
        "✗ Offline"
    };

    if ctx.json {
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
        let mut lox_mut = Config::load()?.client()?;
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
            println!("No energy meters found in structure (type 'Meter' or 'EnergyManager').");
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
        let d = lox.diagnostics().unwrap_or_default();
        let sd_info = lox.sdtest().unwrap_or_else(|_| parse_sdtest("?"));
        if ctx.json {
            let diag = serde_json::json!({
                "cpu": d.cpu, "sps": d.sps, "tasks": d.tasks,
                "context_switches": d.context_switches,
                "context_switches_idle": d.context_switches_i,
                "interrupts": d.interrupts,
                "comm_interrupts": d.comm_interrupts,
                "sd_card": sd_info.to_json(),
            });
            println!("{}", diag);
        } else {
            let num = |v: Option<f64>| v.map_or("—".to_string(), |n| format!("{n}"));
            let cpu = match (d.cpu, d.sps) {
                (Some(c), Some(s)) => format!("{c}% (PLC {s}%)"),
                (Some(c), None) => format!("{c}%"),
                _ => "—".to_string(),
            };
            println!("┌─ Diagnostics ───────────────────────────────────────");
            println!("│  CPU:              {}", cpu);
            println!("│  Tasks:            {}", num(d.tasks));
            for (label, v) in [
                ("Context switches", d.context_switches),
                ("Interrupts", d.interrupts),
                ("Comm interrupts", d.comm_interrupts),
                ("Ctx switches (i)", d.context_switches_i),
            ] {
                if let Some(v) = v {
                    println!("│  {:<17} {}", format!("{label}:"), v);
                }
            }
            print_sd_table(&sd_info);
            println!("└─────────────────────────────────────────────────────");
        }
    }
    if show_net {
        let n = lox.network();
        let dash = |v: &str| if v.is_empty() { "—" } else { v }.to_string();
        let rows = [
            ("ip", "IP", &n.ip),
            ("mac", "MAC", &n.mac),
            ("mask", "Mask", &n.mask),
            ("gateway", "Gateway", &n.gateway),
            ("dns", "DNS 1", &n.dns1),
            ("dns2", "DNS 2", &n.dns2),
            ("dhcp", "DHCP", &n.dhcp),
            ("ntp", "NTP", &n.ntp),
        ];
        if ctx.json {
            let obj: serde_json::Map<_, _> = rows
                .iter()
                .map(|(k, _, v)| (k.to_string(), dash(v).into()))
                .collect();
            println!("{}", serde_json::Value::Object(obj));
        } else {
            println!("┌─ Network ───────────────────────────────────────────");
            for (_, label, v) in rows {
                println!("│  {:<8} {}", format!("{label}:"), dash(v));
            }
            println!("└─────────────────────────────────────────────────────");
        }
    }
    if show_bus {
        let b = lox.bus_stats();
        if ctx.json {
            println!("{}", serde_json::to_value(&b)?);
        } else {
            println!("┌─ CAN Bus ───────────────────────────────────────────");
            for (label, v) in [
                ("Packets sent", b.packets_sent),
                ("Packets received", b.packets_received),
                ("Receive errors", b.receive_errors),
                ("Frame errors", b.frame_errors),
                ("Parity errors", b.parity_errors),
                ("Overruns", b.overruns),
            ] {
                println!("│  {:<17} {}", format!("{label}:"), count(v));
            }
            println!("└─────────────────────────────────────────────────────");
        }
    }
    if show_lan {
        let l = lox.lan_stats();
        if ctx.json {
            println!("{}", serde_json::to_value(&l)?);
        } else {
            println!("┌─ LAN Statistics ────────────────────────────────────");
            for (label, v) in [
                ("TX packets", l.tx_packets),
                ("TX errors", l.tx_errors),
                ("TX collisions", l.tx_collisions),
                ("TX underruns", l.tx_underruns),
                ("RX packets", l.rx_packets),
                ("RX overflow", l.rx_overflow),
                ("EOF errors", l.eof_errors),
                ("Exhaustion", l.exhaustion),
                ("No buffers", l.no_buffers),
            ] {
                println!("│  {:<14} {}", format!("{label}:"), count(v));
            }
            println!("└─────────────────────────────────────────────────────");
        }
    }
    Ok(())
}

/// A counter for the status tables: `—` when missing.
fn count(v: Option<u64>) -> String {
    v.map_or("—".to_string(), |n| n.to_string())
}

pub fn cmd_log(_ctx: &RunContext, lines: usize) -> Result<()> {
    let lox = Config::load()?.client()?;
    let log = lox.log()?;
    let all: Vec<&str> = log.lines().collect();
    for line in &all[all.len().saturating_sub(lines)..] {
        println!("{}", line);
    }
    Ok(())
}

pub fn cmd_time(ctx: &RunContext) -> Result<()> {
    let lox = Config::load()?.client()?;
    let status_xml = lox.get_text("/data/status")?;
    let datetime_val = xml_attr(&status_xml, "Modified").unwrap_or("?");
    let (date_val, time_val) = if datetime_val.contains(' ') {
        let mut parts = datetime_val.splitn(2, ' ');
        (parts.next().unwrap_or("?"), parts.next().unwrap_or("?"))
    } else {
        (datetime_val, "?")
    };
    if ctx.json {
        println!(
            "{}",
            serde_json::json!({"date": date_val, "time": time_val, "datetime": datetime_val})
        );
    } else {
        println!("Miniserver date: {}", date_val);
        println!("Miniserver time: {}", time_val);
    }
    Ok(())
}

pub fn cmd_discover(ctx: &RunContext, timeout: u64) -> Result<()> {
    println!("Scanning for Loxone Miniservers...");
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_secs(timeout)))?;
    socket.send_to(&[0x00], "255.255.255.255:7070")?;
    let mut buf = [0u8; 1024];
    let mut found = Vec::new();
    while let Ok((len, addr)) = socket.recv_from(&mut buf) {
        let msg = String::from_utf8_lossy(&buf[..len]);
        if ctx.json {
            found.push(serde_json::json!({
                "address": addr.to_string(),
                "response": msg.to_string(),
            }));
        } else {
            println!("  Found: {} — {}", addr, msg.trim());
        }
    }
    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&found)?);
    } else if found.is_empty() {
        println!("No Miniservers found. (Timeout: {}s)", timeout);
    }
    Ok(())
}

pub fn cmd_extensions(ctx: &RunContext) -> Result<()> {
    let lox = Config::load()?.client()?;
    let status_xml = lox.get_text("/data/status")?;

    let mut extensions: Vec<serde_json::Value> = Vec::new();

    use quick_xml::Reader;
    use quick_xml::events::Event;

    fn xattr(e: &quick_xml::events::BytesStart, name: &str) -> String {
        crate::xml::raw_attr(e, name).unwrap_or_default()
    }

    let mut reader = Reader::from_str(&status_xml);
    let mut buf = Vec::new();
    let mut parent_name: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                let tag = crate::xml::tag(e);
                match tag.as_str() {
                    "TreeBranch" => {
                        let name = xattr(e, "Name");
                        extensions.push(serde_json::json!({
                            "name": name,
                            "type": "Tree",
                            "serial": xattr(e, "Serial"),
                            "version": xattr(e, "Version"),
                            "online": true,
                            "devices": xattr(e, "Devices").parse::<u32>().unwrap_or(0),
                            "errors": xattr(e, "Errors").parse::<u32>().unwrap_or(0),
                        }));
                        parent_name = Some(name);
                    }
                    "Extension" => {
                        extensions.push(serde_json::json!({
                            "name": xattr(e, "Name"),
                            "type": xattr(e, "Type"),
                            "serial": xattr(e, "Serial"),
                            "version": xattr(e, "Version"),
                            "online": xattr(e, "Online") == "true",
                        }));
                    }
                    "GenericNetworkDevice" => {
                        let name = xattr(e, "Name");
                        extensions.push(serde_json::json!({
                            "name": name,
                            "type": xattr(e, "Type"),
                            "serial": xattr(e, "MAC"),
                            "version": xattr(e, "Version"),
                            "online": xattr(e, "Online") == "true",
                            "place": xattr(e, "Place"),
                        }));
                        parent_name = Some(name);
                    }
                    "TreeDevice" => {
                        extensions.push(serde_json::json!({
                            "name": xattr(e, "Name"),
                            "type": "Tree Device",
                            "serial": xattr(e, "Serial"),
                            "version": xattr(e, "Version"),
                            "online": xattr(e, "Online") == "true",
                            "place": xattr(e, "Place"),
                            "parent": parent_name,
                        }));
                    }
                    "AirDevice" => {
                        extensions.push(serde_json::json!({
                            "name": xattr(e, "Name"),
                            "type": xattr(e, "Type"),
                            "serial": xattr(e, "Serial"),
                            "version": xattr(e, "Version"),
                            "online": xattr(e, "Online") == "true",
                            "place": xattr(e, "Place"),
                            "battery": xattr(e, "Battery").parse::<u32>().ok(),
                            "parent": parent_name,
                        }));
                    }
                    "Plugin" => {
                        extensions.push(serde_json::json!({
                            "name": xattr(e, "Name"),
                            "type": format!("Plugin ({})", xattr(e, "Type")),
                            "serial": "",
                            "version": xattr(e, "Version"),
                            "online": xattr(e, "Online") == "true",
                        }));
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                anyhow::bail!("Failed to parse status XML: {}", e);
            }
            _ => {}
        }
        buf.clear();
    }

    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&extensions)?);
    } else if extensions.is_empty() {
        println!("No extensions found.");
    } else {
        let (top, devices): (Vec<_>, Vec<_>) = extensions.iter().partition(|e| {
            let t = e["type"].as_str().unwrap_or("");
            t == "Tree" || t.contains("Extension") || t.contains("Plugin") || t == "Intercom"
        });

        if !top.is_empty() {
            println!(
                "{:<36} {:<22} {:<12} {:<14} STATUS",
                "NAME", "TYPE", "SERIAL", "VERSION"
            );
            println!("{}", "─".repeat(96));
            for ext in &top {
                let online = ext["online"].as_bool().unwrap_or(false);
                let status = if online { "✓ online" } else { "✗ offline" };
                println!(
                    "{:<36} {:<22} {:<12} {:<14} {}",
                    ext["name"].as_str().unwrap_or("?"),
                    ext["type"].as_str().unwrap_or("?"),
                    ext["serial"].as_str().unwrap_or(""),
                    ext["version"].as_str().unwrap_or(""),
                    status,
                );
            }
        }

        if !devices.is_empty() {
            if !top.is_empty() {
                println!();
            }
            println!(
                "{:<36} {:<26} {:<20} {:<14} STATUS",
                "DEVICE", "TYPE", "PLACE", "VERSION"
            );
            println!("{}", "─".repeat(110));
            for dev in &devices {
                let online = dev["online"].as_bool().unwrap_or(false);
                let status = if online { "✓ online" } else { "✗ offline" };
                println!(
                    "{:<36} {:<26} {:<20} {:<14} {}",
                    dev["name"].as_str().unwrap_or("?"),
                    dev["type"].as_str().unwrap_or("?"),
                    dev["place"].as_str().unwrap_or(""),
                    dev["version"].as_str().unwrap_or(""),
                    status,
                );
            }
        }

        println!(
            "\n{} extensions/branches, {} devices",
            top.len(),
            devices.len()
        );
    }
    Ok(())
}

pub fn cmd_health(ctx: &RunContext, device_type: Option<String>, problems: bool) -> Result<()> {
    let lox = Config::load()?.client()?;
    let status_xml = lox.get_text("/data/status")?;

    use quick_xml::Reader;
    use quick_xml::events::Event;

    fn xattr(e: &quick_xml::events::BytesStart, name: &str) -> String {
        crate::xml::raw_attr(e, name).unwrap_or_default()
    }

    #[derive(Clone)]
    struct DeviceInfo {
        name: String,
        device_type: String,
        place: Option<String>,
        online: bool,
        battery: Option<u32>,
    }

    let mut devices: Vec<DeviceInfo> = Vec::new();
    let mut reader = Reader::from_str(&status_xml);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                let tag = crate::xml::tag(e);
                match tag.as_str() {
                    "TreeBranch" => {
                        devices.push(DeviceInfo {
                            name: xattr(e, "Name"),
                            device_type: "Tree".to_string(),
                            place: None,
                            online: true,
                            battery: None,
                        });
                    }
                    "Extension" => {
                        devices.push(DeviceInfo {
                            name: xattr(e, "Name"),
                            device_type: xattr(e, "Type"),
                            place: None,
                            online: xattr(e, "Online") == "true",
                            battery: None,
                        });
                    }
                    "GenericNetworkDevice" => {
                        let place = xattr(e, "Place");
                        devices.push(DeviceInfo {
                            name: xattr(e, "Name"),
                            device_type: xattr(e, "Type"),
                            place: if place.is_empty() { None } else { Some(place) },
                            online: xattr(e, "Online") == "true",
                            battery: None,
                        });
                    }
                    "TreeDevice" => {
                        let place = xattr(e, "Place");
                        devices.push(DeviceInfo {
                            name: xattr(e, "Name"),
                            device_type: "Tree Device".to_string(),
                            place: if place.is_empty() { None } else { Some(place) },
                            online: xattr(e, "Online") == "true",
                            battery: None,
                        });
                    }
                    "AirDevice" => {
                        let place = xattr(e, "Place");
                        devices.push(DeviceInfo {
                            name: xattr(e, "Name"),
                            device_type: xattr(e, "Type"),
                            place: if place.is_empty() { None } else { Some(place) },
                            online: xattr(e, "Online") == "true",
                            battery: xattr(e, "Battery").parse::<u32>().ok(),
                        });
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                anyhow::bail!("Failed to parse status XML: {}", e);
            }
            _ => {}
        }
        buf.clear();
    }

    if let Some(ref tf) = device_type {
        devices.retain(|d| d.device_type.to_lowercase().contains(&tf.to_lowercase()));
    }

    let bus_errors: Option<(String, String, String)> = {
        let b = lox.bus_stats();
        let errs = [b.receive_errors, b.frame_errors, b.overruns];
        errs.iter().any(Option::is_some).then(|| {
            let [r, f, o] = errs.map(|v| v.unwrap_or(0).to_string());
            (r, f, o)
        })
    };

    let total = devices.len();
    let mut warnings: Vec<&DeviceInfo> = Vec::new();
    let mut offline: Vec<&DeviceInfo> = Vec::new();
    let mut online_count = 0usize;

    for d in &devices {
        if !d.online {
            offline.push(d);
        } else if d.battery.is_some_and(|b| b < 20) {
            warnings.push(d);
        } else {
            online_count += 1;
        }
    }

    let warning_count = warnings.len();
    let offline_count = offline.len();

    if ctx.json {
        let device_json: Vec<serde_json::Value> = devices
            .iter()
            .map(|d| {
                let status = if !d.online {
                    "offline"
                } else if d.battery.is_some_and(|b| b < 20) {
                    "warning"
                } else {
                    "online"
                };
                let mut obj = serde_json::json!({
                    "name": d.name,
                    "type": d.device_type,
                    "status": status,
                    "online": d.online,
                });
                if let Some(ref place) = d.place {
                    obj["place"] = serde_json::json!(place);
                }
                if let Some(battery) = d.battery {
                    obj["battery"] = serde_json::json!(battery);
                }
                obj
            })
            .filter(|d| !problems || d["status"] == "warning" || d["status"] == "offline")
            .collect();

        let mut result = serde_json::json!({
            "total": total,
            "online": online_count,
            "warnings": warning_count,
            "offline": offline_count,
            "devices": device_json,
        });
        if let Some((ref re, ref fe, ref ov)) = bus_errors {
            result["bus"] = serde_json::json!({
                "receive_errors": re,
                "frame_errors": fe,
                "overruns": ov,
            });
        }
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "Device Health Report ({} device{})\n",
            total,
            if total == 1 { "" } else { "s" }
        );
        println!("  ✓ Online:  {}", online_count);
        println!("  ⚠ Warning: {}", warning_count);
        println!("  ✗ Offline: {}", offline_count);

        if let Some((ref re, ref fe, ref ov)) = bus_errors {
            let re_n: u64 = re.parse().unwrap_or(0);
            let fe_n: u64 = fe.parse().unwrap_or(0);
            let ov_n: u64 = ov.parse().unwrap_or(0);
            if re_n > 0 || fe_n > 0 || ov_n > 0 {
                println!("\nBUS ERRORS:");
                println!(
                    "  Tree CAN bus  Receive errors: {}  Frame errors: {}  Overruns: {}",
                    re, fe, ov
                );
            }
        }

        if !warnings.is_empty() {
            println!("\nWARNINGS:");
            for d in &warnings {
                let place_str = d
                    .place
                    .as_deref()
                    .map(|r| format!(" [{}]", r))
                    .unwrap_or_default();
                let mut details = Vec::new();
                details.push(format!("{:<12}", d.device_type));
                if let Some(bat) = d.battery {
                    details.push(format!("Battery: {}%", bat));
                }
                println!("  {}{:<30} {}", d.name, place_str, details.join("  "));
            }
        }

        if !offline.is_empty() {
            println!("\nOFFLINE:");
            for d in &offline {
                let place_str = d
                    .place
                    .as_deref()
                    .map(|r| format!(" [{}]", r))
                    .unwrap_or_default();
                println!("  {}{:<30} {:<12}", d.name, place_str, d.device_type);
            }
        }

        if !problems && warnings.is_empty() && offline.is_empty() {
            println!("\n✓ All devices healthy");
        }
    }
    Ok(())
}

#[derive(Subcommand)]
pub(crate) enum HwCmd {
    /// Compare the devices the Miniserver reports with the program file
    ///
    /// Reads `/data/status` (Tree, Air, extensions, network and 1-Wire
    /// devices) and the program file like `lox config` does (config repo or
    /// cached download; `--at`, `--file`, `--download`). Devices match by
    /// serial (MAC for network devices). Statuses: `serial-changed` (a
    /// replaced device: same name, other serial), `missing` (in the program,
    /// not listed), `not-in-program`, `offline`, `ok`.
    ///
    /// JSON fields: `config`, `devices` (`status`, `name`, `type`, `place`,
    /// `serial`, `program_serial`, `uuid`, `online`, `firmware`), `counts`.
    Status {
        /// Show only devices with a problem
        #[arg(long)]
        problems: bool,
        #[command(flatten)]
        src: crate::commands::config_insight::ConfigSource,
    },
}

pub fn cmd_hw(ctx: &RunContext, action: HwCmd) -> Result<()> {
    use crate::hardware::{self, HwStatus};
    match action {
        HwCmd::Status { problems, src } => {
            let (snap, _) = crate::commands::config_insight::load(&src)?;
            let doc = loxone_config::LoxoneDoc::parse(&snap.bytes)
                .with_context(|| format!("Cannot read the program file {}", snap.label))?;
            let live = hardware::read_live(&Config::load()?.client()?)?;
            let mut entries = hardware::compare(&live, &hardware::program_devices(&doc));
            let counts = hardware::counts(&entries);
            if problems {
                entries.retain(|e| e.status != HwStatus::Ok);
            }
            if ctx.json {
                let counts: serde_json::Map<String, serde_json::Value> = counts
                    .iter()
                    .map(|(s, n)| (s.key().to_string(), (*n).into()))
                    .collect();
                let v =
                    serde_json::json!({"config": snap.label, "devices": entries, "counts": counts});
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                print!(
                    "{}",
                    hw_table(&entries, &counts, &snap.label, !ctx.no_header)
                );
            }
        }
    }
    Ok(())
}

fn hw_table(
    entries: &[crate::hardware::HwEntry],
    counts: &std::collections::BTreeMap<crate::hardware::HwStatus, usize>,
    config: &str,
    header: bool,
) -> String {
    use crate::hardware::HwStatus;
    let dash = |v: &Option<String>| v.clone().unwrap_or_else(|| "—".into());
    let rows: Vec<[String; 6]> = entries
        .iter()
        .map(|e| {
            let serial = match (&e.program_serial, &e.serial) {
                (Some(old), Some(new)) if e.status == HwStatus::SerialChanged => {
                    format!("{old} → {new}")
                }
                (_, Some(s)) | (Some(s), None) => s.clone(),
                (None, None) => "—".into(),
            };
            [
                e.status.label().to_string(),
                e.name.clone(),
                e.kind.clone(),
                dash(&e.place),
                serial,
                dash(&e.firmware),
            ]
        })
        .collect();
    let heads = ["STATUS", "NAME", "TYPE", "PLACE", "SERIAL", "FIRMWARE"];
    let mut widths = heads.map(|h| h.chars().count());
    for r in &rows {
        for (w, c) in widths.iter_mut().zip(r) {
            *w = (*w).max(c.chars().count());
        }
    }
    let line = |cells: &[String]| -> String {
        let mut s = String::new();
        for (i, (c, w)) in cells.iter().zip(widths).enumerate() {
            if i + 1 == cells.len() {
                s.push_str(c);
            } else {
                s.push_str(&format!("{c:<w$}  "));
            }
        }
        s.trim_end().to_string() + "\n"
    };
    let mut out = format!("Program file: {config}\n\n");
    if header {
        out.push_str(&line(&heads.map(String::from)));
        out.push_str(&format!(
            "{}\n",
            "─".repeat(widths.iter().sum::<usize>() + 2 * (widths.len() - 1))
        ));
    }
    for r in &rows {
        out.push_str(&line(r));
    }
    let total: usize = counts.values().sum();
    let parts: Vec<String> = counts
        .iter()
        .map(|(s, n)| format!("{n} {}", s.label()))
        .collect();
    out.push_str(&format!("\n{total} devices: {}\n", parts.join(", ")));
    out
}

#[derive(Subcommand)]
pub(crate) enum UpdateCmd {
    /// Check for available firmware updates
    Check,
    /// Install the latest firmware release and wait until it runs (requires --yes)
    Install {
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
        /// Return right after starting the update instead of waiting for the new version
        #[arg(long)]
        no_wait: bool,
        /// Minutes to wait for the new version [default: 30]
        #[arg(long, default_value_t = 30, value_name = "MINUTES")]
        timeout: u64,
    },
}

/// Loxone's update server. It answers with the release, beta and test
/// versions on offer for one Miniserver.
const UPDATE_SERVER: &str = "https://update.loxone.com/updatecheck.xml";

pub fn cmd_update(ctx: &RunContext, action: UpdateCmd) -> Result<()> {
    match action {
        UpdateCmd::Check => {
            let cfg = Config::load()?;
            let client = cfg.client()?;
            let ver = client.value("/dev/cfg/version")?;
            if !ctx.json {
                println!("Current firmware: {}", ver);
            }
            let Some(serial) = update_serial(&cfg, &client) else {
                if ctx.json {
                    println!("{}", serde_json::json!({ "current": ver }));
                }
                eprintln!(
                    "Set the serial number for update checks: lox setup set --serial <serial>"
                );
                return Ok(());
            };
            let url = format!("{UPDATE_SERVER}?serial={serial}&version={ver}");
            let body = match reqwest::blocking::get(&url).and_then(|r| r.text()) {
                Ok(body) => body,
                Err(e) => {
                    if ctx.json {
                        println!("{}", serde_json::json!({ "current": ver }));
                    }
                    eprintln!("Could not check for updates: {:#}", anyhow::Error::from(e));
                    return Ok(());
                }
            };
            let check = UpdateCheck::parse(&body, &ver);
            let program = project_config_version();
            if ctx.json {
                let mut json = check.json(&ver);
                if let Some(p) = &program {
                    json["program_config_version"] = p.as_str().into();
                }
                println!("{json}");
            } else {
                println!("{}", check.text(&ver));
                for note in check.notes(&ver, program.as_deref(), today()) {
                    println!("{note}");
                }
            }
        }
        UpdateCmd::Install {
            yes,
            no_wait,
            timeout,
        } => {
            if !yes {
                bail!("Firmware update requires --yes flag. This will reboot your Miniserver!");
            }
            let cfg = Config::load()?;
            let lox = cfg.client()?;
            if ctx.dry_run {
                print_system_dry_run(ctx, "firmware update", "/jdev/sys/updatetolatestrelease");
                return Ok(());
            }
            let before = lox.value("/dev/cfg/version")?;
            let val = lox.update_firmware()?;
            if !ctx.json {
                println!(
                    "Update started on firmware {before}: {}",
                    if val.is_empty() { "ok" } else { &val }
                );
            }
            if no_wait {
                if ctx.json {
                    println!("{}", serde_json::json!({ "from": before, "started": true }));
                } else {
                    println!("The Miniserver downloads the firmware, then reboots.");
                }
                return Ok(());
            }
            let started = std::time::Instant::now();
            let after =
                wait_for_new_firmware(&cfg, &before, Duration::from_secs(timeout * 60), |s| {
                    if !ctx.json {
                        println!("{s}");
                    }
                })?;
            let minutes = started.elapsed().as_secs() / 60;
            if ctx.json {
                println!(
                    "{}",
                    serde_json::json!({ "from": before, "to": after, "minutes": minutes })
                );
            } else {
                println!("✓ Firmware {before} → {after} ({minutes} min). The program is running.");
                if let Some(note) = program_note(&after, project_config_version().as_deref()) {
                    println!("{note}");
                }
            }
        }
    }
    Ok(())
}

/// The serial for the update server: `lox setup`, else the project's
/// lox.toml, else the Miniserver's MAC (its serial).
fn update_serial(cfg: &Config, client: &loxone_client::LoxClient) -> Option<String> {
    if !cfg.serial.is_empty() {
        return Some(cfg.serial.clone());
    }
    if let Some(s) = project().and_then(|p| p.serial) {
        return Some(s);
    }
    let mac = client.value("/dev/cfg/mac").ok()?;
    let serial: String = mac.chars().filter(char::is_ascii_hexdigit).collect();
    (serial.len() == 12).then(|| serial.to_ascii_uppercase())
}

/// The Lox Logic project in the working directory, if there is one.
fn project() -> Option<loxone_config::Project> {
    let cwd = std::env::current_dir().ok()?;
    loxone_config::Project::load(&loxone_config::Project::find(&cwd)?).ok()
}

/// The Loxone Config version that saved the project's base, e.g. `17.2.8.28`.
fn project_config_version() -> Option<String> {
    let base = project()?.base;
    let mut head = vec![0u8; 4096];
    let n = std::io::Read::read(&mut fs::File::open(base).ok()?, &mut head).ok()?;
    let head = String::from_utf8_lossy(&head[..n]);
    let raw = xml_attr(&head, "ConfigVersion")?;
    // 17020828 → 17.2.8.28
    (raw.len() == 8 && raw.bytes().all(|b| b.is_ascii_digit())).then(|| {
        (0..4)
            .map(|i| {
                raw[i * 2..i * 2 + 2]
                    .parse::<u32>()
                    .unwrap_or(0)
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(".")
    })
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Polls until the Miniserver reports a firmware other than `before` and its
/// program runs. The Miniserver first downloads the firmware (several
/// minutes, still answering on the old version), then reboots.
fn wait_for_new_firmware(
    cfg: &Config,
    before: &str,
    timeout: Duration,
    mut progress: impl FnMut(&str),
) -> Result<String> {
    let deadline = std::time::Instant::now() + timeout;
    let mut last = String::new();
    loop {
        std::thread::sleep(Duration::from_secs(10));
        // A fresh client per poll: the reboot drops connections and tokens.
        let state = cfg
            .client()
            .ok()
            .and_then(|c| c.system_status().ok())
            .map(|s| (s.firmware, s.plc));
        let line = match &state {
            None => "Miniserver offline (rebooting)…".to_string(),
            Some((v, _)) if v == before => format!("Still on {v}, downloading…"),
            Some((v, plc)) => format!("Firmware {v}, program {plc:?}…"),
        };
        if line != last {
            progress(&line);
            last = line;
        }
        if let Some((v, plc)) = state
            && v != before
            && plc == PlcState::Running
        {
            return Ok(v);
        }
        if std::time::Instant::now() > deadline {
            bail!(
                "The Miniserver still runs {before} after {} minutes. Check `lox log` for an \
                 update error, or run `lox update install --yes` again.",
                timeout.as_secs() / 60
            );
        }
    }
}

/// Compares dotted versions numerically (`17.3.11.12` > `17.2.8.28`).
fn version_key(v: &str) -> Vec<u32> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Major and minor version, the part Loxone Config and firmware must share.
fn major_minor(v: &str) -> Vec<u32> {
    version_key(v).into_iter().take(2).collect()
}

/// What to do about the project's program files when they were saved by a
/// Loxone Config that does not match `firmware`.
fn program_note(firmware: &str, program: Option<&str>) -> Option<String> {
    let program = program?;
    (major_minor(program) != major_minor(firmware)).then(|| {
        format!(
            "⚠ This project's base was saved by Loxone Config {program}. For firmware {firmware}: \
             install the matching Loxone Config, load the program from the Miniserver, save it, \
             then download a new base."
        )
    })
}

/// The answer of Loxone's update server for one Miniserver.
#[derive(Debug, PartialEq)]
struct UpdateCheck {
    /// The release version on offer.
    available: Option<String>,
    update_available: bool,
    /// The Loxone Config installer for that version.
    config_download: Option<String>,
    /// Remote Connect stops working below this version after `deadline`.
    remote_connect: Option<(String, String)>,
}

impl UpdateCheck {
    fn parse(body: &str, current: &str) -> Self {
        let available = element_attr(body, "Release", "Version");
        let config_download = element_attr(body, "Release", "Path").map(str::to_string);
        let remote_connect = element_attr(body, "RemoteConnect", "MinVersion")
            .zip(element_attr(body, "RemoteConnect", "Deadline"))
            .map(|(v, d)| (v.to_string(), d.to_string()));
        UpdateCheck {
            update_available: available.is_some_and(|v| version_key(v) > version_key(current)),
            available: available.map(str::to_string),
            config_download,
            remote_connect,
        }
    }

    fn json(&self, current: &str) -> serde_json::Value {
        let mut json = serde_json::json!({
            "current": current,
            "available": self.available.as_deref().unwrap_or(current),
            "update_available": self.update_available,
        });
        if let Some(url) = &self.config_download {
            json["config_download"] = url.as_str().into();
        }
        if let Some((min, deadline)) = &self.remote_connect {
            json["remote_connect"] =
                serde_json::json!({ "min_version": min, "deadline": deadline });
        }
        json
    }

    fn text(&self, current: &str) -> String {
        match &self.available {
            Some(v) if self.update_available => format!("Update available: {v}"),
            Some(v) if version_key(v) < version_key(current) => {
                format!("✓ Firmware is newer than the release ({v})")
            }
            Some(_) => "✓ Firmware is up to date".to_string(),
            None => "Could not read the update server's answer".to_string(),
        }
    }

    /// Hints after the version line: the matching Loxone Config, a Remote
    /// Connect deadline, and program files saved by another Config version.
    fn notes(&self, current: &str, program: Option<&str>, today: String) -> Vec<String> {
        let mut notes = Vec::new();
        if let (Some(v), true) = (&self.available, self.update_available) {
            if let Some(url) = &self.config_download {
                notes.push(format!("Loxone Config {v}: {url}"));
            }
            if let Some(note) = program_note(v, program) {
                notes.push(note);
            }
        }
        if let Some((min, deadline)) = &self.remote_connect
            && version_key(current) < version_key(min)
            && today.as_str() <= deadline.as_str()
        {
            notes.push(format!(
                "⚠ Remote Connect needs firmware {min} or newer by {deadline}."
            ));
        }
        notes
    }
}

/// `attr` of the first `<element …>` in `xml`.
fn element_attr<'a>(xml: &'a str, element: &str, attr: &str) -> Option<&'a str> {
    let open = format!("<{element} ");
    let start = xml.find(&open)? + 1;
    let end = xml[start..].find('>')? + start;
    xml_attr(&xml[start..end], attr)
}

/// `--dry-run` for a system command: what would be requested.
fn print_system_dry_run(ctx: &RunContext, what: &str, path: &str) {
    if ctx.json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "dry_run": true,
                "would_execute": { "action": what, "request": path },
            })
        );
    } else if !ctx.quiet {
        println!("[dry-run] {} → {}", what, path);
    }
}

pub fn cmd_reboot(ctx: &RunContext, yes: bool) -> Result<()> {
    if !yes {
        bail!("Reboot requires --yes flag. This will restart your Miniserver!");
    }
    let lox = Config::load()?.client()?;
    if ctx.dry_run {
        print_system_dry_run(ctx, "reboot", "/jdev/sys/reboot");
        return Ok(());
    }
    let val = lox.reboot()?;
    println!(
        "Reboot initiated: {}",
        if val.is_empty() { "ok" } else { &val }
    );
    Ok(())
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

pub fn cmd_files(ctx: &RunContext, action: FilesCmd) -> Result<()> {
    let lox = Config::load()?.client()?;
    match action {
        FilesCmd::Ls { path } => {
            let listing = lox.get_text(&format!("/dev/fslist/{}", abs_path(&path)))?;
            println!("{}", listing);
        }
        FilesCmd::Get { path, save_as } => {
            let remote = abs_path(&path);
            let data = lox.get_bytes(&format!("/dev/fsget/{}", remote))?;
            if save_as.as_deref() == Some("-") {
                return write_stdout(&data);
            }
            let default_location = save_as.is_none();
            let out_path = PathBuf::from(save_as.unwrap_or_else(|| {
                let name = remote.rsplit('/').next().unwrap_or_default();
                if name.is_empty() { "download" } else { name }.to_string()
            }));
            fs::write(&out_path, &data)
                .with_context(|| format!("Cannot write {}", out_path.display()))?;
            let saved = std::path::absolute(&out_path).unwrap_or(out_path);
            if ctx.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "ok": true,
                        "path": remote,
                        "saved_as": saved,
                        "bytes": data.len(),
                    }))?
                );
            } else if !ctx.quiet {
                println!(
                    "✓  Downloaded {} → {} ({} bytes)",
                    remote,
                    saved.display(),
                    data.len()
                );
                // `lox files get X > file` would otherwise only capture this line
                if default_location && !std::io::stdout().is_terminal() {
                    eprintln!(
                        "Saved to {} (use --save-as - to write the file to stdout)",
                        saved.display()
                    );
                }
            }
        }
    }
    Ok(())
}

/// Raw bytes to stdout; a reader that went away early is a normal end, as
/// for the crate's `println!`.
fn write_stdout(data: &[u8]) -> Result<()> {
    let mut out = std::io::stdout().lock();
    match out.write_all(data).and_then(|()| out.flush()) {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        r => r.context("Cannot write to stdout"),
    }
}

fn print_sd_table(sd: &SdTestResult) {
    if sd.read_speed_kb.is_some() || sd.write_speed_kb.is_some() {
        let read = sd
            .read_speed_kb
            .map(|v| format!("{:.0} kB/s", v))
            .unwrap_or_else(|| "?".to_string());
        let write = sd
            .write_speed_kb
            .map(|v| format!("{:.0} kB/s", v))
            .unwrap_or_else(|| "?".to_string());
        println!("│  SD read:          {}", read);
        println!("│  SD write:         {}", write);

        let err_str = match (sd.error_type, sd.error_count) {
            (Some(0), _) => "✓ No errors".to_string(),
            (Some(t), Some(c)) => format!("⚠ type {} count {}", t, c),
            _ => "?".to_string(),
        };
        println!("│  SD errors:        {}", err_str);

        if let Some(usage) = sd.usage_pct {
            let warn = if usage > 60.0 { " ⚠" } else { "" };
            println!("│  SD usage:         {:.1}%{}", usage, warn);
        }
        if let Some(ref date) = sd.date {
            let mfr = sd
                .manufacturer_id
                .map(|id| format!(" (mfr {})", id))
                .unwrap_or_default();
            println!("│  SD manufactured:  {}{}", date, mfr);
        }
    } else {
        println!("│  SD card:          {}", sd.raw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{HwEntry, HwStatus, counts};

    /// The update server's answer, shortened (2026-11, signatures cut).
    const UPDATE_ANSWER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Miniserversoftware Version="10" certificate="…">
  <Release Version="17.3.11.12" Path="http://updatefiles.loxone.com/LoxConfig/LoxoneConfigSetup_17031112.zip" crc32="cb99aed6" Filesize="670495691" signature="…"/>
  <Beta Version="17.4.1.2" Path="http://updatefiles.loxone.com/LoxConfig/LoxoneConfigSetup_17040102.zip" crc32="0" Filesize="1" signature="…"/>
  <RemoteConnect MinVersion="17.3.11.10" AutoUpdateAvailable="15.0.0.0" Deadline="2026-12-01" Why="https://loxone.com/rc-update"/>
  <update Name="WebInterface" type="webif">
    <Release Version="20220803" Path="http://updatefiles.loxone.com/WebInterface/Release/x.zip"/>
  </update>
</Miniserversoftware>"#;

    #[test]
    fn update_check_reads_the_release_version() {
        let check = UpdateCheck::parse(UPDATE_ANSWER, "17.2.8.28");
        assert_eq!(check.available.as_deref(), Some("17.3.11.12"));
        assert!(check.update_available);
        assert_eq!(check.text("17.2.8.28"), "Update available: 17.3.11.12");
        assert_eq!(
            check.json("17.2.8.28"),
            serde_json::json!({
                "current": "17.2.8.28",
                "available": "17.3.11.12",
                "update_available": true,
                "config_download": "http://updatefiles.loxone.com/LoxConfig/LoxoneConfigSetup_17031112.zip",
                "remote_connect": {"min_version": "17.3.11.10", "deadline": "2026-12-01"},
            })
        );
    }

    #[test]
    fn update_check_compares_versions_numerically() {
        let same = UpdateCheck::parse(UPDATE_ANSWER, "17.3.11.12");
        assert!(!same.update_available);
        assert_eq!(same.text("17.3.11.12"), "✓ Firmware is up to date");

        // 17.10 is newer than 17.3, though "17.10" < "17.3" as text.
        let newer = UpdateCheck::parse(UPDATE_ANSWER, "17.10.1.1");
        assert!(!newer.update_available);
        assert_eq!(
            newer.text("17.10.1.1"),
            "✓ Firmware is newer than the release (17.3.11.12)"
        );

        let nothing = UpdateCheck::parse("<html/>", "17.2.8.28");
        assert!(!nothing.update_available);
        assert_eq!(nothing.json("17.2.8.28")["available"], "17.2.8.28");
    }

    #[test]
    fn update_check_notes_config_remote_connect_and_program_version() {
        let check = UpdateCheck::parse(UPDATE_ANSWER, "17.2.8.28");
        let notes = check.notes("17.2.8.28", Some("17.2.8.28"), "2026-11-14".into());
        assert_eq!(notes.len(), 3, "{notes:?}");
        assert!(notes[0].starts_with("Loxone Config 17.3.11.12: http"));
        assert!(notes[1].contains("saved by Loxone Config 17.2.8.28"));
        assert_eq!(
            notes[2],
            "⚠ Remote Connect needs firmware 17.3.11.10 or newer by 2026-12-01."
        );

        // Same major.minor: no program note. Past the deadline: no warning.
        let notes = check.notes("17.2.8.28", Some("17.3.1.1"), "2026-12-02".into());
        assert_eq!(notes.len(), 1, "{notes:?}");

        let current = UpdateCheck::parse(UPDATE_ANSWER, "17.3.11.12");
        assert!(
            current
                .notes("17.3.11.12", Some("17.2.8.28"), "2026-11-14".into())
                .is_empty()
        );
    }

    #[test]
    fn program_note_only_on_a_major_minor_change() {
        assert!(program_note("17.3.11.12", Some("17.3.1.1")).is_none());
        assert!(program_note("17.3.11.12", None).is_none());
        assert!(program_note("17.3.11.12", Some("17.2.8.28")).is_some());
    }

    fn entry(status: HwStatus, name: &str, serial: Option<&str>, program: Option<&str>) -> HwEntry {
        HwEntry {
            status,
            name: name.into(),
            kind: "TreeDevice".into(),
            place: None,
            serial: serial.map(String::from),
            program_serial: program.map(String::from),
            uuid: None,
            online: None,
            firmware: Some("15.0.1.1".into()),
        }
    }

    #[test]
    fn hw_table_shows_the_serial_change_and_the_counts() {
        let entries = [
            entry(
                HwStatus::SerialChanged,
                "Kitchen",
                Some("B0000002"),
                Some("B0000001"),
            ),
            entry(HwStatus::Missing, "Hall", None, Some("B0000003")),
            entry(HwStatus::Ok, "Bath", Some("B0000004"), Some("B0000004")),
        ];
        let text = hw_table(&entries, &counts(&entries), "base.Loxone", true);
        assert!(text.starts_with("Program file: base.Loxone\n\nSTATUS"));
        assert!(text.contains("B0000001 → B0000002"));
        assert!(text.contains("missing         Hall"));
        assert!(text.contains("B0000003"));
        assert!(text.ends_with("\n3 devices: 1 serial changed, 1 missing, 1 ok\n"));
        let bare = hw_table(&entries[2..], &counts(&entries[2..]), "x", false);
        assert!(!bare.contains("STATUS"));
        assert!(bare.contains("—"));
    }
}
