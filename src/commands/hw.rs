//! `lox hw ls`, `lox hw search` and `lox hw identify`: the Miniserver's
//! device search (see [`crate::livelearn`]).

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use loxone_client::client::LoxClient;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::commands::RunContext;
use crate::config::Config;
use crate::livelearn::{self, Base, Clock, Found, Http, PairedDevice, Reply};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Tech {
    Tree,
    Air,
    Link,
    All,
}

impl Tech {
    fn names(self) -> Vec<&'static str> {
        match self {
            Tech::Tree => vec!["tree"],
            Tech::Air => vec!["air"],
            Tech::Link => vec!["link"],
            Tech::All => livelearn::TECHS.to_vec(),
        }
    }
}

pub const MAX_SEARCH_SECONDS: u64 = 600;
pub const MAX_IDENTIFY_SECONDS: u64 = 300;

/// The Miniserver over HTTP: one GET each, never retried.
struct LiveHttp<'a>(&'a LoxClient);

impl Http for LiveHttp<'_> {
    fn get(&mut self, path: &str) -> Result<Reply> {
        let (status, body) = self.0.get_once(path, None)?;
        Ok(livelearn::parse_reply(status, &body))
    }
}

/// Real time; Ctrl-C cancels the wait.
struct LiveClock {
    start: Instant,
    cancel: Arc<AtomicBool>,
}

impl Clock for LiveClock {
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
    fn sleep(&mut self, d: Duration) -> bool {
        let until = Instant::now() + d;
        while Instant::now() < until {
            if self.cancel.load(Ordering::SeqCst) {
                return false;
            }
            std::thread::sleep((until - Instant::now()).min(Duration::from_millis(100)));
        }
        !self.cancel.load(Ordering::SeqCst)
    }
}

/// Catch Ctrl-C: the first one sets the flag (lox then sends the stop
/// command), a second one ends lox at once.
fn on_ctrl_c() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let set = flag.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            let _ = ready_tx.send(());
            return;
        };
        rt.block_on(async move {
            let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());
            // the first poll installs the handler
            tokio::select! {
                biased;
                r = &mut ctrl_c => {
                    let _ = ready_tx.send(());
                    if r.is_err() { return; }
                    set.store(true, Ordering::SeqCst);
                }
                _ = async {} => { let _ = ready_tx.send(()); }
            }
            loop {
                ctrl_c.set(tokio::signal::ctrl_c());
                if ctrl_c.as_mut().await.is_err() {
                    return;
                }
                if set.swap(true, Ordering::SeqCst) {
                    std::process::exit(130);
                }
            }
        });
    });
    let _ = ready_rx.recv_timeout(Duration::from_secs(2));
    flag
}

fn read_json(lox: &LoxClient, path: &str) -> Result<Value> {
    let body = lox.get_text(path)?;
    let r = livelearn::parse_reply(200, &body);
    if r.code != 200 {
        bail!(
            "the Miniserver refused {path} (code {}). The user lox connects with needs the \
             rights Device management and Expert mode.",
            r.code
        );
    }
    serde_json::from_str(&r.value)
        .with_context(|| format!("unexpected answer from the Miniserver to {path}"))
}

/// The Miniservers and the technologies each offers.
fn miniservers(lox: &LoxClient) -> Result<Vec<(String, Vec<String>)>> {
    let ms = livelearn::parse_miniservers(&read_json(lox, livelearn::LIST_MINISERVERS)?);
    if ms.is_empty() {
        bail!(
            "the Miniserver lists no device search. It needs firmware 13.3 or later and a user \
             with the rights Device management and Expert mode."
        );
    }
    Ok(ms)
}

/// The paired devices of the given technologies, with room names.
fn paired_devices(lox: &mut LoxClient, techs: &[&str]) -> Result<Vec<PairedDevice>> {
    let mut out = Vec::new();
    for (ms, offered) in miniservers(lox)? {
        for tech in techs {
            if offered.iter().any(|t| t == tech) {
                let v = read_json(lox, &livelearn::listtree_cmd(&ms, tech, true))?;
                out.extend(livelearn::parse_devices(&v, tech));
            }
        }
    }
    // names are a nicety: the list stands without the structure file
    if let Ok(st) = lox.get_structure() {
        let rooms = livelearn::room_names(st);
        for d in &mut out {
            d.room_name = d.room.as_ref().and_then(|r| rooms.get(r)).cloned();
        }
    }
    Ok(out)
}

fn bases(lox: &LoxClient, tech: &str) -> Result<Vec<Base>> {
    let mut out = Vec::new();
    for (ms, offered) in miniservers(lox)? {
        if offered.iter().any(|t| t == tech) {
            let v = read_json(lox, &livelearn::listtree_cmd(&ms, tech, false))?;
            out.extend(livelearn::parse_bases(&v, tech));
        }
    }
    Ok(out)
}

fn print_json(v: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

fn table(heads: &[&str], rows: &[Vec<String>], header: bool) -> String {
    let mut widths: Vec<usize> = heads.iter().map(|h| h.chars().count()).collect();
    for r in rows {
        for (w, c) in widths.iter_mut().zip(r) {
            *w = (*w).max(c.chars().count());
        }
    }
    let line = |cells: &[String]| -> String {
        let mut s = String::new();
        for (c, w) in cells.iter().zip(&widths) {
            s.push_str(&format!("{c:<w$}  "));
        }
        s.trim_end().to_string() + "\n"
    };
    let mut out = String::new();
    if header {
        out.push_str(&line(
            &heads.iter().map(|h| h.to_string()).collect::<Vec<_>>(),
        ));
        out.push_str(&format!(
            "{}\n",
            "─".repeat(widths.iter().sum::<usize>() + 2 * (widths.len() - 1))
        ));
    }
    for r in rows {
        out.push_str(&line(r));
    }
    out
}

fn devices_table(devs: &[PairedDevice], header: bool) -> String {
    let dash = |v: &Option<String>| v.clone().unwrap_or_else(|| "—".into());
    let rows: Vec<Vec<String>> = devs
        .iter()
        .map(|d| {
            vec![
                d.tech.clone(),
                d.name.clone(),
                d.kind.clone(),
                d.serial.clone(),
                d.base.clone(),
                dash(&d.room_name.clone().or_else(|| d.room.clone())),
                match d.online {
                    Some(true) => "online".into(),
                    Some(false) => "offline".into(),
                    None => "—".into(),
                },
            ]
        })
        .collect();
    let heads = ["TECH", "NAME", "TYPE", "SERIAL", "BASE", "ROOM", "STATE"];
    format!("{}\n{} devices\n", table(&heads, &rows, header), devs.len())
}

pub fn cmd_ls(ctx: &RunContext, tech: Tech) -> Result<()> {
    let mut lox = Config::load()?.client()?;
    let devs = paired_devices(&mut lox, &tech.names())?;
    if ctx.json {
        return print_json(&devs);
    }
    print!("{}", devices_table(&devs, !ctx.no_header));
    Ok(())
}

fn check_seconds(seconds: u64, max: u64) -> Result<Duration> {
    if seconds == 0 || seconds > max {
        bail!("--seconds must be between 1 and {max}");
    }
    Ok(Duration::from_secs(seconds))
}

/// The bases a search target names: every base of a technology, or one by
/// serial.
fn search_bases(lox: &LoxClient, target: &str) -> Result<Vec<Base>> {
    match target.to_ascii_lowercase().as_str() {
        t @ ("tree" | "air") => {
            let b = bases(lox, t)?;
            if b.is_empty() {
                bail!("the Miniserver has no {t} base to search on");
            }
            Ok(b)
        }
        _ => {
            livelearn::check_serial(target)?;
            let key = livelearn::normalize_serial(target);
            let all: Vec<Base> = ["tree", "air", "link"]
                .iter()
                .map(|t| bases(lox, t))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect();
            match all.iter().find(|b| {
                b.serial.eq_ignore_ascii_case(target)
                    || livelearn::normalize_serial(&b.serial) == key
            }) {
                Some(b) => Ok(vec![b.clone()]),
                None => {
                    let known: Vec<String> = all
                        .iter()
                        .filter(|b| b.tech != "link")
                        .map(|b| format!("{} ({})", b.serial, b.name))
                        .collect();
                    bail!(
                        "'{target}' is no search base. Search on `tree`, `air` or one of: {}",
                        known.join(", ")
                    )
                }
            }
        }
    }
}

pub fn cmd_search(ctx: &RunContext, target: &str, seconds: u64) -> Result<()> {
    let duration = check_seconds(seconds, MAX_SEARCH_SECONDS)?;
    let lox = Config::load()?.client()?;
    let bases = search_bases(&lox, target)?;

    if ctx.dry_run {
        let mut would_send = Vec::new();
        for b in &bases {
            would_send.push(livelearn::search_cmd(&b.serial));
            would_send.push(format!(
                "{} every {} s, {} every {} s, for {} s",
                livelearn::KEEPALIVE_CMD,
                livelearn::KEEPALIVE.as_secs(),
                livelearn::RESULTS,
                livelearn::POLL.as_secs(),
                duration.as_secs()
            ));
            would_send.push(livelearn::STOP.to_string());
        }
        if ctx.json {
            return print_json(&serde_json::json!({ "bases": bases, "would_send": would_send }));
        }
        for c in would_send {
            println!("[dry-run] would send: {c}");
        }
        return Ok(());
    }

    let cancel = on_ctrl_c();
    let mut clock = LiveClock {
        start: Instant::now(),
        cancel: cancel.clone(),
    };
    let mut http = LiveHttp(&lox);
    let mut done = Vec::new();
    let header = !ctx.no_header;
    let mut printed_header = false;
    for b in &bases {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        if !ctx.quiet {
            eprintln!(
                "▶ Searching {} {} ({}) for {} s; Ctrl-C ends the search…",
                b.tech,
                b.serial,
                b.name,
                duration.as_secs()
            );
        }
        clock.start = Instant::now();
        let s = livelearn::search(&mut http, &mut clock, &b.serial, duration, |f: &Found| {
            if ctx.json {
                return;
            }
            if header && !printed_header {
                println!(
                    "{:<26} {:<24} {:<10} CAPABILITIES",
                    "SERIAL", "TYPE", "BASE"
                );
                printed_header = true;
            }
            println!(
                "{:<26} {:<24} {:<10} {}",
                f.serial,
                f.kind,
                f.base.as_deref().unwrap_or(&b.serial),
                f.capabilities.map_or("—".into(), |c| c.to_string())
            );
        })?;
        for w in &s.warnings {
            eprintln!("warning: {w}");
        }
        done.push(s);
    }
    if ctx.json {
        return print_json(&done);
    }
    let total: usize = done.iter().map(|s| s.found.len()).sum();
    if !ctx.quiet {
        let how = if done.iter().any(|s| s.cancelled) {
            "Search cancelled"
        } else {
            "Search done"
        };
        eprintln!("✓ {how}: {total} devices found");
    }
    Ok(())
}

/// The paired device with this serial, from every technology.
fn find_device(lox: &mut LoxClient, serial: &str) -> Result<PairedDevice> {
    let key = livelearn::normalize_serial(serial);
    let devs = paired_devices(lox, &livelearn::TECHS)?;
    devs.into_iter()
        .find(|d| livelearn::normalize_serial(&d.serial) == key)
        .with_context(|| {
            format!("no paired device has the serial {serial}. `lox hw ls` lists them.")
        })
}

pub fn cmd_identify(ctx: &RunContext, serial: &str, seconds: u64) -> Result<()> {
    let duration = check_seconds(seconds, MAX_IDENTIFY_SECONDS)?;
    livelearn::check_serial(serial)?;
    let mut lox = Config::load()?.client()?;
    let dev = find_device(&mut lox, serial)?;

    if ctx.dry_run {
        let would_send = [
            livelearn::identify_cmd(&dev.base, &dev.serial),
            livelearn::identify_cmd(&dev.base, "0"),
            livelearn::IDENTIFY_STOP.to_string(),
        ];
        if ctx.json {
            return print_json(&serde_json::json!({ "device": dev, "would_send": would_send }));
        }
        for c in would_send {
            println!("[dry-run] would send: {c}");
        }
        return Ok(());
    }

    if !ctx.quiet {
        eprintln!(
            "▶ Identifying {} ({} {}) for {} s; Ctrl-C ends it…",
            dev.name,
            dev.kind,
            dev.serial,
            duration.as_secs()
        );
    }
    let mut clock = LiveClock {
        start: Instant::now(),
        cancel: on_ctrl_c(),
    };
    let i = livelearn::identify(
        &mut LiveHttp(&lox),
        &mut clock,
        &dev.base,
        &dev.serial,
        duration,
    )?;
    for w in &i.warnings {
        eprintln!("warning: {w}");
    }
    if ctx.json {
        return print_json(&serde_json::json!({ "device": dev, "identify": i }));
    }
    if !ctx.quiet {
        eprintln!("✓ Identify of {} ended", dev.name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::livelearn::tests::{AIR_DEVS, TREE_DEVS};

    #[test]
    fn the_device_table_shows_room_names_and_state() {
        let mut devs = livelearn::parse_devices(&serde_json::from_str(TREE_DEVS).unwrap(), "tree");
        devs.extend(livelearn::parse_devices(
            &serde_json::from_str(AIR_DEVS).unwrap(),
            "air",
        ));
        devs[0].room_name = Some("Kitchen".into());
        let t = devices_table(&devs, true);
        let lines: Vec<&str> = t.lines().collect();
        assert!(lines[0].starts_with("TECH"), "{t}");
        assert!(
            lines[2].contains("Kitchen") && lines[2].ends_with("online"),
            "{t}"
        );
        assert!(lines[4].ends_with("offline"), "{t}");
        assert!(t.contains("50:4F:94:FF:FE:00:00:01  0C000A01"), "{t}");
        assert!(t.ends_with("\n5 devices\n"), "{t}");
        assert!(!devices_table(&devs, false).starts_with("TECH"));
    }

    #[test]
    fn seconds_are_bounded() {
        assert!(check_seconds(0, 600).is_err());
        assert!(check_seconds(601, 600).is_err());
        assert_eq!(check_seconds(60, 600).unwrap(), Duration::from_secs(60));
    }
}
