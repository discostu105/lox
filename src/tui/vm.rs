//! View models: what a control looks like right now (glyph, value, meter) and
//! what the action vocabulary (§4.3) does to it.
//!
//! Pure functions over `House` + `Store`, shared by every screen, the hint notches
//! and the tests.

use crate::actions::{
    self, Action, AlarmCmd, BlindCmd, ChargerCmd, DoorCmd, GateCmd, IntercomCmd, MOOD_OFF_ID,
    MoodCmd, ThermoCmd,
};

use super::model::{Cid, House, Kind};
use super::store::Store;
use super::text::{clean, fmt_kw, fmt_kwh, lox_format};
use super::theme::Grad;

/// Semantic tone of a value; the theme maps it to a color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Text,
    Dim,
    Faint,
    Ok,
    Warn,
    Crit,
    Info,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CtrlView {
    pub on: Option<bool>,
    pub tone: Tone,
    /// Fill level for a meter (0..1)
    pub frac: Option<f64>,
    pub grad: Grad,
    /// Main value text ("65 %", "22.1° → 22.0°", "closed"); "—" when unknown
    pub value: String,
    /// Secondary state ("▼ moving", "mood Evening", "auto")
    pub extra: String,
    pub extra_tone: Tone,
    /// Needs attention (window open, low battery, alarm)
    pub attention: bool,
    /// Something is physically moving (blind, gate)
    pub moving: bool,
    pub locked: bool,
}

impl CtrlView {
    fn new(value: impl Into<String>) -> CtrlView {
        CtrlView {
            on: None,
            tone: Tone::Text,
            frac: None,
            grad: Grad::Info,
            value: value.into(),
            extra: String::new(),
            extra_tone: Tone::Dim,
            attention: false,
            moving: false,
            locked: false,
        }
    }
    fn unknown() -> CtrlView {
        let mut v = CtrlView::new("—");
        v.tone = Tone::Faint;
        v
    }
}

fn pct(v: f64) -> String {
    format!("{:.0} %", v)
}

/// Parse `activeMoods` ("[2]", "[778]", "[1,3]") into mood IDs.
pub fn active_moods(s: &Store, h: &House, cid: Cid) -> Option<Vec<u32>> {
    let t = s.st_text(h, cid, "activeMoods")?;
    serde_json::from_str::<Vec<u32>>(t).ok()
}

pub fn mood_list(s: &Store, h: &House, cid: Cid) -> Vec<actions::MoodEntry> {
    s.st_text(h, cid, "moodList")
        .and_then(|t| actions::parse_mood_list(t).ok())
        .unwrap_or_default()
}

/// A light controller is on unless its only active mood is "off" (778).
pub fn light_on(s: &Store, h: &House, cid: Cid) -> Option<bool> {
    active_moods(s, h, cid).map(|m| !(m.is_empty() || m == [MOOD_OFF_ID]))
}

/// Brightness 0–100 from a ColorPicker `hsv(h,s,v)` / `temp(b,k)` text.
pub fn color_level(t: &str) -> Option<f64> {
    let inner = t.split_once('(')?.1.trim_end_matches(')');
    let parts: Vec<&str> = inner.split(',').collect();
    if t.starts_with("hsv") {
        parts.get(2)?.trim().parse().ok()
    } else {
        parts.first()?.trim().parse().ok()
    }
}

/// Main value of a light-ish control, 0–100 (for sums, meters and cards).
pub fn light_level(s: &Store, h: &House, cid: Cid) -> Option<f64> {
    match h.ctrls[cid].kind {
        Kind::Dimmer => s.st(h, cid, "position"),
        Kind::ColorPicker => s.st_text(h, cid, "color").and_then(color_level),
        // some light-circuit switches report `active` as 0/100, not 0/1 (real data)
        Kind::Switch => s
            .st(h, cid, "active")
            .map(|v| if v > 0.0 { 100.0 } else { 0.0 }),
        Kind::LightCtl => {
            let on = light_on(s, h, cid)?;
            if !on {
                return Some(0.0);
            }
            let lv: Vec<f64> = h.ctrls[cid]
                .subs
                .iter()
                .filter_map(|c| light_level(s, h, *c))
                .filter(|v| *v > 0.0)
                .collect();
            if lv.is_empty() {
                Some(100.0)
            } else {
                Some(lv.iter().sum::<f64>() / lv.len() as f64)
            }
        }
        _ => None,
    }
}

pub fn is_light_on(s: &Store, h: &House, cid: Cid) -> bool {
    match h.ctrls[cid].kind {
        Kind::LightCtl => light_on(s, h, cid).unwrap_or(false),
        Kind::Switch if h.cat_name(cid).is_some_and(is_light_cat) => {
            s.st(h, cid, "active").unwrap_or(0.0) > 0.0
        }
        Kind::Dimmer | Kind::ColorPicker => light_level(s, h, cid).unwrap_or(0.0) > 0.0,
        _ => false,
    }
}

/// Lighting category names (Loxone default names in a few languages).
pub fn is_light_cat(n: &str) -> bool {
    let l = n.to_lowercase();
    l.contains("light") || l.contains("beleucht") || l.contains("licht") || l.contains("éclair")
}

/// Is this control a light for aggregates (lights on in a room)?
pub fn is_light(h: &House, cid: Cid) -> bool {
    match h.ctrls[cid].kind {
        Kind::LightCtl | Kind::Dimmer | Kind::ColorPicker => true,
        Kind::Switch => h.cat_name(cid).is_some_and(is_light_cat),
        _ => false,
    }
}

/// Is a blind/gate moving, and in which direction (+1 down/open, -1 up/close)?
pub fn motion(s: &Store, h: &House, cid: Cid) -> Option<i8> {
    match h.ctrls[cid].kind {
        Kind::Blind | Kind::CentralBlind => {
            if s.st(h, cid, "down").unwrap_or(0.0) > 0.0 {
                Some(1)
            } else if s.st(h, cid, "up").unwrap_or(0.0) > 0.0 {
                Some(-1)
            } else {
                None
            }
        }
        Kind::Gate => match s.st(h, cid, "active").unwrap_or(0.0) {
            v if v > 0.0 => Some(1),
            v if v < 0.0 => Some(-1),
            _ => None,
        },
        _ => None,
    }
}

/// Number format of a control's main value.
fn fmt_value(h: &House, cid: Cid, v: f64) -> String {
    match &h.ctrls[cid].format {
        Some(f) => lox_format(f, v),
        None => {
            if v.fract() == 0.0 {
                format!("{}", v as i64)
            } else {
                format!("{:.2}", v)
            }
        }
    }
}

pub fn view(s: &Store, h: &House, cid: Cid) -> CtrlView {
    let c = &h.ctrls[cid];
    let st = |n: &str| s.st(h, cid, n);
    let mut v = match c.kind {
        Kind::LightCtl | Kind::CentralLight => {
            let Some(on) = light_on(s, h, cid) else {
                return CtrlView::unknown();
            };
            let lvl = light_level(s, h, cid);
            let moods = mood_list(s, h, cid);
            let act = active_moods(s, h, cid).unwrap_or_default();
            let name = act
                .iter()
                .filter(|id| **id != MOOD_OFF_ID)
                .map(|id| {
                    moods
                        .iter()
                        .find(|m| m.id == *id as u64)
                        .map(|m| clean(&m.name))
                        .unwrap_or_else(|| format!("mood {}", id))
                })
                .collect::<Vec<_>>()
                .join(" + ");
            let mut v = CtrlView::new(if on {
                lvl.map(pct).unwrap_or_else(|| "on".into())
            } else {
                "off".into()
            });
            v.on = Some(on);
            v.frac = lvl.map(|l| l / 100.0);
            v.grad = Grad::Lamp;
            if on && !name.is_empty() {
                v.extra = format!("mood {}", name);
            }
            v
        }
        Kind::Dimmer | Kind::ColorPicker => {
            let Some(l) = light_level(s, h, cid) else {
                return CtrlView::unknown();
            };
            let mut v = CtrlView::new(if l > 0.0 { pct(l) } else { "off".into() });
            v.on = Some(l > 0.0);
            v.frac = Some(l / 100.0);
            v.grad = Grad::Lamp;
            if c.kind == Kind::ColorPicker
                && let Some(t) = s.st_text(h, cid, "color")
            {
                v.extra = clean(t);
            }
            v
        }
        Kind::Switch | Kind::Pushbutton => {
            let Some(a) = st("active") else {
                return CtrlView::unknown();
            };
            let on = a > 0.0;
            let mut v = CtrlView::new(if on { "on" } else { "off" });
            v.on = Some(on);
            v
        }
        Kind::Blind | Kind::CentralBlind => {
            let Some(p) = st("position") else {
                return CtrlView::unknown();
            };
            let p = p.clamp(0.0, 1.0);
            let mut v = CtrlView::new(if p <= 0.001 {
                "open".to_string()
            } else if p >= 0.999 {
                "closed".to_string()
            } else {
                pct(p * 100.0)
            });
            v.frac = Some(p);
            v.grad = Grad::Info;
            v.on = Some(p > 0.001);
            match motion(s, h, cid) {
                Some(1) => {
                    v.extra = "▼ moving".into();
                    v.moving = true;
                    v.extra_tone = Tone::Info;
                }
                Some(_) => {
                    v.extra = "▲ moving".into();
                    v.moving = true;
                    v.extra_tone = Tone::Info;
                }
                None if st("autoActive").unwrap_or(0.0) > 0.0 => v.extra = "auto".into(),
                None => {}
            }
            v.locked = st("locked").unwrap_or(0.0) > 0.0 || st("safetyActive").unwrap_or(0.0) > 0.0;
            v
        }
        Kind::Gate => {
            let Some(p) = st("position") else {
                return CtrlView::unknown();
            };
            let mut v = CtrlView::new(if p <= 0.001 {
                "closed".to_string()
            } else if p >= 0.999 {
                "open".to_string()
            } else {
                format!("{:.0} % open", p * 100.0)
            });
            v.frac = Some(p);
            v.on = Some(p > 0.001);
            v.attention = p > 0.001;
            v.tone = if p > 0.001 { Tone::Warn } else { Tone::Text };
            if let Some(d) = motion(s, h, cid) {
                v.extra = if d > 0 { "▲ opening" } else { "▼ closing" }.into();
                v.moving = true;
                v.extra_tone = Tone::Info;
            }
            v
        }
        Kind::Climate => {
            let Some(a) = st("tempActual") else {
                return CtrlView::unknown();
            };
            let t = st("tempTarget");
            let mut v = CtrlView::new(match t {
                Some(t) => format!("{:.1}° → {:.1}°", a, t),
                None => format!("{:.1}°", a),
            });
            v.grad = Grad::Temp;
            v.frac = Some(temp_frac(a));
            if let Some(t) = t {
                if a < t - 0.3 {
                    v.extra = "▲ heating".into();
                    v.extra_tone = Tone::Warn;
                } else if a > t + 0.3 {
                    v.extra = "▼ cooling".into();
                    v.extra_tone = Tone::Info;
                }
            }
            if let Some(m) = st("operatingMode") {
                let mode = actions::thermo_mode_name(&format!("{}", m as i64)).to_string();
                v.extra = if v.extra.is_empty() {
                    mode
                } else {
                    format!("{}  {}", mode, v.extra)
                };
            }
            v
        }
        Kind::Alarm => {
            let Some(armed) = st("armed") else {
                return CtrlView::unknown();
            };
            let level = st("level").unwrap_or(0.0);
            let mut v = CtrlView::new(if level > 0.0 {
                "ALARM"
            } else if armed > 0.0 {
                "armed"
            } else {
                "disarmed"
            });
            v.on = Some(armed > 0.0);
            v.tone = if level > 0.0 {
                Tone::Crit
            } else if armed > 0.0 {
                Tone::Ok
            } else {
                Tone::Dim
            };
            v.attention = level > 0.0;
            if armed > 0.0 && st("disabledMove").unwrap_or(0.0) > 0.0 {
                v.extra = "without motion".into();
            }
            v
        }
        Kind::Smoke => {
            let level = st("level").unwrap_or(0.0);
            let mut v = CtrlView::new(if level > 0.0 { "ALARM" } else { "ok" });
            v.tone = if level > 0.0 { Tone::Crit } else { Tone::Ok };
            v.attention = level > 0.0;
            v
        }
        Kind::DoorLock => {
            let Some(a) = st("active") else {
                return CtrlView::unknown();
            };
            let mut v = CtrlView::new(if a > 0.0 { "locked" } else { "unlocked" });
            v.on = Some(a > 0.0);
            v.tone = if a > 0.0 { Tone::Ok } else { Tone::Warn };
            v
        }
        Kind::Intercom => {
            let ring = st("bell").unwrap_or(0.0) > 0.0;
            let mut v = CtrlView::new(if ring { "ringing" } else { "idle" });
            v.tone = if ring { Tone::Warn } else { Tone::Dim };
            v.attention = ring;
            v
        }
        Kind::Charger => {
            let charging = st("charging").unwrap_or(0.0) > 0.0;
            let p = st("power").or_else(|| st("actual")).unwrap_or(0.0);
            let max = c
                .details
                .get("maxPower")
                .and_then(|m| m.as_f64())
                .unwrap_or(11.0);
            let mut v = CtrlView::new(if charging {
                format!("charging {}", fmt_kw(p))
            } else if st("connected").unwrap_or(0.0) > 0.0 {
                "connected".into()
            } else {
                "idle".into()
            });
            v.on = Some(charging);
            v.frac = Some((p / max).clamp(0.0, 1.0));
            v.grad = Grad::Use;
            if let Some(e) = st("energySession").filter(|e| *e > 0.0) {
                v.extra = format!("{} session", fmt_kwh(e));
            }
            v
        }
        Kind::Meter => {
            let Some(a) = st("actual") else {
                return CtrlView::unknown();
            };
            let mut v = CtrlView::new(fmt_kw(a));
            v.grad = Grad::Use;
            if c.meter_type() == "storage"
                && let Some(soc) = st("storage")
            {
                v.value = format!("{:.0} %  {}", soc, fmt_kw(a));
                v.frac = Some(soc / 100.0);
                v.grad = Grad::Batt;
            }
            if let Some(d) = st("totalDay") {
                v.extra = format!("{} today", fmt_kwh(d));
            }
            v
        }
        Kind::Efm => {
            let g = st("Gpwr");
            let p = st("Ppwr");
            let mut v = CtrlView::new(match (p, g) {
                (Some(p), Some(g)) => format!("PV {}  grid {}", fmt_kw(p), fmt_kw(g)),
                (Some(p), None) => format!("PV {}", fmt_kw(p)),
                _ => "—".into(),
            });
            v.grad = Grad::Pv;
            v
        }
        Kind::Analog | Kind::Slider => {
            let Some(x) = st("value") else {
                return CtrlView::unknown();
            };
            let mut v = CtrlView::new(fmt_value(h, cid, x));
            if c.is_temperature() {
                v.grad = Grad::Temp;
                v.frac = Some(temp_frac(x));
            } else if c.format.as_deref().is_some_and(|f| f.contains("%%")) {
                v.frac = Some((x / 100.0).clamp(0.0, 1.0));
                v.grad = Grad::Info;
            }
            v
        }
        Kind::Digital => {
            let Some(a) = st("active") else {
                return CtrlView::unknown();
            };
            let on = a > 0.0;
            let key = if on { "on" } else { "off" };
            let txt = c
                .details
                .pointer(&format!("/text/{}", key))
                .and_then(|t| t.as_str())
                .map(clean)
                .unwrap_or_else(|| key.to_string());
            let mut v = CtrlView::new(txt.clone());
            v.on = Some(on);
            let color = c
                .details
                .pointer(&format!("/color/{}", key))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_lowercase();
            let warnish = color.starts_with("#f") || color.starts_with("#e") || color == "#ff0000";
            let low = txt.to_lowercase();
            if on && (warnish || low.contains("open") || low.contains("offen")) {
                v.tone = Tone::Warn;
                v.attention = true;
            } else if !on {
                v.tone = Tone::Dim;
            }
            v
        }
        Kind::Presence => {
            let Some(a) = st("active") else {
                return CtrlView::unknown();
            };
            let mut v = CtrlView::new(if a > 0.0 { "present" } else { "—" });
            v.on = Some(a > 0.0);
            v.tone = if a > 0.0 { Tone::Text } else { Tone::Faint };
            v
        }
        Kind::Text => {
            let t = c
                .states
                .values()
                .find_map(|u| s.text(u))
                .map(clean)
                .unwrap_or_else(|| "—".into());
            CtrlView::new(t)
        }
        Kind::Daytimer | Kind::Other => {
            // first numeric state we know, else first text
            let num = c.states.iter().find_map(|(n, u)| s.num(u).map(|v| (n, v)));
            match num {
                Some((n, x)) => {
                    let mut v = CtrlView::new(fmt_value(h, cid, x));
                    v.extra = n.clone();
                    v
                }
                None => match c.states.values().find_map(|u| s.text(u)) {
                    Some(t) => CtrlView::new(clean(t)),
                    None => CtrlView::unknown(),
                },
            }
        }
    };
    if st("jLocked").is_some_and(|l| l > 0.0) {
        v.locked = true;
    }
    v
}

/// Temperature → 0..1 on the `temp` gradient (14 ° .. 28 °).
pub fn temp_frac(t: f64) -> f64 {
    ((t - 14.0) / 14.0).clamp(0.0, 1.0)
}

// ── Action vocabulary (§4.3) ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verb {
    Primary,
    Plus,
    Minus,
    Min,
    Max,
    Stop,
}

/// One planned command: an action on a control, with the hint label.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub cid: Cid,
    pub action: Action,
    pub label: String,
}

fn plan(cid: Cid, action: Action, label: impl Into<String>) -> Option<Plan> {
    Some(Plan {
        cid,
        action,
        label: label.into(),
    })
}

/// Current main value as it will be after pending commands (optimistic), 0–100.
fn main_pct(s: &Store, h: &House, cid: Cid, optimistic: Option<f64>) -> Option<f64> {
    optimistic.or_else(|| match h.ctrls[cid].kind {
        Kind::Blind | Kind::CentralBlind => s.st(h, cid, "position").map(|p| p * 100.0),
        Kind::Dimmer | Kind::ColorPicker => light_level(s, h, cid),
        _ => None,
    })
}

/// What a vocabulary key does on this control right now (`None` = not applicable).
/// `last_dir` is the last blind direction the TUI sent (+1 down, -1 up).
pub fn verb(
    s: &Store,
    h: &House,
    cid: Cid,
    verb: Verb,
    optimistic: Option<f64>,
    last_dir: Option<i8>,
) -> Option<Plan> {
    let c = &h.ctrls[cid];
    let step = |d: f64| {
        let cur = main_pct(s, h, cid, optimistic)?;
        Some(((cur + d) / 10.0).round() * 10.0).map(|v: f64| v.clamp(0.0, 100.0))
    };
    match (c.kind, verb) {
        // Switches
        (Kind::Switch, Verb::Primary) => {
            if s.st(h, cid, "active").unwrap_or(0.0) > 0.0 {
                plan(cid, Action::Off, "off")
            } else {
                plan(cid, Action::On, "on")
            }
        }
        (Kind::Switch, Verb::Min) => plan(cid, Action::Off, "off"),
        (Kind::Switch, Verb::Max) => plan(cid, Action::On, "on"),
        (Kind::Pushbutton, Verb::Primary) => plan(cid, Action::Pulse, "pulse"),
        // Light controller: toggle last mood ↔ off, +/- cycle moods
        (Kind::LightCtl | Kind::CentralLight, Verb::Primary) => {
            if light_on(s, h, cid).unwrap_or(false) {
                plan(cid, Action::Mood(MoodCmd::Off), "off")
            } else {
                plan(cid, Action::On, "on")
            }
        }
        (Kind::LightCtl | Kind::CentralLight, Verb::Plus) => {
            plan(cid, Action::Mood(MoodCmd::Plus), "next mood")
        }
        (Kind::LightCtl | Kind::CentralLight, Verb::Minus) => {
            plan(cid, Action::Mood(MoodCmd::Minus), "prev mood")
        }
        (Kind::LightCtl | Kind::CentralLight, Verb::Min) => {
            plan(cid, Action::Mood(MoodCmd::Off), "off")
        }
        (Kind::LightCtl | Kind::CentralLight, Verb::Max) => plan(cid, Action::On, "on"),
        // Dimmer / color: toggle, ±10, 0/100
        (Kind::Dimmer | Kind::ColorPicker, Verb::Primary) => {
            if light_level(s, h, cid).unwrap_or(0.0) > 0.0 {
                plan(cid, Action::Off, "off")
            } else {
                plan(cid, Action::On, "on")
            }
        }
        (Kind::Dimmer, Verb::Plus) => step(10.0).and_then(|v| plan(cid, Action::Dim(v), "+10 %")),
        (Kind::Dimmer, Verb::Minus) => step(-10.0).and_then(|v| plan(cid, Action::Dim(v), "-10 %")),
        (Kind::Dimmer, Verb::Min) => plan(cid, Action::Dim(0.0), "0 %"),
        (Kind::Dimmer, Verb::Max) => plan(cid, Action::Dim(100.0), "100 %"),
        (Kind::ColorPicker, Verb::Min) => plan(cid, Action::Off, "off"),
        (Kind::ColorPicker, Verb::Max) => plan(cid, Action::On, "on"),
        // Blind: stop if moving, else opposite of the last direction
        (Kind::Blind | Kind::CentralBlind, Verb::Primary) => {
            if motion(s, h, cid).is_some() {
                return plan(cid, Action::Blind(BlindCmd::Stop), "stop");
            }
            let pos = s.st(h, cid, "position").unwrap_or(0.0);
            let down = match last_dir {
                Some(d) => d < 0,
                // no history: go towards the far end
                None => pos < 0.5,
            };
            let down = if pos >= 0.999 {
                false
            } else if pos <= 0.001 {
                true
            } else {
                down
            };
            if down {
                plan(cid, Action::Blind(BlindCmd::Down), "▼ down")
            } else {
                plan(cid, Action::Blind(BlindCmd::Up), "▲ up")
            }
        }
        (Kind::Blind, Verb::Plus) => {
            step(10.0).and_then(|v| plan(cid, Action::Blind(BlindCmd::Position(v)), "+10 %"))
        }
        (Kind::Blind, Verb::Minus) => {
            step(-10.0).and_then(|v| plan(cid, Action::Blind(BlindCmd::Position(v)), "-10 %"))
        }
        (Kind::Blind | Kind::CentralBlind, Verb::Min) => {
            plan(cid, Action::Blind(BlindCmd::Up), "▲ open")
        }
        (Kind::Blind | Kind::CentralBlind, Verb::Max) => {
            plan(cid, Action::Blind(BlindCmd::Down), "▼ close")
        }
        (Kind::Blind | Kind::CentralBlind, Verb::Stop) => {
            plan(cid, Action::Blind(BlindCmd::Stop), "stop")
        }
        // Gate
        (Kind::Gate, Verb::Primary) => {
            if motion(s, h, cid).is_some() {
                plan(cid, Action::Gate(GateCmd::Stop), "stop")
            } else if s.st(h, cid, "position").unwrap_or(0.0) > 0.5 {
                plan(cid, Action::Gate(GateCmd::Close), "close")
            } else {
                plan(cid, Action::Gate(GateCmd::Open), "open")
            }
        }
        (Kind::Gate, Verb::Min) => plan(cid, Action::Gate(GateCmd::Close), "close"),
        (Kind::Gate, Verb::Max) => plan(cid, Action::Gate(GateCmd::Open), "open"),
        (Kind::Gate, Verb::Stop) => plan(cid, Action::Gate(GateCmd::Stop), "stop"),
        // Climate: comfort temperature ±0.5
        (Kind::Climate, Verb::Plus | Verb::Minus) => {
            let cur = optimistic
                .or_else(|| s.st(h, cid, "comfortTemperature"))
                .or_else(|| s.st(h, cid, "tempTarget"))?;
            let d = if verb == Verb::Plus { 0.5 } else { -0.5 };
            let t = ((cur + d) * 2.0).round() / 2.0;
            plan(
                cid,
                Action::Thermostat(ThermoCmd::ComfortTemp(t)),
                if d > 0.0 { "+0.5°" } else { "-0.5°" },
            )
        }
        // Door lock
        (Kind::DoorLock, Verb::Primary) => {
            if s.st(h, cid, "active").unwrap_or(0.0) > 0.0 {
                plan(cid, Action::Door(DoorCmd::Unlock), "unlock")
            } else {
                plan(cid, Action::Door(DoorCmd::Lock), "lock")
            }
        }
        // Intercom
        (Kind::Intercom, Verb::Primary) => {
            if s.st(h, cid, "bell").unwrap_or(0.0) > 0.0 {
                plan(cid, Action::Intercom(IntercomCmd::Answer), "answer")
            } else {
                plan(cid, Action::Intercom(IntercomCmd::Hangup), "hang up")
            }
        }
        // Charger
        (Kind::Charger, Verb::Primary) => {
            if s.st(h, cid, "charging").unwrap_or(0.0) > 0.0 {
                plan(cid, Action::Charger(ChargerCmd::Pause), "pause")
            } else {
                plan(cid, Action::Charger(ChargerCmd::Start(None)), "start")
            }
        }
        (Kind::Charger, Verb::Stop) => plan(cid, Action::Charger(ChargerCmd::Stop), "stop"),
        // Virtual inputs / sliders: step within min/max
        (Kind::Slider, Verb::Plus | Verb::Minus | Verb::Min | Verb::Max) => {
            let (lo, hi, st) = slider_range(h, cid);
            let cur = optimistic.or_else(|| s.st(h, cid, "value")).unwrap_or(lo);
            let v = match verb {
                Verb::Plus => (cur + st).min(hi),
                Verb::Minus => (cur - st).max(lo),
                Verb::Min => lo,
                _ => hi,
            };
            plan(cid, Action::Value(trim(v)), trim(v).to_string())
        }
        (Kind::Slider, Verb::Primary) => plan(cid, Action::Pulse, "pulse"),
        _ => None,
    }
}

fn trim(v: f64) -> String {
    let s = format!("{:.3}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn slider_range(h: &House, cid: Cid) -> (f64, f64, f64) {
    let d = &h.ctrls[cid].details;
    let g = |k: &str, def: f64| d.get(k).and_then(|v| v.as_f64()).unwrap_or(def);
    (g("min", 0.0), g("max", 100.0), g("step", 1.0).max(0.001))
}

/// The `=` prompt for a control: what is set, unit, range, current value.
#[derive(Debug, Clone, PartialEq)]
pub struct SetSpec {
    pub what: &'static str,
    pub unit: &'static str,
    pub lo: f64,
    pub hi: f64,
    pub cur: Option<f64>,
    /// Accepts free text (color) instead of a number
    pub text: bool,
}

pub fn set_spec(s: &Store, h: &House, cid: Cid) -> Option<SetSpec> {
    let spec = |what, unit, lo, hi, cur| {
        Some(SetSpec {
            what,
            unit,
            lo,
            hi,
            cur,
            text: false,
        })
    };
    match h.ctrls[cid].kind {
        Kind::Blind => spec(
            "position",
            "% closed",
            0.0,
            100.0,
            s.st(h, cid, "position").map(|p| (p * 100.0).round()),
        ),
        Kind::Dimmer => spec("level", "%", 0.0, 100.0, light_level(s, h, cid)),
        Kind::Climate => spec(
            "comfort temperature",
            "°",
            5.0,
            35.0,
            s.st(h, cid, "comfortTemperature"),
        ),
        Kind::Charger => spec("charge limit", "kWh", 1.0, 100.0, s.st(h, cid, "limit")),
        Kind::Slider => {
            let (lo, hi, _) = slider_range(h, cid);
            spec("value", "", lo, hi, s.st(h, cid, "value"))
        }
        Kind::ColorPicker => Some(SetSpec {
            what: "color",
            unit: "#rrggbb",
            lo: 0.0,
            hi: 0.0,
            cur: None,
            text: true,
        }),
        _ => None,
    }
}

/// Turn a `=` input into an action (validated against the spec).
pub fn set_action(h: &House, cid: Cid, spec: &SetSpec, input: &str) -> Result<Action, String> {
    let input = input.trim();
    if spec.text {
        return actions::parse_color(input).map_err(|e| e.to_string());
    }
    let v: f64 = input
        .trim_end_matches('%')
        .trim_end_matches('°')
        .trim()
        .replace(',', ".")
        .parse()
        .map_err(|_| format!("'{}' is not a number", input))?;
    if v < spec.lo || v > spec.hi {
        return Err(format!(
            "{} must be {}–{}",
            spec.what,
            trim(spec.lo),
            trim(spec.hi)
        ));
    }
    Ok(match h.ctrls[cid].kind {
        Kind::Blind => Action::Blind(BlindCmd::Position(v)),
        Kind::Dimmer => Action::Dim(v),
        Kind::Climate => Action::Thermostat(ThermoCmd::ComfortTemp(v)),
        Kind::Charger => Action::Charger(ChargerCmd::Start(Some(v))),
        _ => Action::Value(trim(v)),
    })
}

/// One entry of a mode/mood picker or action menu.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub label: String,
    pub action: Action,
    pub current: bool,
    /// Needs a PIN (secured alarm)
    pub pin: bool,
}

fn choice(label: &str, action: Action, current: bool) -> Choice {
    Choice {
        label: label.into(),
        action,
        current,
        pin: false,
    }
}

/// `m`: moods, modes, or the alarm/door choices.
pub fn modes(s: &Store, h: &House, cid: Cid) -> Option<Vec<Choice>> {
    let c = &h.ctrls[cid];
    match c.kind {
        Kind::LightCtl | Kind::CentralLight => {
            let act = active_moods(s, h, cid).unwrap_or_default();
            let list = mood_list(s, h, cid);
            if list.is_empty() {
                return None;
            }
            Some(
                list.iter()
                    .map(|m| {
                        choice(
                            &clean(&m.name),
                            if m.id == MOOD_OFF_ID as u64 {
                                Action::Mood(MoodCmd::Off)
                            } else {
                                Action::Mood(MoodCmd::Set(m.id as u32))
                            },
                            act.contains(&(m.id as u32)),
                        )
                    })
                    .collect(),
            )
        }
        Kind::Climate => {
            let cur = s.st(h, cid, "operatingMode").map(|m| m as i64);
            Some(
                actions::THERMO_MODES
                    .iter()
                    .map(|(n, id)| {
                        choice(
                            n,
                            Action::Thermostat(ThermoCmd::Mode((*id).into())),
                            cur.map(|c| c.to_string()).as_deref() == Some(*id),
                        )
                    })
                    .collect(),
            )
        }
        Kind::Blind => Some(vec![
            choice("shade (automatic)", Action::Blind(BlindCmd::Shade), false),
            choice("▲ fully up", Action::Blind(BlindCmd::Up), false),
            choice("▼ fully down", Action::Blind(BlindCmd::Down), false),
        ]),
        Kind::Alarm => {
            let armed = s.st(h, cid, "armed").unwrap_or(0.0) > 0.0;
            let mk = |l: &str, cmd: AlarmCmd, cur: bool| Choice {
                label: l.into(),
                action: Action::Alarm { cmd, pin: None },
                current: cur,
                pin: c.is_secured,
            };
            Some(vec![
                mk("arm", AlarmCmd::Arm { motion: true }, armed),
                mk("arm (home)", AlarmCmd::ArmHome, false),
                mk("arm without motion", AlarmCmd::Arm { motion: false }, false),
                mk("disarm", AlarmCmd::Disarm, !armed),
                mk("acknowledge", AlarmCmd::Quit, false),
            ])
        }
        Kind::DoorLock => Some(vec![
            choice(
                "lock",
                Action::Door(DoorCmd::Lock),
                s.st(h, cid, "active").unwrap_or(0.0) > 0.0,
            ),
            choice("unlock", Action::Door(DoorCmd::Unlock), false),
            choice("open", Action::Door(DoorCmd::Open), false),
        ]),
        Kind::Intercom => Some(vec![
            choice("answer", Action::Intercom(IntercomCmd::Answer), false),
            choice("hang up", Action::Intercom(IntercomCmd::Hangup), false),
            choice("open door", Action::Intercom(IntercomCmd::Open), false),
        ]),
        _ => None,
    }
}

/// `a`: every action for the item (vocabulary + modes + lock/unlock).
pub fn menu(
    s: &Store,
    h: &House,
    cid: Cid,
    optimistic: Option<f64>,
    last_dir: Option<i8>,
) -> Vec<(String, Option<char>, Action)> {
    let mut out = Vec::new();
    for (v, k) in [
        (Verb::Primary, '␣'),
        (Verb::Plus, '+'),
        (Verb::Minus, '-'),
        (Verb::Min, '<'),
        (Verb::Max, '>'),
        (Verb::Stop, 's'),
    ] {
        if let Some(p) = verb(s, h, cid, v, optimistic, last_dir)
            && !out.iter().any(|(_, _, a)| *a == p.action)
        {
            out.push((p.label, Some(k), p.action));
        }
    }
    if let Some(ms) = modes(s, h, cid) {
        for m in ms {
            if !out.iter().any(|(_, _, a)| *a == m.action) {
                out.push((m.label, None, m.action));
            }
        }
    }
    if h.ctrls[cid].kind.is_actuator() {
        if view(s, h, cid).locked {
            out.push(("unlock control".into(), None, Action::UnlockControl));
        } else {
            out.push((
                "lock control".into(),
                None,
                Action::LockControl("locked from lox tui".into()),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::StateEvent;
    use crate::tui::demo;

    fn setup() -> (House, Store) {
        let st = demo::structure();
        let h = House::from_structure(&st);
        let mut s = Store::new();
        let sim = demo::Sim::new(&st);
        s.apply(&h, &sim.initial_events(), 0.0);
        (h, s)
    }

    fn set(h: &House, s: &mut Store, cid: Cid, state: &str, v: f64) {
        let u = h.ctrls[cid].state(state).unwrap().to_string();
        s.apply(h, &[StateEvent::ValueState { uuid: u, value: v }], 1.0);
    }

    #[test]
    fn views_of_demo_controls() {
        let (h, s) = setup();
        let bs = h.resolve("Blind South", None).unwrap();
        let v = view(&s, &h, bs);
        assert_eq!(v.value, "33 %");
        assert_eq!(v.frac, Some(0.33));
        let lc = h.resolve("Lighting [Living room]", None).unwrap();
        let v = view(&s, &h, lc);
        assert_eq!(v.on, Some(true));
        assert_eq!(v.extra, "mood Evening");
        let bed = h.resolve("Lighting [Bedroom]", None).unwrap();
        assert_eq!(view(&s, &h, bed).value, "off");
        let rc = h.resolve("Room climate [Living room]", None).unwrap();
        assert_eq!(view(&s, &h, rc).value, "22.1° → 22.0°");
        let w = h.resolve("Window [Bath]", None).unwrap();
        let v = view(&s, &h, w);
        assert_eq!(v.value, "open");
        assert!(v.attention);
        let hum = h.resolve("Humidity [Living room]", None).unwrap();
        assert_eq!(view(&s, &h, hum).value, "48%");
        let batt = h.resolve("Battery", None).unwrap();
        assert!(view(&s, &h, batt).value.starts_with("74 %"));
    }

    #[test]
    fn unknown_values_show_dash_not_zero() {
        let st = demo::structure();
        let h = House::from_structure(&st);
        let s = Store::new();
        let bs = h.resolve("Blind South", None).unwrap();
        assert_eq!(view(&s, &h, bs).value, "—");
        assert_eq!(view(&s, &h, bs).tone, Tone::Faint);
    }

    #[test]
    fn blind_space_semantics() {
        let (h, mut s) = setup();
        let bs = h.resolve("Blind South", None).unwrap();
        // at rest, no history, 33 % closed → goes down (towards the far end)
        let p = verb(&s, &h, bs, Verb::Primary, None, None).unwrap();
        assert_eq!(p.label, "▼ down");
        // last direction was down → opposite is up
        let p = verb(&s, &h, bs, Verb::Primary, None, Some(1)).unwrap();
        assert_eq!(p.label, "▲ up");
        // moving → stop
        set(&h, &mut s, bs, "down", 1.0);
        let p = verb(&s, &h, bs, Verb::Primary, None, Some(1)).unwrap();
        assert_eq!(p.action, Action::Blind(BlindCmd::Stop));
        // fully closed at rest → up regardless of history
        set(&h, &mut s, bs, "down", 0.0);
        set(&h, &mut s, bs, "position", 1.0);
        let p = verb(&s, &h, bs, Verb::Primary, None, Some(-1)).unwrap();
        assert_eq!(p.label, "▲ up");
    }

    #[test]
    fn steps_round_and_clamp_and_use_optimistic() {
        let (h, s) = setup();
        let bs = h.resolve("Blind South", None).unwrap();
        let p = verb(&s, &h, bs, Verb::Plus, None, None).unwrap();
        assert_eq!(p.action, Action::Blind(BlindCmd::Position(40.0)));
        let p = verb(&s, &h, bs, Verb::Plus, Some(40.0), None).unwrap();
        assert_eq!(p.action, Action::Blind(BlindCmd::Position(50.0)));
        let p = verb(&s, &h, bs, Verb::Plus, Some(100.0), None).unwrap();
        assert_eq!(p.action, Action::Blind(BlindCmd::Position(100.0)));
        let rc = h.resolve("Room climate [Living room]", None).unwrap();
        let p = verb(&s, &h, rc, Verb::Plus, None, None).unwrap();
        assert_eq!(p.action, Action::Thermostat(ThermoCmd::ComfortTemp(22.5)));
        // sensors have no actions
        let hum = h.resolve("Humidity [Living room]", None).unwrap();
        assert!(verb(&s, &h, hum, Verb::Primary, None, None).is_none());
    }

    #[test]
    fn set_input_validation() {
        let (h, s) = setup();
        let bs = h.resolve("Blind South", None).unwrap();
        let spec = set_spec(&s, &h, bs).unwrap();
        assert_eq!(spec.cur, Some(33.0));
        assert_eq!(
            set_action(&h, bs, &spec, "30").unwrap(),
            Action::Blind(BlindCmd::Position(30.0))
        );
        assert!(set_action(&h, bs, &spec, "130").is_err());
        assert!(set_action(&h, bs, &spec, "abc").is_err());
        let rc = h.resolve("Room climate [Living room]", None).unwrap();
        let spec = set_spec(&s, &h, rc).unwrap();
        assert_eq!(
            set_action(&h, rc, &spec, "21,5").unwrap(),
            Action::Thermostat(ThermoCmd::ComfortTemp(21.5))
        );
    }

    #[test]
    fn risky_actions_need_confirmation() {
        let (h, s) = setup();
        let door = h.resolve("Front door", None).unwrap();
        let p = verb(&s, &h, door, Verb::Primary, None, None).unwrap();
        assert_eq!(p.action.risk(), actions::Risk::Confirm);
        let al = h.resolve("House alarm", None).unwrap();
        let m = modes(&s, &h, al).unwrap();
        assert!(m.iter().all(|c| c.pin));
        let lc = h.resolve("Lighting [Living room]", None).unwrap();
        let moods = modes(&s, &h, lc).unwrap();
        assert!(moods.iter().any(|m| m.current && m.label == "Evening"));
    }
}
