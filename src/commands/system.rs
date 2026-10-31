use anyhow::{Context, Result, bail};
use loxone_client::sdtest::{SdTestResult, parse_sdtest};
use loxone_client::system::PlcState;
use std::fs;
use std::io::{IsTerminal, Write};
use std::net::UdpSocket;
use std::path::PathBuf;
use std::time::Duration;

use crate::commands::RunContext;
use crate::config::Config;
use crate::otel;
use crate::util::{abs_path, bar, kb_fmt, xml_attr};
use crate::{FilesCmd, OtelCmd, UpdateCmd};

/// Extract the `value` attribute, returning `"—"` for missing or empty values.
fn nonempty_attr(s: &str) -> &str {
    match xml_attr(s, "value") {
        Some(v) if !v.is_empty() => v,
        _ => "—",
    }
}

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
        let sd = lox.get_text("/jdev/sys/sdtest").unwrap_or_default();
        let sd_val = xml_attr(&sd, "value").unwrap_or("?");
        let sd_info = parse_sdtest(sd_val);
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
        let ip = lox.get_text("/jdev/cfg/ip").unwrap_or_default();
        let mac = lox.get_text("/jdev/cfg/mac").unwrap_or_default();
        let mask = lox.get_text("/jdev/cfg/mask").unwrap_or_default();
        let gw = lox.get_text("/jdev/cfg/gateway").unwrap_or_default();
        let dns1 = lox.get_text("/jdev/cfg/dns1").unwrap_or_default();
        let dns2 = lox.get_text("/jdev/cfg/dns2").unwrap_or_default();
        let dhcp = lox.get_text("/jdev/cfg/dhcp").unwrap_or_default();
        let ntp = lox.get_text("/jdev/cfg/ntp").unwrap_or_default();
        let ip_val = nonempty_attr(&ip);
        let mac_val = nonempty_attr(&mac);
        let mask_val = nonempty_attr(&mask);
        let gw_val = nonempty_attr(&gw);
        let dns1_val = nonempty_attr(&dns1);
        let dns2_val = nonempty_attr(&dns2);
        let dhcp_val = nonempty_attr(&dhcp);
        let ntp_val = nonempty_attr(&ntp);
        if ctx.json {
            println!(
                "{}",
                serde_json::json!({
                    "ip": ip_val, "mac": mac_val, "mask": mask_val,
                    "gateway": gw_val, "dns": dns1_val, "dns2": dns2_val,
                    "dhcp": dhcp_val, "ntp": ntp_val,
                })
            );
        } else {
            println!("┌─ Network ───────────────────────────────────────────");
            println!("│  IP:      {}", ip_val);
            println!("│  MAC:     {}", mac_val);
            println!("│  Mask:    {}", mask_val);
            println!("│  Gateway: {}", gw_val);
            println!("│  DNS 1:   {}", dns1_val);
            println!("│  DNS 2:   {}", dns2_val);
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
        let perr = lox.get_text("/jdev/bus/parityerrors").unwrap_or_default();
        let sent_val = nonempty_attr(&sent);
        let recv_val = nonempty_attr(&recv);
        let rerr_val = nonempty_attr(&rerr);
        let ferr_val = nonempty_attr(&ferr);
        let over_val = nonempty_attr(&over);
        let perr_val = nonempty_attr(&perr);
        if ctx.json {
            println!(
                "{}",
                serde_json::json!({
                    "packets_sent": sent_val, "packets_received": recv_val,
                    "receive_errors": rerr_val, "frame_errors": ferr_val,
                    "overruns": over_val, "parity_errors": perr_val,
                })
            );
        } else {
            println!("┌─ CAN Bus ───────────────────────────────────────────");
            println!("│  Packets sent:     {}", sent_val);
            println!("│  Packets received: {}", recv_val);
            println!("│  Receive errors:   {}", rerr_val);
            println!("│  Frame errors:     {}", ferr_val);
            println!("│  Parity errors:    {}", perr_val);
            println!("│  Overruns:         {}", over_val);
            println!("└─────────────────────────────────────────────────────");
        }
    }
    if show_lan {
        let txp = lox.get_text("/jdev/lan/txp").unwrap_or_default();
        let txe = lox.get_text("/jdev/lan/txe").unwrap_or_default();
        let txc = lox.get_text("/jdev/lan/txc").unwrap_or_default();
        let txu = lox.get_text("/jdev/lan/txu").unwrap_or_default();
        let rxp = lox.get_text("/jdev/lan/rxp").unwrap_or_default();
        let rxo = lox.get_text("/jdev/lan/rxo").unwrap_or_default();
        let eof = lox.get_text("/jdev/lan/eof").unwrap_or_default();
        let exh = lox.get_text("/jdev/lan/exh").unwrap_or_default();
        let nob = lox.get_text("/jdev/lan/nob").unwrap_or_default();
        let txp_val = nonempty_attr(&txp);
        let txe_val = nonempty_attr(&txe);
        let txc_val = nonempty_attr(&txc);
        let txu_val = nonempty_attr(&txu);
        let rxp_val = nonempty_attr(&rxp);
        let rxo_val = nonempty_attr(&rxo);
        let eof_val = nonempty_attr(&eof);
        let exh_val = nonempty_attr(&exh);
        let nob_val = nonempty_attr(&nob);
        if ctx.json {
            println!(
                "{}",
                serde_json::json!({
                    "tx_packets": txp_val, "tx_errors": txe_val,
                    "tx_collisions": txc_val, "tx_underruns": txu_val,
                    "rx_packets": rxp_val, "rx_overflow": rxo_val,
                    "eof_errors": eof_val, "exhaustion": exh_val,
                    "no_buffers": nob_val,
                })
            );
        } else {
            println!("┌─ LAN Statistics ────────────────────────────────────");
            println!("│  TX packets:    {}", txp_val);
            println!("│  TX errors:     {}", txe_val);
            println!("│  TX collisions: {}", txc_val);
            println!("│  TX underruns:  {}", txu_val);
            println!("│  RX packets:    {}", rxp_val);
            println!("│  RX overflow:   {}", rxo_val);
            println!("│  EOF errors:    {}", eof_val);
            println!("│  Exhaustion:    {}", exh_val);
            println!("│  No buffers:    {}", nob_val);
            println!("└─────────────────────────────────────────────────────");
        }
    }
    Ok(())
}

pub fn cmd_log(_ctx: &RunContext, lines: usize) -> Result<()> {
    let lox = Config::load()?.client()?;
    let log = lox.get_text("/dev/fsget/log/def.log")?;
    if log.contains("<errorcode>403</errorcode>") || log.contains("<errorcode>401</errorcode>") {
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

    fn xattr(e: &quick_xml::events::BytesStart, name: &[u8]) -> String {
        e.attributes()
            .flatten()
            .find(|a| a.key.as_ref() == name)
            .map(|a| String::from_utf8_lossy(&a.value).to_string())
            .unwrap_or_default()
    }

    let mut reader = Reader::from_str(&status_xml);
    let mut buf = Vec::new();
    let mut parent_name: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match tag.as_str() {
                    "TreeBranch" => {
                        let name = xattr(e, b"Name");
                        extensions.push(serde_json::json!({
                            "name": name,
                            "type": "Tree",
                            "serial": xattr(e, b"Serial"),
                            "version": xattr(e, b"Version"),
                            "online": true,
                            "devices": xattr(e, b"Devices").parse::<u32>().unwrap_or(0),
                            "errors": xattr(e, b"Errors").parse::<u32>().unwrap_or(0),
                        }));
                        parent_name = Some(name);
                    }
                    "Extension" => {
                        extensions.push(serde_json::json!({
                            "name": xattr(e, b"Name"),
                            "type": xattr(e, b"Type"),
                            "serial": xattr(e, b"Serial"),
                            "version": xattr(e, b"Version"),
                            "online": xattr(e, b"Online") == "true",
                        }));
                    }
                    "GenericNetworkDevice" => {
                        let name = xattr(e, b"Name");
                        extensions.push(serde_json::json!({
                            "name": name,
                            "type": xattr(e, b"Type"),
                            "serial": xattr(e, b"MAC"),
                            "version": xattr(e, b"Version"),
                            "online": xattr(e, b"Online") == "true",
                            "place": xattr(e, b"Place"),
                        }));
                        parent_name = Some(name);
                    }
                    "TreeDevice" => {
                        extensions.push(serde_json::json!({
                            "name": xattr(e, b"Name"),
                            "type": "Tree Device",
                            "serial": xattr(e, b"Serial"),
                            "version": xattr(e, b"Version"),
                            "online": xattr(e, b"Online") == "true",
                            "place": xattr(e, b"Place"),
                            "parent": parent_name,
                        }));
                    }
                    "AirDevice" => {
                        extensions.push(serde_json::json!({
                            "name": xattr(e, b"Name"),
                            "type": xattr(e, b"Type"),
                            "serial": xattr(e, b"Serial"),
                            "version": xattr(e, b"Version"),
                            "online": xattr(e, b"Online") == "true",
                            "place": xattr(e, b"Place"),
                            "battery": xattr(e, b"Battery").parse::<u32>().ok(),
                            "parent": parent_name,
                        }));
                    }
                    "Plugin" => {
                        extensions.push(serde_json::json!({
                            "name": xattr(e, b"Name"),
                            "type": format!("Plugin ({})", xattr(e, b"Type")),
                            "serial": "",
                            "version": xattr(e, b"Version"),
                            "online": xattr(e, b"Online") == "true",
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

    fn xattr(e: &quick_xml::events::BytesStart, name: &[u8]) -> String {
        e.attributes()
            .flatten()
            .find(|a| a.key.as_ref() == name)
            .map(|a| String::from_utf8_lossy(&a.value).to_string())
            .unwrap_or_default()
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
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match tag.as_str() {
                    "TreeBranch" => {
                        devices.push(DeviceInfo {
                            name: xattr(e, b"Name"),
                            device_type: "Tree".to_string(),
                            place: None,
                            online: true,
                            battery: None,
                        });
                    }
                    "Extension" => {
                        devices.push(DeviceInfo {
                            name: xattr(e, b"Name"),
                            device_type: xattr(e, b"Type"),
                            place: None,
                            online: xattr(e, b"Online") == "true",
                            battery: None,
                        });
                    }
                    "GenericNetworkDevice" => {
                        let place = xattr(e, b"Place");
                        devices.push(DeviceInfo {
                            name: xattr(e, b"Name"),
                            device_type: xattr(e, b"Type"),
                            place: if place.is_empty() { None } else { Some(place) },
                            online: xattr(e, b"Online") == "true",
                            battery: None,
                        });
                    }
                    "TreeDevice" => {
                        let place = xattr(e, b"Place");
                        devices.push(DeviceInfo {
                            name: xattr(e, b"Name"),
                            device_type: "Tree Device".to_string(),
                            place: if place.is_empty() { None } else { Some(place) },
                            online: xattr(e, b"Online") == "true",
                            battery: None,
                        });
                    }
                    "AirDevice" => {
                        let place = xattr(e, b"Place");
                        devices.push(DeviceInfo {
                            name: xattr(e, b"Name"),
                            device_type: xattr(e, b"Type"),
                            place: if place.is_empty() { None } else { Some(place) },
                            online: xattr(e, b"Online") == "true",
                            battery: xattr(e, b"Battery").parse::<u32>().ok(),
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
        let rerr = lox.get_text("/jdev/bus/receiveerrors").ok();
        let ferr = lox.get_text("/jdev/bus/frameerrors").ok();
        let over = lox.get_text("/jdev/bus/overruns").ok();
        match (rerr, ferr, over) {
            (Some(r), Some(f), Some(o)) => {
                let rv = xml_attr(&r, "value").unwrap_or("0").to_string();
                let fv = xml_attr(&f, "value").unwrap_or("0").to_string();
                let ov = xml_attr(&o, "value").unwrap_or("0").to_string();
                Some((rv, fv, ov))
            }
            _ => None,
        }
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

pub fn cmd_update(ctx: &RunContext, action: UpdateCmd) -> Result<()> {
    match action {
        UpdateCmd::Check => {
            let lox = Config::load()?.client()?;
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
                        let new_ver = xml_attr(&body, "Firmware")
                            .or_else(|| xml_attr(&body, "version"))
                            .or_else(|| xml_attr(&body, "Version").filter(|v| v.contains('.')));
                        let update_available = xml_attr(&body, "Version")
                            .or_else(|| xml_attr(&body, "update"))
                            .map(|v| v != "0" && v != ver)
                            .unwrap_or(false);
                        let is_update_available =
                            update_available || new_ver.map(|v| v != ver).unwrap_or(false);
                        if ctx.json {
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
                println!("Set serial number for update checks: lox config set --serial <serial>");
            }
        }
        UpdateCmd::Install { yes } => {
            if !yes {
                bail!("Firmware update requires --yes flag. This will reboot your Miniserver!");
            }
            let lox = Config::load()?.client()?;
            let resp = lox.get_text("/jdev/sys/updatetolatestrelease")?;
            let val = xml_attr(&resp, "value").unwrap_or("?");
            println!("Update triggered: {}", val);
            println!("The Miniserver will reboot during the update process.");
        }
    }
    Ok(())
}

pub fn cmd_reboot(_ctx: &RunContext, yes: bool) -> Result<()> {
    if !yes {
        bail!("Reboot requires --yes flag. This will restart your Miniserver!");
    }
    let lox = Config::load()?.client()?;
    let resp = lox.get_text("/jdev/sys/reboot")?;
    let val = xml_attr(&resp, "value").unwrap_or("ok");
    println!("Reboot initiated: {}", val);
    Ok(())
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

pub fn cmd_otel(ctx: &RunContext, action: OtelCmd) -> Result<()> {
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
                ctx.quiet,
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
                ctx.quiet,
                delta,
                no_logs,
            )?;
        }
    }
    Ok(())
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
