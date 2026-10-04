//! End-to-end tests of the TUI core (§10): the user journeys J1–J12 driven
//! through `update()` with the demo house, rendering at several sizes and
//! themes, scale, hostile text and the robustness rules.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::{Value, json};

use super::app::{
    App, ChartData, ChartKey, Conn, Effect, Facet, FacetList, GKey, Msg, Opts, Overlay, PollKind,
    Polled, Screen, Span, SysView,
};
use super::demo;
use super::model::{House, Kind};
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

/// B: the live wiring — type, page and parameters in the header, values on
/// the wires, the selected wire in full; `t` traces down to the actuators.
#[test]
fn wiring_live_header_detail_trace() {
    let mut h = H::new();
    h.run_sim(5.0);
    let light = h.cid("Hallway light", "Hallway");
    update::reveal(&mut h.app, light);
    h.select(light);
    h.keys(&["w"]);
    let st = demo::structure();
    let l = crate::logic::Logic::parse(demo::loxone_xml(&st).as_bytes()).unwrap();
    let epoch = h.app.epoch;
    h.msg(Msg::Wiring {
        epoch,
        doc: super::app::WiringDoc::Ready(std::sync::Arc::new(l), "demo.Loxone".into()),
    });
    let s = h.render(140, 40);
    assert!(s.contains("LightController2 · page Hallway"), "{}", s);
    assert!(s.contains("set MoveOn"), "non-default params: {}", s);
    assert!(s.contains("why is this on"), "{}", s);
    assert!(
        s.contains("Mv ← Motion.Q"),
        "detail of the first wire: {}",
        s
    );
    // p: every parameter
    h.keys(&["p"]);
    assert!(h.render(140, 40).contains("MoveTimeout"));
    // t t: down to the actuators, through the output refs to the Tree device
    h.keys(&["t", "t"]);
    let s = h.render(140, 40);
    assert!(s.contains("trace → actuators"), "{}", s);
    assert!(s.contains("Stairs light"), "{}", s);
    // o: the page in the config browser, the block selected
    h.keys(&["o"]);
    let Some(Overlay::Browse(b)) = h.app.top_overlay() else {
        panic!("no browser: {:?}", h.app.top_overlay());
    };
    assert_eq!(b.page.as_deref(), Some("Hallway"));
    assert!(h.render(140, 40).contains("▌ Hallway light"));
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

/// Loxone reports storage power like a source (+ = discharging): the battery
/// feeds Home, it never flows into PV.
#[test]
fn energy_battery_discharging_feeds_home() {
    let mut h = H::new();
    let hs = &h.app.house;
    let st = |name: &str, s: &str| {
        let c = hs.resolve(name, None).unwrap();
        hs.ctrls[c].state(s).unwrap().to_string()
    };
    let mut batch = Vec::new();
    for (u, v) in [
        (st("Battery", "actual"), 1.5),
        (st("Energy flow", "actual2"), 1.5),
        (st("Energy flow", "Spwr"), 1.5),
        (st("PV", "actual"), 0.0),
        (st("Energy flow", "actual1"), 0.0),
        (st("Energy flow", "Ppwr"), 0.0),
    ] {
        batch.push(crate::stream::StateEvent::ValueState { uuid: u, value: v });
    }
    let e = h.app.epoch;
    h.msg(Msg::States { epoch: e, batch });
    h.keys(&["4"]);
    let f = ui::energy::flows(&h.app);
    assert_eq!(f.batt, Some(-1.5), "+ means charging inside the TUI");
    let s = h.render(140, 40);
    assert!(s.contains("1.5 kW discharging"), "{}", s);
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

/// The demo house plus what real installations have: a door opener wired as a
/// plain push-button in a category with Loxone's door icon, and a pool cover.
fn house_with_access_buttons() -> H {
    let mut st = demo::structure();
    let id_of = |st: &Value, key: &str, name: &str| {
        st[key]
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, v)| v["name"] == name)
            .map(|(k, _)| k.clone())
            .unwrap()
    };
    let access = id_of(&st, "cats", "Access");
    st["cats"][&access]["image"] = json!("IconsFilled/door-open.svg");
    let (hallway, garden) = (
        id_of(&st, "rooms", "Hallway"),
        id_of(&st, "rooms", "Garden"),
    );
    let climate = id_of(&st, "cats", "Climate");
    for (uuid, name, room, cat, state) in [
        (
            "1f00a0a0-0001-0000-ffff000000000001",
            "Door opener",
            &hallway,
            &access,
            "1f00a0a0-0001-0000-ffff0000000000f1",
        ),
        (
            "1f00a0a0-0002-0000-ffff000000000002",
            "Pool cover open",
            &garden,
            &climate,
            "1f00a0a0-0002-0000-ffff0000000000f2",
        ),
    ] {
        st["controls"][uuid] = json!({
            "name": name, "type": "Pushbutton", "uuidAction": uuid, "room": room, "cat": cat,
            "isFavorite": false, "isSecured": false, "states": { "active": state }, "details": {},
        });
    }
    H::from_structure(
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
    )
}

/// J9b: a door opener wired as a push-button asks first, like the door lock.
#[test]
fn j9b_door_opener_push_button_needs_confirmation() {
    let mut h = house_with_access_buttons();
    let opener = h.cid("Door opener", "Hallway");
    h.keys(&["2", "/"]).typed("hallway").keys(&["Enter", "l"]);
    h.select(opener);
    h.keys(&["Space"]);
    assert!(h.sends().is_empty(), "nothing sent before confirming");
    assert!(matches!(h.app.overlays.last(), Some(Overlay::Confirm(_))));
    h.keys(&["y"]);
    assert_eq!(
        h.sends(),
        vec![(
            h.app.house.ctrls[opener].uuid.clone(),
            vec!["pulse".to_string()]
        )]
    );
}

/// J9c: controls on the config's `confirm:` list ask first; others don't.
#[test]
fn j9c_confirm_list() {
    let mut h = house_with_access_buttons();
    let cover = h.cid("Pool cover", "Garden");
    h.keys(&["2", "/"]).typed("garden").keys(&["Enter", "l"]);
    h.select(cover);
    h.keys(&["Space"]);
    assert_eq!(h.sends().len(), 1, "not listed: sent straight away");

    let mut h = house_with_access_buttons();
    h.app
        .house
        .apply_confirm_list(&["pool cover [garden]".to_string()], &Default::default());
    h.keys(&["2", "/"]).typed("garden").keys(&["Enter", "l"]);
    h.select(cover);
    h.keys(&["Space"]);
    assert!(
        h.sends().is_empty(),
        "listed: nothing sent before confirming"
    );
    assert!(matches!(h.app.overlays.last(), Some(Overlay::Confirm(_))));
    h.keys(&["Esc"]);
    assert!(h.sends().is_empty(), "cancelled");
}

/// A listed control's sub-controls (e.g. a lighting controller's circuits) are listed too.
#[test]
fn confirm_list_covers_sub_controls() {
    let mut h = H::new();
    let parent = (0..h.app.house.ctrls.len())
        .find(|&c| !h.app.house.ctrls[c].subs.is_empty())
        .expect("demo has a control with sub-controls");
    let uuid = h.app.house.ctrls[parent].uuid.clone();
    h.app.house.apply_confirm_list(&[uuid], &Default::default());
    assert!(h.app.house.ctrls[parent].listed);
    for &s in &h.app.house.ctrls[parent].subs.clone() {
        assert!(h.app.house.ctrls[s].listed, "{}", h.app.house.ctrls[s].name);
    }
    let others = h.app.house.ctrls.iter().filter(|c| c.listed).count();
    assert_eq!(
        others,
        1 + h.app.house.ctrls[parent].subs.len(),
        "nothing else"
    );
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
    let commits = vec![super::app::Commit::new(
        "c3",
        "2026-09-24 21:10:02 +0200",
        "Night mode",
        "",
    )];
    h.poll(PollKind::ConfigLog, Polled::ConfigLog(Ok(commits), None));
    h.tick(1.0);
    assert!(
        h.all
            .iter()
            .any(|e| matches!(e, Effect::Poll { kind: PollKind::ConfigDiff(c), .. } if c == "c3"))
    );
}

/// Log: `/` finds lines (matches marked), ⏎ opens the whole line, y copies it.
#[test]
fn log_search_and_full_line() {
    let mut h = H::new();
    h.keys(&["5"]);
    while h.app.system.view != SysView::Log {
        h.keys(&["]"]);
    }
    // the demo log ends with a long intercom line
    h.poll(PollKind::Log, Polled::Log(demo::log()));
    let s = h.render(120, 30);
    assert!(s.contains("full line"), "hint: {}", s);
    h.keys(&["/"]).typed("intercom");
    h.keys(&["Enter"]);
    assert_eq!(h.app.system.log_search, "intercom");
    let s = h.render(120, 30);
    assert!(s.contains("/intercom · 1 hits"), "{}", s);
    // the cut-off line, in full
    h.keys(&["Enter"]);
    let Some(Overlay::Text {
        title, sub, text, ..
    }) = h.app.top_overlay()
    else {
        panic!("no text overlay: {:?}", h.app.top_overlay());
    };
    assert_eq!(title, "def.log · 2026-09-27 13:10:00");
    assert_eq!(sub, "important");
    assert!(text.ends_with("192.0.2.40"), "{}", text);
    let s = h.render(120, 30);
    assert!(s.contains("192.0.2.40"), "wrapped, not cut: {}", s);
    h.keys(&["y"]);
    assert!(
        h.fx.iter()
            .any(|e| matches!(e, Effect::Copy(t) if t.contains("QUITTED")))
    );
    h.keys(&["Esc"]);
    assert!(h.app.top_overlay().is_none());
    // Esc clears the search
    h.keys(&["Esc"]);
    assert!(h.app.system.log_search.is_empty());
}

/// Config: ⇥ moves a visible cursor into the diff, ⏎ shows a whole diff line;
/// the footer says what P does and where the backup goes.
#[test]
fn config_diff_focus_and_footer() {
    let mut h = H::new();
    h.keys(&["5"]);
    while h.app.system.view != SysView::Config {
        h.keys(&["]"]);
    }
    let commits = vec![super::app::Commit::new(
        "c2",
        "2026-09-26 08:41:37 +0200",
        "Config backup 2026-09-25 18:39:01 (v273)",
        "",
    )];
    h.poll(
        PollKind::ConfigLog,
        Polled::ConfigLog(Ok(commits), Some("/srv/cfg/ms/config.Loxone".into())),
    );
    h.poll(
        PollKind::ConfigDiff("c2".into()),
        Polled::ConfigDiff(
            "c2".into(),
            vec![
                "= 1 added".into(),
                "# Security".into(),
                "+ wire   Bewegung Vorraum Licht.AQ → Alarmanlage Personenerkennung Außen.HI1"
                    .into(),
            ],
        ),
    );
    // paths are shown with the platform's separator
    let s = h.render(140, 30).replace('\\', "/");
    assert!(s.contains("focus diff"), "{}", s);
    assert!(s.contains("never writes back"), "{}", s);
    assert!(s.contains("git · /srv/cfg"), "repo in the frame: {}", s);
    assert!(s.contains("  ms/config.Loxone"), "{}", s);
    assert!(s.contains("commit c2 · the first backup"), "{}", s);
    h.keys(&["Tab"]);
    assert_eq!(h.app.system.pane, 1);
    let s = h.render(140, 30);
    assert!(s.contains("back to history"), "{}", s);
    h.keys(&["G", "Enter"]);
    let Some(Overlay::Text { text, .. }) = h.app.top_overlay() else {
        panic!("no text overlay");
    };
    assert!(text.ends_with("Außen.HI1"), "{}", text);
    h.keys(&["Esc", "Tab"]);
    assert_eq!(h.app.system.pane, 0);
}

/// After `P`, every diff is fetched again (one-shot polls must forget they
/// ran, or the pane says "loading" forever); failures show, not "loading".
#[test]
fn config_pull_refetches_diffs_and_shows_errors() {
    let mut h = H::new();
    h.keys(&["5"]);
    while h.app.system.view != SysView::Config {
        h.keys(&["]"]);
    }
    let commits = || {
        vec![
            super::app::Commit::new(
                "c2",
                "2026-09-26 08:41:37 +0200",
                "Config backup 2026-09-25 18:39:01 (v273)",
                "+ Added control: \"Registriertes Gerät\" (PuDe)",
            ),
            super::app::Commit::new("c1", "2026-03-18 19:39:46 +0100", "Initial", ""),
        ]
    };
    h.poll(PollKind::ConfigLog, Polled::ConfigLog(Ok(commits()), None));
    h.poll(
        PollKind::ConfigDiff("c2".into()),
        Polled::ConfigDiff(
            "c2".into(),
            vec![
                "= 1 added".into(),
                "# Security".into(),
                "+ block  Registriertes Gerät (PuDe)".into(),
            ],
        ),
    );
    let s = h.render(140, 30);
    assert!(
        s.contains("2026-09-25 18:39 v273"),
        "saved date + version: {}",
        s
    );
    assert!(s.contains("▸ Security"), "{}", s);
    assert!(s.contains("last pulled 2026-09-26 08:41"), "{}", s);
    // pull: running, then done — and the diffs are polled again
    h.keys(&["P"]);
    assert!(h.fx.iter().any(|e| matches!(e, Effect::ConfigPull)));
    assert!(h.render(140, 30).contains("pulling…"));
    h.all.clear();
    h.msg(Msg::ConfigPulled(Ok(false)));
    h.poll(PollKind::ConfigLog, Polled::ConfigLog(Ok(commits()), None));
    h.tick(1.0);
    assert!(
        h.all
            .iter()
            .any(|e| matches!(e, Effect::Poll { kind: PollKind::ConfigDiff(c), .. } if c == "c2")),
        "diff re-fetched after the pull"
    );
    let s = h.render(140, 30);
    assert!(
        s.contains("up to date · last save 2026-09-25 18:39"),
        "{}",
        s
    );
    // a failing diff says so
    let kind = PollKind::ConfigDiff("c2".into());
    let (e, req) = (h.app.epoch, h.app.req());
    h.app.polls.inflight.insert(kind.clone(), req);
    h.msg(Msg::Polled {
        epoch: e,
        req,
        kind,
        result: Err("git show: bad object".into()),
    });
    let s = h.render(140, 30);
    assert!(
        s.contains("couldn't compare") && s.contains("bad object"),
        "{}",
        s
    );
}

/// System › Config with the demo history (c3 newest … c1).
fn config_history() -> H {
    let mut h = H::new();
    h.keys(&["5"]);
    while h.app.system.view != SysView::Config {
        h.keys(&["]"]);
    }
    let st = demo::structure();
    h.poll(
        PollKind::ConfigLog,
        Polled::ConfigLog(Ok(super::exec::demo_commits(&st)), None),
    );
    h
}

fn feed_diff(h: &mut H, key: &str) {
    let st = demo::structure();
    let lines = super::exec::demo_diff(&st, key).unwrap();
    h.poll(
        PollKind::ConfigDiff(key.into()),
        Polled::ConfigDiff(key.into(), lines),
    );
}

/// Answer the last LoadSnapshot effect like the demo backend does.
fn feed_snapshot(h: &mut H) {
    let Some((spec, open)) = h.fx.iter().find_map(|e| match e {
        Effect::LoadSnapshot { spec, open } => Some((spec.clone(), open.clone())),
        _ => None,
    }) else {
        panic!("no LoadSnapshot in {:?}", h.fx);
    };
    let st = demo::structure();
    let result = super::exec::demo_version(&spec)
        .and_then(|v| super::exec::demo_snapshot(&st, v))
        .map_err(|e| e.to_string());
    let epoch = h.app.epoch;
    h.msg(Msg::Snapshot {
        epoch,
        spec,
        open,
        result,
    });
}

/// C: ⏎ on a commit browses that snapshot — pages, a page's blocks and its
/// lxir source, `/` search, then a block's wiring at that snapshot with trace
/// and history.
#[test]
fn config_snapshot_browser() {
    let mut h = config_history();
    h.keys(&["Enter"]);
    feed_snapshot(&mut h);
    assert!(matches!(h.app.top_overlay(), Some(Overlay::Browse(_))));
    let s = h.render(140, 36);
    assert!(s.contains("◷ snapshot c3"), "{}", s);
    assert!(
        s.contains("Hallway") && s.contains("blocks"),
        "pages: {}",
        s
    );
    // the Hallway page: its blocks, then its lxir source
    let Some(Overlay::Browse(b)) = h.app.top_overlay() else {
        unreachable!()
    };
    let i = b
        .doc
        .logic
        .pages()
        .iter()
        .position(|p| p.title == "Hallway")
        .expect("a Hallway page");
    for _ in 0..i {
        h.keys(&["j"]);
    }
    h.keys(&["Enter"]);
    assert!(
        matches!(h.app.top_overlay(), Some(Overlay::Browse(b)) if b.page.as_deref() == Some("Hallway"))
    );
    let s = h.render(140, 36);
    assert!(
        s.contains("Hallway light") && s.contains("LightController2"),
        "{}",
        s
    );
    h.keys(&["s"]);
    let s = h.render(140, 36);
    assert!(s.contains("lxir"), "{}", s);
    assert!(s.contains(" = "), "source: {}", s);
    h.keys(&["s"]);
    // search across the snapshot, ⏎ opens the wiring at that snapshot
    h.keys(&["/"]).typed("stairs pu");
    let s = h.render(140, 36);
    assert!(s.contains("Stairs pulse"), "{}", s);
    h.keys(&["Enter"]);
    let Some(Overlay::Wiring(w)) = h.app.top_overlay() else {
        panic!("no wiring: {:?}", h.app.top_overlay());
    };
    assert!(
        w.snap.is_some(),
        "wiring of the snapshot, not the live config"
    );
    let s = h.render(140, 36);
    assert!(s.contains("◷ snapshot c3"), "{}", s);
    assert!(
        s.contains("Monoflop · page Hallway") || s.contains("Monoflop"),
        "{}",
        s
    );
    assert!(
        s.contains("Time 120") || s.contains("Time"),
        "params: {}",
        s
    );
    assert!(!s.contains("why is this on"), "no live header: {}", s);
    // t: trace up to the sensors
    h.keys(&["t"]);
    let s = h.render(140, 36);
    assert!(s.contains("trace ← sensors"), "{}", s);
    assert!(s.contains("Motion"), "{}", s);
    // H: the block's history
    h.keys(&["H"]);
    let (uuid, title) =
        h.fx.iter()
            .find_map(|e| match e {
                Effect::BlockHistory { uuid, title } => Some((uuid.clone(), title.clone())),
                _ => None,
            })
            .expect("history effect");
    let st = demo::structure();
    let result = super::exec::demo_history(&st, &uuid).map_err(|e| e.to_string());
    let epoch = h.app.epoch;
    h.msg(Msg::BlockHistory {
        epoch,
        title,
        result,
    });
    let Some(Overlay::Text { title, text, .. }) = h.app.top_overlay() else {
        panic!("no history: {:?}", h.app.top_overlay());
    };
    assert!(title.contains("Stairs pulse"), "{}", title);
    assert!(text.starts_with("c3"), "added in c3: {}", text);
    // Esc walks back out
    h.keys(&["Esc", "Esc"]);
    assert!(matches!(h.app.top_overlay(), Some(Overlay::Browse(_))));
}

/// `m` marks a commit; selecting another compares the two; `w` on a diff
/// line opens that block's wiring in the snapshot.
#[test]
fn config_compare_and_diff_wiring() {
    let mut h = config_history();
    h.keys(&["m", "j", "j"]);
    assert_eq!(h.app.config_mark.as_deref(), Some("c3"));
    assert_eq!(update::config_diff_key(&h.app).as_deref(), Some("c1..c3"));
    h.tick(1.0);
    assert!(
        h.all.iter().any(
            |e| matches!(e, Effect::Poll { kind: PollKind::ConfigDiff(c), .. } if c == "c1..c3")
        )
    );
    feed_diff(&mut h, "c1..c3");
    let s = h.render(140, 36);
    assert!(s.contains("Night mode"), "rename across two commits: {}", s);
    assert!(s.contains("Stairs pulse"), "{}", s);
    // w on the added block: wiring at the newer side
    h.keys(&["Tab"]);
    let lines = h.app.diffs["c1..c3"].clone();
    let i = lines
        .iter()
        .position(|l| l.text.contains("Stairs pulse") && l.block.is_some())
        .expect("a line about the stairs pulse");
    for _ in 0..i {
        h.keys(&["j"]);
    }
    h.keys(&["w"]);
    assert!(
        h.fx.iter().any(
            |e| matches!(e, Effect::LoadSnapshot { spec, open: super::app::SnapOpen::Wiring(_) } if spec == "c3")
        ),
        "{:?}",
        h.fx
    );
    feed_snapshot(&mut h);
    assert!(matches!(h.app.top_overlay(), Some(Overlay::Wiring(w)) if w.snap.is_some()));
    // m again on the marked commit clears the mark
    h.keys(&["Esc", "Tab", "k", "k", "m"]);
    assert!(h.app.config_mark.is_none());
}

#[test]
fn commit_summary_from_pull_message() {
    let c = super::app::Commit::new(
        "h",
        "d",
        "Config backup 2026-09-25 18:39:01 (v273)",
        "+ Added control: \"Registriertes Gerät\" (PuDe)",
    );
    assert_eq!(c.saved.as_deref(), Some("2026-09-25 18:39"));
    assert_eq!(c.version.as_deref(), Some("v273"));
    assert_eq!(c.summary, "+ Registriertes Gerät (PuDe)");
    let c = super::app::Commit::new(
        "h",
        "d",
        "x",
        "+ Added control: a\n+ Added control: b\n- Removed control: c",
    );
    assert_eq!(c.summary, "+2 −1 controls");
}

/// System overview: PLC, clock, SD wear and live LAN/CAN rates.
#[test]
fn system_overview_details() {
    let mut h = H::new();
    h.keys(&["5"]);
    h.poll(PollKind::Diag, Polled::Diag(demo::diag(1.0)));
    h.poll(PollKind::Info, Polled::Info(demo::info()));
    h.poll(PollKind::BusLan, Polled::BusLan(demo::bus_lan(1.0)));
    h.tick(5.0);
    h.poll(PollKind::BusLan, Polled::BusLan(demo::bus_lan(6.0)));
    let s = h.render(160, 44);
    for want in [
        "running · 100 cycles/s",
        "✓ in sync",
        "SD life",
        "4 % used",
        "LAN   ↓",
        "CAN   ↓",
        "no new bus or LAN errors",
    ] {
        assert!(s.contains(want), "missing {:?}:\n{}", want, s);
    }
    // a drifting clock warns
    let mut d = demo::diag(2.0);
    d.clock_drift = Some(-95.0);
    h.poll(PollKind::Diag, Polled::Diag(d));
    assert!(h.render(160, 44).contains("⚠ 95 s slow"));
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

/// Drag selects text inside the pane it starts in and copies it on release;
/// the wheel scrolls the pane under the pointer.
#[test]
fn mouse_select_copies_and_wheel_scrolls_pane_under_pointer() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut h = H::new();
    h.keys(&["5"]);
    while h.app.system.view != SysView::Config {
        h.keys(&["]"]);
    }
    let commits = vec![super::app::Commit::new(
        "c2",
        "2026-09-26 08:41:37 +0200",
        "Config backup 2026-09-25 18:39:01 (v273)",
        "",
    )];
    h.poll(PollKind::ConfigLog, Polled::ConfigLog(Ok(commits), None));
    h.poll(
        PollKind::ConfigDiff("c2".into()),
        Polled::ConfigDiff(
            "c2".into(),
            vec![
                "= 1 added".into(),
                "# Security".into(),
                "+ wire   Bewegung.Q → Licht Außen.AI".into(),
            ],
        ),
    );
    let s = h.render(140, 30);
    let find = |s: &str, pat: &str| {
        s.lines().enumerate().find_map(|(y, l)| {
            let b = l.find(pat)?;
            Some((l[..b].chars().count() as u16, y as u16))
        })
    };
    let (x0, y0) = find(&s, "Security").unwrap();
    let (x1, y1) = find(&s, "Außen.AI").unwrap();
    let ev = |kind, (column, row): (u16, u16)| {
        Msg::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    h.msg(ev(MouseEventKind::Down(MouseButton::Left), (x0, y0)));
    h.render(140, 30);
    h.msg(ev(MouseEventKind::Drag(MouseButton::Left), (x1 + 7, y1)));
    h.render(140, 30);
    h.msg(ev(MouseEventKind::Up(MouseButton::Left), (x1 + 7, y1)));
    let copied = h.fx.iter().find_map(|e| match e {
        Effect::Copy(t) => Some(t.clone()),
        _ => None,
    });
    let copied = copied.expect("copied on release");
    assert_eq!(
        copied, "Security\n   + wire   Bewegung.Q → Licht Außen.AI",
        "stays inside the diff pane"
    );
    assert!(h.app.drag.is_some(), "the selection stays visible");
    h.keys(&["j"]);
    assert!(h.app.drag.is_none(), "a key clears it");
    // wheel over the diff focuses and scrolls it, not the commit list
    assert_eq!(h.app.system.pane, 1, "clicking the diff focused it");
    h.keys(&["Tab"]);
    assert_eq!(h.app.system.pane, 0);
    h.render(140, 30);
    h.msg(ev(MouseEventKind::ScrollDown, (x1, y1)));
    assert_eq!(h.app.system.pane, 1);
}

/// A failed one-shot poll (Miniserver info) is retried instead of leaving the
/// System fields blank until the next structure load.
#[test]
fn failed_info_poll_is_retried() {
    let mut h = H::new();
    h.tick(1.0);
    let info_req = |fx: &[Effect]| {
        fx.iter().find_map(|e| match e {
            Effect::Poll {
                req,
                kind: PollKind::Info,
            } => Some(*req),
            _ => None,
        })
    };
    let req = info_req(&h.all).expect("info polled at start");
    let e = h.app.epoch;
    h.msg(Msg::Polled {
        epoch: e,
        req,
        kind: PollKind::Info,
        result: Err("timeout".into()),
    });
    h.all.clear();
    h.tick(5.0);
    assert!(
        info_req(&h.all).is_none(),
        "no hammering right after a failure"
    );
    h.tick(30.0);
    let again = info_req(&h.all).expect("info retried");
    h.msg(Msg::Polled {
        epoch: e,
        req: again,
        kind: PollKind::Info,
        result: Ok(Polled::Info(demo::info())),
    });
    h.all.clear();
    h.tick(120.0);
    assert!(
        info_req(&h.all).is_none(),
        "a successful info poll is not repeated"
    );
}

/// Facets (§4.5a): `f ⏎` keeps the selected control's room; values of a
/// dimension OR, dimensions AND; Esc on the list clears them.
#[test]
fn facets_rooms_this_room_or_and() {
    let mut h = H::new();
    h.keys(&["2"]);
    let blind = h.cid("Blind South", "Living room");
    let living = h.app.house.ctrls[blind].room.unwrap();
    h.keys(&[":"]).typed("living south").keys(&["Enter"]);
    h.keys(&["f"]);
    assert!(matches!(
        h.app.overlays.last(),
        Some(Overlay::Facets { .. })
    ));
    let opts = lists::facet_options(&h.app, FacetList::Controls, "");
    assert!(
        opts[0].suggested && opts[0].facet == Facet::Room(living),
        "{:?}",
        opts[0]
    );
    h.keys(&["Enter"]);
    assert!(h.app.overlays.is_empty());
    assert_eq!(h.app.rooms.facets, vec![Facet::Room(living)]);
    let all = lists::group_ctrls(&h.app, &GKey::All);
    assert!(!all.is_empty());
    assert!(
        all.iter()
            .all(|c| h.app.house.ctrls[*c].room == Some(living))
    );
    let s = h.render(140, 40);
    assert!(s.contains("room:Living room"), "pill in the title");

    // OR within the room dimension
    let office = h.app.house.ctrls[h.cid("Ceiling", "Office")].room.unwrap();
    h.app.rooms.facets.push(Facet::Room(office));
    let both = lists::group_ctrls(&h.app, &GKey::All).len();
    assert!(both > all.len());
    // AND with a type
    h.app.rooms.facets.push(Facet::Type("Jalousie".into()));
    let blinds = lists::group_ctrls(&h.app, &GKey::All);
    assert!(!blinds.is_empty() && blinds.len() < both);
    assert!(blinds.contains(&blind));
    assert!(
        blinds
            .iter()
            .all(|c| h.app.house.ctrls[*c].kind == Kind::Blind)
    );
    // counts ignore the value's own dimension: toggling a room adds its blinds
    let opts = lists::facet_options(&h.app, FacetList::Controls, "");
    let ty = opts
        .iter()
        .find(|o| !o.suggested && o.facet == Facet::Type("Jalousie".into()))
        .unwrap();
    assert!(ty.active);
    assert_eq!(ty.count, blinds.len());
    // typing narrows the values
    let q = lists::facet_options(&h.app, FacetList::Controls, "jalou");
    assert!(!q.is_empty() && q.iter().all(|o| o.label.contains("Jalousie")));
    // Esc on the list clears them
    h.app.rooms.pane = 1;
    h.keys(&["Esc"]);
    assert!(h.app.rooms.facets.is_empty());
}

/// Facet picker keys: ␣ toggles and stays open, C-x clears, Esc clears the
/// query first, then closes.
#[test]
fn facets_picker_keys() {
    let mut h = H::new();
    h.keys(&["2", "f"]).typed("favorite");
    let Some(Overlay::Facets { line, sel, .. }) = h.app.overlays.last() else {
        panic!("picker open");
    };
    assert_eq!((line.buf.as_str(), *sel), ("favorite", 0));
    h.keys(&["Space"]);
    assert_eq!(h.app.rooms.facets, vec![Facet::Fav]);
    assert!(matches!(
        h.app.overlays.last(),
        Some(Overlay::Facets { .. })
    ));
    let favs = lists::group_ctrls(&h.app, &GKey::All);
    assert!(
        favs.iter()
            .all(|c| h.app.house.ctrls[*c].is_favorite || h.app.is_pinned(*c))
    );
    h.keys(&["C-x"]);
    assert!(h.app.rooms.facets.is_empty());
    h.keys(&["Esc"]);
    let Some(Overlay::Facets { line, .. }) = h.app.overlays.last() else {
        panic!("first Esc clears the query");
    };
    assert!(line.buf.is_empty());
    h.keys(&["Esc"]);
    assert!(h.app.overlays.is_empty());
}

/// Events: `e` on a control sets the ctrl facet; the picker offers room and
/// control of the selected event; source "system" for events without a control.
#[test]
fn facets_events() {
    let mut h = H::new();
    h.run_sim(30.0);
    let light = h.cid("Hallway light", "Hallway");
    let u = h.app.house.ctrls[light].uuid.clone();
    let _ = h.sim.command(&u, "on");
    h.run_sim(1.0);
    h.keys(&[":"]).typed("hallway light").keys(&["Enter", "e"]);
    assert_eq!(h.app.screen, Screen::Events);
    let top = h.app.house.top(light);
    assert_eq!(h.app.events.facets, vec![Facet::Ctrl(top)]);
    let rows = lists::event_rows(&h.app);
    assert!(!rows.is_empty());
    assert!(
        rows.iter()
            .all(|i| h.app.store.events[*i].cid.map(|c| h.app.house.top(c)) == Some(top))
    );
    assert!(h.render(140, 40).contains("ctrl:"));
    h.keys(&["f"]);
    let opts = lists::facet_options(&h.app, FacetList::Events, "");
    let room = h.app.house.ctrls[light].room.unwrap();
    assert!(
        opts.iter()
            .any(|o| o.suggested && o.facet == Facet::Room(room))
    );
    assert!(opts.iter().any(|o| o.active && o.facet == Facet::Ctrl(top)));
    h.keys(&["Esc", "Esc"]);
    assert!(h.app.events.facets.is_empty(), "Esc: filter, then facets");
}

/// Deliver the chart windows the app asked for (demo data).
fn feed_chart(h: &mut H) {
    let Some(Overlay::Chart(c)) = h.app.overlays.last().cloned() else {
        panic!("chart open");
    };
    for &cid in &c.cids {
        for prev in [false, true] {
            if prev && !c.compare {
                continue;
            }
            let k = c.key(cid, prev);
            let (from, to) = super::exec::chart_window(h.app.now as i64, &k);
            let name = h.app.house.ctrls[cid].name.clone();
            let series = demo::history_range(&name, from, to);
            h.poll(
                PollKind::Chart(k),
                Polled::Chart(k, ChartData { from, to, series }),
            );
        }
    }
}

fn chart_polls(fx: &[Effect]) -> Vec<ChartKey> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Poll {
                kind: PollKind::Chart(k),
                ..
            } => Some(*k),
            _ => None,
        })
        .collect()
}

/// §5.10: `c` opens the history chart; timeframes, periods, compare, cursor
/// and the CLI equivalent.
#[test]
fn chart_timeframes_periods_compare() {
    let mut h = H::new();
    h.keys(&[":"])
        .typed("room climate living")
        .keys(&["Enter", "c"]);
    let cid = h.cid("Room climate", "Living room");
    let Some(Overlay::Chart(c)) = h.app.overlays.last().cloned() else {
        panic!("chart open");
    };
    assert_eq!((c.cids.clone(), c.span, c.back), (vec![cid], Span::H24, 0));
    assert_eq!(
        chart_polls(&h.fx),
        [c.key(cid, false)],
        "fetches the window"
    );
    let s = h.render(130, 36);
    assert!(s.contains("loading statistics"), "{}", s);
    feed_chart(&mut h);
    let s = h.render(130, 36);
    assert!(s.contains("min ") && s.contains("⌀"), "{}", s);
    assert!(s.contains("°"), "unit from the control");

    // 3 = 7 days, remembered
    h.keys(&["3"]);
    let Some(Overlay::Chart(c)) = h.app.overlays.last().cloned() else {
        panic!()
    };
    assert_eq!(c.span, Span::D7);
    assert_eq!(h.app.ui_state.chart_span, Some(Span::D7));
    assert!(h.fx.iter().any(|e| matches!(e, Effect::SaveState(_))));
    assert_eq!(chart_polls(&h.fx), [c.key(cid, false)]);
    // ← one period back, compare with the one before it
    h.keys(&["Left", "c"]);
    let Some(Overlay::Chart(c)) = h.app.overlays.last().cloned() else {
        panic!()
    };
    assert_eq!((c.back, c.compare), (1, true));
    let polled: Vec<ChartKey> = chart_polls(&h.all);
    assert!(polled.contains(&c.key(cid, false)) && polled.contains(&c.key(cid, true)));
    feed_chart(&mut h);
    let s = h.render(130, 36);
    assert!(s.contains("Δ⌀") && s.contains("1 back"), "{}", s);
    // cursor readout with the previous period
    h.keys(&["h", "H"]);
    let s = h.render(130, 36);
    assert!(s.contains("▲ ") && s.contains("(prev "), "{}", s);
    // y: the CLI equivalent
    h.keys(&["y"]);
    let copied = h.fx.iter().find_map(|e| match e {
        Effect::Copy(s) => Some(s.clone()),
        _ => None,
    });
    let copied = copied.expect("copied");
    assert!(
        copied.starts_with("lox history \"Room climate\" -r \"Living room\" --month "),
        "{}",
        copied
    );
    // Esc: cursor first, then close
    h.keys(&["Esc"]);
    assert!(matches!(h.app.overlays.last(), Some(Overlay::Chart(c)) if c.cursor.is_none()));
    h.keys(&["Esc"]);
    assert!(h.app.overlays.is_empty());
    // the next chart opens with the remembered timeframe
    h.keys(&["c"]);
    assert!(matches!(h.app.overlays.last(), Some(Overlay::Chart(c)) if c.span == Span::D7));
}

/// `+` adds a series by name; `-` removes it; controls without statistics
/// don't open a chart.
#[test]
fn chart_series_and_no_stats() {
    let mut h = H::new();
    h.keys(&[":"])
        .typed("room climate living")
        .keys(&["Enter", "c", "+"]);
    h.typed("temperature office").keys(&["Enter"]);
    let office = h.cid("Temperature", "Office");
    let Some(Overlay::Chart(c)) = h.app.overlays.last().cloned() else {
        panic!("back on the chart")
    };
    assert_eq!(c.cids.len(), 2);
    assert_eq!(c.cids[1], office);
    assert!(chart_polls(&h.fx).contains(&c.key(office, false)));
    feed_chart(&mut h);
    assert!(h.render(130, 36).contains("Office"));
    h.keys(&["+"]).typed("zzqqxx").keys(&["Enter"]);
    assert!(
        matches!(
            h.app.overlays.last(),
            Some(Overlay::Input { err: Some(_), .. })
        ),
        "no match keeps the prompt with an error"
    );
    h.keys(&["Esc", "-"]);
    let Some(Overlay::Chart(c)) = h.app.overlays.last().cloned() else {
        panic!()
    };
    assert_eq!(c.cids.len(), 1);
    h.keys(&["Esc"]);

    let blind = h.cid("Blind South", "Living room");
    assert!(!h.app.house.ctrls[blind].has_stats);
    h.keys(&[":"]).typed("living south").keys(&["Enter", "c"]);
    assert!(h.app.overlays.is_empty(), "no chart without statistics");
    assert!(
        h.app
            .toasts
            .iter()
            .any(|t| t.text.contains("no statistics"))
    );
}

/// The inspector's states are selectable; ⏎ shows the full value (JSON
/// pretty-printed), `y` copies it raw.
#[test]
fn inspector_full_state_value() {
    let mut h = H::new();
    h.msg(Msg::Resize(170, 40));
    h.keys(&[":"]).typed("lighting living").keys(&["Enter"]);
    h.run_sim(1.0);
    h.keys(&["l", "l"]);
    assert_eq!(h.app.rooms.pane, 2);
    let lc = lists::selected_ctrl(&h.app).unwrap();
    let i = h.app.house.ctrls[lc]
        .states
        .keys()
        .position(|k| k == "moodList")
        .unwrap();
    for _ in 0..i {
        h.keys(&["j"]);
    }
    assert_eq!(h.app.insp_sel, i);
    h.keys(&["Enter"]);
    assert!(matches!(
        h.app.overlays.last(),
        Some(Overlay::Value { state, .. }) if state == "moodList"
    ));
    let s = h.render(170, 40);
    assert!(s.contains("\"name\": \"Evening\""), "pretty JSON: {}", s);
    h.keys(&["y"]);
    let copied = h.fx.iter().find_map(|e| match e {
        Effect::Copy(s) => Some(s.clone()),
        _ => None,
    });
    let copied = copied.expect("copied");
    assert!(
        copied.starts_with("[{") && !copied.contains('\n'),
        "raw: {}",
        copied
    );
    h.keys(&["Esc"]);
    assert!(h.app.overlays.is_empty());
    assert_eq!(h.app.rooms.pane, 2, "back in the inspector");
}
