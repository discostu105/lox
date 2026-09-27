//! End-to-end tests of the TUI core (§10): the user journeys J1–J12 driven
//! through `update()` with the demo house, rendering at several sizes and
//! themes, scale, hostile text and the robustness rules.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::{Value, json};

use super::app::{App, Conn, Effect, Msg, Opts, Overlay, PollKind, Polled, Screen, SysView};
use super::demo;
use super::model::House;
use super::theme::{Depth, Theme, ThemeName};
use super::{lists, ui, update};

const T0: f64 = 1_790_000_000.0;

struct H {
    app: App,
    sim: demo::Sim,
    /// Effects of the most recent input
    fx: Vec<Effect>,
    /// Every effect since the start
    all: Vec<Effect>,
}

impl H {
    fn new() -> H {
        H::with(Opts {
            read_only: false,
            demo: true,
            mouse: true,
            motion: false,
            nerd: false,
        })
    }

    fn with(opts: Opts) -> H {
        let st = demo::structure();
        H::from_structure(&st, opts, ThemeName::Night, Depth::TrueColor)
    }

    fn from_structure(st: &Value, opts: Opts, theme: ThemeName, depth: Depth) -> H {
        let house = House::from_structure(st);
        let sim = demo::Sim::new(st);
        let mut app = App::new(house, Theme::new(theme, depth), opts, "demo".into(), T0);
        app.contexts = vec!["demo".into(), "office".into(), "cabin".into()];
        app.scenes = super::exec::DEMO_SCENES
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut h = H {
            app,
            sim,
            fx: Vec::new(),
            all: Vec::new(),
        };
        h.msg(Msg::Resize(120, 36));
        let e = h.app.epoch;
        h.msg(Msg::Conn {
            epoch: e,
            conn: Conn::Live,
        });
        let first = h.sim.initial_events();
        h.msg(Msg::States {
            epoch: e,
            batch: first,
        });
        // past the post-connect quiet period
        h.tick(5.0);
        h
    }

    fn msg(&mut self, m: Msg) -> &mut Self {
        self.fx = update::update(&mut self.app, m);
        self.all.extend(self.fx.clone());
        self
    }

    fn tick(&mut self, dt: f64) -> &mut Self {
        let now = self.app.now + dt;
        self.msg(Msg::Tick { now })
    }

    /// Keys: single characters, or names like `Enter`, `Esc`, `Space`, `Tab`, `C-k`.
    fn keys(&mut self, seq: &[&str]) -> &mut Self {
        for k in seq {
            // a human pace: never mistaken for auto-repeat
            self.tick(0.7);
            let ev = key(k);
            self.fx = update::update(&mut self.app, Msg::Key(ev));
            self.all.extend(self.fx.clone());
        }
        self
    }

    fn typed(&mut self, s: &str) -> &mut Self {
        for c in s.chars() {
            let ev = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            self.fx = update::update(&mut self.app, Msg::Key(ev));
            self.all.extend(self.fx.clone());
        }
        self
    }

    /// Run the simulator forward and feed its changes.
    fn run_sim(&mut self, secs: f64) {
        let e = self.app.epoch;
        let mut t = 0.0;
        while t < secs {
            t += 0.25;
            let batch = self.sim.tick(self.app.now + t, 12.0);
            if !batch.is_empty() {
                self.msg(Msg::States { epoch: e, batch });
            }
        }
        self.tick(secs);
    }

    /// Apply the Send effects to the simulator and feed the resulting events.
    fn deliver(&mut self) {
        let sends: Vec<(u64, usize, String, Vec<String>)> = self
            .fx
            .iter()
            .filter_map(|e| match e {
                Effect::Send {
                    req,
                    cid,
                    uuid,
                    cmds,
                    ..
                } => Some((*req, *cid, uuid.clone(), cmds.clone())),
                _ => None,
            })
            .collect();
        let e = self.app.epoch;
        for (req, cid, uuid, cmds) in sends {
            let mut r = Ok(());
            for c in &cmds {
                if let Err(err) = self.sim.command(&uuid, c) {
                    r = Err(err);
                }
            }
            self.msg(Msg::CmdDone {
                epoch: e,
                req,
                cid,
                result: r,
            });
        }
        self.run_sim(1.0);
    }

    /// Deliver a poll result as the backend would: for a request in flight.
    fn poll(&mut self, kind: PollKind, p: Polled) {
        let (e, req) = (self.app.epoch, self.app.req());
        self.app.polls.inflight.insert(kind.clone(), req);
        self.msg(Msg::Polled {
            epoch: e,
            req,
            kind,
            result: Ok(p),
        });
    }

    /// Move the list selection onto a control (`j` until it is selected).
    fn select(&mut self, cid: usize) {
        for _ in 0..60 {
            if lists::selected_ctrl(&self.app) == Some(cid) {
                return;
            }
            self.keys(&["j"]);
        }
        panic!("could not select {}", self.app.house.display_name(cid));
    }

    fn sends(&self) -> Vec<(String, Vec<String>)> {
        self.fx
            .iter()
            .filter_map(|e| match e {
                Effect::Send { uuid, cmds, .. } => Some((uuid.clone(), cmds.clone())),
                _ => None,
            })
            .collect()
    }

    fn cid(&self, name: &str, room: &str) -> usize {
        self.app.house.resolve(name, Some(room)).unwrap()
    }

    fn render(&self, w: u16, h: u16) -> String {
        render(&self.app, w, h)
    }
}

fn key(k: &str) -> KeyEvent {
    let (code, mods) = match k {
        "Enter" => (KeyCode::Enter, KeyModifiers::NONE),
        "Esc" => (KeyCode::Esc, KeyModifiers::NONE),
        "Space" => (KeyCode::Char(' '), KeyModifiers::NONE),
        "Tab" => (KeyCode::Tab, KeyModifiers::NONE),
        "Down" => (KeyCode::Down, KeyModifiers::NONE),
        "Up" => (KeyCode::Up, KeyModifiers::NONE),
        "Left" => (KeyCode::Left, KeyModifiers::NONE),
        "Right" => (KeyCode::Right, KeyModifiers::NONE),
        s if s.starts_with("C-") => (
            KeyCode::Char(s.chars().nth(2).unwrap()),
            KeyModifiers::CONTROL,
        ),
        s => {
            let c = s.chars().next().unwrap();
            let m = if c.is_ascii_uppercase() {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::NONE
            };
            (KeyCode::Char(c), m)
        }
    };
    KeyEvent::new(code, mods)
}

fn render(app: &App, w: u16, h: u16) -> String {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    ui::render(app, area, &mut buf);
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

// ── Journeys ────────────────────────────────────────────────────────────────

/// J1: Office lights off before leaving — `2 /off⏎ l ␣`.
#[test]
fn j1_office_light_off() {
    let mut h = H::new();
    h.keys(&["2", "/"]).typed("off").keys(&["Enter", "l"]);
    assert_eq!(h.app.screen, Screen::Rooms);
    let sel = lists::selected_ctrl(&h.app).expect("a control is selected");
    assert_eq!(h.app.house.room_name(sel), Some("Office"));
    // move to the ceiling light (on in the demo) and toggle it
    let ceiling = h.cid("Ceiling", "Office");
    h.select(ceiling);
    h.keys(&["Space"]);
    let sends = h.sends();
    assert_eq!(sends.len(), 1, "one press, one command: {:?}", sends);
    assert_eq!(sends[0].1, vec!["off".to_string()]);
    assert!(
        h.app.pending.contains_key(&ceiling),
        "row shows pending until confirmed"
    );
    h.deliver();
    let u = h.app.house.ctrls[ceiling]
        .state("active")
        .unwrap()
        .to_string();
    assert_eq!(h.app.store.num(&u), Some(0.0));
}

/// J2: living room blind to 30 % — `: living south⏎ =30⏎`.
#[test]
fn j2_blind_to_30() {
    let mut h = H::new();
    h.keys(&[":"]).typed("living south").keys(&["Enter"]);
    let blind = h.cid("Blind South", "Living room");
    assert_eq!(lists::selected_ctrl(&h.app), Some(blind));
    h.keys(&["="]).typed("30").keys(&["Enter"]);
    let sends = h.sends();
    assert_eq!(sends.len(), 1, "{:?}", h.fx);
    assert_eq!(sends[0].0, h.app.house.ctrls[blind].uuid);
    assert!(sends[0].1.iter().any(|c| c.contains("30")), "{:?}", sends);
}

/// J3: why did the hallway light turn on — events, filter, detail, wiring.
#[test]
fn j3_events_filter_detail_wiring() {
    let mut h = H::new();
    h.run_sim(40.0);
    // make sure a hallway change is in the buffer
    let light = h.cid("Hallway light", "Hallway");
    h.app.rooms.sel_ctrl.clear();
    let u = h.app.house.ctrls[light].uuid.clone();
    let _ = h.sim.command(&u, "on");
    h.run_sim(1.0);
    h.keys(&["3", "/"]).typed("hallway").keys(&["Enter"]);
    assert_eq!(h.app.screen, Screen::Events);
    let rows = lists::event_rows(&h.app);
    assert!(!rows.is_empty());
    for i in &rows {
        let e = &h.app.store.events[*i];
        let hay = format!(
            "{} {}",
            e.cid
                .map(|c| h.app.house.display_name(c))
                .unwrap_or_default(),
            e.cid.and_then(|c| h.app.house.room_name(c)).unwrap_or("")
        );
        assert!(hay.to_lowercase().contains("hall"), "{}", hay);
    }
    h.keys(&["k", "w"]);
    assert!(
        h.fx.iter().any(|e| matches!(e, Effect::LoadWiring { .. })),
        "{:?}",
        h.fx
    );
    assert!(matches!(h.app.overlays.last(), Some(Overlay::Wiring(_))));
    let s = h.render(120, 36);
    assert!(s.contains("wiring"));
}

/// J4: is the Miniserver OK — System overview and sub-views poll what they show.
#[test]
fn j4_system_views_poll() {
    let mut h = H::new();
    h.keys(&["5"]);
    assert_eq!(h.app.screen, Screen::System);
    h.tick(3.0);
    assert!(h.all.iter().any(|e| matches!(
        e,
        Effect::Poll {
            kind: PollKind::Diag,
            ..
        }
    )));
    for want in [SysView::Devices, SysView::BusLan, SysView::Log] {
        h.keys(&["]"]);
        assert_eq!(h.app.system.view, want);
    }
    h.tick(3.0);
    assert!(h.all.iter().any(|e| matches!(
        e,
        Effect::Poll {
            kind: PollKind::Log,
            ..
        }
    )));
}

/// J5: which sensor needs a battery — Home's attention panel, no keys needed.
#[test]
fn j5_battery_in_attention() {
    let mut h = H::new();
    h.poll(PollKind::Devices, Polled::Devices(demo::devices()));
    let s = h.render(140, 40);
    assert!(s.contains("battery 8"), "{}", s);
}

/// J6: PV now and where it goes — Energy flow and range switching.
#[test]
fn j6_energy() {
    let mut h = H::new();
    h.run_sim(5.0);
    h.keys(&["4"]);
    assert_eq!(h.app.screen, Screen::Energy);
    let r0 = h.app.energy.range;
    h.keys(&["]"]);
    assert_ne!(h.app.energy.range, r0);
    let s = h.render(140, 40);
    assert!(
        s.contains("PV") && s.contains("Grid") && s.contains("Battery"),
        "{}",
        s
    );
}

/// J7: sites — switch the whole TUI to another context.
#[test]
fn j7_switch_site() {
    let mut h = H::new();
    h.keys(&["6", "j", "Enter"]);
    assert!(
        h.fx.iter()
            .any(|e| matches!(e, Effect::SwitchContext(c) if c == "office")),
        "{:?}",
        h.fx
    );
}

/// J8: copy the CLI command / UUID.
#[test]
fn j8_copy_cli_and_uuid() {
    let mut h = H::new();
    h.keys(&[":"]).typed("living south").keys(&["Enter", "y"]);
    let cli = h.fx.iter().find_map(|e| match e {
        Effect::Copy(s) => Some(s.clone()),
        _ => None,
    });
    let cli = cli.expect("y copies");
    assert!(cli.starts_with("lox "), "{}", cli);
    assert!(cli.contains("Blind South"), "{}", cli);
    h.keys(&["Y"]);
    let blind = h.cid("Blind South", "Living room");
    assert!(
        h.fx.iter()
            .any(|e| matches!(e, Effect::Copy(u) if *u == h.app.house.ctrls[blind].uuid))
    );
}

/// J9: arm the alarm — picker, then a confirmation before anything is sent.
#[test]
fn j9_alarm_needs_confirmation() {
    let mut h = H::new();
    h.keys(&[":"]).typed("house alarm").keys(&["Enter", "m"]);
    assert!(
        matches!(h.app.overlays.last(), Some(Overlay::Picker { .. })),
        "{:?}",
        h.app.overlays.len()
    );
    // the picker opens on the current mode (disarmed); `1` picks "arm"
    h.keys(&["1"]);
    // the demo alarm is secured: a masked PIN prompt first
    assert!(matches!(h.app.overlays.last(), Some(Overlay::Input { .. })));
    h.typed("1234");
    let s = h.render(120, 36);
    assert!(!s.contains("1234"), "PIN must be masked");
    h.keys(&["Enter"]);
    assert!(h.sends().is_empty(), "nothing sent before confirming");
    assert!(matches!(h.app.overlays.last(), Some(Overlay::Confirm(_))));
    h.keys(&["y"]);
    assert_eq!(h.sends().len(), 1, "{:?}", h.fx);
}

/// J10: run a scene from the palette.
#[test]
fn j10_scene_from_palette() {
    let mut h = H::new();
    h.keys(&[":"]).typed("movie").keys(&["Enter"]);
    assert!(
        h.fx.iter()
            .any(|e| matches!(e, Effect::RunScene(s) if s == "movie")),
        "{:?}",
        h.fx
    );
}

/// J11: config history — log, then a commit's diff.
#[test]
fn j11_config_history() {
    let mut h = H::new();
    h.keys(&["5"]);
    while h.app.system.view != SysView::Config {
        h.keys(&["]"]);
    }
    h.tick(1.0);
    assert!(h.all.iter().any(|e| matches!(
        e,
        Effect::Poll {
            kind: PollKind::ConfigLog,
            ..
        }
    )));
    let commits = vec![super::app::Commit {
        hash: "c3".into(),
        date: "2026-09-24 21:10:02 +0200".into(),
        subject: "Night mode".into(),
    }];
    h.poll(PollKind::ConfigLog, Polled::ConfigLog(Ok(commits)));
    h.tick(1.0);
    assert!(
        h.all
            .iter()
            .any(|e| matches!(e, Effect::Poll { kind: PollKind::ConfigDiff(c), .. } if c == "c3"))
    );
}

/// J12: wall display — read-only never sends, and says so.
#[test]
fn j12_read_only() {
    let mut h = H::with(Opts {
        read_only: true,
        demo: true,
        mouse: false,
        motion: false,
        nerd: false,
    });
    h.keys(&["2", "l", "Space", "+", "a"]);
    assert!(
        h.all.iter().all(|e| !matches!(e, Effect::Send { .. })),
        "read-only must not send"
    );
    let s = h.render(80, 24);
    assert!(s.contains("READ-ONLY"), "{}", s);
}

// ── Robustness rules ────────────────────────────────────────────────────────

#[test]
fn held_space_sends_one_command() {
    let mut h = H::new();
    h.keys(&["2", "/"]).typed("office").keys(&["Enter", "l"]);
    let c = h.cid("Ceiling", "Office");
    h.select(c);
    let mut n = 0;
    for _ in 0..5 {
        h.tick(0.05);
        h.fx = update::update(&mut h.app, Msg::Key(key("Space")));
        n += h.sends().len();
    }
    assert_eq!(n, 1);
    // an explicit repeat event never re-fires a non-repeating key
    let mut rep = key("Space");
    rep.kind = KeyEventKind::Repeat;
    h.tick(1.0);
    h.fx = update::update(&mut h.app, Msg::Key(rep));
    assert!(h.sends().is_empty());
}

/// Quick double presses of navigation keys both count (only acting keys
/// treat fast presses as a held key).
#[test]
fn quick_navigation_presses_all_count() {
    let mut h = H::new();
    let press = |h: &mut H, k: &str| {
        h.tick(0.1);
        h.msg(Msg::Key(key(k)));
    };
    // Home: each → moves to the next room card (the grid order comes from rendering)
    let _ = h.render(120, 36);
    let r0 = h.app.home.sel_room;
    press(&mut h, "Right");
    press(&mut h, "Right");
    assert_eq!(
        h.app.home.sel_room,
        r0 + 2,
        "second → within 0.1 s was dropped"
    );
    // System: each ] advances a view
    h.keys(&["5"]);
    let v0 = h.app.system.view;
    press(&mut h, "]");
    let v1 = h.app.system.view;
    press(&mut h, "]");
    assert_ne!(v0, v1);
    assert_ne!(v1, h.app.system.view, "second ] within 0.1 s was dropped");
}

/// Holding `+` auto-repeats; the steps of one frame become one command
/// carrying the final value.
#[test]
fn held_plus_coalesces_per_frame() {
    let mut h = H::new();
    h.keys(&["2", "/"]).typed("office").keys(&["Enter", "l"]);
    let lamp = h.cid("Desk lamp", "Office");
    h.select(lamp);
    let mut frame = Vec::new();
    for _ in 0..4 {
        let mut k = key("+");
        k.kind = KeyEventKind::Repeat;
        frame.extend(update::update(&mut h.app, Msg::Key(k)));
    }
    let sent = update::coalesce(frame);
    let sends: Vec<_> = sent
        .iter()
        .filter(|e| matches!(e, Effect::Send { .. }))
        .collect();
    assert_eq!(sends.len(), 1, "{:?}", sent);
    let Effect::Send { req, cmds, .. } = sends[0] else {
        unreachable!()
    };
    // the survivor is the latest step, which the row's pending state tracks
    assert_eq!(h.app.pending[&lamp].req, *req);
    let v: f64 = cmds[0].parse().unwrap();
    assert!(v > 40.0, "{:?}", cmds);
}

#[test]
fn pasted_newline_does_not_submit() {
    let mut h = H::new();
    h.keys(&[":"]);
    h.msg(Msg::Paste("movie\n".into()));
    assert!(h.fx.iter().all(|e| !matches!(e, Effect::RunScene(_))));
    assert!(matches!(h.app.overlays.last(), Some(Overlay::Palette(_))));
}

#[test]
fn stale_results_are_dropped() {
    let mut h = H::new();
    let old = h.app.epoch;
    h.msg(Msg::NewHouse {
        epoch: old + 7,
        house: Box::new(House::from_structure(&demo::structure())),
        ctx: "demo".into(),
    });
    let req = h.app.req();
    h.app.polls.inflight.insert(PollKind::Diag, req);
    h.msg(Msg::Polled {
        epoch: old,
        req,
        kind: PollKind::Diag,
        result: Ok(Polled::Diag(demo::diag(1.0))),
    });
    assert!(
        h.app.diag.is_none(),
        "a result from the old epoch must not land"
    );
    // superseded by a newer request of the same kind
    let (e, older) = (h.app.epoch, h.app.req());
    let newer = h.app.req();
    h.app.polls.inflight.insert(PollKind::Diag, newer);
    h.msg(Msg::Polled {
        epoch: e,
        req: older,
        kind: PollKind::Diag,
        result: Ok(Polled::Diag(demo::diag(1.0))),
    });
    assert!(h.app.diag.is_none(), "a superseded result must not land");
    h.msg(Msg::Polled {
        epoch: e,
        req: newer,
        kind: PollKind::Diag,
        result: Ok(Polled::Diag(demo::diag(1.0))),
    });
    assert!(h.app.diag.is_some());
}

#[test]
fn failures_reach_the_message_log() {
    let mut h = H::new();
    h.keys(&["2", "/"]).typed("office").keys(&["Enter", "l"]);
    let c = h.cid("Ceiling", "Office");
    h.select(c);
    h.keys(&["Space"]);
    let (req, cid) =
        h.fx.iter()
            .find_map(|e| match e {
                Effect::Send { req, cid, .. } => Some((*req, *cid)),
                _ => None,
            })
            .unwrap();
    let e = h.app.epoch;
    h.msg(Msg::CmdDone {
        epoch: e,
        req,
        cid,
        result: Err("Miniserver answered 403".into()),
    });
    assert!(h.app.unread_errors > 0);
    assert!(
        h.app
            .msglog
            .iter()
            .any(|m| m.err && m.detail.contains("403"))
    );
    let s = h.render(120, 36);
    assert!(s.contains('✗') || s.contains("403"), "{}", s);
}

#[test]
fn quit_saves_state() {
    let mut h = H::new();
    h.keys(&["2", "q"]);
    assert!(
        h.fx.iter()
            .any(|e| matches!(e, Effect::SaveState(st) if st.last_screen == Some(Screen::Rooms)))
    );
    assert!(h.fx.iter().any(|e| matches!(e, Effect::Quit)));
}

// ── Rendering ───────────────────────────────────────────────────────────────

/// Every screen and the main overlays at small, medium and large sizes, in
/// color and mono: no panics, the header is intact, nothing overflows.
#[test]
fn all_screens_render_at_all_sizes() {
    for (theme, depth) in [
        (ThemeName::Night, Depth::TrueColor),
        (ThemeName::Mono, Depth::Mono),
        (ThemeName::Day, Depth::Ansi256),
    ] {
        let mut h = H::from_structure(
            &demo::structure(),
            Opts {
                read_only: false,
                demo: true,
                mouse: true,
                motion: true,
                nerd: false,
            },
            theme,
            depth,
        );
        h.run_sim(10.0);
        for (kind, p) in [
            (PollKind::Diag, Polled::Diag(demo::diag(1.0))),
            (PollKind::Info, Polled::Info(demo::info())),
            (PollKind::Devices, Polled::Devices(demo::devices())),
            (PollKind::Log, Polled::Log(demo::log())),
            (PollKind::BusLan, Polled::BusLan(demo::bus_lan(1.0))),
            (PollKind::Sites, Polled::Sites(demo::sites(1.0))),
        ] {
            h.poll(kind, p);
        }
        let (pv, usage) = demo::energy_today(12.0);
        h.poll(PollKind::EnergyDay, Polled::EnergyDay { pv, usage });
        for (w, hh) in [(80, 24), (120, 36), (180, 50), (60, 20)] {
            h.msg(Msg::Resize(w, hh));
            for scr in ["1", "2", "3", "4", "5", "6"] {
                h.keys(&[scr]);
                let s = h.render(w, hh);
                assert_eq!(s.lines().count(), hh as usize);
                if w >= 80 {
                    assert!(
                        s.lines().next().unwrap().contains("lox"),
                        "{}×{} {}\n{}",
                        w,
                        hh,
                        scr,
                        s
                    );
                }
            }
            for ov in [
                &["?"][..],
                &[":"],
                &["2", "l", "a"],
                &["2", "l", "w"],
                &["C"],
            ] {
                h.keys(ov);
                let _ = h.render(w, hh);
                h.keys(&["Esc", "Esc"]);
            }
        }
    }
}

#[test]
fn tiny_terminal_shows_a_hint_and_only_quits() {
    let mut h = H::new();
    h.msg(Msg::Resize(30, 8));
    let s = h.render(30, 8);
    assert!(s.contains("small") || s.contains("≥"), "{}", s);
    h.keys(&["2"]);
    assert_eq!(h.app.screen, Screen::Home);
}

/// Names from the Miniserver are data: escape sequences and control characters
/// never reach the terminal, wide characters don't break the layout.
#[test]
fn hostile_text_is_neutralized() {
    let mut st = demo::structure();
    let (u, _) = st["controls"]
        .as_object()
        .unwrap()
        .iter()
        .next()
        .map(|(u, c)| (u.clone(), c.clone()))
        .unwrap();
    st["controls"][&u]["name"] = json!("\u{1b}[31mEvil\u{1b}]52;c;AAAA\u{7}\r\nname 🏠漢字");
    let mut h = H::from_structure(
        &st,
        Opts {
            read_only: false,
            demo: true,
            mouse: true,
            motion: false,
            nerd: false,
        },
        ThemeName::Night,
        Depth::TrueColor,
    );
    for scr in ["1", "2", "3"] {
        h.keys(&[scr]);
        let s = h.render(120, 36);
        assert!(
            !s.contains('\u{1b}') && !s.contains('\u{7}') && !s.contains('\r'),
            "control chars leaked"
        );
    }
    h.keys(&[":"]).typed("evil");
    let s = h.render(120, 36);
    assert!(!s.contains('\u{1b}'));
}

/// 2000 controls in 80 rooms: builds, filters and renders quickly.
#[test]
fn scales_to_large_installations() {
    let mut rooms = serde_json::Map::new();
    let mut controls = serde_json::Map::new();
    let mut cats = serde_json::Map::new();
    cats.insert("c0".into(), json!({"name": "Lighting", "type": "lights"}));
    cats.insert("c1".into(), json!({"name": "Shading", "type": "shading"}));
    for r in 0..80 {
        rooms.insert(
            format!("r{r:03}"),
            json!({"name": format!("Floor {} Room {}", r / 10, r)}),
        );
    }
    for i in 0..2000 {
        let r = i % 80;
        let (typ, states, cat) = if i % 3 == 0 {
            (
                "Jalousie",
                json!({"position": format!("s{i}-p"), "shadePosition": format!("s{i}-s"), "up": format!("s{i}-u"), "down": format!("s{i}-d")}),
                "c1",
            )
        } else {
            (
                "Dimmer",
                json!({"position": format!("s{i}-p"), "min": format!("s{i}-mi"), "max": format!("s{i}-ma"), "step": format!("s{i}-st")}),
                "c0",
            )
        };
        controls.insert(
            format!("u{i:05}"),
            json!({"name": format!("Control {i}"), "type": typ, "room": format!("r{r:03}"), "cat": cat, "states": states, "details": {}}),
        );
    }
    let st = json!({"msInfo": {"msName": "Big", "serialNr": "000000000000"}, "lastModified": "2026-01-01 00:00:00", "rooms": rooms, "cats": cats, "controls": controls});
    let t = std::time::Instant::now();
    let mut h = H::from_structure(
        &st,
        Opts {
            read_only: false,
            demo: true,
            mouse: true,
            motion: false,
            nerd: false,
        },
        ThemeName::Night,
        Depth::TrueColor,
    );
    assert_eq!(h.app.house.top_level().count(), 2000);
    h.msg(Msg::Resize(180, 50));
    for scr in ["1", "2", "3", "4"] {
        h.keys(&[scr]);
        let _ = h.render(180, 50);
    }
    h.keys(&["2", "/"]).typed("room 7").keys(&["Enter"]);
    h.keys(&[":"]).typed("control 1999");
    let _ = h.render(180, 50);
    let el = t.elapsed();
    // generous: debug build on a slow CI runner
    assert!(el.as_secs_f64() < 20.0, "too slow: {:?}", el);
}

// ── Review fixes ────────────────────────────────────────────────────────────

/// A held key in the inspector sends once, like on the main path.
#[test]
fn held_space_in_inspector_sends_one_command() {
    let mut h = H::new();
    let c = h.cid("Ceiling", "Office");
    h.app
        .overlays
        .push(Overlay::Inspector { cid: c, scroll: 0 });
    let mut n = 0;
    for _ in 0..5 {
        h.tick(0.05);
        h.fx = update::update(&mut h.app, Msg::Key(key("Space")));
        n += h.sends().len();
        // a fast acknowledgement must not open the door to the next repeat
        let acks: Vec<(u64, usize)> =
            h.fx.iter()
                .filter_map(|e| match e {
                    Effect::Send { req, cid, .. } => Some((*req, *cid)),
                    _ => None,
                })
                .collect();
        let e = h.app.epoch;
        for (req, cid) in acks {
            h.msg(Msg::CmdDone {
                epoch: e,
                req,
                cid,
                result: Ok(()),
            });
        }
    }
    assert_eq!(n, 1);
}

/// A same-context structure refresh drops everything that holds control
/// indices, which may now point at other controls.
#[test]
fn structure_refresh_drops_stale_control_indices() {
    let mut h = H::new();
    let c = h.cid("Ceiling", "Office");
    h.app.rooms.marks.insert(c);
    h.app
        .overlays
        .push(Overlay::Inspector { cid: c, scroll: 0 });
    h.app.paused = true;
    let e = h.app.epoch;
    h.msg(Msg::NewHouse {
        epoch: e + 1,
        house: Box::new(House::from_structure(&demo::structure())),
        ctx: "demo".into(),
    });
    assert!(h.app.rooms.marks.is_empty());
    assert!(h.app.overlays.is_empty());
    assert!(!h.app.paused && h.app.paused_buf.is_empty());
}

/// The mouse wheel over a text field moves, it never types.
#[test]
fn wheel_does_not_type_into_inputs() {
    use crossterm::event::{MouseEvent, MouseEventKind};
    let mut h = H::new();
    h.keys(&["2", "/"]);
    assert!(matches!(h.app.overlays.last(), Some(Overlay::Input { .. })));
    h.msg(Msg::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 10,
        row: 10,
        modifiers: KeyModifiers::NONE,
    }));
    match h.app.overlays.last() {
        Some(Overlay::Input { line, .. }) => assert_eq!(line.buf, ""),
        other => panic!("input closed: {:?}", other.is_some()),
    }
}
