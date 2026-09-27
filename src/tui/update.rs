//! The pure `update(app, msg) -> effects` (§8.3). No I/O happens here, so the
//! journeys (J1–J12) and edge cases are plain unit tests.

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};

use super::app::*;
use super::keymap::{self, Cmd, Ctx};
use super::lists::{self, CRow};
use super::model::{Cid, Kind};
use super::palette;
use super::vm::{self, Plan, Verb};
use crate::actions::{Action, AlarmCmd, BlindCmd, Risk, ThermoCmd};
use crate::stream::StateEvent;

/// Minimum terminal size (§6.4)
pub const MIN_W: u16 = 80;
pub const MIN_H: u16 = 24;
/// Rooms shows the inspector as a third pane from this width
pub const WIDE_ROOMS: u16 = 140;
/// Presses of a non-repeating key closer than this are auto-repeat (§4.1 rule 8)
const REPEAT_GAP: f64 = 0.6;
const PENDING_TIMEOUT: f64 = 3.0;
const BULK_CONFIRM: usize = 10;

pub fn update(app: &mut App, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Key(k) => key(app, k),
        Msg::Paste(s) => {
            paste(app, &s);
            Vec::new()
        }
        Msg::Mouse(m) => mouse(app, m),
        Msg::Resize(w, h) => {
            app.size = (w, h);
            Vec::new()
        }
        Msg::Tick { now } => tick(app, now),
        Msg::States { epoch, batch } => {
            if epoch == app.epoch {
                states(app, batch);
            }
            Vec::new()
        }
        Msg::Conn { epoch, conn } => {
            if epoch == app.epoch {
                set_conn(app, conn);
            }
            Vec::new()
        }
        Msg::Polled {
            epoch,
            req,
            kind,
            result,
        } => {
            polled(app, epoch, req, kind, result);
            Vec::new()
        }
        Msg::CmdDone {
            epoch,
            req,
            cid,
            result,
        } => {
            cmd_done(app, epoch, req, cid, result);
            Vec::new()
        }
        Msg::Wiring { epoch, doc } => {
            if epoch == app.epoch {
                app.wiring = doc;
            }
            Vec::new()
        }
        Msg::NewHouse { epoch, house, ctx } => {
            new_house(app, epoch, *house, ctx);
            Vec::new()
        }
        Msg::Toast(kind, text) => {
            app.toast(kind, text);
            Vec::new()
        }
        Msg::ConfigPulled(r) => {
            match r {
                Ok(changed) => {
                    let t = if changed {
                        "config pulled — new commit"
                    } else {
                        "config pulled — no changes"
                    };
                    app.toast(ToastKind::Ok, t);
                    app.log_msg(false, "lox config pull", t, None);
                }
                Err(e) => app.fail("lox config pull", e, Some("lox config pull".into())),
            }
            app.commits = None;
            app.diffs.clear();
            app.polls.last.remove(&PollKind::ConfigLog);
            schedule(app)
        }
        Msg::Log(m) => {
            if m.err {
                app.unread_errors += 1;
            }
            app.msglog.push_back(m);
            Vec::new()
        }
    }
}

// ── Stream / connection ─────────────────────────────────────────────────────

fn states(app: &mut App, batch: Vec<StateEvent>) {
    if batch.iter().any(|e| matches!(e, StateEvent::OutOfService)) {
        set_conn(app, Conn::OutOfService);
    }
    if app.paused {
        app.paused_buf.extend(batch);
        // bounded: apply early rather than grow without limit
        if app.paused_buf.len() > 200_000 {
            let buf = std::mem::take(&mut app.paused_buf);
            apply(app, buf);
        }
        return;
    }
    apply(app, batch);
}

fn apply(app: &mut App, batch: Vec<StateEvent>) {
    app.store.apply(&app.house, &batch, app.now);
    // A stream value for a control confirms its pending command
    let mut touched: Vec<Cid> = Vec::new();
    for e in &batch {
        let uuid = match e {
            StateEvent::ValueState { uuid, .. } | StateEvent::TextState { uuid, .. } => uuid,
            _ => continue,
        };
        if let Some((cid, _)) = app.house.state_owner.get(uuid) {
            touched.push(*cid);
            if let Some(p) = app.house.ctrls[*cid].parent {
                touched.push(p);
            }
            // a light controller's sub-dimmers confirm the controller and vice versa
            touched.extend(app.house.ctrls[*cid].subs.iter().copied());
        }
    }
    for c in touched {
        if app
            .pending
            .get(&c)
            .is_some_and(|p| !matches!(p.state, PendState::Failed(_)))
        {
            app.pending.remove(&c);
        }
    }
    // Events view: follow keeps the selection on the newest row
    if app.events.follow {
        app.events.sel = None;
    }
}

fn set_conn(app: &mut App, conn: Conn) {
    if app.conn == conn {
        return;
    }
    let was_live = app.conn == Conn::Live;
    match &conn {
        Conn::Live => {
            // the initial state dump is applied silently (§7.2)
            app.store.quiet_until = app.now + 2.0;
            if app.conn != Conn::Connecting {
                app.log_msg(false, "connection", "live again", None);
                app.toast(ToastKind::Ok, "● live");
            }
        }
        Conn::Reconnecting { attempt, .. } if was_live || *attempt == 1 => {
            app.log_msg(true, "connection lost", "reconnecting", None);
        }
        Conn::Offline(e) => app.log_msg(true, "offline", e.clone(), None),
        Conn::OutOfService => {
            app.log_msg(true, "Miniserver", "out of service (reboot / update)", None)
        }
        _ => {}
    }
    app.conn = conn;
    app.conn_since = app.now;
}

fn new_house(app: &mut App, epoch: u64, house: super::model::House, ctx: String) {
    let same_ctx = ctx == app.ctx_name;
    if same_ctx {
        // Structure refresh: keep the event buffer, re-own it by UUID
        for e in app.store.events.iter_mut() {
            e.cid = house.state_owner.get(&e.uuid).map(|(c, _)| *c);
        }
        app.store.last_event.clear();
    } else {
        let mutes = std::mem::take(&mut app.store.mutes);
        app.store = super::store::Store::new();
        app.store.mutes = mutes;
        app.rooms = RoomsState::default();
        app.home = HomeState::default();
        app.events = EventsState::default();
        app.system = SystemState::default();
        app.energy = EnergyState::default();
        app.diag = None;
        app.diag_hist.clear();
        app.info = None;
        app.buslan = None;
        app.buslan_prev = None;
        app.buslan_flash.clear();
        app.devices = None;
        app.log = None;
        app.history.clear();
        app.energy_day = None;
        app.commits = None;
        app.diffs.clear();
        app.wiring = WiringDoc::None;
        app.overlays.clear();
    }
    app.pending.clear();
    app.last_dir.clear();
    app.polls = PollState::default();
    app.house = house;
    app.ctx_name = ctx;
    app.epoch = epoch;
    app.conn = Conn::Connecting;
    app.conn_since = app.now;
}

// ── Results ─────────────────────────────────────────────────────────────────

fn polled(app: &mut App, epoch: u64, req: u64, kind: PollKind, result: Result<Polled, String>) {
    // Stale: another context, or superseded by a newer request of the same kind
    if epoch != app.epoch || app.polls.inflight.get(&kind) != Some(&req) {
        return;
    }
    app.polls.inflight.remove(&kind);
    let now = app.now;
    let data = match result {
        Ok(d) => {
            if app.polls.failing.remove(&kind) {
                app.log_msg(false, poll_name(&kind), "recovered", None);
            }
            d
        }
        Err(e) => {
            if app.polls.failing.insert(kind.clone()) {
                app.log_msg(true, poll_name(&kind), e, None);
            }
            return;
        }
    };
    match data {
        Polled::Diag(d) => {
            if let Some(c) = d.cpu {
                app.diag_hist.push_back((c, d.heap_pct().unwrap_or(0.0)));
                while app.diag_hist.len() > 240 {
                    app.diag_hist.pop_front();
                }
            }
            app.diag = Some((now, d));
        }
        Polled::Info(i) => app.info = Some(i),
        Polled::BusLan(b) => {
            if let Some((_, prev)) = &app.buslan {
                for ((name, v), (_, pv)) in b.counters.iter().zip(prev.counters.iter()) {
                    if crate::tui::data::is_error_counter(name) && v.unwrap_or(0) > pv.unwrap_or(0)
                    {
                        app.buslan_flash.insert(name.clone(), now);
                    }
                }
                app.buslan_prev = Some(prev.clone());
            }
            app.buslan = Some((now, b));
        }
        Polled::Devices(d) => app.devices = Some((now, d)),
        Polled::Log(l) => {
            // like `tail`: start at the newest line; keep the place on refresh
            let sel = app.system.sel.entry(SysView::Log).or_insert(usize::MAX);
            let prev_len = app.log.as_ref().map_or(0, |(_, o)| o.len());
            if *sel == usize::MAX || *sel + 1 >= prev_len {
                *sel = l.len().saturating_sub(1);
            }
            app.log = Some((now, l));
        }
        Polled::Sites(s) => app.sites = s,
        Polled::History(cid, s) => {
            app.history.insert(cid, s);
        }
        Polled::EnergyDay { pv, usage } => app.energy_day = Some((pv, usage)),
        Polled::ConfigLog(c) => app.commits = Some(c),
        Polled::ConfigDiff(h, lines) => {
            app.diffs.insert(h, lines);
        }
    }
}

fn poll_name(k: &PollKind) -> &'static str {
    match k {
        PollKind::Diag => "diagnostics",
        PollKind::Info => "Miniserver info",
        PollKind::BusLan => "bus & LAN counters",
        PollKind::Devices => "device health",
        PollKind::Log => "system log",
        PollKind::Sites => "sites",
        PollKind::History(_) => "statistics",
        PollKind::EnergyDay => "energy statistics",
        PollKind::ConfigLog => "config history",
        PollKind::ConfigDiff(_) => "config diff",
    }
}

fn cmd_done(app: &mut App, epoch: u64, req: u64, cid: Cid, result: Result<(), String>) {
    if epoch != app.epoch || cid >= app.house.ctrls.len() {
        return;
    }
    let name = app.house.display_name(cid);
    let current = app.pending.get(&cid).map(|p| p.req);
    match result {
        Ok(()) => {
            if current == Some(req)
                && let Some(p) = app.pending.get_mut(&cid)
            {
                p.acked = true;
                let label = p.label.clone();
                app.toast(ToastKind::Ok, format!("{} → {}", name, label));
            }
        }
        Err(e) => {
            let (label, cli) = match app.pending.get_mut(&cid) {
                Some(p) if p.req == req => {
                    p.state = PendState::Failed(e.clone());
                    p.since = app.now;
                    (p.label.clone(), None)
                }
                _ => (String::new(), None::<String>),
            };
            app.fail(format!("{} {}", name, label).trim().to_string(), e, cli);
        }
    }
}

// ── Tick: timers, pollers ───────────────────────────────────────────────────

fn tick(app: &mut App, now: f64) -> Vec<Effect> {
    app.now = now;
    app.frame += 1;
    app.toasts.retain(|t| t.until > now);
    app.store.advance_rate(now as i64);
    // pending: `⋯` for up to 3 s, then `?` (only if the HTTP call hasn't returned)
    let mut done = Vec::new();
    for (cid, p) in app.pending.iter_mut() {
        let age = now - p.since;
        match &p.state {
            PendState::Sent if age > PENDING_TIMEOUT => {
                if p.acked {
                    done.push(*cid);
                } else {
                    p.state = PendState::Unconfirmed;
                }
            }
            PendState::Unconfirmed if age > 10.0 => done.push(*cid),
            PendState::Failed(_) if age > 5.0 => done.push(*cid),
            _ => {}
        }
    }
    for c in done {
        app.pending.remove(&c);
    }
    // Home card order: refreshed every 30 s, frozen while the cards have focus
    let cards_focused = app.screen == Screen::Home && app.home.pane == HomePane::Rooms;
    if app.home.order.is_empty() || (!cards_focused && now - app.home.order_at > 30.0) {
        app.home.order = lists::home_rooms(app);
        app.home.order_at = now;
    }
    schedule(app)
}

fn due(app: &App, kind: &PollKind, every: f64) -> bool {
    !app.polls.inflight.contains_key(kind)
        && app
            .polls
            .last
            .get(kind)
            .is_none_or(|t| app.now - t >= every)
}

fn poll(app: &mut App, kind: PollKind) -> Effect {
    let req = app.req();
    app.polls.last.insert(kind.clone(), app.now);
    app.polls.inflight.insert(kind.clone(), req);
    Effect::Poll { req, kind }
}

/// Pollers run only for what is visible (§5.6, §5.7); a closed view stops polling.
pub fn schedule(app: &mut App) -> Vec<Effect> {
    let mut out = Vec::new();
    let sys = app.screen == Screen::System;
    let view = app.system.view;
    let mut want: Vec<(PollKind, f64)> = vec![
        (PollKind::Info, f64::INFINITY),
        (
            PollKind::Diag,
            if sys && view == SysView::Overview {
                2.0
            } else {
                30.0
            },
        ),
        (
            PollKind::Devices,
            if sys && view == SysView::Devices {
                10.0
            } else {
                60.0
            },
        ),
    ];
    if sys && matches!(view, SysView::Overview | SysView::BusLan) {
        want.push((PollKind::BusLan, 5.0));
    }
    if sys && view == SysView::Log {
        want.push((PollKind::Log, 30.0));
    }
    if sys && view == SysView::Config {
        want.push((PollKind::ConfigLog, f64::INFINITY));
        if let Some(Ok(commits)) = &app.commits
            && let Some(c) = commits.get(app.system.sel.get(&SysView::Config).copied().unwrap_or(0))
            && !app.diffs.contains_key(&c.hash)
        {
            want.push((PollKind::ConfigDiff(c.hash.clone()), f64::INFINITY));
        }
    }
    if app.screen == Screen::Sites && app.contexts.len() > 1 {
        want.push((PollKind::Sites, 15.0));
    }
    if matches!(app.screen, Screen::Energy | Screen::Home) && !app.house.energy.nodes.is_empty() {
        want.push((PollKind::EnergyDay, 300.0));
    }
    if let Some(cid) = inspected(app)
        && app.house.ctrls[cid].has_stats
        && !app.history.contains_key(&cid)
    {
        want.push((PollKind::History(cid), f64::INFINITY));
    }
    // offline: only the cheap info probe
    let offline = matches!(
        app.conn,
        Conn::Offline(_) | Conn::Reconnecting { .. } | Conn::OutOfService
    );
    for (k, every) in want {
        if offline
            && !matches!(
                k,
                PollKind::ConfigLog | PollKind::ConfigDiff(_) | PollKind::Sites
            )
        {
            continue;
        }
        if due(app, &k, every) {
            out.push(poll(app, k));
        }
    }
    out
}

/// The control whose inspector (pane or overlay) is visible.
pub fn inspected(app: &App) -> Option<Cid> {
    if let Some(Overlay::Inspector { cid, .. }) = app.top_overlay() {
        return Some(*cid);
    }
    if app.screen == Screen::Rooms && app.size.0 >= WIDE_ROOMS {
        return lists::selected_ctrl(app);
    }
    None
}

// ── Input ───────────────────────────────────────────────────────────────────

fn paste(app: &mut App, s: &str) {
    match app.overlays.last_mut() {
        Some(Overlay::Palette(p)) => {
            p.line.insert(s);
            p.sel = 0;
        }
        Some(Overlay::Input { line, err, .. }) => {
            line.insert(s);
            *err = None;
        }
        Some(Overlay::Confirm(c)) if c.typed.is_some() => c.input.insert(s),
        _ => return,
    }
    sync_filter(app);
}

fn is_ctrl(k: &KeyEvent, c: char) -> bool {
    k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char(c)
}

fn key(app: &mut App, k: KeyEvent) -> Vec<Effect> {
    if k.kind == KeyEventKind::Release {
        return Vec::new();
    }
    if is_ctrl(&k, 'c') {
        return quit(app);
    }
    if is_ctrl(&k, 'z') {
        return vec![Effect::Suspend];
    }
    if app.size.0 < MIN_W || app.size.1 < MIN_H {
        // Too small: only quitting works; state is kept
        if k.code == KeyCode::Char('q') && app.overlays.is_empty() {
            return quit(app);
        }
        return Vec::new();
    }
    if !app.overlays.is_empty() {
        return overlay_key(app, k);
    }
    let code = keymap::code(&k);
    let ctxs = contexts(app);
    let Some(b) = keymap::lookup(&ctxs, &code) else {
        return Vec::new();
    };
    // One press, one action: auto-repeat only for movement and steps
    if !b.repeat {
        if k.kind == KeyEventKind::Repeat {
            return Vec::new();
        }
        // Terminals without the kitty protocol report a held key as fast
        // presses. Only keys that act (or flip a toggle) treat those as
        // repeats; navigation must take quick double presses.
        if acts(b.cmd)
            && let Some((last, t)) = &app.last_key
            && *last == code
            && app.now - t < REPEAT_GAP
        {
            app.last_key = Some((code, app.now));
            return Vec::new();
        }
    }
    app.last_key = Some((code, app.now));
    command(app, b.cmd)
}

/// Commands that change the house or flip a toggle: a held key must not
/// fire them twice.
fn acts(cmd: Cmd) -> bool {
    matches!(
        cmd,
        Cmd::Verb(_)
            | Cmd::Set
            | Cmd::Mode
            | Cmd::Menu
            | Cmd::Pull
            | Cmd::Reboot
            | Cmd::Install
            | Cmd::Pin
            | Cmd::Mark
            | Cmd::MarkAll
            | Cmd::Mute
            | Cmd::Pause
            | Cmd::Follow
            | Cmd::Favorites
    )
}

/// Active keymap contexts, most specific first.
pub fn contexts(app: &App) -> Vec<Ctx> {
    let mut v = Vec::new();
    match app.screen {
        Screen::Rooms => v.extend([Ctx::Rooms, Ctx::Item]),
        Screen::Events => v.extend([Ctx::Events, Ctx::Item]),
        Screen::System => {
            match app.system.view {
                SysView::Log => v.push(Ctx::Log),
                SysView::Config => v.push(Ctx::Config),
                SysView::Update => v.push(Ctx::Update),
                _ => {}
            }
            v.push(Ctx::System);
        }
        Screen::Home | Screen::Energy | Screen::Sites => v.push(Ctx::Item),
    }
    v
}

fn quit(app: &mut App) -> Vec<Effect> {
    app.quit = true;
    vec![Effect::SaveState(ui_state(app)), Effect::Quit]
}

pub fn ui_state(app: &App) -> UiState {
    let mut st = app.ui_state.clone();
    st.mutes = app.store.mutes.iter().cloned().collect();
    st.mutes.sort();
    st.last_screen = Some(app.screen);
    st
}

// ── Commands ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum Nav {
    Down,
    Up,
    Top,
    Bottom,
    PageDown,
    PageUp,
}

fn moved(idx: usize, len: usize, nav: Nav, page: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let half = (page / 2).max(1);
    let i = match nav {
        Nav::Down => idx + 1,
        Nav::Up => idx.saturating_sub(1),
        Nav::Top => 0,
        Nav::Bottom => len - 1,
        Nav::PageDown => idx + half,
        Nav::PageUp => idx.saturating_sub(half),
    };
    i.min(len - 1)
}

pub fn command(app: &mut App, cmd: Cmd) -> Vec<Effect> {
    let page = app.ui.borrow().page.max(2);
    match cmd {
        Cmd::Screen(n) => {
            if let Some(s) = Screen::from_num(n) {
                go(app, s);
                return schedule(app);
            }
        }
        Cmd::Quit => return quit(app),
        Cmd::Help => app.overlays.push(Overlay::Help {
            scroll: 0,
            filter: String::new(),
        }),
        Cmd::Palette => app.overlays.push(Overlay::Palette(PaletteState::default())),
        Cmd::Contexts => {
            let sel = app
                .contexts
                .iter()
                .position(|c| *c == app.ctx_name)
                .unwrap_or(0);
            app.overlays.push(Overlay::Contexts { sel });
        }
        Cmd::MsgLog => {
            app.unread_errors = 0;
            app.overlays.push(Overlay::MsgLog { scroll: 0 });
        }
        Cmd::Pause => {
            app.paused = !app.paused;
            if !app.paused {
                let buf = std::mem::take(&mut app.paused_buf);
                apply(app, buf);
            }
        }
        Cmd::Refresh => {
            app.toast(ToastKind::Info, "refreshing structure…");
            return vec![Effect::Refresh];
        }
        Cmd::Back => back(app),
        Cmd::NextPane => pane(app, 1, true),
        Cmd::PrevPane => pane(app, -1, true),
        Cmd::Left => pane(app, -1, false),
        Cmd::Right => pane(app, 1, false),
        Cmd::Down => nav(app, Nav::Down, page),
        Cmd::Up => nav(app, Nav::Up, page),
        Cmd::Top => nav(app, Nav::Top, page),
        Cmd::Bottom => nav(app, Nav::Bottom, page),
        Cmd::PageDown => nav(app, Nav::PageDown, page),
        Cmd::PageUp => nav(app, Nav::PageUp, page),
        Cmd::PrevSub => return sub(app, -1),
        Cmd::NextSub => return sub(app, 1),
        Cmd::Filter => filter(app),
        Cmd::GroupBy => {
            app.rooms.group = match app.rooms.group {
                GroupBy::Room => GroupBy::Category,
                GroupBy::Category => GroupBy::Type,
                GroupBy::Type => GroupBy::Room,
            };
            app.rooms.sel_group = None;
            app.rooms.frozen = None;
        }
        Cmd::Favorites => {
            app.rooms.fav_only = !app.rooms.fav_only;
        }
        Cmd::Sort => {
            app.rooms.sort = match app.rooms.sort {
                RoomSort::Name => RoomSort::Activity,
                RoomSort::Activity => RoomSort::Temp,
                RoomSort::Temp => RoomSort::Name,
            };
            app.rooms.frozen = None;
            freeze_rooms(app);
        }
        Cmd::Follow => {
            app.events.follow = !app.events.follow;
            if app.events.follow {
                app.events.sel = None;
            } else {
                app.events.seen = app.store.next_seq;
                app.events.sel = sel_event(app);
            }
        }
        Cmd::Chip => {
            if let Some(e) = sel_event(app)
                .and_then(|s| app.store.index_of(s))
                .map(|i| &app.store.events[i])
                && let Some(r) = e.cid.and_then(|c| app.house.ctrls[c].room)
            {
                let chip = Chip::Room(r);
                if let Some(i) = app.events.chips.iter().position(|c| *c == chip) {
                    app.events.chips.remove(i);
                } else {
                    app.events.chips.push(chip);
                }
            } else if !app.events.chips.is_empty() {
                app.events.chips.clear();
            }
        }
        Cmd::Mute => {
            if let Some(cid) = target_ctrl(app) {
                let top = app.house.top(cid);
                let u = app.house.ctrls[top].uuid.clone();
                let name = app.house.ctrls[top].name.clone();
                if !app.store.mutes.remove(&u) {
                    app.store.mutes.insert(u);
                    app.toast(ToastKind::Info, format!("muted {} (X shows muted)", name));
                } else {
                    app.toast(ToastKind::Info, format!("unmuted {}", name));
                }
                return vec![Effect::SaveState(ui_state(app))];
            }
        }
        Cmd::ShowMuted => {
            app.events.show_muted = !app.events.show_muted;
            app.store.record_noisy = app.events.show_muted;
        }
        Cmd::NextMatch | Cmd::PrevMatch => next_match(app, matches!(cmd, Cmd::NextMatch)),
        // Pull only reads from the Miniserver (FTP download + local git commit): allowed read-only
        Cmd::Pull => {
            app.toast(ToastKind::Info, "lox config pull …");
            return vec![Effect::ConfigPull];
        }
        Cmd::Reboot | Cmd::Install => {
            if app.opts.read_only {
                app.toast(ToastKind::Info, "read-only");
            } else {
                let reboot = cmd == Cmd::Reboot;
                app.overlays.push(Overlay::Confirm(Confirm {
                    title: if reboot {
                        "Reboot Miniserver".into()
                    } else {
                        "Install firmware update".into()
                    },
                    body: vec![
                        if reboot {
                            "The Miniserver restarts; the house is unresponsive for ~1–2 min."
                                .into()
                        } else {
                            "Installs the latest release and reboots (several minutes).".into()
                        },
                        format!("Type the context name '{}' to confirm.", app.ctx_name),
                    ],
                    plans: Vec::new(),
                    effect: Some(Box::new(if reboot {
                        Effect::Reboot
                    } else {
                        Effect::Install
                    })),
                    typed: Some(app.ctx_name.clone()),
                    input: Line::default(),
                }));
            }
        }
        // item vocabulary
        Cmd::Verb(v) => return verb(app, v),
        Cmd::Set => set(app),
        Cmd::Mode => mode(app),
        Cmd::Menu => menu(app),
        Cmd::Inspect => return inspect(app),
        Cmd::Wiring => return wiring(app),
        Cmd::Pin => {
            if let Some(cid) = target_ctrl(app) {
                let u = app.house.ctrls[cid].uuid.clone();
                let name = app.house.display_name(cid);
                if let Some(i) = app.ui_state.pins.iter().position(|p| *p == u) {
                    app.ui_state.pins.remove(i);
                    app.toast(ToastKind::Info, format!("unpinned {}", name));
                } else {
                    app.ui_state.pins.push(u);
                    app.toast(ToastKind::Ok, format!("★ pinned {} to Home", name));
                }
                return vec![Effect::SaveState(ui_state(app))];
            }
        }
        Cmd::ShowEvents => {
            if let Some(cid) = target_ctrl(app) {
                let top = app.house.top(cid);
                app.events.chips = vec![Chip::Ctrl(top)];
                app.events.follow = true;
                app.events.sel = None;
                go(app, Screen::Events);
            }
        }
        Cmd::Yank => {
            if let Some(s) = yank_text(app) {
                app.toast(ToastKind::Ok, format!("copied: {}", s));
                return vec![Effect::Copy(s)];
            }
        }
        Cmd::YankUuid => {
            if let Some(cid) = target_ctrl(app) {
                let u = app.house.ctrls[cid].uuid.clone();
                app.toast(ToastKind::Ok, format!("copied UUID {}", u));
                return vec![Effect::Copy(u)];
            }
        }
        Cmd::Mark | Cmd::MarkAll => mark(app, cmd == Cmd::MarkAll),
    }
    Vec::new()
}

pub fn go(app: &mut App, s: Screen) {
    app.screen = s;
    if s == Screen::Rooms {
        freeze_rooms(app);
    }
}

/// Freeze the live room sort while the room list has focus (§7.2).
fn freeze_rooms(app: &mut App) {
    if app.screen == Screen::Rooms && app.rooms.pane == 0 && app.rooms.sort != RoomSort::Name {
        if app.rooms.frozen.is_none() {
            app.rooms.frozen = Some(lists::groups(app).into_iter().map(|g| g.key).collect());
        }
    } else {
        app.rooms.frozen = None;
    }
}

fn back(app: &mut App) {
    match app.screen {
        Screen::Rooms => {
            if app.rooms.pane >= 1 && !app.rooms.filter_ctrls.is_empty() {
                app.rooms.filter_ctrls.clear();
            } else if !app.rooms.marks.is_empty() {
                app.rooms.marks.clear();
            } else if app.rooms.pane == 0 && !app.rooms.filter_groups.is_empty() {
                app.rooms.filter_groups.clear();
            } else if app.rooms.pane > 0 {
                app.rooms.pane -= 1;
                freeze_rooms(app);
            }
        }
        Screen::Events => {
            if !app.events.filter.is_empty() {
                app.events.filter.clear();
            } else if !app.events.chips.is_empty() {
                app.events.chips.clear();
            } else if !app.events.follow {
                app.events.follow = true;
                app.events.sel = None;
            }
        }
        Screen::System => {
            if app.system.pane > 0 {
                app.system.pane = 0;
            } else if !app.system.log_search.is_empty() {
                app.system.log_search.clear();
            }
        }
        _ => {}
    }
}

fn pane(app: &mut App, d: i32, cycle: bool) {
    match app.screen {
        Screen::Rooms => {
            let max = if app.size.0 >= WIDE_ROOMS { 2 } else { 1 };
            let p = app.rooms.pane as i32 + d;
            app.rooms.pane = if cycle {
                p.rem_euclid(max + 1)
            } else {
                p.clamp(0, max)
            } as u8;
            freeze_rooms(app);
        }
        Screen::Home => {
            let order = HomePane::ORDER;
            let i = order.iter().position(|p| *p == app.home.pane).unwrap_or(0) as i32;
            if !cycle && app.home.pane == HomePane::Rooms {
                // h/l inside the card grid move between cards
                let len = app.home.order.len();
                let i = app.home.sel_room as i32 + d;
                if i >= 0 && (i as usize) < len {
                    app.home.sel_room = i as usize;
                    return;
                }
                if d < 0 {
                    return;
                }
                app.home.pane = HomePane::Attention;
                return;
            }
            if !cycle && d < 0 && app.home.pane != HomePane::Live {
                app.home.pane = HomePane::Rooms;
                return;
            }
            app.home.pane = order[(i + d).rem_euclid(order.len() as i32) as usize];
        }
        Screen::System => {
            if app.system.view == SysView::Config {
                app.system.pane = if cycle {
                    1 - app.system.pane.min(1)
                } else {
                    (app.system.pane as i32 + d).clamp(0, 1) as u8
                };
            }
        }
        Screen::Events => {}
        _ => {}
    }
}

fn nav(app: &mut App, n: Nav, page: usize) {
    match app.screen {
        Screen::Rooms => match app.rooms.pane {
            0 => {
                let gs = lists::groups(app);
                let cur = lists::current_group(app);
                let i = gs.iter().position(|g| g.key == cur).unwrap_or(0);
                let j = moved(i, gs.len(), n, page);
                app.rooms.sel_group = gs.get(j).map(|g| g.key.clone());
            }
            1 => {
                let key = lists::current_group(app);
                let cids = lists::row_cids(&lists::ctrl_rows(app, &key));
                let cur = lists::selected_ctrl(app);
                let i = cur
                    .and_then(|c| cids.iter().position(|x| *x == c))
                    .unwrap_or(0);
                let j = moved(i, cids.len(), n, page);
                if let Some(c) = cids.get(j) {
                    app.rooms
                        .sel_ctrl
                        .insert(key, app.house.ctrls[*c].uuid.clone());
                }
            }
            _ => {}
        },
        Screen::Home => {
            let cols = app.ui.borrow().home_cols.max(1);
            match app.home.pane {
                HomePane::Rooms => {
                    let len = app.home.order.len();
                    let i = app.home.sel_room;
                    app.home.sel_room = match n {
                        Nav::Down => (i + cols).min(len.saturating_sub(1)),
                        Nav::Up => i.saturating_sub(cols),
                        _ => moved(i, len, n, page),
                    };
                }
                HomePane::Attention => {
                    let len = lists::attention(app).len();
                    app.home.sel_attention = moved(app.home.sel_attention, len, n, page);
                }
                HomePane::Pinned => {
                    let len = app.pinned().len();
                    app.home.sel_pin = moved(app.home.sel_pin, len, n, page);
                }
                HomePane::Quick => {
                    let len = app.scenes.len();
                    app.home.sel_quick = moved(app.home.sel_quick, len, n, page);
                }
                _ => {}
            }
        }
        Screen::Events => {
            let rows = lists::event_rows(app);
            if rows.is_empty() {
                return;
            }
            let cur = sel_event(app)
                .and_then(|s| rows.iter().position(|i| app.store.events[*i].seq == s))
                .unwrap_or(rows.len() - 1);
            let j = moved(cur, rows.len(), n, page);
            if j == rows.len() - 1 && matches!(n, Nav::Down | Nav::Bottom | Nav::PageDown) {
                // reaching the newest row resumes following
                app.events.follow = true;
                app.events.sel = None;
            } else {
                if app.events.follow {
                    app.events.seen = app.store.next_seq;
                }
                app.events.follow = false;
                app.events.sel = Some(app.store.events[rows[j]].seq);
            }
        }
        Screen::Energy => {
            let len = app.house.energy.meters.len();
            app.energy.sel_meter = moved(app.energy.sel_meter, len, n, page);
        }
        Screen::System => {
            let view = app.system.view;
            if view == SysView::Config && app.system.pane == 1 {
                let len = app
                    .commits
                    .as_ref()
                    .and_then(|c| c.as_ref().ok())
                    .and_then(|c| c.get(app.system.sel.get(&view).copied().unwrap_or(0)))
                    .and_then(|c| app.diffs.get(&c.hash))
                    .map_or(0, |d| d.len());
                app.system.diff_scroll = moved(app.system.diff_scroll, len, n, page);
                return;
            }
            let len = sys_len(app, view);
            let cur = app.system.sel.get(&view).copied().unwrap_or(0);
            let j = moved(cur, len, n, page);
            if view == SysView::Config && j != cur {
                app.system.diff_scroll = 0;
            }
            app.system.sel.insert(view, j);
        }
        Screen::Sites => {
            let len = app.contexts.len();
            app.sites_ui.sel = moved(app.sites_ui.sel, len, n, page);
        }
    }
}

pub fn sys_len(app: &App, view: SysView) -> usize {
    match view {
        SysView::Devices => app.devices.as_ref().map_or(0, |(_, d)| d.len()),
        SysView::BusLan => app.buslan.as_ref().map_or(0, |(_, b)| b.counters.len()),
        SysView::Log => log_rows(app).len(),
        SysView::Config => app
            .commits
            .as_ref()
            .and_then(|c| c.as_ref().ok())
            .map_or(0, |c| c.len()),
        _ => 0,
    }
}

/// Log lines matching the search (indices), oldest first.
pub fn log_rows(app: &App) -> Vec<usize> {
    let q = app.system.log_search.to_lowercase();
    app.log
        .as_ref()
        .map(|(_, l)| {
            l.iter()
                .enumerate()
                .filter(|(_, l)| q.is_empty() || l.text.to_lowercase().contains(&q))
                .map(|(i, _)| i)
                .collect()
        })
        .unwrap_or_default()
}

fn next_match(app: &mut App, fwd: bool) {
    let view = app.system.view;
    let len = sys_len(app, view);
    if len == 0 {
        return;
    }
    let cur = app.system.sel.get(&view).copied().unwrap_or(0);
    let j = if fwd {
        (cur + 1) % len
    } else {
        (cur + len - 1) % len
    };
    app.system.sel.insert(view, j);
    app.system.diff_scroll = 0;
}

fn sub(app: &mut App, d: i32) -> Vec<Effect> {
    let cyc = |i: usize, n: usize| ((i as i32 + d).rem_euclid(n as i32)) as usize;
    match app.screen {
        Screen::Rooms => {
            for _ in 0..if d > 0 { 1 } else { 2 } {
                command(app, Cmd::GroupBy);
            }
        }
        Screen::Energy => {
            let i = ERange::ALL
                .iter()
                .position(|r| *r == app.energy.range)
                .unwrap_or(0);
            app.energy.range = ERange::ALL[cyc(i, 4)];
        }
        Screen::System => {
            let i = SysView::ALL
                .iter()
                .position(|r| *r == app.system.view)
                .unwrap_or(0);
            app.system.view = SysView::ALL[cyc(i, SysView::ALL.len())];
            app.system.pane = 0;
            return schedule(app);
        }
        _ => {}
    }
    Vec::new()
}

fn filter(app: &mut App) {
    let target = match app.screen {
        Screen::Rooms if app.rooms.pane == 0 => FilterTarget::Groups,
        Screen::Rooms => FilterTarget::Ctrls,
        Screen::Events => FilterTarget::Events,
        Screen::System if app.system.view == SysView::Log => FilterTarget::Log,
        // Everything else: the palette is the filter over everything
        _ => {
            app.overlays.push(Overlay::Palette(PaletteState::default()));
            return;
        }
    };
    let prev = filter_value(app, target).to_string();
    app.overlays.push(Overlay::Input {
        kind: InputKind::Filter {
            target,
            prev: prev.clone(),
        },
        line: Line::new(&prev),
        err: None,
    });
}

fn filter_value(app: &App, t: FilterTarget) -> &str {
    match t {
        FilterTarget::Groups => &app.rooms.filter_groups,
        FilterTarget::Ctrls => &app.rooms.filter_ctrls,
        FilterTarget::Events => &app.events.filter,
        FilterTarget::Log => &app.system.log_search,
        FilterTarget::Help => match app
            .overlays
            .iter()
            .rev()
            .find(|o| matches!(o, Overlay::Help { .. }))
        {
            Some(Overlay::Help { filter, .. }) => filter,
            _ => "",
        },
    }
}

fn set_filter(app: &mut App, t: FilterTarget, v: String) {
    match t {
        FilterTarget::Groups => app.rooms.filter_groups = v,
        FilterTarget::Ctrls => app.rooms.filter_ctrls = v,
        FilterTarget::Events => app.events.filter = v,
        FilterTarget::Log => {
            app.system.log_search = v;
            app.system.sel.insert(SysView::Log, usize::MAX / 2);
            let len = sys_len(app, SysView::Log);
            app.system.sel.insert(SysView::Log, len.saturating_sub(1));
        }
        FilterTarget::Help => {
            if let Some(Overlay::Help { filter, scroll }) = app
                .overlays
                .iter_mut()
                .rev()
                .find(|o| matches!(o, Overlay::Help { .. }))
            {
                *filter = v;
                *scroll = 0;
            }
        }
    }
}

/// Incremental filter: the target follows the input line as you type.
fn sync_filter(app: &mut App) {
    if let Some(Overlay::Input {
        kind: InputKind::Filter { target, .. },
        line,
        ..
    }) = app.overlays.last()
    {
        let (t, v) = (*target, line.buf.clone());
        set_filter(app, t, v);
    }
}

fn mark(app: &mut App, all: bool) {
    if app.screen != Screen::Rooms || app.rooms.pane != 1 {
        return;
    }
    let key = lists::current_group(app);
    let cids = lists::row_cids(&lists::ctrl_rows(app, &key));
    if all {
        if cids.iter().all(|c| app.rooms.marks.contains(c)) {
            app.rooms.marks.clear();
        } else {
            app.rooms.marks.extend(cids);
        }
    } else if let Some(c) = lists::selected_ctrl(app) {
        if !app.rooms.marks.remove(&c) {
            app.rooms.marks.insert(c);
        }
        nav(app, Nav::Down, 2);
    }
}

// ── Targets ─────────────────────────────────────────────────────────────────

/// What the item keys act on.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Ctrl(Cid),
    Room(usize),
    Scene(String),
    Site(String),
    Event(u64),
    None,
}

pub fn target(app: &App) -> Target {
    if let Some(Overlay::Inspector { cid, .. }) = app.top_overlay() {
        return Target::Ctrl(*cid);
    }
    match app.screen {
        Screen::Rooms => {
            if app.rooms.pane == 0 {
                match lists::current_group(app) {
                    GKey::Room(r) => Target::Room(r),
                    _ => Target::None,
                }
            } else {
                lists::selected_ctrl(app).map_or(Target::None, Target::Ctrl)
            }
        }
        Screen::Home => match app.home.pane {
            HomePane::Rooms => app
                .home
                .order
                .get(app.home.sel_room)
                .map_or(Target::None, |r| Target::Room(*r)),
            HomePane::Attention => lists::attention(app)
                .get(app.home.sel_attention)
                .and_then(|a| a.cid)
                .map_or(Target::None, Target::Ctrl),
            HomePane::Pinned => app
                .pinned()
                .get(app.home.sel_pin)
                .map_or(Target::None, |c| Target::Ctrl(*c)),
            HomePane::Quick => app
                .scenes
                .get(app.home.sel_quick)
                .map_or(Target::None, |s| Target::Scene(s.clone())),
            HomePane::Energy => app.house.energy.efm.map_or(Target::None, Target::Ctrl),
            HomePane::Live => Target::None,
        },
        Screen::Events => sel_event(app).map_or(Target::None, Target::Event),
        Screen::Energy => app
            .house
            .energy
            .meters
            .get(app.energy.sel_meter)
            .map_or(Target::None, |c| Target::Ctrl(*c)),
        Screen::Sites => app
            .contexts
            .get(app.sites_ui.sel)
            .map_or(Target::None, |s| Target::Site(s.clone())),
        Screen::System => Target::None,
    }
}

/// The selected event (newest row while following).
pub fn sel_event(app: &App) -> Option<u64> {
    match app.events.sel {
        Some(s) if app.store.index_of(s).is_some() => Some(s),
        _ => {
            let rows = lists::event_rows(app);
            rows.last().map(|i| app.store.events[*i].seq)
        }
    }
}

/// Target resolved to a control (events → their control).
pub fn target_ctrl(app: &App) -> Option<Cid> {
    match target(app) {
        Target::Ctrl(c) => Some(c),
        Target::Event(s) => app.store.index_of(s).and_then(|i| app.store.events[i].cid),
        _ => None,
    }
}

fn yank_text(app: &App) -> Option<String> {
    let q = crate::actions::shell_quote;
    match target(app) {
        Target::Ctrl(cid) => {
            let c = &app.house.ctrls[cid];
            let room = app.house.room_name(cid);
            let optimistic = pending_opt(app, cid);
            Some(
                match vm::verb(
                    &app.store,
                    &app.house,
                    cid,
                    Verb::Primary,
                    optimistic,
                    app.last_dir.get(&cid).copied(),
                ) {
                    Some(p) => p.action.to_cli(&c.name, room),
                    None => match room {
                        Some(r) => format!("lox get {} -r {}", q(&c.name), q(r)),
                        None => format!("lox get {}", q(&c.name)),
                    },
                },
            )
        }
        Target::Event(s) => {
            let e = &app.store.events[app.store.index_of(s)?];
            let cid = app.house.top(e.cid?);
            let c = &app.house.ctrls[cid];
            Some(match app.house.room_name(cid) {
                Some(r) => format!("lox stream -c {} -r {}", q(&c.name), q(r)),
                None => format!("lox stream -c {}", q(&c.name)),
            })
        }
        Target::Scene(s) => Some(format!("lox run {}", q(&s))),
        Target::Room(r) => Some(format!(
            "lox ls -r {} --values",
            q(&app.house.rooms[r].name)
        )),
        Target::Site(s) => Some(format!("lox ctx use {}", q(&s))),
        Target::None => None,
    }
}

fn pending_opt(app: &App, cid: Cid) -> Option<f64> {
    app.pending
        .get(&cid)
        .filter(|p| p.state == PendState::Sent)
        .and_then(|p| p.optimistic)
}

// ── Actions ─────────────────────────────────────────────────────────────────

fn marked(app: &App) -> Vec<Cid> {
    if app.screen == Screen::Rooms
        && app.rooms.pane == 1
        && !app.rooms.marks.is_empty()
        && app.overlays.is_empty()
    {
        let mut v: Vec<Cid> = app.rooms.marks.iter().copied().collect();
        v.sort_unstable();
        v
    } else {
        Vec::new()
    }
}

fn verb(app: &mut App, v: Verb) -> Vec<Effect> {
    let marks = marked(app);
    if !marks.is_empty() {
        let plans: Vec<Plan> = marks
            .iter()
            .filter_map(|&c| {
                vm::verb(
                    &app.store,
                    &app.house,
                    c,
                    v,
                    pending_opt(app, c),
                    app.last_dir.get(&c).copied(),
                )
            })
            .collect();
        if plans.is_empty() {
            app.toast(ToastKind::Info, "nothing to do for the marked controls");
            return Vec::new();
        }
        if plans.len() < marks.len() {
            app.toast(
                ToastKind::Info,
                format!("{} of {} applied", plans.len(), marks.len()),
            );
        }
        return run_plans(app, plans);
    }
    match target(app) {
        Target::Ctrl(cid) => {
            let optimistic = pending_opt(app, cid);
            match vm::verb(
                &app.store,
                &app.house,
                cid,
                v,
                optimistic,
                app.last_dir.get(&cid).copied(),
            ) {
                Some(p) => run_plans(app, vec![p]),
                None => {
                    if v == Verb::Primary && !app.house.ctrls[cid].kind.is_actuator() {
                        return inspect(app);
                    }
                    let what = match v {
                        Verb::Primary => "a primary action",
                        Verb::Plus | Verb::Minus => "a value to step",
                        Verb::Min | Verb::Max => "a range",
                        Verb::Stop => "anything to stop",
                    };
                    let name = app.house.display_name(cid);
                    app.toast(ToastKind::Info, format!("{} has no {}", name, what));
                    Vec::new()
                }
            }
        }
        Target::Room(r) => match v {
            Verb::Primary => {
                if app.screen == Screen::Home {
                    return inspect(app);
                }
                app.rooms.pane = 1;
                freeze_rooms(app);
                Vec::new()
            }
            Verb::Max | Verb::Min => {
                let plans = palette::room_lights(app, r, v == Verb::Max);
                if plans.is_empty() {
                    app.toast(ToastKind::Info, "no lights in this room");
                    return Vec::new();
                }
                run_plans(app, plans)
            }
            Verb::Stop => {
                let plans: Vec<Plan> = app.house.rooms[r]
                    .ctrls
                    .iter()
                    .filter(|c| {
                        app.house.ctrls[**c].kind == Kind::Blind
                            && vm::motion(&app.store, &app.house, **c).is_some()
                    })
                    .map(|&cid| Plan {
                        cid,
                        action: Action::Blind(BlindCmd::Stop),
                        label: "stop".into(),
                    })
                    .collect();
                if plans.is_empty() {
                    app.toast(ToastKind::Info, "no blinds moving");
                    return Vec::new();
                }
                run_plans(app, plans)
            }
            _ => Vec::new(),
        },
        Target::Scene(s) if v == Verb::Primary => run_scene(app, s),
        Target::Site(_) | Target::Scene(_) if v == Verb::Primary => inspect(app),
        Target::Event(_) => {
            if let Some(cid) = target_ctrl(app) {
                let optimistic = pending_opt(app, cid);
                if let Some(p) = vm::verb(
                    &app.store,
                    &app.house,
                    cid,
                    v,
                    optimistic,
                    app.last_dir.get(&cid).copied(),
                ) {
                    return run_plans(app, vec![p]);
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn run_scene(app: &mut App, s: String) -> Vec<Effect> {
    if app.opts.read_only {
        app.toast(ToastKind::Info, "read-only — actions are disabled");
        return Vec::new();
    }
    app.toast(ToastKind::Info, format!("▶ {}", s));
    vec![Effect::RunScene(s)]
}

/// Check read-only / connection / locks / risk, then send or ask.
pub fn run_plans(app: &mut App, plans: Vec<Plan>) -> Vec<Effect> {
    if app.opts.read_only {
        app.toast(ToastKind::Info, "read-only — actions are disabled");
        return Vec::new();
    }
    if !app.live() && !app.opts.demo {
        app.fail(
            "not sent",
            "not connected",
            plans.first().map(|p| cli(app, p)),
        );
        return Vec::new();
    }
    let mut ok = Vec::new();
    for p in plans {
        let locked = vm::view(&app.store, &app.house, p.cid).locked;
        if locked && p.action != Action::UnlockControl {
            let name = app.house.display_name(p.cid);
            app.toast(
                ToastKind::Info,
                format!("{} is locked (a → unlock control)", name),
            );
            continue;
        }
        ok.push(p);
    }
    if ok.is_empty() {
        return Vec::new();
    }
    // secured alarm without a PIN → ask for it (masked)
    if ok.iter().any(|p| needs_pin(app, p)) {
        app.overlays.push(Overlay::Input {
            kind: InputKind::Pin { plans: ok },
            line: Line::default(),
            err: None,
        });
        return Vec::new();
    }
    let risky = ok.iter().any(|p| p.action.risk() != Risk::None);
    if risky || ok.len() > BULK_CONFIRM {
        let body = ok
            .iter()
            .take(8)
            .map(|p| format!("{} → {}", app.house.display_name(p.cid), p.label))
            .chain((ok.len() > 8).then(|| format!("… and {} more", ok.len() - 8)))
            .collect();
        app.overlays.push(Overlay::Confirm(Confirm {
            title: if ok.len() == 1 {
                format!("{}: {}?", app.house.display_name(ok[0].cid), ok[0].label)
            } else {
                format!("Apply to {} controls?", ok.len())
            },
            body,
            plans: ok,
            effect: None,
            typed: None,
            input: Line::default(),
        }));
        return Vec::new();
    }
    send_all(app, ok)
}

fn needs_pin(app: &App, p: &Plan) -> bool {
    matches!(&p.action, Action::Alarm { cmd, pin: None } if *cmd != AlarmCmd::Quit)
        && app.house.ctrls[p.cid].is_secured
}

fn cli(app: &App, p: &Plan) -> String {
    let c = &app.house.ctrls[app.house.top(p.cid)];
    let name = if app.house.ctrls[p.cid].parent.is_some() {
        app.house.ctrls[p.cid].uuid.clone()
    } else {
        c.name.clone()
    };
    p.action.to_cli(&name, app.house.room_name(p.cid))
}

fn send_all(app: &mut App, plans: Vec<Plan>) -> Vec<Effect> {
    let mut out = Vec::new();
    for p in plans {
        if let Some(e) = send(app, p) {
            out.push(e);
        }
    }
    out
}

/// Absolute value steps (`+`/`-` held down) sent to the same control within
/// one frame collapse into the last one (§4.1 rule 8). Everything else passes
/// through in order.
pub fn coalesce(effects: Vec<Effect>) -> Vec<Effect> {
    let last: std::collections::HashMap<Cid, usize> = effects
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Effect::Send {
                cid,
                coalesce: true,
                ..
            } => Some((*cid, i)),
            _ => None,
        })
        .collect();
    effects
        .into_iter()
        .enumerate()
        .filter(|(i, e)| !matches!(e, Effect::Send { cid, coalesce: true, .. } if last[cid] != *i))
        .map(|(_, e)| e)
        .collect()
}

fn optimistic_of(a: &Action) -> Option<f64> {
    match a {
        Action::Dim(v) => Some(*v),
        Action::Blind(BlindCmd::Position(v)) => Some(*v),
        Action::Thermostat(ThermoCmd::ComfortTemp(v)) => Some(*v),
        Action::Value(v) => v.parse().ok(),
        _ => None,
    }
}

fn send(app: &mut App, p: Plan) -> Option<Effect> {
    let step = optimistic_of(&p.action).is_some();
    // A conflicting write while the first is unconfirmed waits (§7.3)
    if !step
        && let Some(pe) = app.pending.get(&p.cid)
        && pe.state == PendState::Sent
        && !pe.acked
    {
        let name = app.house.display_name(p.cid);
        app.toast(
            ToastKind::Info,
            format!("{}: waiting for the previous command", name),
        );
        return None;
    }
    let req = app.req();
    let cli = cli(app, &p);
    match &p.action {
        Action::Blind(BlindCmd::Up) => {
            app.last_dir.insert(p.cid, -1);
        }
        Action::Blind(BlindCmd::Down) => {
            app.last_dir.insert(p.cid, 1);
        }
        _ => {}
    }
    app.pending.insert(
        p.cid,
        Pending {
            req,
            since: app.now,
            label: p.label.clone(),
            optimistic: optimistic_of(&p.action),
            state: PendState::Sent,
            acked: false,
        },
    );
    Some(Effect::Send {
        req,
        cid: p.cid,
        uuid: app.house.ctrls[p.cid].uuid.clone(),
        cmds: p.action.commands(),
        cli,
        coalesce: step,
    })
}

fn set(app: &mut App) {
    let cids = {
        let m = marked(app);
        if m.is_empty() {
            target_ctrl(app).into_iter().collect()
        } else {
            m
        }
    };
    let Some(&first) = cids.first() else { return };
    match vm::set_spec(&app.store, &app.house, first) {
        Some(spec) => {
            let cur = pending_opt(app, first).or(spec.cur).map(|v| {
                let s = format!("{:.1}", v);
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            });
            app.overlays.push(Overlay::Input {
                kind: InputKind::Value { cids, spec },
                line: Line::prefilled(&cur.unwrap_or_default()),
                err: None,
            });
        }
        None => {
            let name = app.house.display_name(first);
            app.toast(ToastKind::Info, format!("{} has no value to set", name));
        }
    }
}

fn mode(app: &mut App) {
    let (cids, title_cid) = match target(app) {
        Target::Room(r) => {
            // room mood: the room's first light controller
            match app.house.rooms[r]
                .ctrls
                .iter()
                .find(|c| app.house.ctrls[**c].kind == Kind::LightCtl)
            {
                Some(c) => (vec![*c], *c),
                None => {
                    app.toast(ToastKind::Info, "no light controller in this room");
                    return;
                }
            }
        }
        _ => {
            let m = marked(app);
            let cids: Vec<Cid> = if m.is_empty() {
                target_ctrl(app).into_iter().collect()
            } else {
                m
            };
            match cids.first() {
                Some(c) => (cids.clone(), *c),
                None => return,
            }
        }
    };
    match vm::modes(&app.store, &app.house, title_cid) {
        Some(choices) if !choices.is_empty() => {
            let sel = choices.iter().position(|c| c.current).unwrap_or(0);
            app.overlays.push(Overlay::Picker {
                cids,
                title: app.house.display_name(title_cid),
                choices,
                sel,
            });
        }
        _ => {
            let name = app.house.display_name(title_cid);
            app.toast(ToastKind::Info, format!("{} has no modes", name));
        }
    }
}

fn menu(app: &mut App) {
    let Some(cid) = target_ctrl(app) else {
        if let Target::Room(r) = target(app) {
            let items = vec![
                ("all lights on".to_string(), Some('>'), Action::On),
                ("all lights off".to_string(), Some('<'), Action::Off),
                (
                    "stop all blinds".to_string(),
                    Some('s'),
                    Action::Blind(BlindCmd::Stop),
                ),
            ];
            app.overlays.push(Overlay::Menu {
                cids: Vec::new(),
                room: Some(r),
                items,
                sel: 0,
            });
        }
        return;
    };
    let items = vm::menu(
        &app.store,
        &app.house,
        cid,
        pending_opt(app, cid),
        app.last_dir.get(&cid).copied(),
    );
    if items.is_empty() {
        let name = app.house.display_name(cid);
        app.toast(ToastKind::Info, format!("{} is read-only", name));
        return;
    }
    app.overlays.push(Overlay::Menu {
        cids: vec![cid],
        room: None,
        items,
        sel: 0,
    });
}

fn inspect(app: &mut App) -> Vec<Effect> {
    match target(app) {
        Target::Room(r) => {
            // Home card / Rooms list → the room's controls
            app.rooms.sel_group = Some(GKey::Room(r));
            if app.rooms.group != GroupBy::Room {
                app.rooms.group = GroupBy::Room;
            }
            app.rooms.pane = 1;
            go(app, Screen::Rooms);
        }
        Target::Ctrl(cid) => {
            if app.screen == Screen::Rooms && app.rooms.pane == 1 && app.size.0 >= WIDE_ROOMS {
                app.rooms.pane = 2;
            } else if app.screen == Screen::Rooms && app.rooms.pane == 2 {
                app.rooms.pane = 1;
            } else if matches!(app.top_overlay(), Some(Overlay::Inspector { .. })) {
                app.overlays.pop();
            } else {
                app.overlays.push(Overlay::Inspector { cid, scroll: 0 });
                return schedule(app);
            }
        }
        Target::Event(_) => {
            if let Some(cid) = target_ctrl(app) {
                reveal(app, cid);
            }
        }
        Target::Scene(s) => return run_scene(app, s),
        Target::Site(s) => {
            if s != app.ctx_name {
                return vec![Effect::SwitchContext(s)];
            }
            app.toast(ToastKind::Info, format!("{} is the active context", s));
        }
        Target::None => {
            if app.screen == Screen::System && app.system.view == SysView::Config {
                app.system.pane = 1;
            }
        }
    }
    Vec::new()
}

/// Go to Rooms with a control selected (its room, restoring place).
pub fn reveal(app: &mut App, cid: Cid) {
    let top = app.house.top(cid);
    let key = match app.house.ctrls[top].room {
        Some(r) => GKey::Room(r),
        None => GKey::Unassigned,
    };
    app.rooms.group = GroupBy::Room;
    app.rooms.filter_ctrls.clear();
    app.rooms.fav_only = false;
    app.rooms
        .sel_ctrl
        .insert(key.clone(), app.house.ctrls[cid].uuid.clone());
    app.rooms.sel_group = Some(key);
    app.rooms.pane = 1;
    go(app, Screen::Rooms);
}

fn wiring(app: &mut App) -> Vec<Effect> {
    let Some(cid) = target_ctrl(app) else {
        return Vec::new();
    };
    // The block is the top-level control (sub-controls are its outputs)
    let top = app.house.top(cid);
    app.overlays.push(Overlay::Wiring(WiringState {
        center: app.house.ctrls[top].uuid.clone(),
        sel: 0,
        back: Vec::new(),
    }));
    if matches!(app.wiring, WiringDoc::None) {
        app.wiring = WiringDoc::Loading;
        return vec![Effect::LoadWiring { download: false }];
    }
    Vec::new()
}

// ── Overlays ────────────────────────────────────────────────────────────────

fn overlay_key(app: &mut App, k: KeyEvent) -> Vec<Effect> {
    let Some(top) = app.overlays.last().cloned() else {
        return Vec::new();
    };
    let code = keymap::code(&k);
    let esc = k.code == KeyCode::Esc;
    let enter = k.code == KeyCode::Enter && k.kind == KeyEventKind::Press;
    match top {
        Overlay::Help { .. } => {
            let Some(Overlay::Help { scroll, .. }) = app.overlays.last_mut() else {
                return Vec::new();
            };
            match code.as_str() {
                "Esc" | "?" | "q" => {
                    app.overlays.pop();
                }
                "j" | "Down" => *scroll += 1,
                "k" | "Up" => *scroll = scroll.saturating_sub(1),
                "C-d" | "PageDown" => *scroll += 10,
                "C-u" | "PageUp" => *scroll = scroll.saturating_sub(10),
                "g" | "Home" => *scroll = 0,
                "/" => {
                    let prev = filter_value(app, FilterTarget::Help).to_string();
                    app.overlays.push(Overlay::Input {
                        kind: InputKind::Filter {
                            target: FilterTarget::Help,
                            prev: prev.clone(),
                        },
                        line: Line::new(&prev),
                        err: None,
                    });
                }
                _ => {}
            }
        }
        Overlay::Palette(_) => return palette_key(app, k, &code),
        Overlay::Menu {
            cids,
            room,
            items,
            sel,
        } => {
            let n = items.len();
            let pick = |i: usize| items.get(i).map(|(_, _, a)| a.clone());
            let chosen = match code.as_str() {
                "Esc" | "q" | "a" => {
                    app.overlays.pop();
                    None
                }
                "j" | "Down" => {
                    set_sel(app, (sel + 1).min(n.saturating_sub(1)));
                    None
                }
                "k" | "Up" => {
                    set_sel(app, sel.saturating_sub(1));
                    None
                }
                "Enter" if enter => pick(sel),
                c => {
                    let ch = if c == "Space" {
                        Some('␣')
                    } else {
                        c.chars().next().filter(|_| c.chars().count() == 1)
                    };
                    items
                        .iter()
                        .position(|(_, k, _)| k.is_some() && *k == ch)
                        .and_then(pick)
                }
            };
            if let Some(action) = chosen {
                app.overlays.pop();
                return act_on(app, &cids, room, action);
            }
        }
        Overlay::Picker {
            cids, choices, sel, ..
        } => {
            let n = choices.len();
            let chosen = match code.as_str() {
                "Esc" | "q" | "m" => {
                    app.overlays.pop();
                    None
                }
                "j" | "Down" => {
                    set_sel(app, (sel + 1).min(n.saturating_sub(1)));
                    None
                }
                "k" | "Up" => {
                    set_sel(app, sel.saturating_sub(1));
                    None
                }
                "Enter" | "Space" if k.kind == KeyEventKind::Press => choices.get(sel).cloned(),
                c if c.len() == 1
                    && c.chars()
                        .next()
                        .is_some_and(|d| d.is_ascii_digit() && d != '0') =>
                {
                    choices.get(c.parse::<usize>().unwrap_or(1) - 1).cloned()
                }
                _ => None,
            };
            if let Some(ch) = chosen {
                app.overlays.pop();
                return act_on(app, &cids, None, ch.action);
            }
        }
        Overlay::Confirm(c) => {
            if esc {
                app.overlays.pop();
                return Vec::new();
            }
            match &c.typed {
                Some(word) => {
                    if enter {
                        if c.input.buf.trim() == word {
                            app.overlays.pop();
                            return confirmed(app, c);
                        }
                        app.toast(ToastKind::Info, format!("type '{}' to confirm", word));
                    } else if let Some(Overlay::Confirm(cm)) = app.overlays.last_mut() {
                        edit_line(&mut cm.input, &k);
                    }
                }
                None => match code.as_str() {
                    "y" | "Y" => {
                        app.overlays.pop();
                        return confirmed(app, c);
                    }
                    // default is No
                    "n" | "N" | "Enter" | "q" => {
                        app.overlays.pop();
                    }
                    _ => {}
                },
            }
        }
        Overlay::Input { kind, line, .. } => {
            if esc {
                app.overlays.pop();
                if let InputKind::Filter { target, prev } = kind {
                    // Esc restores the previous filter
                    set_filter(app, target, prev);
                }
                return Vec::new();
            }
            if enter {
                app.overlays.pop();
                return input_submit(app, kind, line);
            }
            if let Some(Overlay::Input { line, err, .. }) = app.overlays.last_mut() {
                // Filters over lists keep ↑/↓ for moving through results
                if matches!(kind, InputKind::Filter { .. })
                    && matches!(k.code, KeyCode::Up | KeyCode::Down)
                {
                    let cmd = if k.code == KeyCode::Up {
                        Cmd::Up
                    } else {
                        Cmd::Down
                    };
                    let saved = app.overlays.pop();
                    let out = command(app, cmd);
                    app.overlays.extend(saved);
                    return out;
                }
                edit_line(line, &k);
                *err = None;
            }
            sync_filter(app);
        }
        Overlay::Contexts { sel } => match code.as_str() {
            "Esc" | "q" | "C" => {
                app.overlays.pop();
            }
            "j" | "Down" => {
                let n = app.contexts.len();
                set_sel(app, (sel + 1).min(n.saturating_sub(1)));
            }
            "k" | "Up" => set_sel(app, sel.saturating_sub(1)),
            "Enter" if enter => {
                app.overlays.pop();
                if let Some(c) = app.contexts.get(sel).cloned()
                    && c != app.ctx_name
                {
                    return vec![Effect::SwitchContext(c)];
                }
            }
            _ => {}
        },
        Overlay::MsgLog { .. } => {
            let Some(Overlay::MsgLog { scroll }) = app.overlays.last_mut() else {
                return Vec::new();
            };
            match code.as_str() {
                "Esc" | "q" | "!" => {
                    app.overlays.pop();
                }
                "j" | "Down" => *scroll += 1,
                "k" | "Up" => *scroll = scroll.saturating_sub(1),
                "c" => {
                    app.msglog.clear();
                    app.overlays.pop();
                }
                _ => {}
            }
        }
        Overlay::Wiring(w) => return wiring_key(app, w, &code),
        Overlay::Inspector { .. } => match code.as_str() {
            "Esc" | "q" | "Enter" => {
                app.overlays.pop();
            }
            "j" | "Down" => {
                if let Some(Overlay::Inspector { scroll, .. }) = app.overlays.last_mut() {
                    *scroll += 1;
                }
            }
            "k" | "Up" => {
                if let Some(Overlay::Inspector { scroll, .. }) = app.overlays.last_mut() {
                    *scroll = scroll.saturating_sub(1);
                }
            }
            _ => {
                // Item keys act on the inspected control
                if let Some(b) = keymap::lookup(&[Ctx::Item], &code)
                    && b.ctx == Ctx::Item
                {
                    if !b.repeat && k.kind == KeyEventKind::Repeat {
                        return Vec::new();
                    }
                    return command(app, b.cmd);
                }
            }
        },
    }
    Vec::new()
}

fn set_sel(app: &mut App, v: usize) {
    match app.overlays.last_mut() {
        Some(Overlay::Menu { sel, .. })
        | Some(Overlay::Picker { sel, .. })
        | Some(Overlay::Contexts { sel })
        | Some(Overlay::Wiring(WiringState { sel, .. })) => *sel = v,
        Some(Overlay::Palette(p)) => p.sel = v,
        _ => {}
    }
}

fn edit_line(line: &mut Line, k: &KeyEvent) {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    if !matches!(k.code, KeyCode::Char(_)) || ctrl {
        // Backspace on a selected prefill clears it, like a text field
        if std::mem::take(&mut line.fresh) && matches!(k.code, KeyCode::Backspace | KeyCode::Delete)
        {
            line.kill();
            return;
        }
    }
    match k.code {
        KeyCode::Char('u') if ctrl => line.kill(),
        KeyCode::Char('w') if ctrl => line.kill_word(),
        KeyCode::Char('a') if ctrl => line.home(),
        KeyCode::Char('e') if ctrl => line.end(),
        KeyCode::Char(c) if !ctrl => line.insert(&c.to_string()),
        KeyCode::Backspace => line.backspace(),
        KeyCode::Delete => line.delete(),
        KeyCode::Left => line.left(),
        KeyCode::Right => line.right(),
        KeyCode::Home => line.home(),
        KeyCode::End => line.end(),
        _ => {}
    }
}

/// An action chosen from a menu or picker, for one or many controls.
fn act_on(app: &mut App, cids: &[Cid], room: Option<usize>, action: Action) -> Vec<Effect> {
    // Room menu: its actions expand to the room's lights / blinds
    if let Some(r) = room {
        let plans = match action {
            Action::On | Action::Off => palette::room_lights(app, r, action == Action::On),
            _ => app.house.rooms[r]
                .ctrls
                .iter()
                .filter(|c| app.house.ctrls[**c].kind == Kind::Blind)
                .map(|&cid| Plan {
                    cid,
                    action: Action::Blind(BlindCmd::Stop),
                    label: "stop".into(),
                })
                .collect(),
        };
        return run_plans(app, plans);
    }
    let plans = cids
        .iter()
        .map(|&cid| Plan {
            cid,
            label: action.describe(),
            action: action.clone(),
        })
        .collect();
    run_plans(app, plans)
}

fn confirmed(app: &mut App, c: Confirm) -> Vec<Effect> {
    if let Some(e) = c.effect {
        if matches!(*e, Effect::Reboot | Effect::Install) {
            app.log_msg(
                false,
                "requested",
                if matches!(*e, Effect::Reboot) {
                    "reboot"
                } else {
                    "firmware update"
                },
                None,
            );
        }
        return vec![*e];
    }
    send_all(app, c.plans)
}

fn input_submit(app: &mut App, kind: InputKind, line: Line) -> Vec<Effect> {
    match kind {
        InputKind::Filter { target, .. } => {
            set_filter(app, target, line.buf.trim().to_string());
            Vec::new()
        }
        InputKind::Value { cids, spec } => {
            let mut plans = Vec::new();
            for &cid in &cids {
                match vm::set_action(&app.house, cid, &spec, &line.buf) {
                    Ok(a) => plans.push(Plan {
                        cid,
                        label: a.describe(),
                        action: a,
                    }),
                    Err(e) => {
                        // keep the prompt open with the error
                        app.overlays.push(Overlay::Input {
                            kind: InputKind::Value {
                                cids: cids.clone(),
                                spec: spec.clone(),
                            },
                            line: line.clone(),
                            err: Some(e),
                        });
                        return Vec::new();
                    }
                }
            }
            run_plans(app, plans)
        }
        InputKind::Pin { plans } => {
            let pin = line.buf.trim().to_string();
            if pin.is_empty() {
                return Vec::new();
            }
            let plans: Vec<Plan> = plans
                .into_iter()
                .map(|mut p| {
                    if let Action::Alarm { pin: pp, .. } = &mut p.action {
                        *pp = Some(pin.clone());
                    }
                    p
                })
                .collect();
            run_plans(app, plans)
        }
    }
}

fn palette_key(app: &mut App, k: KeyEvent, code: &str) -> Vec<Effect> {
    let Some(Overlay::Palette(p)) = app.overlays.last().cloned() else {
        return Vec::new();
    };
    let items = palette::items(app, &p.line.buf);
    let n = items.len();
    match code {
        "Esc" => {
            app.overlays.pop();
        }
        "Down" | "C-n" | "C-j" => set_sel(app, (p.sel + 1).min(n.saturating_sub(1))),
        "Up" | "C-p" => {
            if p.line.buf.is_empty() || p.hist_pos.is_some() {
                // history browse (newest first)
                let hist = &app.ui_state.palette_history;
                if hist.is_empty() {
                    return Vec::new();
                }
                let pos = p.hist_pos.map_or(0, |i| (i + 1).min(hist.len() - 1));
                let line = hist[hist.len() - 1 - pos].clone();
                if let Some(Overlay::Palette(pp)) = app.overlays.last_mut() {
                    pp.hist_pos = Some(pos);
                    pp.line.set(&line);
                    pp.sel = 0;
                }
            } else {
                set_sel(app, p.sel.saturating_sub(1));
            }
        }
        "Tab" => {
            if let Some((_, super::app::PalItem::Ctrl(c))) = items.get(p.sel) {
                let name = app.house.ctrls[*c].name.clone();
                let s = palette::complete(&p.line.buf, &name);
                if let Some(Overlay::Palette(pp)) = app.overlays.last_mut() {
                    pp.line.set(&s);
                    pp.sel = 0;
                }
            }
        }
        "Enter" if k.kind == KeyEventKind::Press => {
            let Some((_, item)) = items.get(p.sel).cloned() else {
                return Vec::new();
            };
            let is_cmd = palette::is_command(&p.line.buf);
            if is_cmd && !matches!(item, PalItem::Note(_)) {
                remember(app, &p.line.buf);
            }
            app.overlays.pop();
            return palette_run(app, item, &p.line.buf);
        }
        _ => {
            if let Some(Overlay::Palette(pp)) = app.overlays.last_mut() {
                edit_line(&mut pp.line, &k);
                pp.sel = 0;
                pp.hist_pos = None;
            }
        }
    }
    Vec::new()
}

/// Palette history (secrets never stored, §9).
fn remember(app: &mut App, line: &str) {
    let line = line.trim().to_string();
    if line.is_empty() || line.contains("--code") || line.contains("--secured") {
        return;
    }
    let h = &mut app.ui_state.palette_history;
    h.retain(|l| *l != line);
    h.push(line);
    while h.len() > 50 {
        h.remove(0);
    }
}

fn palette_run(app: &mut App, item: PalItem, input: &str) -> Vec<Effect> {
    match item {
        PalItem::Ctrl(c) => {
            if palette::is_command(input) {
                // incomplete command: complete with this control and reopen
                let s = palette::complete(input, &app.house.ctrls[c].name);
                app.overlays.push(Overlay::Palette(PaletteState {
                    line: Line::new(&s),
                    sel: 0,
                    hist_pos: None,
                }));
                return Vec::new();
            }
            reveal(app, c);
            Vec::new()
        }
        PalItem::Room(r) => {
            app.rooms.group = GroupBy::Room;
            app.rooms.sel_group = Some(GKey::Room(r));
            app.rooms.pane = 1;
            go(app, Screen::Rooms);
            Vec::new()
        }
        PalItem::Screen(s) => {
            go(app, s);
            schedule(app)
        }
        PalItem::Site(s) => {
            if s == app.ctx_name {
                Vec::new()
            } else {
                vec![Effect::SwitchContext(s)]
            }
        }
        PalItem::Scene(s) | PalItem::RunScene(s) => {
            let mut out = run_scene(app, s);
            out.push(Effect::SaveState(ui_state(app)));
            out
        }
        PalItem::Run { plans, .. } => {
            let mut out = run_plans(app, plans);
            out.push(Effect::SaveState(ui_state(app)));
            out
        }
        PalItem::Note(n) => {
            app.toast(ToastKind::Info, n);
            app.overlays.push(Overlay::Palette(PaletteState {
                line: Line::new(input),
                sel: 0,
                hist_pos: None,
            }));
            Vec::new()
        }
    }
}

/// Rows of the wiring overlay: inputs, then outputs.
pub fn wiring_rows(app: &App, w: &WiringState) -> Vec<(bool, crate::logic::Wire)> {
    match &app.wiring {
        WiringDoc::Ready(l, _) => l
            .neighborhood(&w.center)
            .map(|n| {
                n.inputs
                    .into_iter()
                    .map(|x| (true, x))
                    .chain(n.outputs.into_iter().map(|x| (false, x)))
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn wiring_key(app: &mut App, w: WiringState, code: &str) -> Vec<Effect> {
    let rows = wiring_rows(app, &w);
    let n = rows.len();
    let move_to = |app: &mut App, center: String| {
        if let Some(Overlay::Wiring(ws)) = app.overlays.last_mut() {
            ws.back.push(std::mem::replace(&mut ws.center, center));
            ws.sel = 0;
        }
    };
    match code {
        "Esc" | "q" | "w" => {
            app.overlays.pop();
        }
        "j" | "Down" => set_sel(app, (w.sel + 1).min(n.saturating_sub(1))),
        "k" | "Up" => set_sel(app, w.sel.saturating_sub(1)),
        "Enter" => {
            if let Some((_, wire)) = rows.get(w.sel) {
                move_to(app, wire.other.clone());
            }
        }
        // upstream: the selected input (or the first one)
        "h" | "Left" => {
            let pick = rows
                .get(w.sel)
                .filter(|r| r.0)
                .or_else(|| rows.iter().find(|r| r.0));
            if let Some((_, wire)) = pick {
                move_to(app, wire.other.clone());
            }
        }
        "l" | "Right" => {
            let pick = rows
                .get(w.sel)
                .filter(|r| !r.0)
                .or_else(|| rows.iter().find(|r| !r.0));
            if let Some((_, wire)) = pick {
                move_to(app, wire.other.clone());
            }
        }
        "Backspace" | "b" => {
            if let Some(Overlay::Wiring(ws)) = app.overlays.last_mut()
                && let Some(c) = ws.back.pop()
            {
                ws.center = c;
                ws.sel = 0;
            }
        }
        "e" => {
            if let Some(&cid) = app.house.by_uuid.get(&w.center) {
                app.overlays.clear();
                app.events.chips = vec![Chip::Ctrl(cid)];
                app.events.follow = true;
                go(app, Screen::Events);
            }
        }
        "d" => {
            if matches!(app.wiring, WiringDoc::Missing | WiringDoc::Failed(_)) {
                app.wiring = WiringDoc::Loading;
                return vec![Effect::LoadWiring { download: true }];
            }
        }
        _ => {}
    }
    Vec::new()
}

// ── Mouse ───────────────────────────────────────────────────────────────────

fn mouse(app: &mut App, m: crossterm::event::MouseEvent) -> Vec<Effect> {
    if !app.opts.mouse {
        return Vec::new();
    }
    match m.kind {
        MouseEventKind::ScrollDown => return command_or_overlay(app, Cmd::Down, "j"),
        MouseEventKind::ScrollUp => return command_or_overlay(app, Cmd::Up, "k"),
        MouseEventKind::Down(MouseButton::Left) => {
            let hit = {
                let ui = app.ui.borrow();
                ui.hits
                    .iter()
                    .rev()
                    .find(|(r, _)| {
                        m.column >= r.x
                            && m.column < r.x + r.width
                            && m.row >= r.y
                            && m.row < r.y + r.height
                    })
                    .map(|(_, h)| *h)
            };
            if !app.overlays.is_empty() {
                return Vec::new();
            }
            match hit {
                Some(Hit::Tab(s)) => {
                    go(app, s);
                    return schedule(app);
                }
                Some(Hit::Pane(p)) => focus_pane(app, p),
                Some(Hit::Row(list, i)) => {
                    focus_pane(app, list);
                    select_row(app, list, i);
                }
                Some(Hit::SubTab(i)) => match app.screen {
                    Screen::System => {
                        if let Some(v) = SysView::ALL.get(i) {
                            app.system.view = *v;
                            return schedule(app);
                        }
                    }
                    Screen::Energy => {
                        if let Some(r) = ERange::ALL.get(i) {
                            app.energy.range = *r;
                        }
                    }
                    _ => {}
                },
                None => {}
            }
        }
        _ => {}
    }
    Vec::new()
}

fn command_or_overlay(app: &mut App, cmd: Cmd, key_char: &str) -> Vec<Effect> {
    if app.overlays.is_empty() {
        command(app, cmd)
    } else {
        let c = key_char.chars().next().unwrap_or('j');
        overlay_key(app, KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }
}

/// List ids used by render for hit testing.
pub mod list_id {
    pub const ROOM_GROUPS: u8 = 1;
    pub const ROOM_CTRLS: u8 = 2;
    pub const INSPECTOR: u8 = 3;
    pub const HOME_CARDS: u8 = 10;
    pub const HOME_ATTENTION: u8 = 11;
    pub const HOME_PINNED: u8 = 12;
    pub const HOME_QUICK: u8 = 13;
    pub const HOME_ENERGY: u8 = 14;
    pub const HOME_LIVE: u8 = 15;
    pub const EVENTS: u8 = 20;
    pub const METERS: u8 = 30;
    pub const SYS_LIST: u8 = 40;
    pub const SYS_DIFF: u8 = 41;
    pub const SITES: u8 = 50;
}

fn focus_pane(app: &mut App, list: u8) {
    use list_id::*;
    match list {
        ROOM_GROUPS => app.rooms.pane = 0,
        ROOM_CTRLS => app.rooms.pane = 1,
        INSPECTOR => app.rooms.pane = 2,
        HOME_CARDS => app.home.pane = HomePane::Rooms,
        HOME_ATTENTION => app.home.pane = HomePane::Attention,
        HOME_PINNED => app.home.pane = HomePane::Pinned,
        HOME_QUICK => app.home.pane = HomePane::Quick,
        HOME_ENERGY => app.home.pane = HomePane::Energy,
        HOME_LIVE => app.home.pane = HomePane::Live,
        SYS_LIST => app.system.pane = 0,
        SYS_DIFF => app.system.pane = 1,
        _ => {}
    }
    freeze_rooms(app);
}

fn select_row(app: &mut App, list: u8, i: usize) {
    use list_id::*;
    match list {
        ROOM_GROUPS => {
            if let Some(g) = lists::groups(app).get(i) {
                app.rooms.sel_group = Some(g.key.clone());
            }
        }
        ROOM_CTRLS => {
            let key = lists::current_group(app);
            if let Some(CRow::Ctrl { cid, .. }) = lists::ctrl_rows(app, &key).get(i) {
                let u = app.house.ctrls[*cid].uuid.clone();
                app.rooms.sel_ctrl.insert(key, u);
            }
        }
        HOME_CARDS => app.home.sel_room = i,
        HOME_ATTENTION => app.home.sel_attention = i,
        HOME_PINNED => app.home.sel_pin = i,
        HOME_QUICK => app.home.sel_quick = i,
        EVENTS => {
            let rows = lists::event_rows(app);
            if let Some(ix) = rows.get(i) {
                app.events.follow = false;
                app.events.sel = Some(app.store.events[*ix].seq);
            }
        }
        METERS => app.energy.sel_meter = i,
        SYS_LIST => {
            app.system.sel.insert(app.system.view, i);
        }
        SITES => app.sites_ui.sel = i,
        _ => {}
    }
}
