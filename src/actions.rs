//! Shared action layer: user intent → Loxone command strings.
//!
//! Both the CLI control commands and `lox tui` go through this module, so the
//! two can never disagree about what "blind down" or "set mood" sends.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;

/// Standard Loxone mood ID for "off". System-defined, not configurable.
pub const MOOD_OFF_ID: u32 = 778;

#[derive(Debug, Clone, PartialEq)]
pub enum BlindCmd {
    Up,
    Down,
    Stop,
    /// Automatic shading
    Shade,
    /// Slat (lamella) position 0–100
    Lamella(f64),
    /// Position 0–100 (100 = closed)
    Position(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum MoodCmd {
    Plus,
    Minus,
    Off,
    Set(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GateCmd {
    Open,
    Close,
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ThermoCmd {
    ComfortTemp(f64),
    /// Operating mode ID (0 auto, 1 manual, 2 comfort, 3 eco, 4 building protection)
    Mode(String),
    Override {
        temp: f64,
        minutes: u64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlarmCmd {
    Arm { motion: bool },
    ArmHome,
    Disarm,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DoorCmd {
    Lock,
    Unlock,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntercomCmd {
    Answer,
    Hangup,
    Open,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChargerCmd {
    Start(Option<f64>),
    Stop,
    Pause,
}

/// A user intent on one control.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    On,
    Off,
    Pulse,
    Blind(BlindCmd),
    Mood(MoodCmd),
    /// Dimmer level 0–100
    Dim(f64),
    /// Color command as the Miniserver expects it: `hsv(h,s,v)` or `temp(b,k)`
    Color(String),
    Gate(GateCmd),
    Thermostat(ThermoCmd),
    Alarm {
        cmd: AlarmCmd,
        pin: Option<String>,
    },
    Door(DoorCmd),
    Intercom(IntercomCmd),
    Charger(ChargerCmd),
    LockControl(String),
    UnlockControl,
    /// Set an analog/virtual input value (already a plain value string)
    Value(String),
    /// Raw command string, sent as-is
    Raw(String),
}

/// How much confirmation an action needs before it is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    /// Harmless: lights, blinds, moods, thermostat targets
    None,
    /// Needs a y/N confirmation: doors, gates opening, alarm, charger start
    Confirm,
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

impl Action {
    /// The Loxone command strings to send, in order.
    ///
    /// Moods are switched with `changeTo/{id}` (LightControllerV2; 778 = off). The
    /// Miniserver silently ignores the undocumented `setMood/{id}`.
    pub fn commands(&self) -> Vec<String> {
        let one = |s: &str| vec![s.to_string()];
        match self {
            Action::On => one("on"),
            Action::Off => one("off"),
            Action::Pulse => one("pulse"),
            Action::Blind(b) => match b {
                BlindCmd::Up => one("FullUp"),
                BlindCmd::Down => one("FullDown"),
                BlindCmd::Stop => one("off"),
                BlindCmd::Shade => one("AutomaticDown"),
                BlindCmd::Lamella(p) => vec![format!("manualLamella/{:.4}", p)],
                BlindCmd::Position(p) => vec![format!("manualPosition/{:.4}", p)],
            },
            Action::Mood(m) => match m {
                MoodCmd::Plus => one("plus"),
                MoodCmd::Minus => one("minus"),
                MoodCmd::Off => vec![format!("changeTo/{}", MOOD_OFF_ID)],
                MoodCmd::Set(id) => vec![format!("changeTo/{}", id)],
            },
            Action::Dim(level) => vec![format!("{}", level)],
            Action::Color(c) => vec![c.clone()],
            Action::Gate(g) => one(match g {
                GateCmd::Open => "open",
                GateCmd::Close => "close",
                GateCmd::Stop => "stop",
            }),
            Action::Thermostat(t) => match t {
                ThermoCmd::ComfortTemp(v) => vec![format!("setComfortTemperature/{}", v)],
                ThermoCmd::Mode(id) => vec![format!("setOperatingMode/{}", id)],
                ThermoCmd::Override { temp, minutes } => {
                    vec![format!("override/{}/{}", temp, minutes)]
                }
            },
            Action::Alarm { cmd, pin } => {
                let base = match cmd {
                    AlarmCmd::Arm { motion: true } => "delayedon/1",
                    AlarmCmd::Arm { motion: false } | AlarmCmd::ArmHome => "delayedon/0",
                    AlarmCmd::Disarm => "off",
                    AlarmCmd::Quit => return one("quit"),
                };
                match pin {
                    Some(p) => vec![format!("{}/{}", base, p)],
                    None => one(base),
                }
            }
            Action::Door(d) => one(match d {
                DoorCmd::Lock => "on",
                DoorCmd::Unlock => "off",
                DoorCmd::Open => "open",
            }),
            Action::Intercom(i) => one(match i {
                IntercomCmd::Answer => "answer",
                IntercomCmd::Hangup => "hangup",
                IntercomCmd::Open => "open",
            }),
            Action::Charger(c) => match c {
                ChargerCmd::Start(Some(kwh)) => vec![format!("start/{:.1}", kwh)],
                ChargerCmd::Start(None) => one("start"),
                ChargerCmd::Stop => one("stop"),
                ChargerCmd::Pause => one("pause"),
            },
            Action::LockControl(reason) => {
                vec![format!(
                    "lockcontrol/1/{}",
                    crate::encode_path_value(reason)
                )]
            }
            Action::UnlockControl => one("unlockcontrol"),
            Action::Value(v) => vec![crate::encode_path_value(v)],
            Action::Raw(c) => vec![c.clone()],
        }
    }

    /// The main command (the last one sent) — used for dry-run output and logs.
    pub fn main_command(&self) -> String {
        self.commands().pop().unwrap_or_default()
    }

    /// Confirmation level (§9 of the TUI design).
    pub fn risk(&self) -> Risk {
        match self {
            Action::Door(_) => Risk::Confirm,
            // closing can trap a car or a person as easily as opening lets someone in
            Action::Gate(GateCmd::Open | GateCmd::Close) => Risk::Confirm,
            Action::Alarm { cmd, .. } if *cmd != AlarmCmd::Quit => Risk::Confirm,
            Action::Intercom(IntercomCmd::Open) => Risk::Confirm,
            Action::Charger(ChargerCmd::Start(_)) => Risk::Confirm,
            Action::LockControl(_) | Action::UnlockControl => Risk::Confirm,
            _ => Risk::None,
        }
    }

    /// Short human description for toasts: "pos 30", "mood 777", "on".
    pub fn describe(&self) -> String {
        match self {
            Action::On => "on".into(),
            Action::Off => "off".into(),
            Action::Pulse => "pulse".into(),
            Action::Blind(b) => match b {
                BlindCmd::Up => "▲ up".into(),
                BlindCmd::Down => "▼ down".into(),
                BlindCmd::Stop => "stop".into(),
                BlindCmd::Shade => "shade".into(),
                BlindCmd::Lamella(p) => format!("slats {}", fmt_num(*p)),
                BlindCmd::Position(p) => format!("pos {}", fmt_num(*p)),
            },
            Action::Mood(m) => match m {
                MoodCmd::Plus => "next mood".into(),
                MoodCmd::Minus => "previous mood".into(),
                MoodCmd::Off => "off".into(),
                MoodCmd::Set(id) => format!("mood {}", id),
            },
            Action::Dim(v) => format!("{} %", fmt_num(*v)),
            Action::Color(c) => c.clone(),
            Action::Gate(g) => format!("{:?}", g).to_lowercase(),
            Action::Thermostat(t) => match t {
                ThermoCmd::ComfortTemp(v) => format!("comfort {}°", fmt_num(*v)),
                ThermoCmd::Mode(id) => format!("mode {}", thermo_mode_name(id)),
                ThermoCmd::Override { temp, minutes } => {
                    format!("override {}° for {} min", fmt_num(*temp), minutes)
                }
            },
            Action::Alarm { cmd, .. } => match cmd {
                AlarmCmd::Arm { motion: true } => "arm".into(),
                AlarmCmd::Arm { motion: false } => "arm without motion".into(),
                AlarmCmd::ArmHome => "arm (home)".into(),
                AlarmCmd::Disarm => "disarm".into(),
                AlarmCmd::Quit => "acknowledge".into(),
            },
            Action::Door(d) => format!("{:?}", d).to_lowercase(),
            Action::Intercom(i) => format!("{:?}", i).to_lowercase(),
            Action::Charger(c) => match c {
                ChargerCmd::Start(Some(k)) => format!("start (limit {} kWh)", fmt_num(*k)),
                ChargerCmd::Start(None) => "start".into(),
                ChargerCmd::Stop => "stop".into(),
                ChargerCmd::Pause => "pause".into(),
            },
            Action::LockControl(_) => "lock".into(),
            Action::UnlockControl => "unlock".into(),
            Action::Value(v) => format!("= {}", v),
            Action::Raw(c) => c.clone(),
        }
    }

    /// The equivalent `lox` CLI invocation. PINs are never included.
    pub fn to_cli(&self, name: &str, room: Option<&str>) -> String {
        let n = shell_quote(name);
        let r = room
            .map(|r| format!(" -r {}", shell_quote(r)))
            .unwrap_or_default();
        let body = match self {
            Action::On => format!("on {n}"),
            Action::Off => format!("off {n}"),
            Action::Pulse => format!("input pulse {n}"),
            Action::Blind(b) => match b {
                BlindCmd::Up => format!("blind {n} up"),
                BlindCmd::Down => format!("blind {n} down"),
                BlindCmd::Stop => format!("blind {n} stop"),
                BlindCmd::Shade => format!("blind {n} shade"),
                BlindCmd::Lamella(p) => format!("blind {n} shade {}", fmt_num(*p)),
                BlindCmd::Position(p) => format!("blind {n} pos {}", fmt_num(*p)),
            },
            Action::Mood(m) => match m {
                MoodCmd::Plus => format!("light mood {n} plus"),
                MoodCmd::Minus => format!("light mood {n} minus"),
                MoodCmd::Off => format!("light mood {n} off"),
                MoodCmd::Set(id) => format!("light mood {n} {id}"),
            },
            Action::Dim(v) => format!("light dim {n} {}", fmt_num(*v)),
            Action::Color(c) => format!("light color {n} {}", shell_quote(c)),
            Action::Gate(g) => format!("gate {n} {}", format!("{:?}", g).to_lowercase()),
            Action::Thermostat(t) => match t {
                ThermoCmd::ComfortTemp(v) => format!("thermostat {n} temp {}", fmt_num(*v)),
                ThermoCmd::Mode(id) => format!("thermostat {n} mode {}", thermo_mode_name(id)),
                ThermoCmd::Override { temp, minutes } => {
                    format!("thermostat {n} override {} {}", fmt_num(*temp), minutes)
                }
            },
            Action::Alarm { cmd, .. } => match cmd {
                AlarmCmd::Arm { motion: true } => format!("alarm {n} arm"),
                AlarmCmd::Arm { motion: false } => format!("alarm {n} arm --no-motion"),
                AlarmCmd::ArmHome => format!("alarm {n} arm-home"),
                AlarmCmd::Disarm => format!("alarm {n} disarm"),
                AlarmCmd::Quit => format!("alarm {n} quit"),
            },
            Action::Door(d) => format!("door {n} {}", format!("{:?}", d).to_lowercase()),
            Action::Intercom(i) => format!("intercom {n} {}", format!("{:?}", i).to_lowercase()),
            Action::Charger(c) => match c {
                ChargerCmd::Start(Some(k)) => format!("charger {n} start --limit {}", fmt_num(*k)),
                ChargerCmd::Start(None) => format!("charger {n} start"),
                ChargerCmd::Stop => format!("charger {n} stop"),
                ChargerCmd::Pause => format!("charger {n} pause"),
            },
            Action::LockControl(reason) => format!("lock {n} --reason {}", shell_quote(reason)),
            Action::UnlockControl => format!("unlock {n}"),
            Action::Value(v) => format!("input set {n} {}", shell_quote(v)),
            Action::Raw(c) => format!("send {n} {}", shell_quote(c)),
        };
        format!("lox {body}{r}")
    }

    /// Control types this action applies to (`None` = any type).
    pub fn expected_types(&self) -> Option<&'static [&'static str]> {
        match self {
            Action::Blind(_) => Some(&["Jalousie", "CentralJalousie"]),
            Action::Mood(_) => Some(&[
                "LightControllerV2",
                "LightController",
                "CentralLightController",
            ]),
            // a LightControllerV2 ignores a bare level and still answers 200
            Action::Dim(_) => Some(&["Dimmer", "EIBDimmer"]),
            Action::Color(_) => Some(&["ColorPickerV2", "ColorPicker"]),
            Action::Gate(_) => Some(&["Gate", "CentralGate"]),
            Action::Thermostat(_) => Some(&["IRoomControllerV2", "IRoomController", "Fronius"]),
            Action::Alarm { .. } => Some(&["Alarm"]),
            _ => None,
        }
    }

    /// Fail with a helpful message when the action doesn't fit the control type.
    pub fn check_type(&self, name: &str, typ: &str) -> Result<()> {
        let ok = match self {
            Action::Door(_) => typ.contains("DoorLock") || typ.contains("Lock"),
            Action::Intercom(_) => typ.contains("Intercom"),
            Action::Charger(_) => {
                typ.contains("Charger") || typ.contains("EV") || typ.contains("Wallbox")
            }
            _ => match self.expected_types() {
                Some(types) => types.contains(&typ),
                None => true,
            },
        };
        if !ok {
            let what = match self {
                Action::Blind(_) => "a Jalousie",
                Action::Mood(_) => "a LightController",
                Action::Dim(_) if typ.contains("LightController") => {
                    "a Dimmer; switch a lighting controller with a mood instead"
                }
                Action::Dim(_) => "a Dimmer",
                Action::Color(_) => "a ColorPicker",
                Action::Gate(_) => "a Gate",
                Action::Thermostat(_) => "a room controller",
                Action::Alarm { .. } => "an Alarm",
                Action::Door(_) => "a DoorLock",
                Action::Intercom(_) => "an Intercom",
                Action::Charger(_) => "a Charger",
                _ => "a matching control",
            };
            bail!("'{}' is type '{}', not {}", name, typ, what);
        }
        Ok(())
    }
}

/// IRoomControllerV2 `operatingMode` values (`setOperatingMode/{id}`).
/// Comfort / eco / building protection are *temperature* modes (`activeMode`),
/// not operating modes.
pub const THERMO_MODES: [(&str, &str); 6] = [
    ("auto", "0"),
    ("auto-heat", "1"),
    ("auto-cool", "2"),
    ("manual", "3"),
    ("manual-heat", "4"),
    ("manual-cool", "5"),
];

/// Human name for a thermostat operating mode ID.
pub fn thermo_mode_name(id: &str) -> &str {
    THERMO_MODES
        .iter()
        .find(|(_, i)| *i == id)
        .map(|(n, _)| *n)
        .unwrap_or(id)
}

/// Quote a CLI argument only when needed.
pub fn shell_quote(s: &str) -> String {
    let safe = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:#%+,".contains(c));
    if safe {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn check_pct(p: f64, what: &str) -> Result<f64> {
    if !(0.0..=100.0).contains(&p) {
        bail!("{} must be 0-100", what);
    }
    Ok(p)
}

// ── Risk on a specific control (shared by the TUI and `lox mcp`) ─────────────

/// What the risk rules need to know about the control an action targets.
#[derive(Debug, Clone, Copy, Default)]
pub struct RiskTarget<'a> {
    pub typ: &'a str,
    /// The control's `defaultIcon`
    pub icon: Option<&'a str>,
    /// Its category's `image`
    pub cat_icon: Option<&'a str>,
    /// Loxone asks for the visualization password (`isSecured`)
    pub is_secured: bool,
    /// On the user's `confirm:` list (see [`on_confirm_list`])
    pub listed: bool,
}

impl Action {
    /// The risk of this action on a particular control: its own risk (doors,
    /// gates, alarm, …), plus any generic command (on/off/pulse/raw/value) aimed
    /// at a door lock, gate, alarm or a door opener wired as a plain switch, so
    /// these cannot bypass the confirmation, plus anything on a listed control.
    pub fn risk_on(&self, t: &RiskTarget) -> Risk {
        let generic = matches!(
            self,
            Action::On | Action::Off | Action::Pulse | Action::Raw(_) | Action::Value(_)
        );
        let access = t.is_secured || is_access_icon(t.icon) || is_access_icon(t.cat_icon);
        if self.risk() == Risk::Confirm || t.listed || (generic && (is_risky_type(t.typ) || access))
        {
            Risk::Confirm
        } else {
            Risk::None
        }
    }
}

/// Control types where a generic action (on/off/pulse/raw) can open a door,
/// move a gate or change the alarm state.
pub fn is_risky_type(typ: &str) -> bool {
    matches!(typ, "Alarm" | "SmokeAlarm" | "Gate" | "CentralGate") || typ.contains("DoorLock")
}

/// An icon showing a door, gate, garage, lock or key (`IconsFilled/door-open.svg`,
/// `login-key.svg`, `garage-closed-2.svg`). Installations often wire a door opener
/// as a `Pushbutton`/`Switch`; category names are user-chosen and localized, the
/// icons are not. Matched per word, so `alarm-clock.svg` is not a lock.
pub fn is_access_icon(icon: Option<&str>) -> bool {
    const ACCESS: &[&str] = &["door", "gate", "garage", "lock", "padlock", "key", "keypad"];
    icon.is_some_and(|path| {
        let file = path.rsplit('/').next().unwrap_or(path);
        let stem = file.split('.').next().unwrap_or(file);
        stem.to_lowercase()
            .split(['-', '_'])
            .any(|t| ACCESS.contains(&t))
    })
}

/// Is a control on the user's `confirm:` list (config)? An entry matches the
/// UUID, an alias of it, or the name as a case-insensitive substring with an
/// optional `[Room]` qualifier ("Pool Abdeckung" covers both cover buttons).
/// Over-matching only means one more question, so the match is deliberately loose.
pub fn on_confirm_list(
    confirm: &[String],
    aliases: &HashMap<String, String>,
    uuid: &str,
    name: &str,
    room: Option<&str>,
) -> bool {
    let contains =
        |haystack: &str, needle: &str| haystack.to_lowercase().contains(&needle.to_lowercase());
    confirm.iter().map(|e| e.trim()).any(|entry| {
        if entry.is_empty() {
            return false;
        }
        if entry.eq_ignore_ascii_case(uuid) || aliases.get(entry).is_some_and(|u| u == uuid) {
            return true;
        }
        let (n, r) = match entry.strip_suffix(']').and_then(|e| e.rsplit_once('[')) {
            Some((n, r)) => (n.trim(), Some(r.trim())),
            None => (entry, None),
        };
        !n.is_empty() && contains(name, n) && r.is_none_or(|r| contains(room.unwrap_or(""), r))
    })
}

// ── CLI argument parsers (shared with the TUI palette) ────────────────────────

/// `lox blind <name> <action> [pos]`
pub fn parse_blind(action: &str, pos: Option<f64>) -> Result<Action> {
    let cmd = match action.to_lowercase().as_str() {
        "up" | "open" => BlindCmd::Up,
        "down" | "close" => BlindCmd::Down,
        "stop" => BlindCmd::Stop,
        "shade" | "auto" => match pos {
            Some(p) => BlindCmd::Lamella(check_pct(p, "Position")?),
            None => BlindCmd::Shade,
        },
        "pos" | "position" => {
            let p = pos.ok_or_else(|| anyhow::anyhow!("pos requires a value 0-100"))?;
            BlindCmd::Position(check_pct(p, "Position")?)
        }
        other => match other.parse::<f64>() {
            Ok(p) => BlindCmd::Position(check_pct(p, "Position")?),
            Err(_) => bail!(
                "Unknown action '{}'. Use: up down stop shade [<0-100>] pos <0-100>",
                other
            ),
        },
    };
    Ok(Action::Blind(cmd))
}

/// `lox light mood <name> <plus|minus|off|id>`
pub fn parse_mood(action: &str) -> Result<Action> {
    let cmd = match action.to_lowercase().as_str() {
        "plus" | "next" | "+" => MoodCmd::Plus,
        "minus" | "prev" | "-" => MoodCmd::Minus,
        "off" => MoodCmd::Off,
        other => match other.parse::<u32>() {
            Ok(id) => MoodCmd::Set(id),
            Err(_) => bail!(
                "Unknown mood action '{}'. Use: plus, minus, off, or a numeric mood ID",
                other
            ),
        },
    };
    Ok(Action::Mood(cmd))
}

/// `lox light dim <name> <0-100>`
pub fn parse_dim(level: f64) -> Result<Action> {
    if !(0.0..=100.0).contains(&level) {
        bail!("Dimmer level must be 0-100");
    }
    Ok(Action::Dim(level))
}

/// `lox light color <name> <#RRGGBB|hsv(h,s,v)|temp(b,k)>`
pub fn parse_color(value: &str) -> Result<Action> {
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() != 6 {
            bail!("Hex color must be 6 digits: #RRGGBB");
        }
        let r = u8::from_str_radix(&hex[0..2], 16).context("Invalid red component")?;
        let g = u8::from_str_radix(&hex[2..4], 16).context("Invalid green component")?;
        let b = u8::from_str_radix(&hex[4..6], 16).context("Invalid blue component")?;
        let (h, s, v) = crate::rgb_to_hsv(r, g, b);
        Ok(Action::Color(format!("hsv({},{},{})", h, s, v)))
    } else {
        Ok(Action::Color(value.to_string()))
    }
}

/// `lox gate <name> <open|close|stop>`
pub fn parse_gate(action: &str) -> Result<Action> {
    Ok(Action::Gate(match action.to_lowercase().as_str() {
        "open" => GateCmd::Open,
        "close" => GateCmd::Close,
        "stop" => GateCmd::Stop,
        other => bail!("Unknown gate action '{}'. Use: open, close, stop", other),
    }))
}

/// `lox thermostat <name> <temp|mode|override> <value> [minutes]`
pub fn parse_thermostat(action: &str, value: Option<&str>, minutes: Option<u64>) -> Result<Action> {
    let cmd = match action.to_lowercase().as_str() {
        "temp" | "temperature" => {
            let t: f64 = value
                .ok_or_else(|| anyhow::anyhow!("Usage: lox thermostat <name> temp <°C>"))?
                .parse()
                .context("Temperature must be a number")?;
            ThermoCmd::ComfortTemp(t)
        }
        "mode" => {
            let m = value.ok_or_else(|| {
                anyhow::anyhow!(
                    "Usage: lox thermostat <name> mode <{}>",
                    THERMO_MODES.map(|(n, _)| n).join("|")
                )
            })?;
            let m = m.to_lowercase();
            let id = match m.as_str() {
                "automatic" => "0",
                n if n.len() == 1 && THERMO_MODES.iter().any(|(_, id)| *id == n) => n,
                n => match THERMO_MODES.iter().find(|(name, _)| *name == n) {
                    Some((_, id)) => id,
                    None if matches!(n, "comfort" | "eco" | "economy" | "building-protection") => {
                        bail!(
                            "'{}' is a temperature mode, not an operating mode \
                             (operating modes: {}); set the target with `lox thermostat <name> temp`",
                            n,
                            THERMO_MODES.map(|(n, _)| n).join(", ")
                        )
                    }
                    None => bail!(
                        "Unknown operating mode '{}' (expected: {})",
                        n,
                        THERMO_MODES.map(|(n, _)| n).join(", ")
                    ),
                },
            };
            ThermoCmd::Mode(id.to_string())
        }
        "override" => {
            let t: f64 = value
                .ok_or_else(|| {
                    anyhow::anyhow!("Usage: lox thermostat <name> override <°C> [minutes]")
                })?
                .parse()
                .context("Override temperature must be a number")?;
            ThermoCmd::Override {
                temp: t,
                minutes: minutes.unwrap_or(60),
            }
        }
        other => bail!(
            "Unknown thermostat action '{}'. Use: temp, mode, override",
            other
        ),
    };
    Ok(Action::Thermostat(cmd))
}

/// `lox alarm <name> <arm|arm-home|disarm|quit> [--no-motion] [--code PIN]`
pub fn parse_alarm(action: &str, no_motion: bool, pin: Option<String>) -> Result<Action> {
    let cmd = match action.to_lowercase().as_str() {
        "arm" | "on" => AlarmCmd::Arm { motion: !no_motion },
        "arm-home" | "home" => AlarmCmd::ArmHome,
        "disarm" | "off" => AlarmCmd::Disarm,
        "quit" | "ack" | "acknowledge" => AlarmCmd::Quit,
        other => bail!(
            "Unknown alarm action '{}'. Use: arm, arm-home, disarm, quit",
            other
        ),
    };
    Ok(Action::Alarm { cmd, pin })
}

/// `lox door <name> <lock|unlock|open>`
pub fn parse_door(action: &str) -> Result<Action> {
    Ok(Action::Door(match action.to_lowercase().as_str() {
        "lock" => DoorCmd::Lock,
        "unlock" => DoorCmd::Unlock,
        "open" => DoorCmd::Open,
        other => bail!(
            "Unknown doorlock action '{}'. Use: lock, unlock, open",
            other
        ),
    }))
}

/// `lox intercom <name> <answer|hangup|open>`
pub fn parse_intercom(action: &str) -> Result<Action> {
    Ok(Action::Intercom(match action.to_lowercase().as_str() {
        "answer" => IntercomCmd::Answer,
        "hangup" | "decline" => IntercomCmd::Hangup,
        "open" => IntercomCmd::Open,
        other => bail!(
            "Unknown intercom action '{}'. Use: answer, hangup, open",
            other
        ),
    }))
}

/// `lox charger <name> <start|stop|pause> [--limit kWh]`
pub fn parse_charger(action: &str, limit: Option<f64>) -> Result<Action> {
    Ok(Action::Charger(match action.to_lowercase().as_str() {
        "start" => ChargerCmd::Start(limit),
        "stop" => ChargerCmd::Stop,
        "pause" => ChargerCmd::Pause,
        other => bail!(
            "Unknown charger action '{}'. Use: start, stop, pause",
            other
        ),
    }))
}

/// One entry of a LightControllerV2 `moodList` text state.
#[derive(Debug, Clone, PartialEq)]
pub struct MoodEntry {
    pub id: u64,
    pub name: String,
}

/// Parse the `moodList` JSON array, sorted by ID. Malformed entries are skipped.
pub fn parse_mood_list(json: &str) -> Result<Vec<MoodEntry>> {
    let entries: Vec<serde_json::Value> = serde_json::from_str(json)
        .map_err(|e| anyhow::anyhow!("Failed to parse moodList JSON: {}", e))?;
    let mut moods: Vec<MoodEntry> = entries
        .iter()
        .filter_map(|v| {
            let id = v.get("id").and_then(|i| i.as_u64())?;
            let name = v.get("name").and_then(|n| n.as_str())?.to_string();
            Some(MoodEntry { id, name })
        })
        .collect();
    moods.sort_by_key(|m| m.id);
    Ok(moods)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds(a: &Action) -> Vec<String> {
        a.commands()
    }

    #[test]
    fn blind_mapping() {
        assert_eq!(cmds(&parse_blind("up", None).unwrap()), ["FullUp"]);
        assert_eq!(cmds(&parse_blind("close", None).unwrap()), ["FullDown"]);
        assert_eq!(cmds(&parse_blind("stop", None).unwrap()), ["off"]);
        assert_eq!(
            cmds(&parse_blind("shade", None).unwrap()),
            ["AutomaticDown"]
        );
        assert_eq!(
            cmds(&parse_blind("shade", Some(40.0)).unwrap()),
            ["manualLamella/40.0000"]
        );
        assert_eq!(
            cmds(&parse_blind("pos", Some(30.0)).unwrap()),
            ["manualPosition/30.0000"]
        );
        assert_eq!(
            cmds(&parse_blind("55", None).unwrap()),
            ["manualPosition/55.0000"]
        );
        assert!(parse_blind("pos", Some(101.0)).is_err());
        assert!(parse_blind("pos", None).is_err());
        assert!(parse_blind("sideways", None).is_err());
    }

    #[test]
    fn mood_mapping_sends_on_first() {
        assert_eq!(cmds(&parse_mood("plus").unwrap()), ["plus"]);
        assert_eq!(cmds(&parse_mood("-").unwrap()), ["minus"]);
        assert_eq!(cmds(&parse_mood("off").unwrap()), ["changeTo/778"]);
        assert_eq!(cmds(&parse_mood("778").unwrap()), ["changeTo/778"]);
        assert_eq!(cmds(&parse_mood("777").unwrap()), ["changeTo/777"]);
        assert_eq!(parse_mood("777").unwrap().main_command(), "changeTo/777");
        assert!(parse_mood("bright").is_err());
    }

    #[test]
    fn simple_mappings() {
        assert_eq!(cmds(&Action::On), ["on"]);
        assert_eq!(cmds(&Action::Off), ["off"]);
        assert_eq!(cmds(&Action::Pulse), ["pulse"]);
        assert_eq!(cmds(&parse_dim(40.0).unwrap()), ["40"]);
        assert!(parse_dim(-1.0).is_err());
        let dim = parse_dim(40.0).unwrap();
        assert!(dim.check_type("Spots", "Dimmer").is_ok());
        let err = dim.check_type("Büro", "LightControllerV2").unwrap_err();
        assert!(err.to_string().contains("mood"), "{err}");
        assert_eq!(cmds(&parse_gate("open").unwrap()), ["open"]);
        assert_eq!(cmds(&parse_door("lock").unwrap()), ["on"]);
        assert_eq!(cmds(&parse_door("unlock").unwrap()), ["off"]);
        assert_eq!(cmds(&parse_intercom("decline").unwrap()), ["hangup"]);
        assert_eq!(
            cmds(&parse_charger("start", Some(10.0)).unwrap()),
            ["start/10.0"]
        );
        assert_eq!(cmds(&parse_charger("pause", None).unwrap()), ["pause"]);
    }

    #[test]
    fn color_hex_to_hsv() {
        assert_eq!(cmds(&parse_color("#FF0000").unwrap()), ["hsv(0,100,100)"]);
        assert_eq!(
            cmds(&parse_color("temp(50,3000)").unwrap()),
            ["temp(50,3000)"]
        );
        assert!(parse_color("#FFF").is_err());
        assert!(parse_color("#GG0000").is_err());
    }

    #[test]
    fn thermostat_mapping() {
        assert_eq!(
            cmds(&parse_thermostat("temp", Some("21.5"), None).unwrap()),
            ["setComfortTemperature/21.5"]
        );
        assert_eq!(
            cmds(&parse_thermostat("mode", Some("manual"), None).unwrap()),
            ["setOperatingMode/3"]
        );
        assert_eq!(
            cmds(&parse_thermostat("mode", Some("auto"), None).unwrap()),
            ["setOperatingMode/0"]
        );
        assert_eq!(
            cmds(&parse_thermostat("mode", Some("5"), None).unwrap()),
            ["setOperatingMode/5"]
        );
        // comfort/eco are temperature modes; they used to send operating modes 2/3
        assert!(parse_thermostat("mode", Some("eco"), None).is_err());
        assert!(parse_thermostat("mode", Some("comfort"), None).is_err());
        assert!(parse_thermostat("mode", Some("9"), None).is_err());
        assert_eq!(thermo_mode_name("3"), "manual");
        assert_eq!(thermo_mode_name("7"), "7");
        assert_eq!(
            cmds(&parse_thermostat("override", Some("23"), None).unwrap()),
            ["override/23/60"]
        );
        assert_eq!(
            cmds(&parse_thermostat("override", Some("23"), Some(30)).unwrap()),
            ["override/23/30"]
        );
        assert!(parse_thermostat("temp", Some("warm"), None).is_err());
    }

    #[test]
    fn alarm_mapping_and_pin() {
        assert_eq!(
            cmds(&parse_alarm("arm", false, None).unwrap()),
            ["delayedon/1"]
        );
        assert_eq!(
            cmds(&parse_alarm("arm", true, None).unwrap()),
            ["delayedon/0"]
        );
        assert_eq!(
            cmds(&parse_alarm("arm-home", false, None).unwrap()),
            ["delayedon/0"]
        );
        assert_eq!(
            cmds(&parse_alarm("disarm", false, Some("1234".into())).unwrap()),
            ["off/1234"]
        );
        assert_eq!(
            cmds(&parse_alarm("quit", false, Some("1234".into())).unwrap()),
            ["quit"]
        );
        // PINs never leak into the CLI rendering
        let a = parse_alarm("disarm", false, Some("1234".into())).unwrap();
        assert!(!a.to_cli("House", None).contains("1234"));
    }

    #[test]
    fn risk_levels() {
        assert_eq!(Action::On.risk(), Risk::None);
        assert_eq!(parse_blind("down", None).unwrap().risk(), Risk::None);
        assert_eq!(parse_door("unlock").unwrap().risk(), Risk::Confirm);
        assert_eq!(parse_gate("open").unwrap().risk(), Risk::Confirm);
        assert_eq!(parse_gate("close").unwrap().risk(), Risk::Confirm);
        assert_eq!(parse_gate("stop").unwrap().risk(), Risk::None);
        assert_eq!(
            parse_alarm("arm", false, None).unwrap().risk(),
            Risk::Confirm
        );
        assert_eq!(parse_alarm("quit", false, None).unwrap().risk(), Risk::None);
        assert_eq!(parse_charger("start", None).unwrap().risk(), Risk::Confirm);
    }

    #[test]
    fn cli_rendering() {
        assert_eq!(
            parse_blind("pos", Some(30.0))
                .unwrap()
                .to_cli("Blind South", Some("Living room")),
            "lox blind \"Blind South\" pos 30 -r \"Living room\""
        );
        assert_eq!(Action::On.to_cli("Terrace", None), "lox on Terrace");
        assert_eq!(
            parse_mood("777").unwrap().to_cli("Ceiling", None),
            "lox light mood Ceiling 777"
        );
        assert_eq!(shell_quote("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn type_checks() {
        let a = parse_blind("up", None).unwrap();
        assert!(a.check_type("B", "Jalousie").is_ok());
        let err = a.check_type("B", "Switch").unwrap_err().to_string();
        assert!(err.contains("not a Jalousie"), "{err}");
        assert!(
            parse_door("open")
                .unwrap()
                .check_type("D", "DoorLock")
                .is_ok()
        );
        assert!(Action::On.check_type("X", "Anything").is_ok());
    }

    #[test]
    fn parse_mood_list_typical() {
        let json = r#"[
            {"name":"Bright","id":777,"static":false},
            {"name":"Off","id":778,"static":true},
            {"name":"Dimmed","id":1,"static":false}
        ]"#;
        let moods = parse_mood_list(json).unwrap();
        assert_eq!(
            moods.iter().map(|m| m.id).collect::<Vec<_>>(),
            [1, 777, 778]
        );
        assert!(parse_mood_list("[]").unwrap().is_empty());
        assert_eq!(
            parse_mood_list(r#"[{"id":1},{"id":2,"name":"OK"}]"#)
                .unwrap()
                .len(),
            1
        );
        assert!(parse_mood_list("not json").is_err());
    }
}
