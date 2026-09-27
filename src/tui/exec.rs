//! Effect executors (§8.3): the live Miniserver and the `--demo` simulator.
//!
//! Everything slow runs off the UI thread and reports back as a [`Msg`] tagged
//! with the epoch it was started under; `update` drops results from an older
//! epoch (context switch, structure refresh).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::runtime::Handle;
use tokio::sync::watch;

use super::app::{App, Commit, Conn, Effect, LogMsg, Msg, PollKind, Polled, ToastKind, WiringDoc};
use super::data::{self, BusLan, Diag, MsInfo, Series, SiteStatus};
use super::demo;
use super::model::{Cid, House, Role};
use crate::client::LoxClient;
use crate::config::{Config, GlobalConfig};
use crate::logic::Logic;
use crate::stream::StateEvent;

static EPOCH: AtomicU64 = AtomicU64::new(1);

/// A fresh epoch (context switch, structure refresh).
pub fn next_epoch() -> u64 {
    EPOCH.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Statistics timestamps count seconds of *local wall time* since 2009-01-01;
/// convert with today's UTC offset (off by an hour only across a DST switch).
fn stats_epoch_unix() -> i64 {
    1_230_768_000 - tz_offset()
}

/// Local hour of day (fractional).
pub fn local_hour() -> f64 {
    use chrono::Timelike;
    let t = chrono::Local::now();
    t.hour() as f64 + t.minute() as f64 / 60.0 + t.second() as f64 / 3600.0
}

pub fn tz_offset() -> i64 {
    chrono::Local::now().offset().local_minus_utc() as i64
}

pub fn build_house(st: &Value, roles: &BTreeMap<String, String>) -> House {
    let mut h = House::from_structure(st);
    h.apply_role_overrides(roles);
    h
}

// ── Backends ────────────────────────────────────────────────────────────────

pub enum Backend {
    Live(Box<Live>),
    Demo(Box<Demo>),
}

impl Backend {
    pub fn start(&mut self, app: &App) {
        match self {
            Backend::Live(l) => l.start_stream(app.epoch, app.house.last_modified.clone()),
            Backend::Demo(d) => d.start(app),
        }
    }

    pub fn handle(&mut self, e: Effect, app: &App) {
        match self {
            Backend::Live(l) => l.handle(e, app),
            Backend::Demo(d) => d.handle(e, app),
        }
    }

    pub fn stop(&mut self) {
        if let Backend::Live(l) = self
            && let Some(s) = l.stop.take()
        {
            let _ = s.send(true);
        }
    }

    pub fn cfg(&self) -> Option<&Config> {
        match self {
            Backend::Live(l) => Some(&l.cfg),
            Backend::Demo(_) => None,
        }
    }
}

// ── Live ────────────────────────────────────────────────────────────────────

pub struct Live {
    rt: Handle,
    tx: Sender<Msg>,
    pub cfg: Config,
    client: Arc<LoxClient>,
    stop: Option<watch::Sender<bool>>,
    roles: BTreeMap<String, String>,
}

/// `/jdev/...` value with the `LL.Code` checked.
fn jdev(client: &LoxClient, path: &str) -> Result<String> {
    let v = client.get_json(path)?;
    check_code(&v)?;
    Ok(match v.pointer("/LL/value") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    })
}

fn check_code(v: &Value) -> Result<()> {
    let code = v
        .pointer("/LL/Code")
        .or_else(|| v.pointer("/LL/code"))
        .and_then(|c| {
            c.as_str()
                .map(|s| s.to_string())
                .or_else(|| c.as_i64().map(|n| n.to_string()))
        });
    match code.as_deref() {
        None | Some("200") => Ok(()),
        Some(c) => {
            let hint = match c {
                "401" => " (unauthorized)",
                "403" => " (forbidden — the user lacks rights for this control)",
                "404" => " (unknown control or command)",
                "500" => " (Miniserver error)",
                _ => "",
            };
            bail!("Miniserver answered {}{}", c, hint)
        }
    }
}

impl Live {
    pub fn new(
        rt: Handle,
        tx: Sender<Msg>,
        cfg: Config,
        roles: BTreeMap<String, String>,
    ) -> Result<Live> {
        let client = Arc::new(LoxClient::new(cfg.clone())?);
        Ok(Live {
            rt,
            tx,
            cfg,
            client,
            stop: None,
            roles,
        })
    }

    /// Load the structure (cache or fetch), blocking. Used at startup.
    pub fn load_structure(cfg: &Config) -> Result<Value> {
        let mut c = LoxClient::new(cfg.clone())?;
        Ok(c.get_structure()?.clone())
    }

    fn start_stream(&mut self, epoch: u64, last_modified: String) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(true);
        }
        let (stop_tx, stop_rx) = watch::channel(false);
        self.stop = Some(stop_tx);
        let cfg = self.cfg.clone();
        let tx = self.tx.clone();
        let client = self.client.clone();
        let roles = self.roles.clone();
        let ctx = ctx_name(&self.cfg);
        self.rt.spawn(stream_loop(
            cfg,
            client,
            tx,
            epoch,
            last_modified,
            stop_rx,
            roles,
            ctx,
            None,
        ));
    }

    fn handle(&mut self, e: Effect, app: &App) {
        let epoch = app.epoch;
        let tx = self.tx.clone();
        match e {
            Effect::Send {
                req,
                cid,
                uuid,
                cmds,
                cli,
                secret,
                ..
            } => {
                let client = self.client.clone();
                self.rt.spawn_blocking(move || {
                    let mut result = Ok(());
                    for c in &cmds {
                        if let Err(e) = client.send_cmd(&uuid, c).and_then(|v| check_code(&v)) {
                            // transport errors quote the URL, PIN included
                            let mut msg = short_err(&e);
                            if let Some(s) = secret.as_deref().filter(|s| !s.is_empty()) {
                                msg = msg.replace(s, "****");
                            }
                            result = Err(msg);
                            break;
                        }
                    }
                    if let Err(e) = &result {
                        let _ = tx.send(Msg::Log(LogMsg {
                            t: now(),
                            err: true,
                            what: "command failed".into(),
                            detail: e.clone(),
                            cli: Some(cli),
                        }));
                    }
                    let _ = tx.send(Msg::CmdDone {
                        epoch,
                        req,
                        cid,
                        result,
                    });
                });
            }
            Effect::Poll { req, kind } => {
                let client = self.client.clone();
                let ctx = PollCtx::new(app, Some(&self.cfg), &kind);
                self.rt.spawn_blocking(move || {
                    let result = poll_live(&client, &ctx, &kind).map_err(|e| short_err(&e));
                    let _ = tx.send(Msg::Polled {
                        epoch,
                        req,
                        kind,
                        result,
                    });
                });
            }
            Effect::RunScene(name) => {
                let steps = match scene_steps(&self.cfg, &name, &app.house) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx.send(Msg::Log(LogMsg {
                            t: now(),
                            err: true,
                            what: format!("scene {}", name),
                            detail: e,
                            cli: Some(format!("lox run {}", crate::actions::shell_quote(&name))),
                        }));
                        let _ = tx.send(Msg::Toast(
                            ToastKind::Err,
                            format!("✗ scene {} failed", name),
                        ));
                        return;
                    }
                };
                let client = self.client.clone();
                self.rt.spawn_blocking(move || {
                    for (uuid, cmd, delay, label) in steps {
                        if let Err(e) = client.send_cmd(&uuid, &cmd).and_then(|v| check_code(&v)) {
                            let _ = tx.send(Msg::Log(LogMsg {
                                t: now(),
                                err: true,
                                what: format!("scene {} · {}", name, label),
                                detail: short_err(&e),
                                cli: Some(format!(
                                    "lox run {}",
                                    crate::actions::shell_quote(&name)
                                )),
                            }));
                            let _ = tx.send(Msg::Toast(
                                ToastKind::Err,
                                format!("✗ scene {} stopped at {}", name, label),
                            ));
                            return;
                        }
                        if delay > 0 {
                            std::thread::sleep(Duration::from_millis(delay));
                        }
                    }
                    let _ = tx.send(Msg::Toast(ToastKind::Ok, format!("✓ scene {}", name)));
                });
            }
            Effect::LoadWiring { download } => {
                let cfg = self.cfg.clone();
                let version = app.house.last_modified.clone();
                self.rt.spawn_blocking(move || {
                    let doc = load_wiring(&cfg, &version, download);
                    let _ = tx.send(Msg::Wiring { epoch, doc });
                });
            }
            Effect::ConfigPull => {
                let cfg = self.cfg.clone();
                self.rt.spawn_blocking(move || {
                    let r = match repo_dir(&cfg) {
                        Some(repo) => {
                            crate::gitops::pull(&repo, &cfg, true).map_err(|e| short_err(&e))
                        }
                        None => Err("no config repository — run `lox config init <dir>`".into()),
                    };
                    let _ = tx.send(Msg::ConfigPulled(r));
                });
            }
            Effect::Reboot | Effect::Install => {
                let reboot = matches!(e, Effect::Reboot);
                let client = self.client.clone();
                self.rt.spawn_blocking(move || {
                    let path = if reboot {
                        "/jdev/sys/reboot"
                    } else {
                        "/jdev/sys/updatetolatestrelease"
                    };
                    let what = if reboot { "reboot" } else { "firmware update" };
                    match jdev(&client, path) {
                        Ok(_) => {
                            let _ = tx.send(Msg::Toast(
                                ToastKind::Ok,
                                format!(
                                    "✓ {} requested — the Miniserver will be out of service",
                                    what
                                ),
                            ));
                        }
                        Err(err) => {
                            let _ = tx.send(Msg::Log(LogMsg {
                                t: now(),
                                err: true,
                                what: what.into(),
                                detail: short_err(&err),
                                cli: Some(if reboot {
                                    "lox reboot".into()
                                } else {
                                    "lox update install".into()
                                }),
                            }));
                            let _ =
                                tx.send(Msg::Toast(ToastKind::Err, format!("✗ {} failed", what)));
                        }
                    }
                });
            }
            Effect::Refresh => {
                let _ = std::fs::remove_file(LoxClient::cache_path(&self.cfg));
                self.restart(app.ctx_name.clone(), true);
            }
            Effect::SwitchContext(name) => match context_config(&name) {
                Ok(cfg) => match LoxClient::new(cfg.clone()) {
                    Ok(c) => {
                        self.cfg = cfg;
                        self.client = Arc::new(c);
                        self.restart(name, false);
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Toast(
                            ToastKind::Err,
                            format!("✗ {}: {}", name, short_err(&e)),
                        ));
                    }
                },
                Err(e) => {
                    let _ = tx.send(Msg::Toast(ToastKind::Err, format!("✗ {}", short_err(&e))));
                }
            },
            // handled by the runtime
            Effect::Copy(_) | Effect::SaveState(_) | Effect::Suspend | Effect::Quit => {}
        }
    }

    /// Load the structure for the current config, then (re)start the stream under a new epoch.
    fn restart(&mut self, ctx: String, refresh: bool) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(true);
        }
        let (stop_tx, stop_rx) = watch::channel(false);
        self.stop = Some(stop_tx);
        let cfg = self.cfg.clone();
        let tx = self.tx.clone();
        let client = self.client.clone();
        let roles = self.roles.clone();
        let _ = tx.send(Msg::Toast(
            ToastKind::Info,
            if refresh {
                "refreshing structure…".into()
            } else {
                format!("switching to {}…", ctx)
            },
        ));
        self.rt.spawn(async move {
            let c2 = cfg.clone();
            let st = tokio::task::spawn_blocking(move || Live::load_structure(&c2)).await;
            match st {
                Ok(Ok(st)) => {
                    // superseded by a newer switch while loading: its house
                    // must not replace the newer one
                    if *stop_rx.borrow() {
                        drop_blocking(client);
                        return;
                    }
                    let house = build_house(&st, &roles);
                    let lm = house.last_modified.clone();
                    let epoch = next_epoch();
                    let _ = tx.send(Msg::NewHouse {
                        epoch,
                        house: Box::new(house),
                        ctx: ctx.clone(),
                    });
                    stream_loop(cfg, client, tx, epoch, lm, stop_rx, roles, ctx, None).await;
                }
                Ok(Err(e)) => {
                    drop_blocking(client);
                    let _ = tx.send(Msg::Toast(
                        ToastKind::Err,
                        format!("✗ {}: {}", ctx, short_err(&e)),
                    ));
                    let _ = tx.send(Msg::Log(LogMsg {
                        t: now(),
                        err: true,
                        what: format!("load structure ({})", ctx),
                        detail: short_err(&e),
                        cli: Some("lox cache refresh".into()),
                    }));
                }
                Err(_) => drop_blocking(client),
            }
        });
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(true);
        }
    }
}

fn ctx_name(cfg: &Config) -> String {
    cfg.context_name.clone().unwrap_or_else(|| {
        if cfg.is_local {
            "local".into()
        } else {
            "default".into()
        }
    })
}

pub fn short_err(e: &anyhow::Error) -> String {
    let s = format!("{:#}", e);
    let s = crate::tui::text::clean(&s);
    crate::tui::text::trunc(&s, 160)
}

/// Stream with reconnect/backoff (§7.3). On reconnect, a changed structure
/// version reloads the structure under a new epoch.
#[allow(clippy::too_many_arguments)]
async fn stream_loop(
    cfg: Config,
    client: Arc<LoxClient>,
    tx: Sender<Msg>,
    epoch: u64,
    last_modified: String,
    stop: watch::Receiver<bool>,
    roles: BTreeMap<String, String>,
    ctx: String,
    first_error: Option<String>,
) {
    stream_loop_inner(
        cfg,
        &client,
        tx,
        epoch,
        last_modified,
        stop,
        roles,
        ctx,
        first_error,
    )
    .await;
    drop_blocking(client);
}

/// A blocking reqwest client must not be dropped on an async worker (it
/// panics); hand the last reference to the blocking pool instead.
fn drop_blocking(c: Arc<LoxClient>) {
    tokio::task::spawn_blocking(move || drop(c));
}

#[allow(clippy::too_many_arguments)]
async fn stream_loop_inner(
    cfg: Config,
    client: &Arc<LoxClient>,
    tx: Sender<Msg>,
    mut epoch: u64,
    mut last_modified: String,
    mut stop: watch::Receiver<bool>,
    roles: BTreeMap<String, String>,
    ctx: String,
    _first_error: Option<String>,
) {
    let mut attempt: u32 = 0;
    loop {
        if *stop.borrow() {
            return;
        }
        if attempt > 0 {
            // structure changed while we were away?
            let c = client.clone();
            if let Ok(Ok(v)) =
                tokio::task::spawn_blocking(move || jdev(&c, "/jdev/sps/LoxAPPversion3")).await
                && !v.is_empty()
                && !last_modified.is_empty()
                && v.trim() != last_modified.trim()
            {
                let c2 = cfg.clone();
                let _ = std::fs::remove_file(LoxClient::cache_path(&cfg));
                if let Ok(Ok(st)) =
                    tokio::task::spawn_blocking(move || Live::load_structure(&c2)).await
                {
                    if *stop.borrow() {
                        return;
                    }
                    let house = build_house(&st, &roles);
                    last_modified = house.last_modified.clone();
                    epoch = next_epoch();
                    let _ = tx.send(Msg::NewHouse {
                        epoch,
                        house: Box::new(house),
                        ctx: ctx.clone(),
                    });
                    let _ = tx.send(Msg::Toast(
                        ToastKind::Info,
                        "structure changed — reloaded".into(),
                    ));
                }
            }
        }
        let tx_live = tx.clone();
        let tx_states = tx.clone();
        let ep = epoch;
        let live_flag = Arc::new(Mutex::new(false));
        let lf = live_flag.clone();
        let res = crate::stream::stream_session(
            &cfg,
            stop.clone(),
            move || {
                *lf.lock().unwrap() = true;
                let _ = tx_live.send(Msg::Conn {
                    epoch: ep,
                    conn: Conn::Live,
                });
            },
            move |batch: Vec<StateEvent>| tx_states.send(Msg::States { epoch: ep, batch }).is_ok(),
        )
        .await;
        if *stop.borrow() {
            return;
        }
        if *live_flag.lock().unwrap() {
            attempt = 0;
        }
        attempt += 1;
        let err = match res {
            Err(e) => short_err(&e),
            Ok(()) => "connection closed".into(),
        };
        let delay = (1u64 << (attempt - 1).min(5)).min(30) as f64;
        let conn = if attempt >= 6 {
            Conn::Offline(err.clone())
        } else {
            Conn::Reconnecting {
                attempt,
                at: now() + delay,
            }
        };
        if attempt == 1 || attempt == 6 {
            let _ = tx.send(Msg::Log(LogMsg {
                t: now(),
                err: true,
                what: "stream".into(),
                detail: err,
                cli: None,
            }));
        }
        if tx.send(Msg::Conn { epoch, conn }).is_err() {
            return;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs_f64(delay)) => {}
            _ = stop.changed() => return,
        }
    }
}

// ── Polls ───────────────────────────────────────────────────────────────────

/// What a poll needs from the app, captured on the UI thread.
/// Where a control's statistics live.
#[derive(Debug, Clone)]
pub enum StatSrc {
    /// Legacy `/stats/{uuid}.{YYYYMM}` files with this many outputs per record
    Files(usize),
    /// Statistics V2: (group id, output)
    V2(String, String),
}

impl StatSrc {
    fn of(c: &super::model::Ctrl) -> StatSrc {
        match &c.stat_v2 {
            Some((g, o)) => StatSrc::V2(g.clone(), o.clone()),
            None => StatSrc::Files(c.stat_outputs.max(1)),
        }
    }
}

pub struct PollCtx {
    pub house_ms_type: String,
    pub house_serial: String,
    /// History: control uuid + where its statistics are
    pub stats: Option<(String, StatSrc)>,
    /// EnergyDay: (role, control uuid, statistics) of meters
    pub meters: Vec<(Role, String, StatSrc)>,
    pub ctx: String,
    pub contexts: Vec<String>,
    pub cfg: Option<Config>,
}

impl PollCtx {
    fn new(app: &App, cfg: Option<&Config>, kind: &PollKind) -> PollCtx {
        let h = &app.house;
        // the control of this poll, not any History poll in flight
        let stats = match kind {
            PollKind::History(c) => Some(*c),
            _ => None,
        };
        let stats = stats.map(|c: Cid| (h.ctrls[c].uuid.clone(), StatSrc::of(&h.ctrls[c])));
        let mut meters = Vec::new();
        // Only meters whose statistics record power: real Miniservers often
        // have none, or log energy counters, which would read as nonsense kW.
        for n in &h.energy.nodes {
            if let Some(c) = n.ctrl
                && h.ctrls[c].stat_power
            {
                meters.push((n.role, h.ctrls[c].uuid.clone(), StatSrc::of(&h.ctrls[c])));
            }
        }
        PollCtx {
            house_ms_type: h.ms_type.clone(),
            house_serial: h.serial.clone(),
            stats,
            meters,
            ctx: app.ctx_name.clone(),
            contexts: app.contexts.clone(),
            cfg: cfg.cloned(),
        }
    }
}

fn poll_live(client: &LoxClient, ctx: &PollCtx, kind: &PollKind) -> Result<Polled> {
    Ok(match kind {
        PollKind::Diag => Polled::Diag(diag(client)?),
        PollKind::Info => Polled::Info(info(client, ctx)?),
        PollKind::BusLan => Polled::BusLan(buslan(client)?),
        PollKind::Devices => {
            Polled::Devices(data::parse_status_xml(&client.get_text("/data/status")?))
        }
        PollKind::Log => Polled::Log(data::parse_log(
            &client.get_bytes("/dev/fsget/log/def.log")?,
            500,
        )),
        PollKind::Sites => Polled::Sites(sites(ctx)),
        PollKind::History(cid) => {
            let (uuid, src) = ctx.stats.clone().context("no statistics")?;
            Polled::History(*cid, history(client, &uuid, &src)?)
        }
        PollKind::EnergyDay => energy_day(client, ctx),
        PollKind::ConfigLog => {
            let cfg = ctx.cfg.as_ref().context("no config")?;
            Polled::ConfigLog(config_log(cfg))
        }
        PollKind::ConfigDiff(hash) => {
            let cfg = ctx.cfg.as_ref().context("no config")?;
            Polled::ConfigDiff(hash.clone(), config_diff(cfg, hash)?)
        }
    })
}

fn diag(client: &LoxClient) -> Result<Diag> {
    // CPU + heap are required; the rest is best effort (Gen 2 leaves some empty)
    let (cpu, sps) = data::parse_lastcpu(&jdev(client, "/jdev/sys/lastcpu")?);
    let (used, total) = data::parse_heap(&jdev(client, "/jdev/sys/heap")?);
    let opt = |p: &str| jdev(client, p).ok().and_then(|v| data::parse_num(&v));
    Ok(Diag {
        cpu,
        sps,
        heap_used_kb: used,
        heap_total_kb: total,
        tasks: opt("/jdev/sys/numtasks"),
        ctx_switches: opt("/jdev/sys/contextswitches"),
        ints: opt("/jdev/sys/ints"),
        comints: opt("/jdev/sys/comints"),
        sd: sd_test(client),
    })
}

/// `sdtest` exercises the SD card: run it at most every 10 minutes, not with
/// every 2 s diagnostics poll.
fn sd_test(client: &LoxClient) -> Option<String> {
    static LAST: Mutex<Option<(Instant, Option<String>)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((t, v)) = last.as_ref()
        && t.elapsed() < Duration::from_secs(600)
    {
        return v.clone();
    }
    let v = jdev(client, "/jdev/sys/sdtest")
        .ok()
        .filter(|s| !s.is_empty());
    *last = Some((Instant::now(), v.clone()));
    v
}

fn info(client: &LoxClient, ctx: &PollCtx) -> Result<MsInfo> {
    let get = |p: &str| jdev(client, p).unwrap_or_default();
    let firmware = jdev(client, "/jdev/cfg/version")?;
    let yes = |s: String| match s.to_lowercase().as_str() {
        "" => None,
        "1" | "true" | "on" | "yes" => Some(true),
        _ => Some(false),
    };
    Ok(MsInfo {
        firmware,
        serial: ctx.house_serial.clone(),
        ms_type: ctx.house_ms_type.clone(),
        ip: get("/jdev/cfg/ip"),
        mask: get("/jdev/cfg/mask"),
        gateway: get("/jdev/cfg/gateway"),
        dns: [get("/jdev/cfg/dns1"), get("/jdev/cfg/dns2")]
            .into_iter()
            .filter(|d| !d.is_empty() && d != "0.0.0.0")
            .collect(),
        mac: get("/jdev/cfg/mac"),
        dhcp: yes(get("/jdev/cfg/dhcp")),
        ntp: yes(get("/jdev/cfg/ntp")),
        structure_version: get("/jdev/sps/LoxAPPversion3"),
        sps_state: None,
        ms_time: None,
    })
}

const BUS: [(&str, &str); 6] = [
    ("CAN sent", "/jdev/bus/packetssent"),
    ("CAN received", "/jdev/bus/packetsreceived"),
    ("CAN receive errors", "/jdev/bus/receiveerrors"),
    ("CAN frame errors", "/jdev/bus/frameerrors"),
    ("CAN overruns", "/jdev/bus/overruns"),
    ("CAN parity errors", "/jdev/bus/parityerrors"),
];
const LAN: [(&str, &str); 9] = [
    ("LAN tx packets", "/jdev/lan/txp"),
    ("LAN tx errors", "/jdev/lan/txe"),
    ("LAN tx collisions", "/jdev/lan/txc"),
    ("LAN tx underruns", "/jdev/lan/txu"),
    ("LAN rx packets", "/jdev/lan/rxp"),
    ("LAN rx overflows", "/jdev/lan/rxo"),
    ("LAN rx EOF", "/jdev/lan/eof"),
    ("LAN exhausted", "/jdev/lan/exh"),
    ("LAN no buffer", "/jdev/lan/nob"),
];

fn buslan(client: &LoxClient) -> Result<BusLan> {
    let mut counters = Vec::new();
    let mut ok = 0;
    for (name, path) in BUS.iter().chain(LAN.iter()) {
        let v = jdev(client, path)
            .ok()
            .and_then(|v| data::parse_num(&v))
            .map(|f| f as u64);
        ok += v.is_some() as usize;
        counters.push((name.to_string(), v));
    }
    if ok == 0 {
        bail!("no bus/LAN counters available");
    }
    Ok(BusLan { counters })
}

fn sites(ctx: &PollCtx) -> Vec<SiteStatus> {
    let mut out = Vec::new();
    for name in &ctx.contexts {
        if *name == ctx.ctx {
            if let Some(cfg) = &ctx.cfg {
                out.push(SiteStatus {
                    name: name.clone(),
                    host: host_of(cfg),
                    online: true,
                    ..Default::default()
                });
            }
            continue;
        }
        let mut s = SiteStatus {
            name: name.clone(),
            ..Default::default()
        };
        match context_config(name) {
            Ok(cfg) => {
                s.host = host_of(&cfg);
                match LoxClient::new(cfg) {
                    Ok(c) => {
                        let t0 = Instant::now();
                        match jdev(&c, "/jdev/cfg/version") {
                            Ok(fw) => {
                                s.latency_ms = Some(t0.elapsed().as_millis() as u64);
                                s.online = true;
                                s.firmware = Some(fw);
                                s.cpu = jdev(&c, "/jdev/sys/lastcpu")
                                    .ok()
                                    .and_then(|v| data::parse_lastcpu(&v).0);
                                s.heap_pct = jdev(&c, "/jdev/sys/heap").ok().and_then(|v| {
                                    let (u, t) = data::parse_heap(&v);
                                    Some(u? / t? * 100.0)
                                });
                            }
                            Err(e) => s.error = Some(short_err(&e)),
                        }
                    }
                    Err(e) => s.error = Some(short_err(&e)),
                }
            }
            Err(e) => s.error = Some(short_err(&e)),
        }
        out.push(s);
    }
    out
}

fn host_of(cfg: &Config) -> String {
    cfg.host
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

/// Stats for a control: this month's file (and last month's early in the month).
fn history(client: &LoxClient, uuid: &str, src: &StatSrc) -> Result<Series> {
    let outputs = match src {
        StatSrc::Files(n) => *n,
        StatSrc::V2(group, output) => {
            // the views need today and the last 24 h
            let to = now() as i64;
            let from = to - 2 * 86_400;
            let data = client.get_bytes(&crate::statv2::raw_path(
                uuid,
                from,
                to,
                group,
                Some(output),
            ))?;
            let points: Vec<(i64, f64)> = crate::statv2::parse(&data, 1)
                .into_iter()
                .filter_map(|(t, v)| v.first().copied().filter(|v| v.is_finite()).map(|v| (t, v)))
                .collect();
            if points.is_empty() {
                bail!("no statistics recorded");
            }
            return Ok(Series { points });
        }
    };
    let now = chrono::Local::now();
    let mut periods = vec![now.format("%Y%m").to_string()];
    let yesterday = now - chrono::Duration::days(1);
    let yp = yesterday.format("%Y%m").to_string();
    if yp != periods[0] {
        periods.insert(0, yp);
    }
    let mut points = Vec::new();
    let mut listing: Option<String> = None;
    let mut any = false;
    for p in periods {
        let data = match client.get_bytes(&crate::stats_file_path(uuid, &p)) {
            Ok(d) if d.len() > 12 => vec![d],
            _ => {
                // some firmware versions name the files differently: look them up
                if listing.is_none() {
                    listing = client.get_text("/dev/fslist//stats").ok();
                }
                let files: Vec<String> = listing
                    .as_deref()
                    .map(|l| {
                        crate::find_stats_files(l, uuid, &p)
                            .into_iter()
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                files
                    .iter()
                    .filter_map(|f| client.get_bytes(&format!("/dev/fsget//stats/{}", f)).ok())
                    .collect()
            }
        };
        for d in data {
            any = true;
            points.extend(data::parse_stats(&d, outputs, stats_epoch_unix()).points);
        }
    }
    if !any {
        bail!("no statistics file");
    }
    points.sort_by_key(|p| p.0);
    Ok(Series { points })
}

fn energy_day(client: &LoxClient, ctx: &PollCtx) -> Polled {
    let tz = tz_offset();
    let now_unix = now() as i64;
    let mut by_role: BTreeMap<u8, Vec<f64>> = BTreeMap::new();
    for (role, uuid, src) in &ctx.meters {
        let Ok(s) = history(client, uuid, src) else {
            continue;
        };
        let q = data::quarter_hours(&s.points, tz, now_unix);
        let key = *role as u8;
        let e = by_role.entry(key).or_insert_with(|| vec![f64::NAN; 96]);
        for (a, b) in e.iter_mut().zip(q) {
            if b.is_finite() {
                *a = if a.is_finite() { *a + b } else { b };
            }
        }
    }
    let get = |r: Role| by_role.get(&(r as u8)).cloned();
    let pv = get(Role::Production).unwrap_or_default();
    let usage = match get(Role::Load) {
        Some(l) => l,
        None => {
            let grid = get(Role::Grid);
            let st = get(Role::Storage);
            if grid.is_none() && pv.is_empty() {
                Vec::new()
            } else {
                (0..96)
                    .map(|i| {
                        let p = pv.get(i).copied().filter(|v| v.is_finite()).unwrap_or(0.0);
                        let g = grid.as_ref().and_then(|g| g.get(i).copied());
                        let s = st
                            .as_ref()
                            .and_then(|s| s.get(i).copied())
                            .filter(|v| v.is_finite())
                            .unwrap_or(0.0);
                        match g {
                            Some(g) if g.is_finite() => p + g - s,
                            _ => f64::NAN,
                        }
                    })
                    .collect()
            }
        }
    };
    Polled::EnergyDay { pv, usage }
}

// ── Config repo (gitops) ────────────────────────────────────────────────────

fn repo_dir(cfg: &Config) -> Option<PathBuf> {
    let r = cfg.config_repo.as_ref()?;
    let p = PathBuf::from(r);
    p.join(".git").exists().then_some(p)
}

fn git(repo: &std::path::Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = std::process::Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .context("git not found")?;
    if !out.status.success() {
        bail!(
            "git {}: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}

fn config_log(cfg: &Config) -> Result<Vec<Commit>, String> {
    let repo = repo_dir(cfg).ok_or_else(|| "no config repository for this context".to_string())?;
    let ms = crate::gitops::ms_dir(cfg);
    let prefix = format!("[{}] ", ms);
    let out = git(
        &repo,
        &[
            "log",
            "-n",
            "100",
            "--format=%H%x09%ci%x09%s",
            "--",
            &format!("{}/config.Loxone", ms),
        ],
    )
    .map_err(|e| short_err(&e))?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|l| {
            let mut p = l.splitn(3, '\t');
            Some(Commit {
                hash: p.next()?.to_string(),
                date: p.next()?.to_string(),
                // `lox config pull` prefixes subjects with the Miniserver dir: redundant here
                subject: crate::tui::text::clean(
                    p.next().unwrap_or("").trim_start_matches(prefix.as_str()),
                ),
            })
        })
        .collect())
}

fn config_diff(cfg: &Config, hash: &str) -> Result<Vec<String>> {
    let repo = repo_dir(cfg).context("no config repository")?;
    let path = format!("{}/config.Loxone", crate::gitops::ms_dir(cfg));
    let new = git(&repo, &["show", &format!("{}:{}", hash, path)])?;
    let old = git(&repo, &["show", &format!("{}^:{}", hash, path)]).unwrap_or_default();
    if old.is_empty() {
        return Ok(vec!["+ initial config".into()]);
    }
    crate::logic::diff_lines(&old, &new)
}

// ── Wiring source ───────────────────────────────────────────────────────────

fn wiring_cache(cfg: &Config) -> PathBuf {
    cfg.cache_dir().join("config.Loxone")
}

/// gitops checkout → per-context cache (keyed by structure version) → FTP download.
fn load_wiring(cfg: &Config, version: &str, download: bool) -> WiringDoc {
    let parse = |bytes: &[u8], src: String| match Logic::parse(bytes) {
        Ok(l) => WiringDoc::Ready(Arc::new(l), src),
        Err(e) => WiringDoc::Failed(short_err(&e)),
    };
    if !download {
        if let Some(repo) = repo_dir(cfg) {
            let p = repo.join(crate::gitops::ms_dir(cfg)).join("config.Loxone");
            if let Ok(b) = std::fs::read(&p) {
                return parse(&b, "gitops".into());
            }
        }
        let cache = wiring_cache(cfg);
        let ver = std::fs::read_to_string(cache.with_extension("version")).unwrap_or_default();
        if let Ok(b) = std::fs::read(&cache) {
            let src = if ver.trim() == version.trim() {
                "cached".to_string()
            } else {
                "cached · config may be stale".to_string()
            };
            return parse(&b, src);
        }
        return WiringDoc::Missing;
    }
    let r = (|| -> Result<(Vec<u8>, String)> {
        let backups = crate::ftp::list_backups(cfg)?;
        let newest = backups
            .first()
            .context("no config backups on the Miniserver")?;
        let zip = crate::ftp::download_backup(cfg, &newest.filename)?;
        let xml = crate::loxcc::extract_and_decompress(&zip)?;
        Ok((xml, newest.filename.clone()))
    })();
    match r {
        Ok((xml, name)) => {
            let cache = wiring_cache(cfg);
            if let Some(p) = cache.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::write(&cache, &xml);
            let _ = std::fs::write(cache.with_extension("version"), version);
            parse(&xml, name)
        }
        Err(e) => WiringDoc::Failed(short_err(&e)),
    }
}

// ── Contexts / scenes ───────────────────────────────────────────────────────

pub fn context_config(name: &str) -> Result<Config> {
    let g = GlobalConfig::load_or_default();
    let e = g
        .contexts
        .get(name)
        .cloned()
        .with_context(|| format!("unknown context {}", name))?;
    Ok(e.into_config(name, Config::context_data_dir(name)))
}

pub fn context_names() -> Vec<String> {
    let mut v: Vec<String> = GlobalConfig::load_or_default()
        .contexts
        .into_keys()
        .collect();
    v.sort();
    v
}

type Step = (String, String, u64, String);

fn scene_steps(cfg: &Config, name: &str, house: &House) -> Result<Vec<Step>, String> {
    let s = crate::scene::Scene::load_with_config(name, cfg).map_err(|e| short_err(&e))?;
    s.steps
        .iter()
        .map(|st| {
            let uuid = match cfg.aliases.get(&st.control) {
                Some(u) => u.clone(),
                None => house
                    .resolve(&st.control, None)
                    .map(|c| house.ctrls[c].uuid.clone())?,
            };
            Ok((uuid, st.cmd.clone(), st.delay_ms, st.control.clone()))
        })
        .collect()
}

// ── Demo ────────────────────────────────────────────────────────────────────

pub const DEMO_SCENES: [&str; 4] = ["movie", "leave", "night", "morning"];

pub struct Demo {
    tx: Sender<Msg>,
    rt: Handle,
    sim: Arc<Mutex<demo::Sim>>,
    started: f64,
    st: Value,
    stop: Option<watch::Sender<bool>>,
}

impl Demo {
    pub fn new(rt: Handle, tx: Sender<Msg>, st: Value) -> Demo {
        let sim = Arc::new(Mutex::new(demo::Sim::new(&st)));
        Demo {
            tx,
            rt,
            sim,
            started: now(),
            st,
            stop: None,
        }
    }

    fn start(&mut self, app: &App) {
        let epoch = app.epoch;
        let tx = self.tx.clone();
        let sim = self.sim.clone();
        let (stop_tx, mut stop_rx) = watch::channel(false);
        self.stop = Some(stop_tx);
        self.rt.spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let first = sim.lock().unwrap().initial_events();
            let _ = tx.send(Msg::Conn {
                epoch,
                conn: Conn::Live,
            });
            let _ = tx.send(Msg::States {
                epoch,
                batch: first,
            });
            let mut iv = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! {
                    _ = iv.tick() => {}
                    _ = stop_rx.changed() => return,
                }
                let batch = sim.lock().unwrap().tick(now(), local_hour());
                if !batch.is_empty() && tx.send(Msg::States { epoch, batch }).is_err() {
                    return;
                }
            }
        });
    }

    fn handle(&mut self, e: Effect, app: &App) {
        let epoch = app.epoch;
        let tx = self.tx.clone();
        match e {
            Effect::Send {
                req,
                cid,
                uuid,
                cmds,
                cli,
                ..
            } => {
                let sim = self.sim.clone();
                self.rt.spawn(async move {
                    // a believable round trip
                    tokio::time::sleep(Duration::from_millis(120)).await;
                    let mut result = Ok(());
                    for c in &cmds {
                        if let Err(e) = sim.lock().unwrap().command(&uuid, c) {
                            result = Err(e);
                            break;
                        }
                    }
                    if let Err(e) = &result {
                        let _ = tx.send(Msg::Log(LogMsg {
                            t: now(),
                            err: true,
                            what: "command failed".into(),
                            detail: e.clone(),
                            cli: Some(cli),
                        }));
                    }
                    let _ = tx.send(Msg::CmdDone {
                        epoch,
                        req,
                        cid,
                        result,
                    });
                });
            }
            Effect::Poll { req, kind } => {
                let t = now() - self.started;
                let result: Result<Polled, String> = match &kind {
                    PollKind::Diag => Ok(Polled::Diag(demo::diag(t))),
                    PollKind::Info => Ok(Polled::Info(demo::info())),
                    PollKind::BusLan => Ok(Polled::BusLan(demo::bus_lan(t))),
                    PollKind::Devices => Ok(Polled::Devices(demo::devices())),
                    PollKind::Log => Ok(Polled::Log(demo::log())),
                    PollKind::Sites => Ok(Polled::Sites(demo::sites(t))),
                    PollKind::History(c) => {
                        let name = app.house.ctrls[*c].name.clone();
                        Ok(Polled::History(
                            *c,
                            demo::history(&name, now() as i64, local_hour()),
                        ))
                    }
                    PollKind::EnergyDay => {
                        let (pv, usage) = demo::energy_today(local_hour());
                        Ok(Polled::EnergyDay { pv, usage })
                    }
                    PollKind::ConfigLog => Ok(Polled::ConfigLog(Ok(demo_commits()))),
                    PollKind::ConfigDiff(h) => Ok(Polled::ConfigDiff(h.clone(), demo_diff(h))),
                };
                let delay = if matches!(kind, PollKind::History(_) | PollKind::EnergyDay) {
                    400
                } else {
                    60
                };
                self.rt.spawn(async move {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    let _ = tx.send(Msg::Polled {
                        epoch,
                        req,
                        kind,
                        result,
                    });
                });
            }
            Effect::RunScene(name) => {
                let plan = demo_scene(&app.house, &name);
                let sim = self.sim.clone();
                self.rt.spawn(async move {
                    for (uuid, cmd) in plan {
                        let _ = sim.lock().unwrap().command(&uuid, &cmd);
                        tokio::time::sleep(Duration::from_millis(80)).await;
                    }
                    let _ = tx.send(Msg::Toast(ToastKind::Ok, format!("✓ scene {}", name)));
                });
            }
            Effect::LoadWiring { .. } => {
                let xml = demo::loxone_xml(&self.st);
                let doc = match Logic::parse(xml.as_bytes()) {
                    Ok(l) => WiringDoc::Ready(Arc::new(l), "demo.Loxone".into()),
                    Err(e) => WiringDoc::Failed(short_err(&e)),
                };
                let _ = tx.send(Msg::Wiring { epoch, doc });
            }
            Effect::ConfigPull => {
                let _ = tx.send(Msg::ConfigPulled(Ok(false)));
            }
            Effect::Reboot | Effect::Install => {
                // simulate the out-of-service cycle
                let rt = self.rt.clone();
                rt.spawn(async move {
                    let _ = tx.send(Msg::States {
                        epoch,
                        batch: vec![StateEvent::OutOfService],
                    });
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    let _ = tx.send(Msg::Conn {
                        epoch,
                        conn: Conn::Reconnecting {
                            attempt: 1,
                            at: now() + 2.0,
                        },
                    });
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    let _ = tx.send(Msg::Conn {
                        epoch,
                        conn: Conn::Live,
                    });
                });
            }
            Effect::Refresh => {
                let house = House::from_structure(&self.st);
                let e2 = next_epoch();
                let _ = tx.send(Msg::NewHouse {
                    epoch: e2,
                    house: Box::new(house),
                    ctx: app.ctx_name.clone(),
                });
                if let Some(s) = self.stop.take() {
                    let _ = s.send(true);
                }
                self.start_with_epoch(e2);
            }
            Effect::SwitchContext(name) => {
                let _ = tx.send(Msg::Toast(
                    ToastKind::Info,
                    format!("demo: '{}' is a sample site — nothing to switch to", name),
                ));
            }
            Effect::Copy(_) | Effect::SaveState(_) | Effect::Suspend | Effect::Quit => {}
        }
    }

    fn start_with_epoch(&mut self, epoch: u64) {
        let tx = self.tx.clone();
        let sim = self.sim.clone();
        let (stop_tx, mut stop_rx) = watch::channel(false);
        self.stop = Some(stop_tx);
        self.rt.spawn(async move {
            let first = sim.lock().unwrap().initial_events();
            let _ = tx.send(Msg::Conn {
                epoch,
                conn: Conn::Live,
            });
            let _ = tx.send(Msg::States {
                epoch,
                batch: first,
            });
            let mut iv = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! {
                    _ = iv.tick() => {}
                    _ = stop_rx.changed() => return,
                }
                let batch = sim.lock().unwrap().tick(now(), local_hour());
                if !batch.is_empty() && tx.send(Msg::States { epoch, batch }).is_err() {
                    return;
                }
            }
        });
    }
}

/// Synthetic scenes for the demo house.
fn demo_scene(h: &House, name: &str) -> Vec<(String, String)> {
    use super::model::Kind;
    let mut out = Vec::new();
    for c in h.top_level() {
        let k = h.ctrls[c].kind;
        let u = h.ctrls[c].uuid.clone();
        let room = h.room_name(c).unwrap_or("").to_string();
        match (name, k) {
            ("night" | "leave", Kind::LightCtl) => out.push((u, "changeTo/778".into())),
            ("night", Kind::Blind) => out.push((u, "FullDown".into())),
            ("morning", Kind::Blind) => out.push((u, "FullUp".into())),
            ("movie", Kind::LightCtl) if room == "Living room" => {
                out.push((u, "changeTo/2".into()))
            }
            ("movie", Kind::Blind) if room == "Living room" => out.push((u, "FullDown".into())),
            _ => {}
        }
    }
    out
}

fn demo_commits() -> Vec<Commit> {
    vec![
        Commit {
            hash: "c3".into(),
            date: "2026-09-24 21:10:02 +0200".into(),
            subject: "Night mode: hallway off-delay 120 → 300 s".into(),
        },
        Commit {
            hash: "c2".into(),
            date: "2026-09-12 18:44:40 +0200".into(),
            subject: "Add Terrace spot, rename Office blinds".into(),
        },
        Commit {
            hash: "c1".into(),
            date: "2026-08-30 09:02:13 +0200".into(),
            subject: "Initial config".into(),
        },
    ]
}

fn demo_diff(hash: &str) -> Vec<String> {
    match hash {
        "c3" => vec![
            "~ param Hallway light · Off-delay 120 → 300".into(),
            "~ rename Night → Night mode".into(),
            "+ wire Night mode.Q → Hallway light.DisP".into(),
        ],
        "c2" => vec![
            "+ block Terrace spot (Switch)".into(),
            "~ rename Blinds office → Office blind".into(),
            "+ wire Terrace button.Q → Terrace spot.Tg".into(),
        ],
        _ => vec!["+ initial config".into()],
    }
}

#[cfg(test)]
mod live_probe {
    /// Read-only probe of the initial state dump of a real Miniserver:
    /// `LOX_TUI_LIVE=1 cargo test live_probe -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn initial_dump_coverage() {
        if std::env::var("LOX_TUI_LIVE").is_err() {
            return;
        }
        let cfg = crate::config::Config::load().unwrap();
        let st = super::Live::load_structure(&cfg).unwrap();
        let h = super::House::from_structure(&st);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::<
            String,
            f64,
        >::new()));
        let s2 = seen.clone();
        rt.block_on(async move {
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(6)).await;
                let _ = stop_tx.send(true);
            });
            let r = crate::stream::stream_session(
                &cfg,
                stop_rx,
                || {},
                move |b| {
                    for e in b {
                        if let crate::stream::StateEvent::ValueState { uuid, value } = e {
                            s2.lock().unwrap().insert(uuid, value);
                        }
                    }
                    true
                },
            )
            .await;
            println!("session: {:?}", r.err());
        });
        let seen = seen.lock().unwrap();
        let total = h.state_owner.len();
        println!("states {} received {}", total, seen.len());
        let mut missing = std::collections::BTreeMap::<String, usize>::new();
        for (u, (c, s)) in &h.state_owner {
            if !seen.contains_key(u) {
                *missing
                    .entry(format!("{:?}.{}", h.ctrls[*c].kind, s))
                    .or_default() += 1;
            }
        }
        for (k, n) in missing.iter().filter(|(_, n)| **n > 1) {
            println!("  missing {} ×{}", k, n);
        }
        // the initial dump carries (nearly) every value state (§ stream framing)
        assert!(seen.len() * 2 > total, "initial dump incomplete");
    }
}
