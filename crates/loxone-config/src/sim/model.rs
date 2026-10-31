//! Block behavior, one model per simulated type (D68). Every model is
//! written from Loxone's documentation of the block — the KB page and its
//! timing diagram, cited per type — or from a format fact in
//! loxone-format.md; [docs/simulator.md](../../docs/simulator.md) lists
//! them with what each leaves out. A type without a model is never
//! guessed: a test that depends on it is refused.

use super::clock::Clock;
use super::formula::{self, Expr};
use crate::xml::Element;
use std::collections::HashMap;

/// One input as the model sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Input {
    pub v: f64,
    /// Wired, injected, or an unwired `Inv=` constant: a value the block
    /// was given, as opposed to an open input.
    pub connected: bool,
}

impl Input {
    pub fn on(self) -> bool {
        self.v != 0.0
    }
}

/// What a model gets for one program cycle.
pub struct Io<'a> {
    /// Inputs in the order of [`Model::reads`].
    pub ins: &'a [Input],
    /// Simulated seconds since the start, after this cycle's advance.
    pub now: f64,
    pub clock: &'a Clock,
    /// Operating-mode object UUID → active.
    pub modes: &'a HashMap<String, bool>,
    /// Outputs written this cycle.
    pub out: Vec<(&'static str, f64)>,
}

impl Io<'_> {
    fn i(&self, k: usize) -> Input {
        self.ins[k]
    }
    fn set(&mut self, key: &'static str, v: f64) {
        self.out.push((key, v));
    }
    fn set_bool(&mut self, key: &'static str, b: bool) {
        self.set(key, if b { 1.0 } else { 0.0 });
    }
}

/// How several wires into one input combine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FanIn {
    /// Documented AND-linked (loxone.com/enen/kb/and).
    And,
    /// Documented OR-linked (kb/or, kb/exclusive-or).
    Or,
    /// Undocumented: a second wire is refused.
    Single,
}

/// Sim seconds compare with a little slack: `n` ticks of a decimal `dt`
/// do not add up exactly.
const EPS: f64 = 1e-9;

/// Remembers the previous value of a digital input for edge detection.
#[derive(Debug, Clone, Copy, Default)]
pub struct Edge(bool);

impl Edge {
    /// `(rising, falling)` for the new value.
    fn update(&mut self, now: bool) -> (bool, bool) {
        let was = std::mem::replace(&mut self.0, now);
        (now && !was, !now && was)
    }
}

/// A pulse output that stays on until a deadline.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pulse(Option<f64>);

impl Pulse {
    fn start(&mut self, now: f64, len: f64) {
        self.0 = Some(now + len);
    }
    /// On at `now`? Clears itself once the deadline has passed.
    fn on(&mut self, now: f64) -> bool {
        match self.0 {
            Some(end) if now + EPS < end => true,
            Some(_) => {
                self.0 = None;
                false
            }
            None => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone)]
pub struct Schedule {
    /// Operating-mode UUIDs, the columns (`Modes=`).
    modes: Vec<String>,
    entries: Vec<Entry>,
    analog: bool,
    default: f64,
}

#[derive(Debug, Clone)]
struct Entry {
    column: usize,
    from: f64,
    to: f64,
    value: f64,
    need_activate: bool,
}

#[derive(Debug, Clone)]
pub enum Model {
    /// `InputRef`: `AQ` mirrors `AI`; `Q` mirrors `I`, or `AI` when `I`
    /// is open (refs of block outputs wire `AI` only — loxone-format.md,
    /// *Refs* — and the house's alarm chain triggers a Monoflop from such
    /// a ref's `Q`).
    InputRef,
    /// `OutputRef`/`OutputRefLM`: `AQ` mirrors `AI`.
    OutputRef,
    /// Memory flag (kb/memory-flags): the output is the input; with the
    /// input open it holds what was injected or set.
    Memory {
        held: f64,
    },
    And,
    Or,
    Xor,
    Not,
    Cmp(Cmp),
    Arith(Arith),
    Add4,
    Formula(Box<Expr>),
    Monoflop {
        pulse: Pulse,
        tr: Edge,
    },
    OnDelay {
        since: Option<f64>,
    },
    OffDelay {
        q: bool,
        tr: Edge,
        until: Option<f64>,
    },
    OnOffDelay {
        q: bool,
        since: Option<f64>,
    },
    /// `PushButton` is Loxone's *Switch* (Tg/On/Off/DisPc, pulse
    /// outputs On/Off — kb/switch).
    Switch {
        q: bool,
        tg: Edge,
        on_pulse: bool,
        off_pulse: bool,
    },
    /// `PButtonT` is Loxone's *Push-button* (kb/push-button).
    PushButton {
        q: bool,
        pulse: Pulse,
        tr: Edge,
    },
    /// `Switch2Button` is Loxone's *Push switch* (kb/push-switch).
    PushSwitch {
        latched: bool,
        pulse: Pulse,
        qon: Pulse,
        qoff: Pulse,
        tr: Edge,
        on: Edge,
    },
    /// `PulseAt` with a fixed time of day (`Typ="0"`, kb/pulse-at).
    PulseAt {
        /// Seconds since midnight (`Sec=`, loxone-format.md).
        sec: f64,
        last_now: f64,
        /// The running pulse's end, in clock seconds.
        end: Option<f64>,
    },
    /// `MeterDig` is Loxone's *Fixed Value Meter* (kb/fixed-value-meter).
    FixedMeter {
        mr: f64,
        last: Option<(f64, f64)>,
    },
    /// `MeterAbsUni` is Loxone's *Meter* (kb/meter).
    Meter {
        mr: f64,
        last: Option<(f64, f64)>,
        reading: Reading,
    },
    /// `MeterAbsBi` is Loxone's *Meter Bidirectional*
    /// (kb/meter-bidirectional).
    MeterBi {
        consumption: Reading,
        delivery: Reading,
    },
    /// `AutoJalousie` is Loxone's *Automatic Shading*
    /// (kb/automatic-blinds): complete open/close, safety and lock.
    Blind(Box<Blind>),
    /// `LightController2` is Loxone's *Lighting Controller*
    /// (kb/lighting-controller): its motion mood from all off, Off,
    /// DisPc, M+/M−, T5/1–8, Mood, On, Buzzer, Alarm and output M.
    Lights(Box<Lights>),
    Threshold {
        q: bool,
        rise: Pulse,
        fall: Pulse,
    },
    SrFlipFlop {
        q: bool,
        tg: Edge,
    },
    EdgeDetection {
        last: Edge,
        any: Pulse,
        rise: Pulse,
        fall: Pulse,
    },
    /// An operating mode (`Mode`, built-in number from its fixed UUID).
    /// Monday (4) … Sunday (10) follow the simulated clock; every other
    /// mode is off unless a test sets it.
    Mode {
        number: Option<u8>,
    },
    DayTimer {
        sched: Box<Schedule>,
        act: Edge,
        q: f64,
        activated: Option<(usize, f64)>,
    },
}

/// A modelled block, or why a type is not simulated.
pub enum Support {
    Model(Model),
    /// Periphery and virtual inputs: their outputs come from outside the
    /// program, so they read 0 unless a test sets them.
    Source,
    Unsupported(String),
}

/// Types whose outputs come from outside the program.
const SOURCES: &[&str] = &[
    "AnalogInput",
    "DigitalIn",
    "EIBextsensor",
    "EIBsensor",
    "EIBtextsensor",
    "Lox1wireAsensor",
    "Lox1wiresensor",
    "Lox485sensor",
    "LoxAIRAsensor",
    "LoxAIRsensor",
    "LoxIRsensor",
    "ModbusASensor",
    "ModbusSensor",
    // A device's thermal-shutdown status (`IName="TSD"`, a child of the
    // hardware device): hardware behaviour, not program logic.
    "OvertempShutdown",
    "TreeAsensor",
    "TreeSensor",
    "VirtualHttpIn",
    "VirtualHttpInCmd",
    "VirtualIn",
    "VirtualTextIn",
    "VirtualUdpIn",
    "VirtualUdpInCmd",
    "VoltageIn",
];

impl Model {
    /// The model for an object, from its type and element.
    pub fn for_object(block_type: &str, el: &Element, wired: &dyn Fn(&str) -> bool) -> Support {
        use Support::Model as M;
        if SOURCES.contains(&block_type) {
            return Support::Source;
        }
        let model = match block_type {
            "InputRef" => Model::InputRef,
            "OutputRef" | "OutputRefLM" => Model::OutputRef,
            "Memory" => Model::Memory { held: 0.0 },
            "And" => Model::And,
            "Or" => Model::Or,
            "Xor" => Model::Xor,
            "Not" => Model::Not,
            "Equal" => Model::Cmp(Cmp::Eq),
            "NotEqual" => Model::Cmp(Cmp::Ne),
            "Greater" => Model::Cmp(Cmp::Gt),
            "GreaterEqual" => Model::Cmp(Cmp::Ge),
            "Less" => Model::Cmp(Cmp::Lt),
            "LessEqual" => Model::Cmp(Cmp::Le),
            "Add" => Model::Arith(Arith::Add),
            "Sub" => Model::Arith(Arith::Sub),
            "Mult" => Model::Arith(Arith::Mul),
            "Div" => Model::Arith(Arith::Div),
            "Add4" => Model::Add4,
            "Formula" => {
                let src = el.attr_decoded("Formula").unwrap_or_default();
                match formula::parse(&src) {
                    Ok(e) => Model::Formula(Box::new(e)),
                    Err(e) => return Support::Unsupported(format!("its formula: {e}")),
                }
            }
            "Monoflop" => Model::Monoflop {
                pulse: Pulse::default(),
                tr: Edge::default(),
            },
            "OnDelay" => Model::OnDelay { since: None },
            "OffDelay" => Model::OffDelay {
                q: false,
                tr: Edge::default(),
                until: None,
            },
            "OnOffDelay" => Model::OnOffDelay {
                q: false,
                since: None,
            },
            "PushButton" => Model::Switch {
                q: false,
                tg: Edge::default(),
                on_pulse: false,
                off_pulse: false,
            },
            "PButtonT" => Model::PushButton {
                q: false,
                pulse: Pulse::default(),
                tr: Edge::default(),
            },
            "Switch2Button" => Model::PushSwitch {
                latched: false,
                pulse: Pulse::default(),
                qon: Pulse::default(),
                qoff: Pulse::default(),
                tr: Edge::default(),
                on: Edge::default(),
            },
            "PulseAt" => match pulse_at_time(el) {
                Ok(sec) => Model::PulseAt {
                    sec,
                    last_now: 0.0,
                    end: None,
                },
                Err(e) => return Support::Unsupported(e),
            },
            "MeterDig" => Model::FixedMeter {
                mr: 0.0,
                last: None,
            },
            "MeterAbsUni" => Model::Meter {
                mr: 0.0,
                last: None,
                reading: Reading::default(),
            },
            "MeterAbsBi" => Model::MeterBi {
                consumption: Reading::default(),
                delivery: Reading::default(),
            },
            "AutoJalousie" => Model::Blind(Box::default()),
            "LightController2" => match Lights::new(el) {
                Ok(l) => Model::Lights(Box::new(l)),
                Err(e) => return Support::Unsupported(e),
            },
            "AnalogThresholdTrigger" => Model::Threshold {
                q: false,
                rise: Pulse::default(),
                fall: Pulse::default(),
            },
            "SRFlipFlop" => Model::SrFlipFlop {
                q: false,
                tg: Edge::default(),
            },
            "EdgeDetection" => Model::EdgeDetection {
                last: Edge::default(),
                any: Pulse::default(),
                rise: Pulse::default(),
                fall: Pulse::default(),
            },
            "Mode" => {
                if wired("I") {
                    return Support::Unsupported(
                        "an operating mode switched by logic (its `I` is wired) — \
                         how that input acts is undocumented"
                            .into(),
                    );
                }
                Model::Mode {
                    number: mode_number(el.attr("U").unwrap_or_default()),
                }
            }
            "DayTimer" => match schedule(el) {
                Ok(sched) => Model::DayTimer {
                    sched: Box::new(sched),
                    act: Edge::default(),
                    q: 0.0,
                    activated: None,
                },
                Err(e) => return Support::Unsupported(e),
            },
            other => {
                return Support::Unsupported(format!("no model for `{other}` blocks"));
            }
        };
        M(model)
    }

    /// The input and parameter ports the model reads, in [`Io::ins`]
    /// order.
    pub fn reads(&self) -> &'static [&'static str] {
        match self {
            Model::InputRef => &["AI", "I"],
            Model::OutputRef => &["AI"],
            Model::Memory { .. } => &["Input"],
            Model::And | Model::Or | Model::Xor => &["I1", "I2"],
            Model::Not => &["I"],
            Model::Cmp(_) | Model::Arith(_) => &["Input1", "Input2"],
            Model::Add4 | Model::Formula(_) => &["Input1", "Input2", "Input3", "Input4"],
            Model::Monoflop { .. } | Model::OnDelay { .. } | Model::OffDelay { .. } => {
                &["InputTrigger", "Reset", "Time"]
            }
            Model::OnOffDelay { .. } => &["InputTrigger", "Reset", "TimeOn", "TimeOff"],
            Model::Switch { .. } => &["InputTrigger", "On", "Reset", "InputDisable"],
            Model::PushButton { .. } => &["InputTrigger", "Reset", "InputDisable", "Time"],
            Model::PushSwitch { .. } => &["InputTrigger", "On", "Reset", "InputDisable", "Time"],
            Model::PulseAt { .. } => &["InputDisable", "Time"],
            Model::FixedMeter { .. } => &["A", "R", "Ppwr", "Mro"],
            Model::Meter { .. } => &["Pf", "Mr", "R", "Mro"],
            Model::MeterBi { .. } => &["Pf", "Mr1", "Mr2", "R", "Mro1", "Mro2"],
            Model::Blind(_) => BLIND_READS,
            Model::Lights(_) => LIGHTS_READS,
            Model::Threshold { .. } => &["Input", "On", "Off", "PulseTime"],
            Model::SrFlipFlop { .. } => &["InputS", "InputR", "InputTrigger"],
            Model::EdgeDetection { .. } => &["Input", "PulseTime"],
            Model::Mode { .. } => &[],
            Model::DayTimer { .. } => &["InputTrigger", "Reset", "Manual", "PulseTime"],
        }
    }

    /// The outputs the model computes. Any other output of the block is
    /// not simulated.
    pub fn writes(&self) -> &'static [&'static str] {
        match self {
            Model::InputRef => &["AQ", "Q"],
            Model::OutputRef => &["AQ"],
            Model::Memory { .. } => &["AQ", "Q"],
            Model::And
            | Model::Or
            | Model::Xor
            | Model::Not
            | Model::Cmp(_)
            | Model::Monoflop { .. }
            | Model::OnDelay { .. }
            | Model::OffDelay { .. }
            | Model::OnOffDelay { .. }
            | Model::PushButton { .. }
            | Model::SrFlipFlop { .. }
            | Model::PulseAt { .. }
            | Model::Mode { .. } => &["Q"],
            Model::Arith(_) | Model::Add4 | Model::Formula(_) => &["AQ"],
            Model::Switch { .. } | Model::PushSwitch { .. } => &["Q", "Qon", "Qoff"],
            Model::FixedMeter { .. } | Model::Meter { .. } => &["OPf", "OMr"],
            Model::MeterBi { .. } => &["OPf", "OMr1", "OMr2"],
            Model::Blind(_) => &[
                "OutputUp",
                "OutputDown",
                "OutputPos",
                "OutputSafety",
                "OutputLock",
            ],
            Model::Lights(_) => LIGHTS_WRITES,
            Model::Threshold { .. } => &["Q", "RisingEdge", "FallingEdge"],
            Model::EdgeDetection { .. } => &["Edge", "RisingEdge", "FallingEdge"],
            Model::DayTimer { .. } => &["AQ", "Qon", "Qoff"],
        }
    }

    /// How several wires into one of this block's inputs combine.
    pub fn fan_in(&self) -> FanIn {
        match self {
            Model::And => FanIn::And,
            Model::Or | Model::Xor => FanIn::Or,
            _ => FanIn::Single,
        }
    }

    /// Operating modes the block reads besides its wires.
    pub fn mode_deps(&self) -> &[String] {
        match self {
            Model::DayTimer { sched, .. } => &sched.modes,
            _ => &[],
        }
    }

    /// One program cycle.
    pub fn step(&mut self, io: &mut Io) -> Result<(), String> {
        let now = io.now;
        match self {
            Model::InputRef => {
                let (ai, i) = (io.i(0), io.i(1));
                io.set("AQ", ai.v);
                let q = if i.connected { i.on() } else { ai.on() };
                io.set_bool("Q", q);
            }
            Model::OutputRef => {
                let ai = io.i(0).v;
                io.set("AQ", ai);
            }
            Model::Memory { held } => {
                let input = io.i(0);
                if input.connected {
                    *held = input.v;
                }
                let v = *held;
                io.set("AQ", v);
                io.set_bool("Q", v != 0.0);
            }
            // "The output will be active when all connected inputs are
            // active" (kb/and): open inputs do not count.
            Model::And => {
                let connected: Vec<Input> =
                    io.ins.iter().copied().filter(|i| i.connected).collect();
                let q = !connected.is_empty() && connected.iter().all(|i| i.on());
                io.set_bool("Q", q);
            }
            Model::Or => {
                let q = io.ins.iter().any(|i| i.on());
                io.set_bool("Q", q);
            }
            // "Active when the connected inputs have unequal values"
            // (kb/exclusive-or).
            Model::Xor => {
                let q = io.i(0).on() != io.i(1).on();
                io.set_bool("Q", q);
            }
            Model::Not => {
                let q = !io.i(0).on();
                io.set_bool("Q", q);
            }
            Model::Cmp(op) => {
                let (a, b) = (io.i(0).v, io.i(1).v);
                let q = match op {
                    Cmp::Eq => a == b,
                    Cmp::Ne => a != b,
                    Cmp::Gt => a > b,
                    Cmp::Ge => a >= b,
                    Cmp::Lt => a < b,
                    Cmp::Le => a <= b,
                };
                io.set_bool("Q", q);
            }
            Model::Arith(op) => {
                let (a, b) = (io.i(0).v, io.i(1).v);
                let v = match op {
                    Arith::Add => a + b,
                    Arith::Sub => a - b,
                    Arith::Mul => a * b,
                    Arith::Div => a / b,
                };
                if !v.is_finite() {
                    return Err(format!("{a} / {b} has no value"));
                }
                io.set("AQ", v);
            }
            Model::Add4 => {
                let v = io.ins.iter().map(|i| i.v).sum();
                io.set("AQ", v);
            }
            Model::Formula(expr) => {
                let ins = [io.i(0).v, io.i(1).v, io.i(2).v, io.i(3).v];
                match formula::eval(expr, &ins) {
                    Some(v) => io.set("AQ", v),
                    None => {
                        return Err(format!(
                            "the formula has no value for I1..I4 = {ins:?} (Loxone \
                             reports that at the block's error output)"
                        ));
                    }
                }
            }
            // A rising edge at Tr starts a pulse of D seconds, a new edge
            // restarts it, its length does not depend on the input's; Off
            // ends it and, held, locks the block (kb/monoflop, timing
            // diagram).
            Model::Monoflop { pulse, tr } => {
                let (rising, _) = tr.update(io.i(0).on());
                if io.i(1).on() {
                    pulse.0 = None;
                } else if rising {
                    pulse.start(now, io.i(2).v);
                }
                let q = pulse.on(now);
                io.set_bool("Q", q);
            }
            // On once Tr has been on for Don without a break; off with Tr.
            // Off resets and, held, locks; the delay restarts when Off
            // drops while Tr is still on (kb/switch-on-delay, timing
            // diagram).
            Model::OnDelay { since } => {
                let q = if io.i(1).on() || !io.i(0).on() {
                    *since = None;
                    false
                } else {
                    let start = *since.get_or_insert(now);
                    now - start + EPS >= io.i(2).v
                };
                io.set_bool("Q", q);
            }
            // On with a rising edge at Tr, off Don seconds after the
            // falling edge; Off switches off until the next rising edge
            // (kb/switch-off-delay, timing diagram).
            Model::OffDelay { q, tr, until } => {
                let (rising, falling) = tr.update(io.i(0).on());
                if io.i(1).on() {
                    *q = false;
                    *until = None;
                } else {
                    if rising {
                        *q = true;
                        *until = None;
                    }
                    if falling && *q {
                        *until = Some(now + io.i(2).v);
                    }
                    if let Some(end) = *until
                        && now + EPS >= end
                    {
                        *q = false;
                        *until = None;
                    }
                }
                io.set_bool("Q", *q);
            }
            // On after Tr has been on for Don, off after it has been off
            // for Doff (kb/switch-on-and-off-delay, timing diagram).
            Model::OnOffDelay { q, since } => {
                if io.i(1).on() {
                    *q = false;
                    *since = None;
                } else {
                    let tr = io.i(0).on();
                    if tr == *q {
                        *since = None;
                    } else {
                        let start = *since.get_or_insert(now);
                        let wait = if tr { io.i(2).v } else { io.i(3).v };
                        if now - start + EPS >= wait {
                            *q = tr;
                            *since = None;
                        }
                    }
                }
                io.set_bool("Q", *q);
            }
            // Tg toggles on its rising edge; On switches on and holds the
            // output on while it is on; Off switches off and, held,
            // locks; DisPc disables all three. The On/Off outputs pulse
            // for one cycle when O changes (kb/switch, timing diagram).
            Model::Switch {
                q,
                tg,
                on_pulse,
                off_pulse,
            } => {
                let (rising, _) = tg.update(io.i(0).on());
                let was = *q;
                if !io.i(3).on() {
                    if io.i(2).on() {
                        *q = false;
                    } else if io.i(1).on() {
                        *q = true;
                    } else if rising {
                        *q = !*q;
                    }
                }
                *on_pulse = *q && !was;
                *off_pulse = !*q && was;
                let (q, on, off) = (*q, *on_pulse, *off_pulse);
                io.set_bool("Q", q);
                io.set_bool("Qon", on);
                io.set_bool("Qoff", off);
            }
            // A rising edge at Tr switches O on for Don, a new edge
            // restarts it; Don 0 keeps O on while Tr is; Off resets and,
            // held, locks; DisPc disables Tr and Off (kb/push-button,
            // timing diagram).
            Model::PushButton { q, pulse, tr } => {
                let (rising, _) = tr.update(io.i(0).on());
                if !io.i(2).on() {
                    let don = io.i(3).v;
                    if io.i(1).on() {
                        pulse.0 = None;
                        *q = false;
                    } else if don <= 0.0 {
                        *q = io.i(0).on();
                    } else {
                        if rising {
                            pulse.start(now, don);
                        }
                        *q = pulse.on(now);
                    }
                } else if pulse.0.is_some() {
                    *q = pulse.on(now);
                }
                io.set_bool("Q", *q);
            }
            // Every edge at Tr pulses O for Don, the rising one Qon,
            // the falling one Qoff; On switches O on and it stays on
            // until Off; Off switches off and, held, locks; DisPc
            // disables Tr, On and Off (kb/push-switch, its table and
            // timing diagram). The diagram also pulses Qon/Qoff when On
            // and Off switch O, which the table does not mention: the
            // model follows the diagram. A Tr edge during a running pulse
            // or while On holds O on is undocumented and fails.
            Model::PushSwitch {
                latched,
                pulse,
                qon,
                qoff,
                tr,
                on,
            } => {
                let don = io.i(4).v;
                if don <= 0.0 {
                    return Err(format!("Don = {don}: an on-duration of 0 is undocumented"));
                }
                let (tr_rise, tr_fall) = tr.update(io.i(0).on());
                let (on_rise, _) = on.update(io.i(1).on());
                if !io.i(3).on() {
                    if io.i(2).on() {
                        if *latched {
                            qoff.start(now, don);
                        }
                        *latched = false;
                        pulse.0 = None;
                    } else {
                        if on_rise && !*latched {
                            *latched = true;
                            qon.start(now, don);
                        }
                        if tr_rise || tr_fall {
                            if *latched {
                                return Err("an edge at Tr while On holds O on is \
                                            undocumented"
                                    .into());
                            }
                            if pulse.on(now) {
                                return Err(format!(
                                    "an edge at Tr while O still pulses (Don = {don} s) \
                                     is undocumented"
                                ));
                            }
                            pulse.start(now, don);
                            if tr_rise {
                                qon.start(now, don);
                            } else {
                                qoff.start(now, don);
                            }
                        }
                    }
                }
                let q = *latched || pulse.on(now);
                let (on, off) = (qon.on(now), qoff.on(now));
                io.set_bool("Q", q);
                io.set_bool("Qon", on);
                io.set_bool("Qoff", off);
            }
            // O for Don from the configured time of day, every day; Off
            // ends the pulse and, held, locks (dominating) (kb/pulse-at).
            // The pulse starts when the simulated time passes the time
            // during a tick; a `clock` statement that jumps over it is not
            // time passing and starts none. A pulse shorter than the tick
            // shows for that one cycle, like the other pulse outputs.
            Model::PulseAt { sec, last_now, end } => {
                const TOL: f64 = 1e-6;
                let don = io.i(1).v;
                if don <= 0.0 {
                    return Err(format!("Don = {don}: an on-duration of 0 is undocumented"));
                }
                let to = io.clock.secs;
                let from = to - (now - *last_now);
                *last_now = now;
                let first_day = (from / 86_400.0).floor() as i64;
                let mut started = false;
                for day in first_day..=io.clock.day() {
                    let start = day as f64 * 86_400.0 + *sec;
                    if start > from + TOL && start <= to + TOL {
                        *end = Some(start + don);
                        started = true;
                    }
                }
                if io.i(0).on() {
                    *end = None;
                }
                let q = match *end {
                    Some(e) => {
                        let running = to + TOL < e;
                        if !running {
                            *end = None;
                        }
                        running || started
                    }
                    None => false,
                };
                io.set_bool("Q", q);
            }
            // While S is 1, Pf is the power rating and the meter runs; the
            // reading comes from the switch-on duration (kb/fixed-value-meter).
            Model::FixedMeter { mr, last } => {
                meter_reset(io.i(1))?;
                let s = io.i(0).v;
                if s != 0.0 && s != 1.0 {
                    return Err(format!("S = {s}: only 0 and 1 are documented"));
                }
                let pf = if s == 1.0 { io.i(2).v } else { 0.0 };
                integrate(mr, last, now, pf);
                let mro = io.i(3).v;
                io.set("OPf", pf);
                io.set("OMr", *mr + mro);
            }
            // Pf alone: the block is the meter and integrates Pf over
            // hours; with Mr it maps the reading and passes Pf through
            // (kb/meter, *Meter reading and units*).
            Model::Meter { mr, last, reading } => {
                meter_reset(io.i(2))?;
                let (pf, mr_in, mro) = (io.i(0), io.i(1), io.i(3).v);
                let reading_v = if mr_in.connected {
                    meter_power_needs_pf(pf)?;
                    reading.update(mr_in.v)?
                } else {
                    if pf.v < 0.0 {
                        return Err(format!(
                            "Pf = {}: a negative power at a unidirectional meter is \
                             undocumented (its range is 0…∞)",
                            pf.v
                        ));
                    }
                    integrate(mr, last, now, pf.v);
                    *mr
                };
                io.set("OPf", pf.v);
                io.set("OMr", reading_v + mro);
            }
            // Mrc/Mrd map the consumption and delivery readings, Pf passes
            // through (kb/meter-bidirectional). How the block splits a Pf
            // alone into the two readings is not on the block's page.
            Model::MeterBi {
                consumption,
                delivery,
            } => {
                meter_reset(io.i(3))?;
                let (pf, mr1, mr2) = (io.i(0), io.i(1), io.i(2));
                if !mr1.connected && !mr2.connected && pf.connected {
                    return Err("Pf alone: how the block splits it into consumption and \
                                delivery readings is undocumented"
                        .into());
                }
                meter_power_needs_pf(pf)?;
                let c = if mr1.connected {
                    consumption.update(mr1.v)?
                } else {
                    0.0
                };
                let d = if mr2.connected {
                    delivery.update(mr2.v)?
                } else {
                    0.0
                };
                let (mro1, mro2) = (io.i(4).v, io.i(5).v);
                io.set("OPf", pf.v);
                io.set("OMr1", c + mro1);
                io.set("OMr2", d + mro2);
            }
            Model::Blind(b) => b.step(io)?,
            Model::Lights(l) => l.step(io)?,
            // O switches on above Von and off below Voff; with Von below
            // Voff the other way round; with both equal O is on only at
            // exactly that value. On/Off pulse for Pd at the edges
            // (kb/threshold-switch).
            Model::Threshold { q, rise, fall } => {
                let (v, von, voff, pd) = (io.i(0).v, io.i(1).v, io.i(2).v, io.i(3).v);
                let was = *q;
                if von > voff {
                    if v > von {
                        *q = true;
                    } else if v < voff {
                        *q = false;
                    }
                } else if von < voff {
                    if v > voff {
                        *q = false;
                    } else if v < von {
                        *q = true;
                    }
                } else {
                    *q = v == von;
                }
                if *q && !was {
                    rise.start(now, pd);
                }
                if !*q && was {
                    fall.start(now, pd);
                }
                let (q, r, f) = (*q, rise.on(now), fall.on(now));
                io.set_bool("Q", q);
                io.set_bool("RisingEdge", r);
                io.set_bool("FallingEdge", f);
            }
            // S sets and dominates, R resets, a rising edge at Tg toggles
            // (kb/flipflop-sr, timing diagram).
            Model::SrFlipFlop { q, tg } => {
                let (rising, _) = tg.update(io.i(2).on());
                if io.i(0).on() {
                    *q = true;
                } else if io.i(1).on() {
                    *q = false;
                } else if rising {
                    *q = !*q;
                }
                io.set_bool("Q", *q);
            }
            // A pulse of Pd at every edge (P), at rising (On) and falling
            // (Off) edges; a new edge restarts the pulse (kb/edge-detection,
            // timing diagram).
            Model::EdgeDetection {
                last,
                any,
                rise,
                fall,
            } => {
                let pd = io.i(1).v;
                let (rising, falling) = last.update(io.i(0).on());
                if rising || falling {
                    any.start(now, pd);
                }
                if rising {
                    rise.start(now, pd);
                }
                if falling {
                    fall.start(now, pd);
                }
                let (p, r, f) = (any.on(now), rise.on(now), fall.on(now));
                io.set_bool("Edge", p);
                io.set_bool("RisingEdge", r);
                io.set_bool("FallingEdge", f);
            }
            Model::Mode { number } => {
                let on = match number {
                    Some(n @ 4..=10) => io.clock.weekday() == *n - 3,
                    _ => false,
                };
                io.set_bool("Q", on);
            }
            Model::DayTimer {
                sched,
                act,
                q,
                activated,
            } => {
                let v = day_timer(sched, act, activated, io)?;
                let was = *q;
                *q = v;
                io.set("AQ", v);
                io.set_bool("Qon", v != 0.0 && was == 0.0);
                io.set_bool("Qoff", v == 0.0 && was != 0.0);
            }
        }
        Ok(())
    }
}

/// Inputs of an `AutoJalousie` in [`Model::reads`] order; the first
/// [`BLIND_REFUSED`] are inputs without a model, the last two of those
/// taking a value.
const BLIND_READS: &[&str] = &[
    "InputTrigger",
    "InputTriggerUp",
    "InputTriggerDown",
    "AutoShade",
    "ReactAutoShade",
    "Gesture",
    "ManualPosition",
    "ManualLamelle",
    "EndUp",
    "EndDown",
    "Shade",
    "Safety",
    "Window",
    "Stop",
    "InputDisable",
    "TimeEnd",
    "TimeEndDown",
    "SO",
    "Rdd",
    "Type",
    "Back",
];
const BLIND_REFUSED: usize = 8;

/// Which way a blind is driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
}

/// A running drive: its direction and when the output ends, in sim
/// seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drive {
    dir: Dir,
    until: f64,
}

/// An `AutoJalousie`'s state. The position is the block's estimate from
/// run time, 0 open … 1 closed; a fresh simulation starts open.
#[derive(Debug, Clone, Default)]
pub struct Blind {
    pos: f64,
    drive: Option<Drive>,
    last_now: f64,
    co: Edge,
    cc: Edge,
    so: Edge,
    /// Wa or Dwc as last acted on: `(wind, window)`.
    safety: (bool, bool),
    stop_held: bool,
}

impl Blind {
    /// Complete open runs Op for the whole Opd "regardless of the current
    /// position" (a reference run); complete close runs Cl only for "the
    /// determined remaining run time", or the whole Cld with Rdd
    /// (kb/automatic-blinds, *Complete movement, end positions and
    /// remanence*; parameter Rdd).
    fn complete(&self, dir: Dir, now: f64, opd: f64, cld: f64, rdd: bool) -> Drive {
        let len = match dir {
            Dir::Up => opd,
            Dir::Down if rdd => cld,
            Dir::Down => (1.0 - self.pos) * cld,
        };
        Drive {
            dir,
            until: now + len,
        }
    }

    fn start(&mut self, d: Drive) -> Result<(), String> {
        if let Some(run) = self.drive
            && run.dir != d.dir
        {
            return Err("a move reverses a running one: the motor lock between \
                        direction changes (Mld) is not simulated"
                .into());
        }
        self.drive = Some(d);
        Ok(())
    }

    fn step(&mut self, io: &mut Io) -> Result<(), String> {
        let now = io.now;
        let v = |k: usize| io.i(k);
        for (k, key) in BLIND_READS[..BLIND_REFUSED].iter().enumerate() {
            let input = v(k);
            // Pos and Slat take a value, the others a pulse.
            if input.on() || (k >= 6 && input.connected) {
                return Err(not_simulated("AutoJalousie", key));
            }
        }
        let (opd, cld) = (v(15).v, v(16).v);
        let (wap, rdd, kind, rd) = (v(17).v, v(18).on(), v(19).v, v(20).v);
        if opd <= 0.0 || cld <= 0.0 {
            return Err("an opening or closing duration of 0 is undocumented".into());
        }
        // The position follows the drive since the last cycle, linearly
        // over Opd and Cld — the reading of "remaining run time".
        if let Some(d) = self.drive {
            let ran = d.until.min(now) - self.last_now;
            if ran > 0.0 {
                self.pos = match d.dir {
                    Dir::Up => (self.pos - ran / opd).max(0.0),
                    Dir::Down => (self.pos + ran / cld).min(1.0),
                };
            }
            if now + EPS >= d.until {
                self.drive = None;
            }
        }
        self.last_now = now;

        let disabled = v(14).on();
        let (co, _) = self.co.update(v(8).on());
        let (cc, _) = self.cc.update(v(9).on());
        let (so, _) = self.so.update(v(10).on());
        let (wind, window, stop) = (v(11).on(), v(12).on(), v(13).on());

        // Off dominates and "overrules the safetyActive, as not even
        // moving to the safety position is allowed" (Structure File,
        // Jalousie `locked`): it stops, and locks while held.
        if stop {
            self.drive = None;
            self.stop_held = true;
        } else {
            if self.stop_held && (wind || window) {
                return Err("Off ends while Wa or Dwc is on: what the block does \
                            then is undocumented"
                    .into());
            }
            self.stop_held = false;
            if (wind, window) != self.safety {
                let (was_wind, was_window) = self.safety;
                if was_window && !window {
                    return Err("Dwc ends: what the block does when the window \
                                closes is undocumented"
                        .into());
                }
                if wind && window && wap != 0.0 {
                    return Err("Wa (to close, Wap = 1) and Dwc (to open) at once \
                                is undocumented"
                        .into());
                }
                // Wa moves to the wind alarm position Wap (0 complete open,
                // 1 complete close), Dwc opens completely; both lock. At the
                // end of Wa "automatic conditions are not re-evaluated": the
                // lock lifts and nothing moves.
                if (window && !was_window) || (wind && !was_wind && wap == 0.0) {
                    self.start(self.complete(Dir::Up, now, opd, cld, rdd))?;
                } else if wind && !was_wind {
                    if wap != 1.0 {
                        return Err(format!("Wap = {wap}: only 0 and 1 are documented"));
                    }
                    self.start(self.complete(Dir::Down, now, opd, cld, rdd))?;
                }
                self.safety = (wind, window);
            }
            if !wind && !window && !disabled {
                // Co and Cc: "If in motion, stops."
                if (co || cc) && self.drive.is_some() {
                    self.drive = None;
                } else if co {
                    self.start(self.complete(Dir::Up, now, opd, cld, rdd))?;
                } else if cc {
                    self.start(self.complete(Dir::Down, now, opd, cld, rdd))?;
                } else if so {
                    self.slightly_open(now, kind, rd, opd, cld)?;
                }
            }
        }
        let dir = self.drive.map(|d| d.dir);
        let pos = self.pos;
        io.set_bool("OutputUp", dir == Some(Dir::Up));
        io.set_bool("OutputDown", dir == Some(Dir::Down));
        io.set("OutputPos", pos);
        io.set_bool("OutputSafety", wind || window);
        io.set_bool("OutputLock", stop);
        Ok(())
    }

    /// So: roller blinds, curtains and awnings "move into position
    /// according to parameter (Rd)", open 0.1 … closed 1.0
    /// (kb/automatic-blinds). Venetian blinds tilt their slats, which is
    /// not simulated.
    fn slightly_open(
        &mut self,
        now: f64,
        kind: f64,
        rd: f64,
        opd: f64,
        cld: f64,
    ) -> Result<(), String> {
        if !matches!(kind as i64, 1 | 2 | 4 | 5 | 6) || kind.fract() != 0.0 {
            return Err(format!(
                "So on shading type {kind}: venetian blinds tilt their slats, \
                 which is not simulated"
            ));
        }
        if !(0.1..=1.0).contains(&rd) {
            return Err(format!("So with Rd = {rd}: the position must be 0.1 … 1.0"));
        }
        if self.drive.is_some() {
            return Err("So while the blind moves is undocumented".into());
        }
        let d = if rd > self.pos {
            Drive {
                dir: Dir::Down,
                until: now + (rd - self.pos) * cld,
            }
        } else {
            Drive {
                dir: Dir::Up,
                until: now + (self.pos - rd) * opd,
            }
        };
        if d.until > now {
            self.start(d)?;
        }
        Ok(())
    }
}

/// Inputs of a `LightController2` in [`Model::reads`] order: the
/// [`LIGHTS_MODELLED`] ones, then those without a model.
const LIGHTS_READS: &[&str] = &[
    "Move",
    "Presence",
    "Brightness",
    "Reset",
    "InputDisable",
    "BrightnessLimit",
    "MoveOn",
    "MoveTimeout",
    "MoveIgnore",
    "MoveScene",
    // The pulse inputs of `LIGHTS_KEYS`, in its order.
    "InputTriggerUp",
    "InputTriggerDown",
    "Sel1",
    "Sel2",
    "Sel3",
    "Sel4",
    "Sel5",
    "Sel6",
    "Sel7",
    "Sel8",
    "On",
    "AlarmClock",
    "MaxP",
    "SceneMixTime",
    "Select",
    "Alarm",
    "AlarmPeriod",
    "AlarmBrightness",
    "I1",
    "I2",
    "I3",
    "I4",
    "I5",
    "I6",
    "I7",
    "I8",
    "EnMove",
    "RtD",
    "MasterBr",
];
const LIGHTS_MODELLED: usize = 28;
/// Where the pulse inputs start in [`LIGHTS_READS`]: M+, M−, T5/1 … T5/8,
/// On, Buzzer.
const LIGHTS_KEYS: usize = 10;
const KEY_UP: usize = 0;
const KEY_DOWN: usize = 1;
const KEY_ON: usize = 10;
const KEY_BUZZER: usize = 11;
const LIGHTS_WRITES: &[&str] = &[
    "AQ1",
    "AQ2",
    "AQ3",
    "AQ4",
    "AQ5",
    "AQ6",
    "AQ7",
    "AQ8",
    "AQ9",
    "AQ10",
    "AQ11",
    "AQ12",
    "AQ13",
    "AQ14",
    "AQ15",
    "AQ16",
    "AQ17",
    "AQ18",
    "OutputReset",
    "Scene",
];

/// A pulse input of a `LightController2`: its edge and when it last rose.
#[derive(Debug, Clone, Copy, Default)]
struct Key {
    edge: Edge,
    rose: Option<f64>,
}

/// A `LightController2`'s state: which of its moods shows, and who chose
/// it.
///
/// The manual inputs follow Loxone Config 17.2.8.28's simulation (oracle
/// sessions 35–36, not a Miniserver): M+/M− step through the mood list in
/// `<LightscenesC>` order, the off mood included, cyclically; T5/n selects
/// the mood `SID="n"`, and steps on like M+ while it made the last mood
/// change or that mood shows; both act when the input falls. On selects
/// `SID="777"` when it rises; Mood selects by ID when its value changes.
#[derive(Debug, Clone, Default)]
pub struct Lights {
    moods: Vec<crate::moods::Mood>,
    /// The index of the motion mood, and of the built-in off mood.
    motion: usize,
    off: usize,
    /// The mood that shows.
    cur: usize,
    /// The motion automatic switched the current mood on.
    auto: bool,
    /// When a manual input last changed the mood (cleared by Off and the
    /// automatic switch-off).
    manual_at: Option<f64>,
    /// The Buzzer chose the current mood: Met runs from its falling edge.
    buzzer: bool,
    /// The T5 input that made the last mood change, and when.
    chain: Option<(usize, f64)>,
    /// When the lights go off: after motion ended, or after the Buzzer.
    off_at: Option<f64>,
    /// Set by a manual off: the automatic stays disabled until this
    /// time, which motion pushes back by Pto.
    suppressed_until: Option<f64>,
    move_edge: Edge,
    presence_edge: Edge,
    reset_edge: Edge,
    reset_pulse: bool,
    keys: [Key; 12],
    /// The last value seen on Mood: a change selects.
    select: Option<f64>,
    /// When Alarm rose, while it is on.
    alarm_since: Option<f64>,
    alarm_edge: Edge,
    /// Afi and MaxAbr carry a `Def=` (without one Config's simulation
    /// flashed faster than the documented 4 s).
    alarm_set: bool,
}

impl Lights {
    fn new(el: &Element) -> Result<Lights, String> {
        let (moods, motion) = lights_moods(el)?;
        let off = moods
            .iter()
            .position(crate::moods::Mood::is_off)
            .ok_or("it has no off mood (`SID=\"778\"`)")?;
        let has_def = |key: &str| {
            el.child_elements()
                .any(|c| c.name == "Co" && c.attr("K") == Some(key) && c.attr("Def").is_some())
        };
        Ok(Lights {
            moods,
            motion,
            off,
            cur: off,
            alarm_set: has_def("AlarmPeriod") && has_def("AlarmBrightness"),
            ..Lights::default()
        })
    }

    /// The index of the mood `SID="sid"`.
    fn by_sid(&self, sid: &str) -> Option<usize> {
        self.moods
            .iter()
            .position(|m| m.sid.as_deref() == Some(sid))
    }

    /// The mood `by` steps from the current one: M+/M− and T5's "next",
    /// through the list in order and around its ends.
    fn step_mood(&self, by: isize) -> Result<usize, String> {
        let n = self.moods.len() as isize;
        let next = (self.cur as isize + by).rem_euclid(n) as usize;
        if self.moods[next].sid.as_deref() == Some("776") {
            return Err(
                "stepping onto the alarm-clock mood (`SID=\"776\"`) is not simulated: \
                 whether M+/M−/T5 skip it is unverified"
                    .into(),
            );
        }
        Ok(next)
    }

    /// The mood Mood (`Select`) = `v` selects: 0 and 778 off, a user
    /// mood's SID that mood, 99 and 777 "Viel Licht" — and so does every
    /// other value Config's simulation was given (4, 50, 51, 98 without
    /// an alarm-clock mood, 100, 102, 779, 876, 877).
    fn by_id(&self, v: f64) -> Result<usize, String> {
        if v.fract() != 0.0 || v < 0.0 {
            return Err(format!("Mood = {v}: not a mood ID"));
        }
        let bright = || {
            self.by_sid("777")
                .ok_or_else(|| format!("Mood = {v} without a mood `SID=\"777\"` is undocumented"))
        };
        match v as u32 {
            0 | 778 => Ok(self.off),
            98 | 776 if self.by_sid("776").is_some() => Err(format!(
                "Mood = {v} with an alarm-clock mood (`SID=\"776\"`) is not simulated"
            )),
            id @ 1..=89 => match self.by_sid(&id.to_string()) {
                Some(ix) => Ok(ix),
                None => bright(),
            },
            _ => bright(),
        }
    }

    fn step(&mut self, io: &mut Io) -> Result<(), String> {
        let now = io.now;
        let v = |k: usize| io.i(k);
        for (k, key) in LIGHTS_READS.iter().enumerate().skip(LIGHTS_MODELLED) {
            let input = v(k);
            // MasterBr takes a value, the others a pulse.
            if input.on() || (*key == "MasterBr" && input.connected) {
                return Err(not_simulated("LightController2", key));
            }
        }
        let (motion, presence, br) = (v(0), v(1), v(2).v);
        let (reset, disabled) = (v(3).on(), v(4).on());
        let (brt, moet, met, pto, pm) = (v(5).v, v(6).v, v(7).v, v(8).v, v(9).v);
        let (tdc, mmd) = (v(22).v, v(23).v);
        let (afi, max_abr) = (v(26).v, v(27).v);
        if pm != 0.0 {
            return Err(format!(
                "presence mood Pm = {pm}: how its ID names a mood is undocumented"
            ));
        }
        if motion.on() && presence.on() {
            return Err("Mo and P at once is not simulated".into());
        }
        let (move_rose, move_fell) = self.move_edge.update(motion.on());
        let (presence_rose, presence_fell) = self.presence_edge.update(presence.on());
        let (reset_rose, _) = self.reset_edge.update(reset && !disabled);
        let wants = motion.on() || presence.on();
        let timeout = if presence.connected { 0.01 } else { pto };

        // The pulse inputs; DisPc disables them all ("Lc1-4, M+, M-,
        // Mood, Off, T5/1-8, On, Buzzer, MBr"). M+, M− and T5 act when
        // they fall, On and Buzzer when they rise.
        let mut acted = None;
        let mut buzzer_fell = false;
        let held = |k: &Key| k.edge.0;
        let locked = held(&self.keys[KEY_ON]) || held(&self.keys[KEY_BUZZER]);
        for (i, key) in self.keys.iter_mut().enumerate() {
            let name = LIGHTS_READS[LIGHTS_KEYS + i];
            let (r, f) = key.edge.update(v(LIGHTS_KEYS + i).on());
            if disabled {
                key.rose = None;
                continue;
            }
            if r {
                // Config's simulation took two clicks 0.22 s apart for two
                // single clicks, the KB a double click (off and 2C).
                if i < KEY_ON && key.rose.is_some_and(|t| now - t + EPS < tdc) {
                    return Err(format!(
                        "a double click on {}: two pulses within Tdc = {tdc} s are not simulated",
                        crate::canvas_labels::label("LightController2", name).unwrap_or(name)
                    ));
                }
                key.rose = Some(now);
            }
            // A long click on T5 mixes in its mood (Mmd) by the KB; Config's
            // simulation took it for a click.
            let limit = if mmd > 0.0 { mmd } else { tdc };
            if held(key)
                && (KEY_DOWN + 1..KEY_ON).contains(&i)
                && key.rose.is_some_and(|t| now - t + EPS >= limit)
            {
                return Err(format!(
                    "T5/{} held for {limit} s: a long click (mixing in a mood, Mmd) is \
                     not simulated",
                    i - 1
                ));
            }
            let acts = if i < KEY_ON {
                f && key.rose.is_some()
            } else {
                r
            };
            if acts && acted.replace(i).is_some() {
                return Err("two of M+, M−, T5, On and Buzzer at once is not simulated".into());
            }
            buzzer_fell |= f && i == KEY_BUZZER;
        }
        // Mood selects when its value changes: a constant selects nothing.
        let select = v(24);
        let selected = match self.select.replace(select.v) {
            Some(was) if select.connected && (select.v - was).abs() > EPS && !disabled => {
                if acted.is_some() {
                    return Err(
                        "Mood together with M+, M−, T5, On or Buzzer is not simulated".into(),
                    );
                }
                Some(self.by_id(select.v)?)
            }
            _ => None,
        };
        let manual = acted.is_some() || selected.is_some();
        if manual && (locked && !matches!(acted, Some(KEY_ON | KEY_BUZZER))) {
            return Err("a mood change while On or Buzzer is held is not simulated".into());
        }
        if locked && reset && !disabled {
            return Err("Off while On or Buzzer is held is not simulated".into());
        }

        // Alarm: "All used outputs (Lc1-18) start flashing when 1" between 0
        // and MaxAbr, Afi the period ("2s = 1s On, 1s Off"), "ID 99 is
        // displayed at the output (M)". In Config's simulation every wired
        // circuit flashed, on first, M 99 and 0 in step, and the mood
        // returned when Alarm ended.
        let alarm = v(25).on();
        let (alarm_rose, _) = self.alarm_edge.update(alarm);
        if alarm_rose {
            if !self.alarm_set {
                return Err("Alarm without Afi and MaxAbr set on the block: Config's \
                            simulation did not flash at the documented 4 s"
                    .into());
            }
            self.alarm_since = Some(now);
        }
        if !alarm {
            self.alarm_since = None;
        }
        if let Some(since) = self.alarm_since {
            if afi <= 0.0 {
                return Err(format!("Alarm with Afi = {afi}"));
            }
            if manual
                || (wants && self.suppressed_until.is_some())
                || reset
                || move_rose
                || move_fell
                || presence_rose
                || presence_fell
                || buzzer_fell
                || self.off_at.is_some_and(|t| now + EPS >= t)
            {
                return Err("an input or timer during Alarm is not simulated".into());
            }
            let lit = ((now - since + EPS) / (afi / 2.0)).floor() as i64 % 2 == 0;
            let mood = &self.moods[self.cur];
            for (n, key) in LIGHTS_WRITES[..18].iter().enumerate() {
                let q = mood.qs.get(n).copied().unwrap_or(0);
                if q >> 28 > 1 {
                    return Err(format!(
                        "Alarm on the colour circuit AQ{}: its flashing value is undocumented",
                        n + 1
                    ));
                }
                io.set(key, if lit { max_abr } else { 0.0 });
            }
            io.set_bool("OutputReset", false);
            io.set("Scene", if lit { 99.0 } else { 0.0 });
            return Ok(());
        }

        // Off: "Pulse (< 1 s): Outputs are reset / switched off. Constant
        // 1 (> 1 s): Block is locked. Dominating input"; 2C pulses on it
        // (one cycle, like the other pulse outputs). DisPc disables it.
        self.reset_pulse = reset_rose;
        let lit = self.cur != self.off;
        if reset && !disabled {
            // Switching the lights off by hand disables the automatic for
            // Pto, restarted by motion; with P wired Pto is 0.01 s, so the
            // automatic returns once presence has ended (parameter Pto).
            if lit && pto > 0.0 {
                self.suppressed_until = Some(now + timeout);
            }
            self.switch_off();
        } else {
            // Motion with a mood chosen by hand: the mood stays and Met
            // runs from the end of motion (Config's simulation); after a
            // manual step to off the automatic held back for a time that
            // fits neither Pto from the off nor Pto from the motion.
            if wants && self.manual_at.is_some() && (!lit || self.buzzer || presence.on()) {
                return Err(
                    "Mo or P after the lights were switched off by M+, M−, T5 or \
                            Mood, during the Buzzer's mood, or P with a manual mood is \
                            not simulated"
                        .into(),
                );
            }
            if manual {
                if presence.on()
                    || (wants && !lit)
                    || (self.off_at.is_some() && !wants)
                    || (self.buzzer && !buzzer_fell)
                {
                    return Err("a manual mood change while P is on, while motion is held \
                                back, or while the lights wait to go off is not simulated"
                        .into());
                }
                match selected {
                    Some(ix) => {
                        self.cur = ix;
                        self.chain = None;
                        self.manual_at = Some(now);
                        self.auto = false;
                        self.buzzer = false;
                    }
                    None => self.manual(acted.unwrap_or_default(), now)?,
                }
            }
            // Buzzer: "Lighting is switched off after the time set in
            // parameter (Met) has expired"; Met "starts … with a falling
            // edge at the input (Buzzer)", 0 never switches off.
            if buzzer_fell && met > 0.0 && self.buzzer {
                self.off_at = Some(now + met);
            }
            if let Some(t) = self.suppressed_until {
                if wants {
                    self.suppressed_until = Some(now + timeout);
                } else if now + EPS >= t {
                    self.suppressed_until = None;
                }
            }
            // Mo and P activate the motion mood "when 1"; the brightness
            // counts only while every light is off, and above Brt motion
            // and presence are ignored.
            if wants
                && self.cur == self.off
                && self.manual_at.is_none()
                && br <= brt
                && self.suppressed_until.is_none()
            {
                self.cur = self.motion;
                self.auto = true;
                self.chain = None;
            }
            let lit = self.cur != self.off;
            if wants {
                self.off_at = None;
            }
            // P: the mood "remains active as long as (P) is ON".
            if presence_fell && lit && !motion.on() {
                self.switch_off();
            }
            // Mo: off Moet after the falling edge, or Met when that is
            // shorter; Moet 0 never switches off. A mood chosen by hand
            // goes off Met after it ("If the lighting is operated
            // manually, the lighting is only switched off after parameter
            // (Met) has expired").
            if move_fell && lit {
                if met <= 0.0 {
                    return Err("Met = 0 with motion: which timeout applies is \
                                undocumented"
                        .into());
                }
                if self.manual_at.is_some() {
                    self.off_at = Some(now + met);
                } else if moet > 0.0 {
                    self.off_at = Some(now + moet.min(met));
                }
            }
            if let Some(t) = self.off_at
                && now + EPS >= t
            {
                self.switch_off();
            }
        }
        let mood = &self.moods[self.cur];
        for (n, key) in LIGHTS_WRITES[..18].iter().enumerate() {
            let q = mood.qs.get(n).copied().unwrap_or(0);
            let value = mood_output(&mood.name, n, q)?;
            io.set(key, value);
        }
        let pulse = self.reset_pulse;
        io.set_bool("OutputReset", pulse);
        io.set("Scene", scene_id(mood)?);
        Ok(())
    }

    fn switch_off(&mut self) {
        self.cur = self.off;
        self.auto = false;
        self.manual_at = None;
        self.buzzer = false;
        self.chain = None;
        self.off_at = None;
    }

    /// A click on pulse input `i` (of [`LIGHTS_KEYS`]).
    fn manual(&mut self, i: usize, now: f64) -> Result<(), String> {
        let mut chain = None;
        self.cur = match i {
            KEY_UP => self.step_mood(1)?,
            KEY_DOWN => self.step_mood(-1)?,
            KEY_ON => self.by_sid("777").ok_or(
                "On without a mood `SID=\"777\"` (ID 99): which outputs it sets to MaxAbr is undocumented",
            )?,
            // "Activates alarm clock mood (ID 98) … If no alarm clock mood
            // is configured, the mood with ID 99 is used instead."
            KEY_BUZZER => self
                .by_sid("776")
                .or_else(|| self.by_sid("777"))
                .ok_or("Buzzer without an alarm-clock or `SID=\"777\"` mood is undocumented")?,
            _ => {
                let n = i - 1;
                let assigned = self.by_sid(&n.to_string());
                let continues = self.chain.is_some_and(|(k, _)| k == i);
                chain = Some((i, now));
                if continues || assigned == Some(self.cur) {
                    // The KB: "If there is no additional pulse for 30
                    // seconds, a pulse selects the assigned mood again";
                    // Config's simulation still stepped on after 65 s.
                    if let Some((_, t)) = self.chain.filter(|_| continues)
                        && now - t > 30.0
                    {
                        return Err(format!(
                            "T5/{n} more than 30 s after its last pulse: the KB selects the \
                             assigned mood again, Loxone Config's simulation steps on"
                        ));
                    }
                    self.step_mood(1)?
                } else {
                    assigned.ok_or_else(|| {
                        format!(
                            "T5/{n} without a mood `SID=\"{n}\"`: what it selects is undocumented"
                        )
                    })?
                }
            }
        };
        self.chain = chain;
        self.auto = false;
        self.buzzer = i == KEY_BUZZER;
        self.manual_at = Some(now);
        Ok(())
    }
}

/// "its input Tg (`InputTrigger`) is not simulated": by canvas label where
/// one is known.
fn not_simulated(block_type: &str, key: &str) -> String {
    match crate::canvas_labels::label(block_type, key) {
        Some(label) if label != key => format!("its input {label} (`{key}`) is not simulated"),
        _ => format!("its input `{key}` is not simulated"),
    }
}

/// The output value of a mood's circuit: off is 0, a percentage that
/// value (on a switch or dimmer circuit), and a switch circuit's `on`
/// (stored 1) reads 1 (live Miniserver read); colour values have no
/// documented output encoding.
fn mood_output(mood: &str, n: usize, q: u32) -> Result<f64, String> {
    use crate::moods::MoodValue;
    match crate::moods::decode(q) {
        MoodValue::Off => Ok(0.0),
        MoodValue::On => Ok(1.0),
        MoodValue::Pct(p) if q >> 28 <= 1 => Ok(f64::from(p)),
        v => Err(format!(
            "mood \"{mood}\" sets AQ{} to `{v}`: what the output reads for it is \
             undocumented (give it a percentage)",
            n + 1
        )),
    }
}

/// What output M (`Scene`) shows for a mood: 0 off, 99 for `SID="777"`,
/// 98 for the alarm clock `SID="776"`, else the SID (Loxone Config's
/// simulation: a user mood `SID="n"` shows n).
fn scene_id(mood: &crate::moods::Mood) -> Result<f64, String> {
    match mood.sid.as_deref() {
        Some(crate::moods::OFF_SID) => Ok(0.0),
        Some("777") => Ok(99.0),
        Some("776") => Ok(98.0),
        Some(s) => s
            .parse::<u8>()
            .ok()
            .filter(|n| (1..=89).contains(n))
            .map(f64::from)
            .ok_or_else(|| format!("mood \"{}\" has `SID=\"{s}\"`, not a mood ID", mood.name)),
        None => Err(format!("mood \"{}\" has no `SID=`", mood.name)),
    }
}

/// The controller's moods and the index of its motion mood: the mood
/// flagged `Move="true"`, which `<LSConfig ScMv=>` names (loxone-format.md,
/// *LightController2 moods*).
fn lights_moods(el: &Element) -> Result<(Vec<crate::moods::Mood>, usize), String> {
    let moods = crate::moods::Moods::read(el)
        .ok_or("it has no moods yet (Loxone Config writes them on its first save)")?;
    let scenes = el
        .child_elements()
        .find(|c| c.name == "LightscenesC")
        .ok_or("no moods")?;
    let flagged: Vec<usize> = scenes
        .child_elements()
        .filter(|c| c.name == "LightsceneC")
        .enumerate()
        .filter(|(_, c)| c.attr("Move") == Some("true"))
        .map(|(i, _)| i)
        .collect();
    let [ix] = flagged[..] else {
        return Err(format!("{} moods flagged for motion", flagged.len()));
    };
    let named = el
        .child_elements()
        .find(|c| c.name == "LSConfig")
        .and_then(|c| c.attr("ScMv"));
    if named != moods.moods[ix].sid.as_deref() {
        return Err(
            "its motion settings name another mood than the one flagged \
                    for motion"
                .into(),
        );
    }
    Ok((moods.moods, ix))
}

/// The time of day of a `PulseAt` (`Sec=`), for the fixed-time kind
/// only: other `Typ=` values (sunrise, hour pulse, …) and `AutP=` have no
/// documented format.
fn pulse_at_time(el: &Element) -> Result<f64, String> {
    match el.attr("Typ") {
        None | Some("0") => {}
        Some(t) => {
            return Err(format!(
                "a pulse of kind `Typ=\"{t}\"` (undocumented format)"
            ));
        }
    }
    if el.attr("AutP").is_some() {
        return Err("its `AutP=` setting (undocumented format)".into());
    }
    let sec = el
        .attr("Sec")
        .and_then(parse_num)
        .ok_or("no `Sec=` time of day")?;
    if !(0.0..86_400.0).contains(&sec) {
        return Err(format!("`Sec=\"{sec}\"` is not a time of day"));
    }
    Ok(sec)
}

/// A meter reading from an Mr input. "Only increasing or constant meter
/// readings are allowed; decreasing or jumping to 0 readings are
/// discarded" (kb/meter, *Detection of Invalid Meter Readings*).
#[derive(Debug, Clone, Copy, Default)]
pub struct Reading(Option<f64>);

impl Reading {
    fn update(&mut self, v: f64) -> Result<f64, String> {
        match self.0 {
            None => self.0 = Some(v),
            Some(was) if v < was => {}
            // The page discards "unrealistically high" increases by two
            // conditions without saying whether both must hold.
            Some(was) if v - was > 10_000.0 => {
                return Err(format!(
                    "the meter reading jumps from {was} to {v}: whether Loxone \
                     discards that as invalid is ambiguous"
                ));
            }
            Some(_) => self.0 = Some(v),
        }
        Ok(self.0.unwrap_or(0.0))
    }
}

/// R resets "meter reading outputs" on a pulse and locks while on; which
/// outputs and to what is not documented.
fn meter_reset(r: Input) -> Result<(), String> {
    if r.on() {
        return Err("R is on: which readings a reset clears, and to what, is \
                    undocumented"
            .into());
    }
    Ok(())
}

/// With Mr and no Pf the block estimates the power from the last two
/// readings, which "only works if the same value causes an additional
/// edge" — edges the simulator does not see.
fn meter_power_needs_pf(pf: Input) -> Result<(), String> {
    if !pf.connected {
        return Err("Mr without Pf: the power from meter readings is not simulated".into());
    }
    Ok(())
}

/// Add the energy since the last cycle: the power of the last cycle times
/// the time in hours ("the units for Pf/Mr must be hour-related", kb/meter).
fn integrate(mr: &mut f64, last: &mut Option<(f64, f64)>, now: f64, pf: f64) {
    if let Some((t, p)) = *last {
        *mr += p * (now - t) / 3600.0;
    }
    *last = Some((now, pf));
}

/// The built-in mode number from a mode's fixed UUID
/// (`00000000-0000-000N-1500000000000000`, loxone-format.md).
fn mode_number(uuid: &str) -> Option<u8> {
    let rest = uuid.strip_prefix("00000000-0000-")?;
    let (n, tail) = rest.split_once('-')?;
    (tail == "1500000000000000")
        .then(|| u8::from_str_radix(n, 16).ok())
        .flatten()
}

fn schedule(el: &Element) -> Result<Schedule, String> {
    let modes: Vec<String> = el
        .attr("Modes")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let num = |e: &Element, k: &str, default: f64| -> Result<f64, String> {
        match e.attr_decoded(k) {
            Some(s) => {
                parse_num(&s).ok_or_else(|| format!("`<Entry {k}=\"{s}\">` is not a number"))
            }
            None => Ok(default),
        }
    };
    let mut entries = Vec::new();
    for e in el.child_elements().filter(|c| c.name == "Entry") {
        let column = num(e, "Ix", 0.0)? as usize;
        if column >= modes.len() {
            return Err(format!("an entry names column {column} of {}", modes.len()));
        }
        entries.push(Entry {
            column,
            from: num(e, "From", 0.0)?,
            to: num(e, "To", 0.0)?,
            value: num(e, "V", 1.0)?,
            need_activate: e.attr("NeedActivate") == Some("true"),
        });
    }
    let default = match el.attr_decoded("DefValue") {
        Some(s) => parse_num(&s).ok_or_else(|| format!("`DefValue=\"{s}\"` is not a number"))?,
        None => 0.0,
    };
    Ok(Schedule {
        modes,
        entries,
        analog: el.attr("Analog") == Some("true"),
        default,
    })
}

/// The schedule's value now (kb/timerscheduler): the entry of the active
/// operating mode's column that covers the time of day; digital 1 / off
/// 0, analog its value / the default value. An entry that needs
/// activation switches on only with a rising edge at Act while it runs,
/// for at most Don seconds when Don is set. Off switches off and, held,
/// locks.
fn day_timer(
    s: &Schedule,
    act: &mut Edge,
    activated: &mut Option<(usize, f64)>,
    io: &Io,
) -> Result<f64, String> {
    let (act_rising, _) = act.update(io.i(0).on());
    if io.i(2).on() {
        return Err("manual mode (Am) is not simulated".into());
    }
    let active: Vec<usize> = s
        .modes
        .iter()
        .enumerate()
        .filter(|(_, m)| io.modes.get(*m).copied().unwrap_or(false))
        .map(|(i, _)| i)
        .collect();
    let off_value = if s.analog { s.default } else { 0.0 };
    let column = match active.as_slice() {
        [] => {
            *activated = None;
            return Ok(off_value);
        }
        [c] => *c,
        _ => {
            return Err(format!(
                "{} of its operating modes are active at once; which one a timer \
                 uses is not documented — set the other modes to 0 in the test",
                active.len()
            ));
        }
    };
    let minute = io.clock.minute_of_day();
    let entry = s
        .entries
        .iter()
        .enumerate()
        .find(|(_, e)| e.column == column && e.from <= minute && minute < e.to);
    let Some((ix, e)) = entry else {
        *activated = None;
        return Ok(off_value);
    };
    if io.i(1).on() {
        *activated = None;
        return Ok(off_value);
    }
    let on_value = if s.analog { e.value } else { 1.0 };
    if !e.need_activate {
        return Ok(on_value);
    }
    if activated.is_some_and(|(i, _)| i != ix) {
        *activated = None;
    }
    if act_rising && activated.is_none() {
        *activated = Some((ix, io.now));
    }
    let don = io.i(3).v;
    match *activated {
        Some((_, since)) if don <= 0.0 || io.now - since + EPS < don => Ok(on_value),
        _ => Ok(off_value),
    }
}

/// A `Def=` or attribute number; Loxone writes either decimal separator.
pub fn parse_num(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_numbers_come_from_the_fixed_uuids() {
        assert_eq!(mode_number("00000000-0000-0004-1500000000000000"), Some(4));
        assert_eq!(mode_number("00000000-0000-000a-1500000000000000"), Some(10));
        assert_eq!(mode_number("1e96adaf-0136-8265-ffffed57184a04d2"), None);
    }
}
