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
    /// (kb/lighting-controller): its motion mood from all off, Off and
    /// DisPc.
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
            "LightController2" => match motion_mood(el) {
                Ok(mood) => Model::Lights(Box::new(Lights {
                    mood,
                    ..Lights::default()
                })),
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
    "I1",
    "I2",
    "I3",
    "I4",
    "I5",
    "I6",
    "I7",
    "I8",
    "InputTriggerUp",
    "InputTriggerDown",
    "Select",
    "Sel1",
    "Sel2",
    "Sel3",
    "Sel4",
    "Sel5",
    "Sel6",
    "Sel7",
    "Sel8",
    "EnMove",
    "On",
    "Alarm",
    "AlarmClock",
    "RtD",
    "MasterBr",
];
const LIGHTS_MODELLED: usize = 10;
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
];

/// A `LightController2`'s state: off, or showing its motion mood.
#[derive(Debug, Clone, Default)]
pub struct Lights {
    /// The motion mood: its name and `Q1` … `Q18`.
    mood: (String, Vec<u32>),
    lit: bool,
    /// When the lights go off after motion ended.
    off_at: Option<f64>,
    /// Set by a manual off: the automatic stays disabled until this
    /// time, which motion pushes back by Pto.
    suppressed_until: Option<f64>,
    move_edge: Edge,
    presence_edge: Edge,
    reset_edge: Edge,
    reset_pulse: bool,
}

impl Lights {
    fn step(&mut self, io: &mut Io) -> Result<(), String> {
        let now = io.now;
        let v = |k: usize| io.i(k);
        for (k, key) in LIGHTS_READS.iter().enumerate().skip(LIGHTS_MODELLED) {
            let input = v(k);
            // Select (Mood) and MasterBr take a value, the others a pulse.
            let valued = matches!(*key, "Select" | "MasterBr");
            if input.on() || (valued && input.connected) {
                return Err(not_simulated("LightController2", key));
            }
        }
        let (motion, presence, br) = (v(0), v(1), v(2).v);
        let (reset, disabled) = (v(3).on(), v(4).on());
        let (brt, moet, met, pto, pm) = (v(5).v, v(6).v, v(7).v, v(8).v, v(9).v);
        if pm != 0.0 {
            return Err(format!(
                "presence mood Pm = {pm}: how its ID names a mood is undocumented"
            ));
        }
        if motion.on() && presence.on() {
            return Err("Mo and P at once is not simulated".into());
        }
        let (_, move_fell) = self.move_edge.update(motion.on());
        let (_, presence_fell) = self.presence_edge.update(presence.on());
        let (reset_rose, _) = self.reset_edge.update(reset && !disabled);
        let wants = motion.on() || presence.on();
        let timeout = if presence.connected { 0.01 } else { pto };

        // Off: "Pulse (< 1 s): Outputs are reset / switched off. Constant
        // 1 (> 1 s): Block is locked. Dominating input"; 2C pulses on it
        // (one cycle, like the other pulse outputs). DisPc disables it.
        self.reset_pulse = reset_rose;
        if reset && !disabled {
            // Switching the lights off by hand disables the automatic for
            // Pto, restarted by motion; with P wired Pto is 0.01 s, so the
            // automatic returns once presence has ended (parameter Pto).
            if self.lit && pto > 0.0 {
                self.suppressed_until = Some(now + timeout);
            }
            self.lit = false;
            self.off_at = None;
        } else {
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
            if wants && !self.lit && br <= brt && self.suppressed_until.is_none() {
                self.lit = true;
            }
            if wants {
                self.off_at = None;
            }
            // P: the mood "remains active as long as (P) is ON".
            if presence_fell && self.lit && !motion.on() {
                self.lit = false;
            }
            // Mo: off Moet after the falling edge, or Met when that is
            // shorter; Moet 0 never switches off.
            if move_fell && self.lit {
                if met <= 0.0 {
                    return Err("Met = 0 with motion: which timeout applies is \
                                undocumented"
                        .into());
                }
                if moet > 0.0 {
                    self.off_at = Some(now + moet.min(met));
                }
            }
            if let Some(t) = self.off_at
                && now + EPS >= t
            {
                self.lit = false;
                self.off_at = None;
            }
        }
        let (name, qs) = &self.mood;
        for (n, key) in LIGHTS_WRITES[..18].iter().enumerate() {
            let q = qs.get(n).copied().unwrap_or(0);
            let value = if self.lit {
                mood_output(name, n, q)?
            } else {
                0.0
            };
            io.set(key, value);
        }
        let pulse = self.reset_pulse;
        io.set_bool("OutputReset", pulse);
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

/// The output value of a mood's circuit: off is 0 and a percentage is
/// that value (on a switch or dimmer circuit); a switch circuit's `on`
/// (stored 1) could read 1 or 100, and colour values have no documented
/// output encoding.
fn mood_output(mood: &str, n: usize, q: u32) -> Result<f64, String> {
    use crate::moods::MoodValue;
    match crate::moods::decode(q) {
        MoodValue::Off => Ok(0.0),
        MoodValue::Pct(p) if q >> 28 <= 1 => Ok(f64::from(p)),
        v => Err(format!(
            "mood \"{mood}\" sets AQ{} to `{v}`: what the output reads for it is \
             undocumented (give it a percentage)",
            n + 1
        )),
    }
}

/// The controller's motion mood: the mood flagged `Move="true"`, which
/// `<LSConfig ScMv=>` names (loxone-format.md, *LightController2 moods*).
fn motion_mood(el: &Element) -> Result<(String, Vec<u32>), String> {
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
    let mood = &moods.moods[ix];
    let named = el
        .child_elements()
        .find(|c| c.name == "LSConfig")
        .and_then(|c| c.attr("ScMv"));
    if named != mood.sid.as_deref() {
        return Err(
            "its motion settings name another mood than the one flagged \
                    for motion"
                .into(),
        );
    }
    Ok((mood.name.clone(), mood.qs.clone()))
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
