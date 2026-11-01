//! Block behavior, one model per simulated type. Every model is
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
    /// What central blocks tell this block this cycle (a shade: the
    /// `CentralShade`s whose `rec=` lists it).
    pub central: &'a [ShadeCmd],
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
    /// is open. Refs of block outputs wire `AI` only (loxone-format.md,
    /// *Refs*), and a real config triggers a Monoflop from such a ref's `Q`.
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
    FixedMeter(Box<Meter>),
    /// `MeterAbsUni` is Loxone's *Meter* (kb/meter).
    Meter(Box<Meter>),
    /// `MeterAbsBi` is Loxone's *Meter Bidirectional*
    /// (kb/meter-bidirectional).
    MeterBi(Box<[(Count, Reading); 2]>),
    /// `AutoJalousie` is Loxone's *Automatic Shading*
    /// (kb/automatic-blinds): complete open/close, safety and lock.
    Blind(Box<Blind>),
    /// `CentralShade` is Loxone's *Automatic Shading Central*
    /// (kb/shading-overview): Co, Cc and So for the shades of its `rec=`
    /// list.
    Central(Box<Central>),
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
            "MeterDig" => Model::FixedMeter(Box::default()),
            // The block's page names a "relative metering" property whose
            // attribute is not established; `Acum="true"` is a candidate.
            "MeterAbsUni" | "MeterAbsBi" if el.attr("Acum").is_some_and(|a| a != "false") => {
                return Support::Unsupported(
                    "it carries `Acum=`, probably the relative-metering property, whose \
                     meaning in the file is unverified"
                        .into(),
                );
            }
            "MeterAbsUni" => Model::Meter(Box::default()),
            "MeterAbsBi" => Model::MeterBi(Box::default()),
            "AutoJalousie" => Model::Blind(Box::default()),
            "CentralShade" => {
                let members: Vec<String> = el
                    .attr("rec")
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
                if members.is_empty() {
                    return Support::Unsupported("it controls no shades (`rec=` is empty)".into());
                }
                Model::Central(Box::new(Central {
                    members,
                    ..Central::default()
                }))
            }
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
                        "an operating mode switched by logic (its `I` is wired), and how that input acts is undocumented"
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
            Model::FixedMeter(_) => &["A", "R", "Ppwr", "Mro"],
            Model::Meter(_) => &["Pf", "Mr", "R", "Mro"],
            Model::MeterBi(_) => &["Pf", "Mr1", "Mr2", "R", "Mro1", "Mro2"],
            Model::Blind(_) => BLIND_READS,
            Model::Central(_) => CENTRAL_READS,
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
            Model::FixedMeter(_) | Model::Meter(_) => {
                &["OPf", "OMr", "ORd", "ORld", "ORm", "ORlm", "ORy", "ORly"]
            }
            Model::MeterBi(_) => &[
                "OPf", "OMr1", "ORd1", "ORld1", "ORm1", "ORlm1", "ORy1", "ORly1", "OMr2", "ORd2",
                "ORld2", "ORm2", "ORlm2", "ORy2", "ORly2",
            ],
            Model::Blind(_) => &[
                "OutputUp",
                "OutputDown",
                "OutputPos",
                "OutputSafety",
                "OutputLock",
            ],
            Model::Central(_) => &["OutOpen", "OutClose"],
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
            // From the kb/push-button timing diagram:
            // - A rising edge at Tr switches O on for Don. A new edge restarts it.
            // - Don 0 keeps O on while Tr is on.
            // - Off resets and, held, locks.
            // - DisPc disables Tr and Off.
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
            Model::FixedMeter(meter) => {
                let (r, mro) = (io.i(1), io.i(3).v);
                let s = io.i(0).v;
                if s != 0.0 && s != 1.0 {
                    return Err(format!("S = {s}: only 0 and 1 are documented"));
                }
                let pf = if s == 1.0 { io.i(2).v } else { 0.0 };
                meter.integrate(io, pf, r, mro)?;
                io.set("OPf", pf);
                meter.count.set(io, &METER_KEYS, mro);
            }
            // Pf alone: the block is the meter and integrates Pf over
            // hours; with Mr it maps the reading and passes Pf through
            // (kb/meter, *Meter reading and units*).
            Model::Meter(meter) => {
                let (pf, mr_in, r, mro) = (io.i(0), io.i(1), io.i(2), io.i(3).v);
                let power = if mr_in.connected {
                    meter_reset(r)?;
                    let at = io.clock.secs;
                    let before = meter.reading.0;
                    let v = meter.reading.update(mr_in.v)?;
                    meter.count.mr = v;
                    meter
                        .count
                        .periods
                        .add(at, at, before.map_or(0.0, |b| v - b))?;
                    if pf.connected {
                        pf.v
                    } else {
                        meter.estimate(now, mr_in.v, before)?
                    }
                } else {
                    if pf.v < 0.0 {
                        return Err(format!(
                            "Pf = {}: a negative power at a unidirectional meter is \
                             undocumented (its range is 0…∞)",
                            pf.v
                        ));
                    }
                    meter.integrate(io, pf.v, r, mro)?;
                    pf.v
                };
                io.set("OPf", power);
                meter.count.set(io, &METER_KEYS, mro);
            }
            // Mrc/Mrd map the consumption and delivery readings, Pf passes
            // through (kb/meter-bidirectional). How the block splits a Pf
            // alone into the two readings is not on the block's page.
            Model::MeterBi(sides) => {
                meter_reset(io.i(3))?;
                let pf = io.i(0);
                if !io.i(1).connected && !io.i(2).connected && pf.connected {
                    return Err("Pf alone: how the block splits it into consumption and \
                                delivery readings is undocumented"
                        .into());
                }
                meter_power_needs_pf(pf)?;
                let at = io.clock.secs;
                for (k, (count, reading)) in sides.iter_mut().enumerate() {
                    let input = io.i(1 + k);
                    if input.connected {
                        let before = reading.0;
                        let v = reading.update(input.v)?;
                        count.mr = v;
                        count.periods.add(at, at, before.map_or(0.0, |b| v - b))?;
                    }
                }
                io.set("OPf", pf.v);
                let (mro1, mro2) = (io.i(4).v, io.i(5).v);
                sides[0].0.set(io, &METER_BI_KEYS[0], mro1);
                sides[1].0.set(io, &METER_BI_KEYS[1], mro2);
            }
            Model::Blind(b) => b.step(io)?,
            Model::Central(c) => c.step(io)?,
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
/// [`BLIND_REFUSED`] are inputs without a model, the last of those
/// taking a value.
const BLIND_READS: &[&str] = &[
    "AutoShade",
    "ReactAutoShade",
    "Gesture",
    "ManualLamelle",
    "InputTrigger",
    "InputTriggerUp",
    "InputTriggerDown",
    "ManualPosition",
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
    "MinPulse",
    "DblClk",
    "MinMove",
    "TimeBlock",
];
const BLIND_REFUSED: usize = 4;

/// Which way a blind is driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
}

impl Dir {
    fn label(self) -> &'static str {
        match self {
            Dir::Up => "Po",
            Dir::Down => "Pc",
        }
    }
}

/// What a `CentralShade` tells the shades of its `rec=` list in one
/// cycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShadeCmd {
    Complete(Dir),
    SlightlyOpen,
}

/// A drive: its direction, when the output switches on (later than the
/// command after a reversal, by the motor lock) and off, in sim seconds.
/// `hold`: Po or Pc started it and is still held.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drive {
    dir: Dir,
    from: f64,
    until: f64,
    hold: bool,
}

/// The last press on Po or Pc, for the double click.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Press {
    dir: Dir,
    rose: f64,
    released: Option<f64>,
}

/// An `AutoJalousie`'s state. The position is the block's estimate from
/// run time, 0 open … 1 closed; a fresh simulation starts open.
#[derive(Debug, Clone, Default)]
pub struct Blind {
    pos: f64,
    drive: Option<Drive>,
    last_now: f64,
    /// The direction of the last drive and when its output went off.
    last_end: Option<(Dir, f64)>,
    press: Option<Press>,
    /// After Tg stopped a drive: the direction its next press drives.
    tg_next: Option<Dir>,
    /// The last value seen on Pos: a change moves.
    pos_in: Option<f64>,
    co: Edge,
    cc: Edge,
    so: Edge,
    tg: Edge,
    po: Edge,
    pc: Edge,
    /// Wa or Dwc as last acted on: `(wind, window)`.
    safety: (bool, bool),
    stop_held: bool,
}

/// The parameters a drive needs.
#[derive(Debug, Clone, Copy)]
struct Times {
    opd: f64,
    cld: f64,
    rdd: bool,
    mld: f64,
}

impl Blind {
    /// The position, and whether the blind moves (or waits for the motor
    /// lock to move).
    pub fn state(&self) -> (f64, bool) {
        (self.pos, self.drive.is_some())
    }

    /// Complete open runs Op for the whole Opd "regardless of the current
    /// position" (a reference run); complete close runs Cl only for "the
    /// determined remaining run time", or the whole Cld with Rdd
    /// (kb/automatic-blinds, *Complete movement, end positions and
    /// remanence*; parameter Rdd).
    fn complete(&mut self, dir: Dir, now: f64, t: Times) {
        let len = match dir {
            Dir::Up => t.opd,
            Dir::Down if t.rdd => t.cld,
            Dir::Down => (1.0 - self.pos) * t.cld,
        };
        self.start(dir, now, len, t.mld);
    }

    /// Start a drive of `len` seconds. Mld is the "duration of motor lock
    /// between direction changes": a drive against the running or the
    /// last one starts Mld after that one's output went off. A drive the
    /// same way continues the running one.
    fn start(&mut self, dir: Dir, now: f64, len: f64, mld: f64) {
        if self.drive.is_some_and(|d| d.dir != dir) {
            self.stop(now);
        }
        let from = match (self.drive, self.last_end) {
            (Some(d), _) => d.from.max(now),
            (None, Some((last, end))) if last != dir => (end + mld).max(now),
            _ => now,
        };
        self.drive = Some(Drive {
            dir,
            from,
            until: from + len,
            hold: false,
        });
    }

    fn stop(&mut self, now: f64) {
        if let Some(d) = self.drive.take()
            && now + EPS >= d.from
        {
            self.last_end = Some((d.dir, now));
        }
    }

    fn step(&mut self, io: &mut Io) -> Result<(), String> {
        let now = io.now;
        let v = |k: usize| io.i(k);
        for (k, key) in BLIND_READS[..BLIND_REFUSED].iter().enumerate() {
            let input = v(k);
            // Slat takes a value, the others a pulse.
            if input.on() || (k == BLIND_REFUSED - 1 && input.connected) {
                return Err(not_simulated("AutoJalousie", key));
            }
        }
        let (opd, cld) = (v(15).v, v(16).v);
        let (wap, rdd, kind, rd) = (v(17).v, v(18).on(), v(19).v, v(20).v);
        let (tlc, tdc, min_td, mld) = (v(21).v, v(22).v, v(23).v, v(24).v);
        let times = Times { opd, cld, rdd, mld };
        if opd <= 0.0 || cld <= 0.0 {
            return Err("an opening or closing duration of 0 is undocumented".into());
        }
        // The position follows the drive since the last cycle, linearly
        // over Opd and Cld — the reading of "remaining run time".
        if let Some(d) = self.drive {
            let ran = d.until.min(now) - d.from.max(self.last_now);
            if ran > 0.0 {
                self.pos = match d.dir {
                    Dir::Up => (self.pos - ran / opd).max(0.0),
                    Dir::Down => (self.pos + ran / cld).min(1.0),
                };
            }
            if now + EPS >= d.until {
                self.drive = None;
                self.last_end = Some((d.dir, d.until));
            }
        }
        self.last_now = now;

        let disabled = v(14).on();
        let (co, _) = self.co.update(v(8).on());
        let (cc, _) = self.cc.update(v(9).on());
        let (so, _) = self.so.update(v(10).on());
        let (tg, _) = self.tg.update(v(4).on());
        let (po, po_fell) = self.po.update(v(5).on());
        let (pc, pc_fell) = self.pc.update(v(6).on());
        let (wind, window, stop) = (v(11).on(), v(12).on(), v(13).on());
        let target = v(7);
        let moved_to = match self.pos_in.replace(target.v) {
            Some(was) if target.connected && (target.v - was).abs() > EPS => Some(target.v),
            _ => None,
        };

        // Push & hold: the drive lasts while the button is held, at least
        // minTd ("minimum travel time at pulse on input (Po) or (Pc)").
        for (fell, dir) in [(po_fell, Dir::Up), (pc_fell, Dir::Down)] {
            if !fell {
                continue;
            }
            if let Some(p) = &mut self.press
                && p.dir == dir
                && p.released.is_none()
            {
                p.released = Some(now);
            }
            if let Some(d) = &mut self.drive
                && d.hold
                && d.dir == dir
            {
                d.hold = false;
                d.until = (d.from + min_td).max(now);
                if now + EPS >= d.until {
                    self.stop(now);
                }
            }
        }
        // Held for Tlc, a press becomes the complete open or close
        // ("long-click duration on inputs (Po), (Pc) for complete open /
        // close").
        if let Some(d) = self.drive
            && d.hold
            && let Some(p) = self.press
            && tlc > 0.0
            && now - p.rose + EPS >= tlc
        {
            let at = (p.rose + tlc).max(d.from);
            let until = match d.dir {
                Dir::Up => at + opd,
                Dir::Down if rdd => at + cld,
                Dir::Down => now + (1.0 - self.pos) * cld,
            };
            self.drive = Some(Drive {
                hold: false,
                until,
                ..d
            });
            self.press = None;
        }
        if let Some(d) = self.drive
            && d.hold
        {
            if disabled {
                return Err(format!(
                    "DisPc while {} is held is undocumented",
                    d.dir.label()
                ));
            }
            let end = match d.dir {
                Dir::Up => self.pos <= 0.0,
                Dir::Down => self.pos >= 1.0,
            };
            if end {
                return Err(format!(
                    "{} held up to the end position: whether the output keeps \
                     driving is undocumented",
                    d.dir.label()
                ));
            }
        }

        // Off dominates and "overrules the safetyActive, as not even
        // moving to the safety position is allowed" (Structure File,
        // Jalousie `locked`): it stops, and locks while held.
        if stop || wind || window {
            self.press = None;
            self.tg_next = None;
            if moved_to.is_some() {
                return Err("Pos changes while the block is locked (Off, Wa or Dwc): \
                            whether it moves once the lock ends is undocumented"
                    .into());
            }
            if !io.central.is_empty() {
                return Err(
                    "a central command reaches a locked shade (Off, Wa or Dwc): \
                            whether it acts is undocumented"
                        .into(),
                );
            }
        }
        if stop {
            self.stop(now);
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
                    self.complete(Dir::Up, now, times);
                } else if wind && !was_wind {
                    if wap != 1.0 {
                        return Err(format!("Wap = {wap}: only 0 and 1 are documented"));
                    }
                    self.complete(Dir::Down, now, times);
                }
                self.safety = (wind, window);
            }
            if !wind && !window {
                // DisPc disables "Tg, Po, Pc, Co, Cc, So, T5"; "central
                // commands are not blocked by an active (DisPc) input"
                // (kb/shading-overview), nor is Pos.
                let pulses = if disabled {
                    [false; 6]
                } else {
                    [co, cc, so, tg, po, pc]
                };
                let n = pulses.iter().filter(|&&b| b).count()
                    + usize::from(moved_to.is_some())
                    + io.central.len();
                if n > 1 {
                    return Err("two commands in one cycle (of Co, Cc, So, Tg, Po, Pc, Pos \
                                and a central block) are undocumented"
                        .into());
                }
                let [co, cc, so, tg, po, pc] = pulses;
                if n == 1 && !tg {
                    self.tg_next = None;
                }
                if n == 1 && !(po || pc) {
                    self.press = None;
                }
                if co || cc {
                    // Co and Cc: "If in motion, stops."
                    if self.drive.is_some() {
                        self.stop(now);
                    } else {
                        self.complete(if co { Dir::Up } else { Dir::Down }, now, times);
                    }
                } else if so {
                    self.slightly_open(now, kind, rd, times)?;
                } else if tg {
                    self.toggle(now, times)?;
                } else if po || pc {
                    let dir = if po { Dir::Up } else { Dir::Down };
                    self.press_button(dir, now, times, tlc, tdc)?;
                } else if let Some(p) = moved_to {
                    self.move_to(p, now, times)?;
                } else if let Some(&cmd) = io.central.first() {
                    // The central block's Co and Cc: "Stopping not possible".
                    if self.drive.is_some() {
                        return Err("a central command while the shade moves is \
                                    undocumented"
                            .into());
                    }
                    match cmd {
                        ShadeCmd::Complete(dir) => self.complete(dir, now, times),
                        ShadeCmd::SlightlyOpen => self.slightly_open(now, kind, rd, times)?,
                    }
                }
            }
        }
        let dir = self.drive.filter(|d| now + EPS >= d.from).map(|d| d.dir);
        io.set_bool("OutputUp", dir == Some(Dir::Up));
        io.set_bool("OutputDown", dir == Some(Dir::Down));
        io.set("OutputPos", self.pos);
        io.set_bool("OutputSafety", wind || window);
        io.set_bool("OutputLock", stop);
        Ok(())
    }

    /// Tg "toggles between open, stop, close": a press stops a moving
    /// shade, and the next one drives the other way, completely (the
    /// reading of "open" and "close"). Where the cycle starts on a shade
    /// at rest is undocumented.
    fn toggle(&mut self, now: f64, t: Times) -> Result<(), String> {
        if let Some(d) = self.drive {
            self.stop(now);
            self.tg_next = Some(match d.dir {
                Dir::Up => Dir::Down,
                Dir::Down => Dir::Up,
            });
            return Ok(());
        }
        match self.tg_next.take() {
            Some(dir) => {
                self.complete(dir, now, t);
                Ok(())
            }
            None => Err("Tg on a shade at rest: where its open, stop, close cycle \
                         starts is undocumented"
                .into()),
        }
    }

    /// A press on Po or Pc: "partial open (close) with push & hold". A
    /// second press within Tdc of the first (Tdc 0: none) and a press held
    /// for Tlc (0: every press) start the complete open or close.
    fn press_button(
        &mut self,
        dir: Dir,
        now: f64,
        t: Times,
        tlc: f64,
        tdc: f64,
    ) -> Result<(), String> {
        let name = dir.label();
        if tdc > 0.0
            && let Some(p) = self.press
            && p.dir == dir
        {
            if now - p.rose + EPS < tdc {
                self.press = None;
                self.complete(dir, now, t);
                return Ok(());
            }
            if p.released.is_some_and(|r| now - r + EPS < tdc) {
                return Err(format!(
                    "a second press on {name} within Tdc of releasing the first but \
                     not of pressing it: whether that is a double click is undocumented"
                ));
            }
        }
        if self.drive.is_some() {
            return Err(format!("{name} while the shade moves is undocumented"));
        }
        self.press = None;
        if tlc <= 0.0 {
            self.complete(dir, now, t);
            return Ok(());
        }
        if dir == Dir::Down && t.rdd {
            return Err("Pc with Rdd: whether a partial close drives the whole Cld \
                        is undocumented"
                .into());
        }
        let end = match dir {
            Dir::Up => self.pos <= 0.0,
            Dir::Down => self.pos >= 1.0,
        };
        if end {
            return Err(format!(
                "{name} at the end position: whether the output drives is undocumented"
            ));
        }
        self.start(dir, now, f64::INFINITY, t.mld);
        if let Some(d) = &mut self.drive {
            d.hold = true;
        }
        self.press = Some(Press {
            dir,
            rose: now,
            released: None,
        });
        Ok(())
    }

    /// Pos (%): "move the shading to the specified position", 0 open …
    /// 100 closed like the Pos output, when the value changes. The run
    /// time is the share of Opd or Cld.
    fn move_to(&mut self, p: f64, now: f64, t: Times) -> Result<(), String> {
        if !(0.0..=100.0).contains(&p) {
            return Err(format!("Pos = {p}: the range is 0 … 100 %"));
        }
        let target = p / 100.0;
        if target <= 0.0 || target >= 1.0 {
            return Err(format!(
                "Pos = {p} %: whether that is a complete open or close (a reference \
                 run, Rdd) is undocumented"
            ));
        }
        if self.drive.is_some() {
            return Err("Pos while the shade moves is undocumented".into());
        }
        if target > self.pos {
            if t.rdd {
                return Err("Pos closing with Rdd: whether a partial close drives the \
                            whole Cld is undocumented"
                    .into());
            }
            self.start(Dir::Down, now, (target - self.pos) * t.cld, t.mld);
        } else if target < self.pos {
            self.start(Dir::Up, now, (self.pos - target) * t.opd, t.mld);
        }
        Ok(())
    }

    /// So: roller blinds, curtains and awnings "move into position
    /// according to parameter (Rd)", open 0.1 … closed 1.0
    /// (kb/automatic-blinds). Venetian blinds tilt their slats, which is
    /// not simulated.
    fn slightly_open(&mut self, now: f64, kind: f64, rd: f64, t: Times) -> Result<(), String> {
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
        if rd > self.pos {
            self.start(Dir::Down, now, (rd - self.pos) * t.cld, t.mld);
        } else if rd < self.pos {
            self.start(Dir::Up, now, (self.pos - rd) * t.opd, t.mld);
        }
        Ok(())
    }
}

/// Inputs of a `CentralShade` in [`Model::reads`] order; the first
/// [`CENTRAL_REFUSED`] are inputs without a model, the last two of those
/// taking a value.
const CENTRAL_READS: &[&str] = &[
    "InputTrigger",
    "InputTriggerUp",
    "InputTriggerDown",
    "AutoShade",
    "ReactAutoShade",
    "Safety",
    "Stop",
    "Gesture",
    "ManualPosition",
    "ManualLamelle",
    "EndUp",
    "EndDown",
    "Shade",
    "InputDisable",
];
const CENTRAL_REFUSED: usize = 10;

/// A `CentralShade`'s state: its shades and the command of this cycle.
/// No and Nc (`OutOpen`, `OutClose`) count its shades; the simulator
/// computes them from the shades.
#[derive(Debug, Clone, Default)]
pub struct Central {
    pub members: Vec<String>,
    pub cmd: Option<ShadeCmd>,
    co: Edge,
    cc: Edge,
    so: Edge,
}

impl Central {
    fn step(&mut self, io: &mut Io) -> Result<(), String> {
        self.cmd = None;
        let v = |k: usize| io.i(k);
        for (k, key) in CENTRAL_READS[..CENTRAL_REFUSED].iter().enumerate() {
            let input = v(k);
            if input.on() || (k >= CENTRAL_REFUSED - 2 && input.connected) {
                return Err(not_simulated("CentralShade", key));
            }
        }
        let (co, _) = self.co.update(v(10).on());
        let (cc, _) = self.cc.update(v(11).on());
        let (so, _) = self.so.update(v(12).on());
        // DisPc disables "Tg, Po, Pc, Co, Cc, So, T5" (kb/shading-overview).
        if v(13).on() {
            return Ok(());
        }
        if usize::from(co) + usize::from(cc) + usize::from(so) > 1 {
            return Err("two of Co, Cc and So in one cycle are undocumented".into());
        }
        self.cmd = if co {
            Some(ShadeCmd::Complete(Dir::Up))
        } else if cc {
            Some(ShadeCmd::Complete(Dir::Down))
        } else if so {
            Some(ShadeCmd::SlightlyOpen)
        } else {
            None
        };
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
    // Lc1 … Lc8, from `LIGHTS_LC`.
    "I1",
    "I2",
    "I3",
    "I4",
    "I5",
    "I6",
    "I7",
    "I8",
    "EnMove",
    "MasterBr",
    "RtD",
];
const LIGHTS_MODELLED: usize = 38;
/// Where Lc1 … Lc8 start in [`LIGHTS_READS`].
const LIGHTS_LC: usize = 28;
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
    "OutputResetAll",
    "Scene",
];

/// A pulse input of a `LightController2`: its edge, when it last rose, and
/// the clicks of its open click sequence (closed Tdc after the last rise
/// once the input is released, or by a long click).
#[derive(Debug, Clone, Copy, Default)]
struct Key {
    edge: Edge,
    rose: Option<f64>,
    clicks: u8,
}

/// What a closed click sequence or a long click does.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Act {
    /// A single click on pulse input `i` (of [`LIGHTS_KEYS`]), or On or
    /// Buzzer rising.
    Click(usize),
    /// Two or three clicks within Tdc each: off, 2C (and 3C).
    Multi(u8),
    /// T5 held for Mmd: mix its mood in or out.
    Long(usize),
}

/// A `LightController2`'s state: which of its moods shows, and who chose
/// it.
///
/// The manual inputs follow a live Miniserver (17.2.8.28) where it was
/// tested, else Loxone Config 17.2.8.28's simulation.
///
/// - M+/M− step through the mood list in `<LightscenesC>` order, the off
///   mood included, cyclically.
/// - T5/n selects the mood `SID="n"`. It steps on like M+ while that mood
///   shows or it made the last change within 30 s.
/// - A click acts Tdc after it began or when released, whichever is later.
///   Two or three clicks within Tdc switch off and pulse 2C (and 3C).
/// - T5 held for Mmd mixes its mood in or out.
/// - On selects `SID="777"` when it rises. Mood selects by ID when its
///   value changes.
#[derive(Debug, Clone, Default)]
pub struct Lights {
    moods: Vec<crate::moods::Mood>,
    /// The index of the motion mood, and of the built-in off mood.
    motion: usize,
    off: usize,
    /// The mood that shows (with a mix: its first).
    cur: usize,
    /// The moods mixed by long clicks, two or more (M −3); empty without
    /// a mix.
    mix: Vec<usize>,
    /// Circuit values set by Lc1–8 (`false`) or MBr (`true`) over the
    /// mood: M shows 0.
    custom: Option<([f64; 18], bool)>,
    /// Circuits with an actuator assigned in Loxone Config's mood editor
    /// (`NameAI<n>=`); without one a circuit is a switch (live: 20 %
    /// reads 1).
    named: [bool; 18],
    /// Circuits some mood switches on.
    used: [bool; 18],
    /// The last value seen on MBr: a change acts.
    master: Option<f64>,
    lc_edges: [Edge; 8],
    lc_rose: Option<f64>,
    dis_edge: Edge,
    /// Why the automatic is held back where that is untested.
    suppress_unverified: Option<&'static str>,
    reset_all_pulse: bool,
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
        let named = std::array::from_fn(|n| el.attr(&format!("NameAI{n}")).is_some());
        let used = std::array::from_fn(|n| {
            moods.iter().any(|m| {
                m.qs.get(n)
                    .is_some_and(|&q| crate::moods::decode(q) != crate::moods::MoodValue::Off)
            })
        });
        Ok(Lights {
            moods,
            motion,
            off,
            cur: off,
            named,
            used,
            alarm_set: has_def("AlarmPeriod") && has_def("AlarmBrightness"),
            ..Lights::default()
        })
    }

    /// Any light on: a mood, a mix or a circuit set by Lc/MBr.
    fn lit(&self) -> bool {
        match &self.custom {
            Some((out, _)) => out.iter().any(|&v| v > 0.0),
            None => self.cur != self.off,
        }
    }

    /// The circuit values of the moods that show (one, or a mix).
    fn outputs(&self) -> Result<[f64; 18], String> {
        let shown: &[usize] = if self.mix.is_empty() {
            std::slice::from_ref(&self.cur)
        } else {
            &self.mix
        };
        let mut out = [0.0; 18];
        for (n, slot) in out.iter_mut().enumerate() {
            let mut on: Option<f64> = None;
            for &ix in shown {
                let mood = &self.moods[ix];
                let q = mood.qs.get(n).copied().unwrap_or(0);
                let v = mood_output(&mood.name, n, q, self.named[n])?;
                // "If an output is used by two lighting moods, the higher
                // brightness of the output takes priority" (kb, *Moods*).
                on = Some(on.map_or(v, |w: f64| w.max(v)));
            }
            *slot = on.unwrap_or(0.0);
        }
        Ok(out)
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
            if v(k).on() {
                return Err(not_simulated("LightController2", key));
            }
        }
        let (motion_in, presence, br) = (v(0), v(1), v(2).v);
        let (reset, disabled) = (v(3).on(), v(4).on());
        let (brt, moet, met, pto, pm) = (v(5).v, v(6).v, v(7).v, v(8).v, v(9).v);
        let (tdc, mmd) = (v(22).v, v(23).v);
        let (afi, max_abr) = (v(26).v, v(27).v);
        let (dis, master) = (v(LIGHTS_LC + 8).on(), v(LIGHTS_LC + 9));
        if pm != 0.0 {
            return Err(format!(
                "presence mood Pm = {pm}: how its ID names a mood is undocumented"
            ));
        }
        // DisP "disables inputs (P) and (Mo) when 1" (live: Mo did not
        // switch on, and did once DisP was 0 again). "Lights activated by
        // presence (P) are switched off immediately. If lights were
        // activated by motion (Mo), (Moet) timer is started on rising edge
        // at (DisP) … If the extend time of (Met) is shorter than (Moet),
        // (Met) is used instead" (kb): with both masked, the end of P
        // switches off and the end of Mo starts Moet; a Moet already
        // running starts again.
        let (dis_rose, dis_fell) = self.dis_edge.update(dis);
        let sensed = motion_in.on() || presence.on();
        let by_motion = self.auto && self.manual_at.is_none();
        if (dis_rose && self.lit() && !by_motion && (sensed || self.off_at.is_some()))
            || (dis && sensed && (self.suppressed_until.is_some() || (self.lit() && !self.auto)))
            || (dis_fell && (sensed || self.auto || self.off_at.is_some()))
        {
            return Err(
                "DisP over a mood chosen by hand during motion, with motion while \
                        the automatic is held back, or ending during motion, presence or \
                        while the motion mood shows is not simulated"
                    .into(),
            );
        }
        if dis_rose && by_motion && !motion_in.on() && !presence.on() && self.off_at.is_some() {
            self.off_at = Some(now + moet.min(met));
        }
        let presence = Input {
            v: f64::from(u8::from(presence.on() && !dis)),
            ..presence
        };
        let motion = motion_in.on() && !dis;
        if motion && presence.on() {
            return Err("Mo and P at once is not simulated".into());
        }
        let (move_rose, move_fell) = self.move_edge.update(motion);
        let (presence_rose, presence_fell) = self.presence_edge.update(presence.on());
        let (reset_rose, _) = self.reset_edge.update(reset && !disabled);
        let wants = motion || presence.on();
        let timeout = if presence.connected { 0.01 } else { pto };
        // How long a manual switch-off holds the automatic back.
        let hold = (pto > 0.0).then_some(timeout);

        // The pulse inputs; DisPc disables them all ("Lc1-4, M+, M-,
        // Mood, Off, T5/1-8, On, Buzzer, MBr"). On and Buzzer act when
        // they rise. M+, M− and T5 click (live, #261): a click acts Tdc
        // after it began or when released, whichever is later; another
        // within Tdc makes it a double (triple) click; T5 held for Mmd
        // is a long click, acting while held.
        let mut acted: Option<Act> = None;
        let mut buzzer_fell = false;
        let held = |k: &Key| k.edge.0;
        let locked = held(&self.keys[KEY_ON]) || held(&self.keys[KEY_BUZZER]);
        let open = self.keys[..KEY_ON].iter().position(|k| k.clicks > 0);
        for i in 0..self.keys.len() {
            let name = LIGHTS_READS[LIGHTS_KEYS + i];
            let label = crate::canvas_labels::label("LightController2", name).unwrap_or(name);
            let key = &mut self.keys[i];
            let (r, f) = key.edge.update(v(LIGHTS_KEYS + i).on());
            if disabled {
                key.rose = None;
                key.clicks = 0;
                continue;
            }
            let mut act = None;
            if i >= KEY_ON {
                if r {
                    act = Some(Act::Click(i));
                }
                buzzer_fell |= f && i == KEY_BUZZER;
            } else {
                if r {
                    if open.is_some_and(|o| o != i) {
                        return Err(format!(
                            "a click on {label} while another input's click is open \
                             (within Tdc) is not simulated"
                        ));
                    }
                    let again = key.clicks > 0 && key.rose.is_some_and(|t| now - t + EPS < tdc);
                    key.clicks = if again { key.clicks + 1 } else { 1 };
                    if key.clicks > 3 {
                        return Err(format!(
                            "{} clicks on {label} within Tdc each are not simulated",
                            key.clicks
                        ));
                    }
                    key.rose = Some(now);
                }
                let since = key.rose.map_or(0.0, |t| now - t + EPS);
                if held(key) && key.clicks > 0 && i > KEY_DOWN && since >= tdc.min(mmd) {
                    if mmd <= tdc {
                        return Err(format!(
                            "{label} held with Mmd = {mmd} s, not longer than Tdc = {tdc} s, \
                             is not simulated"
                        ));
                    }
                    if since >= mmd {
                        if key.clicks > 1 {
                            return Err(format!(
                                "a long click on {label} after a click within Tdc is not \
                                 simulated"
                            ));
                        }
                        key.clicks = 0;
                        act = Some(Act::Long(i));
                    }
                }
                if key.clicks > 0 && !held(key) && since >= tdc {
                    act = Some(match key.clicks {
                        1 => Act::Click(i),
                        n => Act::Multi(n),
                    });
                    key.clicks = 0;
                }
            }
            if let Some(a) = act
                && acted.replace(a).is_some()
            {
                return Err("two of M+, M−, T5, On and Buzzer at once is not simulated".into());
            }
        }
        // Lc1–8: a click toggles its circuit.
        let mut toggled = None;
        for k in 0..8 {
            let (r, _) = self.lc_edges[k].update(v(LIGHTS_LC + k).on());
            if r && !disabled {
                if self.lc_rose.is_some_and(|t| now - t + EPS < tdc) {
                    return Err("two Lc clicks within Tdc are not simulated".into());
                }
                self.lc_rose = Some(now);
                if toggled.replace(k).is_some() {
                    return Err("two Lc inputs at once are not simulated".into());
                }
            }
        }
        // MBr acts when its value changes, like Mood.
        let dimmed = match self.master.replace(master.v) {
            Some(was) if master.connected && (master.v - was).abs() > EPS && !disabled => {
                Some(master.v)
            }
            _ => None,
        };
        // Mood selects when its value changes: a constant selects nothing.
        let select = v(24);
        let selected = match self.select.replace(select.v) {
            Some(was) if select.connected && (select.v - was).abs() > EPS && !disabled => {
                Some(self.by_id(select.v)?)
            }
            _ => None,
        };
        let inputs = usize::from(acted.is_some())
            + usize::from(toggled.is_some())
            + usize::from(dimmed.is_some())
            + usize::from(selected.is_some());
        if inputs > 1 {
            return Err(
                "two of Mood, MBr, Lc, M+, M−, T5, On and Buzzer at once are not simulated".into(),
            );
        }
        let manual = inputs > 0;
        if manual && (locked && !matches!(acted, Some(Act::Click(KEY_ON | KEY_BUZZER)))) {
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
                || self.custom.is_some()
                || !self.mix.is_empty()
            {
                return Err(
                    "an input or timer during Alarm, or Alarm over a mix or Lc/MBr \
                            values, is not simulated"
                        .into(),
                );
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
                if !self.named[n] && self.used[n] {
                    return Err(format!(
                        "Alarm on AQ{}, a circuit without an actuator: its flashing value is \
                         untested (live, such a circuit reads 1 for a percentage)",
                        n + 1
                    ));
                }
                io.set(key, if lit { max_abr } else { 0.0 });
            }
            io.set_bool("OutputReset", false);
            io.set_bool("OutputResetAll", false);
            io.set("Scene", if lit { 99.0 } else { 0.0 });
            return Ok(());
        }

        // Off: "Pulse (< 1 s): Outputs are reset / switched off. Constant
        // 1 (> 1 s): Block is locked. Dominating input"; 2C pulses on it
        // (one cycle, like the other pulse outputs). DisPc disables it.
        self.reset_pulse = reset_rose;
        self.reset_all_pulse = false;
        if reset && !disabled {
            // Switching the lights off by hand disables the automatic for
            // Pto, restarted by motion; with P wired Pto is 0.01 s, so the
            // automatic returns once presence has ended (parameter Pto).
            let lit = self.lit();
            self.switch_off();
            if lit && let Some(t) = hold {
                self.suppressed_until = Some(now + t);
            }
        } else {
            // Motion with a mood chosen by hand: the mood stays and Met
            // runs from the end of motion (Config's simulation).
            if wants
                && self.manual_at.is_some()
                && (!self.lit()
                    || self.buzzer
                    || presence.on()
                    || !self.mix.is_empty()
                    || self.custom.is_some())
            {
                return Err(
                    "Mo or P after Mood switched the lights off, during the Buzzer's \
                            mood, over a mix or Lc/MBr values, or P with a manual mood is not \
                            simulated"
                        .into(),
                );
            }
            if manual {
                if presence.on()
                    || (wants && !self.lit())
                    || (self.off_at.is_some() && !wants)
                    || (self.buzzer && !buzzer_fell)
                {
                    return Err("a manual mood change while P is on, while motion is held \
                                back, or while the lights wait to go off is not simulated"
                        .into());
                }
                if let Some(k) = toggled {
                    self.toggle(k, now)?;
                } else if let Some(m) = dimmed {
                    self.dim(m, now)?;
                } else if let Some(ix) = selected {
                    if self.custom.is_some() {
                        return Err("Mood after Lc or MBr set circuits is not simulated".into());
                    }
                    self.cur = ix;
                    self.mix.clear();
                    self.chain = None;
                    self.manual_at = Some(now);
                    self.auto = false;
                    self.buzzer = false;
                } else {
                    match acted.unwrap_or(Act::Click(KEY_UP)) {
                        Act::Click(i) => self.manual(i, now, hold)?,
                        Act::Multi(n) => {
                            // Live: off and 2C, a triple click also 3C, from
                            // off as well as from a mood.
                            let lit = self.lit();
                            self.switch_off();
                            self.reset_pulse = true;
                            self.reset_all_pulse = n >= 3;
                            if lit && let Some(t) = hold {
                                self.suppressed_until = Some(now + t);
                                self.suppress_unverified = Some("a double or triple click");
                            }
                        }
                        Act::Long(i) => self.long(i, now, hold)?,
                    }
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
                    if let Some(why) = self.suppress_unverified {
                        return Err(format!(
                            "motion after {why} switched the lights off is not simulated"
                        ));
                    }
                    self.suppressed_until = Some(now + timeout);
                } else if now + EPS >= t {
                    self.suppressed_until = None;
                    self.suppress_unverified = None;
                }
            }
            // Mo and P activate the motion mood "when 1"; the brightness
            // counts only while every light is off, and above Brt motion
            // and presence are ignored.
            if wants
                && self.cur == self.off
                && self.custom.is_none()
                && self.manual_at.is_none()
                && br <= brt
                && self.suppressed_until.is_none()
            {
                self.cur = self.motion;
                self.auto = true;
                self.chain = None;
            }
            let lit = self.lit();
            if wants {
                self.off_at = None;
            }
            // P: the mood "remains active as long as (P) is ON".
            if presence_fell && lit && !motion {
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
        let out = match &self.custom {
            Some((out, _)) => *out,
            None => self.outputs()?,
        };
        for (key, value) in LIGHTS_WRITES[..18].iter().zip(out) {
            io.set(key, value);
        }
        io.set_bool("OutputReset", self.reset_pulse);
        io.set_bool("OutputResetAll", self.reset_all_pulse);
        // M: 0 with circuits set by Lc/MBr, −3 for a mix (live).
        let m = if self.custom.is_some() {
            0.0
        } else if !self.mix.is_empty() {
            -3.0
        } else {
            scene_id(&self.moods[self.cur])?
        };
        io.set("Scene", m);
        Ok(())
    }

    fn switch_off(&mut self) {
        self.cur = self.off;
        self.mix.clear();
        self.custom = None;
        self.auto = false;
        self.manual_at = None;
        self.buzzer = false;
        self.chain = None;
        self.off_at = None;
        self.suppress_unverified = None;
    }

    /// A click on pulse input `i` (of [`LIGHTS_KEYS`]). A step to the off
    /// mood switches off like Off, holding the automatic back for `hold`
    /// (live: motion after M+, M− or T5 stepped to off waited for Pto
    /// without motion).
    fn manual(&mut self, i: usize, now: f64, hold: Option<f64>) -> Result<(), String> {
        if self.custom.is_some() {
            return Err(
                "M+, M−, T5, On or Buzzer after Lc or MBr set circuits is not \
                        simulated"
                    .into(),
            );
        }
        let t5 = (KEY_DOWN + 1..KEY_ON).contains(&i);
        if !self.mix.is_empty() && !t5 {
            return Err("M+, M−, On or Buzzer during a mix is not simulated".into());
        }
        let was_lit = self.lit();
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
                .ok_or("Buzzer without an alarm-clock or `SID=\"776\"` mood is undocumented")?,
            _ => {
                let n = i - 1;
                let assigned = self.by_sid(&n.to_string()).ok_or_else(|| {
                    format!("T5/{n} without a mood `SID=\"{n}\"`: what it selects is undocumented")
                })?;
                // Live: within 30 s of its last pulse T5/n steps on, after
                // that only while its mood shows; during a mix it selects
                // its mood alone.
                let continues = self.chain.is_some_and(|(k, t)| k == i && now - t < 30.0);
                chain = Some((i, now));
                if self.mix.is_empty() && (continues || assigned == self.cur) {
                    self.step_mood(1)?
                } else {
                    assigned
                }
            }
        };
        self.mix.clear();
        self.auto = false;
        if self.cur == self.off {
            self.switch_off();
            self.chain = chain;
            if was_lit && let Some(t) = hold {
                self.suppressed_until = Some(now + t);
            }
            return Ok(());
        }
        self.chain = chain;
        self.buzzer = i == KEY_BUZZER;
        self.manual_at = Some(now);
        Ok(())
    }

    /// A long click on T5/n (pulse input `i`): from off it selects its
    /// mood; else it mixes the mood in, or out when it is part of the mix
    /// (live: M −3 for a mix; the last mood out switches off).
    fn long(&mut self, i: usize, now: f64, hold: Option<f64>) -> Result<(), String> {
        let n = i - 1;
        let ix = self.by_sid(&n.to_string()).ok_or_else(|| {
            format!("T5/{n} without a mood `SID=\"{n}\"`: what it mixes in is undocumented")
        })?;
        if self.custom.is_some() || self.auto {
            return Err(format!(
                "a long click on T5/{n} while the motion mood or Lc/MBr values show is not \
                 simulated"
            ));
        }
        if !self.lit() {
            self.cur = ix;
            self.mix.clear();
        } else {
            let mut set = if self.mix.is_empty() {
                vec![self.cur]
            } else {
                std::mem::take(&mut self.mix)
            };
            match set.iter().position(|&m| m == ix) {
                Some(p) => {
                    set.remove(p);
                }
                None => set.push(ix),
            }
            match set[..] {
                [] => {
                    self.switch_off();
                    if let Some(t) = hold {
                        self.suppressed_until = Some(now + t);
                        self.suppress_unverified = Some("a long click");
                    }
                    return Ok(());
                }
                [one] => self.cur = one,
                _ => {
                    self.cur = set[0];
                    self.mix = set;
                }
            }
        }
        self.chain = None;
        self.auto = false;
        self.buzzer = false;
        self.manual_at = Some(now);
        Ok(())
    }

    /// Lc<k+1> toggles circuit `AQ<k+1>` (live: on reads 1, off 0, the
    /// other circuits stay, M shows 0) — on a circuit without an actuator.
    fn toggle(&mut self, k: usize, now: f64) -> Result<(), String> {
        if self.named[k] {
            return Err(format!(
                "Lc{} on a circuit with an actuator: the value it switches on is untested",
                k + 1
            ));
        }
        if self.auto || !self.mix.is_empty() || matches!(self.custom, Some((_, true))) {
            return Err(format!(
                "Lc{} while the motion mood, a mix or MBr values show is not simulated",
                k + 1
            ));
        }
        let mut out = match &self.custom {
            Some((out, _)) => *out,
            None => self.outputs()?,
        };
        out[k] = if out[k] > 0.0 { 0.0 } else { 1.0 };
        self.custom = Some((out, false));
        self.chain = None;
        self.buzzer = false;
        self.manual_at = Some(now);
        Ok(())
    }

    /// MBr changed to `m`: the mood's circuits scaled by it (live: on
    /// circuits without an actuator 50 reads 1, 0 switches off, 100 back
    /// on, M shows 0); while every light is off it changes nothing.
    fn dim(&mut self, m: f64, now: f64) -> Result<(), String> {
        if self.custom.is_none() && !self.lit() {
            return Ok(());
        }
        if self.auto || !self.mix.is_empty() || matches!(self.custom, Some((_, false))) {
            return Err(
                "MBr while the motion mood, a mix or Lc values show is not simulated".into(),
            );
        }
        let base = self.outputs()?;
        let mut out = [0.0; 18];
        for n in 0..18 {
            if self.named[n] && self.used[n] {
                return Err(format!(
                    "MBr on AQ{}, a circuit with an actuator: its scaled value is untested",
                    n + 1
                ));
            }
            out[n] = if base[n] > 0.0 && m > 0.0 { 1.0 } else { 0.0 };
        }
        self.custom = Some((out, true));
        self.chain = None;
        self.buzzer = false;
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

/// The output value of a mood's circuit: off is 0, a switch circuit's
/// `on` (stored 1) reads 1 (live Miniserver read), a percentage that value
/// on a circuit with an actuator (`named`, Loxone Config's simulation) and
/// 1 on one without (live: 20 % read 1); colour values have no documented
/// output encoding.
fn mood_output(mood: &str, n: usize, q: u32, named: bool) -> Result<f64, String> {
    use crate::moods::MoodValue;
    match crate::moods::decode(q) {
        MoodValue::Off => Ok(0.0),
        MoodValue::On => Ok(1.0),
        MoodValue::Pct(p) if q >> 28 <= 1 && !named => Ok(if p > 0 { 1.0 } else { 0.0 }),
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

/// R with a mapped meter reading: what a reset does to a reading the
/// physical meter keeps sending is undocumented.
fn meter_reset(r: Input) -> Result<(), String> {
    if r.on() {
        return Err(
            "R is on at a meter that maps readings (Mr): what a reset does \
                    to them is undocumented"
                .into(),
        );
    }
    Ok(())
}

/// With Mr and no Pf the block estimates the power from the last two
/// readings, which "only works if the same value causes an additional
/// edge" — edges the simulator does not see.
fn meter_power_needs_pf(pf: Input) -> Result<(), String> {
    if !pf.connected {
        return Err(
            "Mr without Pf at a bidirectional meter: how the power of two \
                    readings combines is undocumented"
                .into(),
        );
    }
    Ok(())
}

/// The outputs of a meter's count: the reading, then today, yesterday,
/// this month, last month, this year, last year.
const METER_KEYS: [&str; 7] = ["OMr", "ORd", "ORld", "ORm", "ORlm", "ORy", "ORly"];
const METER_BI_KEYS: [[&str; 7]; 2] = [
    ["OMr1", "ORd1", "ORld1", "ORm1", "ORlm1", "ORy1", "ORly1"],
    ["OMr2", "ORd2", "ORld2", "ORm2", "ORlm2", "ORy2", "ORly2"],
];

/// What a meter has counted: its reading and the period readings.
#[derive(Debug, Clone, Default)]
pub struct Count {
    mr: f64,
    periods: Periods,
}

impl Count {
    /// Mro "is added to output (Mr)" only: the period readings are
    /// amounts, not readings.
    fn set(&self, io: &mut Io, keys: &[&'static str; 7], mro: f64) {
        io.set(keys[0], self.mr + mro);
        for (key, v) in keys[1..].iter().zip(self.periods.values()) {
            io.set(key, v);
        }
    }
}

/// A unidirectional meter's state.
#[derive(Debug, Clone, Default)]
pub struct Meter {
    count: Count,
    /// When the last cycle ran, and the power it counted with.
    last: Option<(f64, f64)>,
    reading: Reading,
    reset: Edge,
    /// Mr without Pf: the last value received and when, and the power
    /// estimated from it.
    received: Option<(f64, f64)>,
    power: f64,
}

impl Meter {
    /// Add the energy since the last cycle: the power of the last cycle
    /// times the time in hours ("the units for Pf/Mr must be
    /// hour-related", kb/meter). R: "Pulse: Meter reading outputs are
    /// reset. On: Block is locked" — a rising edge clears the reading and
    /// every period reading ("reset: Resets all values to 0", Structure
    /// File, Meter); while R is on the meter counts nothing.
    fn integrate(&mut self, io: &Io, pf: f64, r: Input, mro: f64) -> Result<(), String> {
        let (rose, _) = self.reset.update(r.on());
        if rose && mro != 0.0 {
            return Err(format!(
                "R with Mro = {mro}: whether a reset reading shows 0 or Mro is undocumented"
            ));
        }
        let now = io.now;
        if let Some((t, p)) = self.last {
            let amount = p * (now - t) / 3600.0;
            self.count.mr += amount;
            let to = io.clock.secs;
            self.count.periods.add(to - (now - t), to, amount)?;
        }
        self.last = Some((now, if r.on() { 0.0 } else { pf }));
        if rose {
            self.count = Count::default();
        }
        Ok(())
    }

    /// Mr alone: "The power is estimated using the difference between the
    /// last two meter readings and their time interval. If the same or a
    /// smaller meter value is received, power is set to zero. The power
    /// output remains unchanged until a new meter value is received"
    /// (kb/meter). A value is received when it changes; the power has the
    /// unit of the reading per hour, as for Pf alone.
    fn estimate(&mut self, now: f64, v: f64, accepted: Option<f64>) -> Result<f64, String> {
        match self.received {
            None => self.received = Some((now, v)),
            Some((t, was)) if (v - was).abs() > EPS => {
                if v > was {
                    if accepted.is_some_and(|a| was < a) {
                        return Err("Mr rises after a discarded reading: which two readings \
                                    the power comes from is undocumented"
                            .into());
                    }
                    self.power = (v - was) / ((now - t) / 3600.0);
                } else {
                    self.power = 0.0;
                }
                self.received = Some((now, v));
            }
            Some(_) => {}
        }
        Ok(self.power)
    }
}

/// The period readings: today, this month and this year by the simulated
/// clock's calendar, and the previous day, month and year (kb/meter
/// outputs Rd … Rly; "Meter reading today", Structure File, Meter
/// `totalDay`). Each counts what was metered in its period; at its end
/// the amount moves to the previous one. A simulation starts with
/// nothing metered.
#[derive(Debug, Clone, Default)]
pub struct Periods([Period; 3]);

#[derive(Debug, Clone, Copy, Default)]
struct Period {
    key: Option<i64>,
    cur: f64,
    prev: f64,
}

impl Periods {
    /// Count `amount`, metered evenly over the clock seconds `from..to`
    /// (at `to` when they are equal).
    fn add(&mut self, from: f64, to: f64, amount: f64) -> Result<(), String> {
        for (g, period) in self.0.iter_mut().enumerate() {
            let mut t = from;
            while t + EPS < to {
                let k = period_key(g, t);
                period.roll(g, k)?;
                let end = period_end(g, k).min(to);
                period.cur += amount * (end - t) / (to - from);
                t = end;
            }
            period.roll(g, period_key(g, to))?;
            if to <= from + EPS {
                period.cur += amount;
            }
        }
        Ok(())
    }

    fn values(&self) -> [f64; 6] {
        let [d, m, y] = self.0;
        [d.cur, d.prev, m.cur, m.prev, y.cur, y.prev]
    }
}

impl Period {
    fn roll(&mut self, g: usize, k: i64) -> Result<(), String> {
        match self.key {
            Some(was) if k == was + 1 => {
                self.prev = self.cur;
                self.cur = 0.0;
            }
            Some(was) if k > was => {
                self.prev = 0.0;
                self.cur = 0.0;
            }
            Some(was) if k < was => {
                return Err(format!(
                    "the clock went back to an earlier {}: the meter's period readings \
                     are undefined",
                    ["day", "month", "year"][g]
                ));
            }
            _ => {}
        }
        self.key = Some(k);
        Ok(())
    }
}

/// The day (0), month (1) or year (2) a clock time falls in.
fn period_key(g: usize, secs: f64) -> i64 {
    let day = (secs / 86_400.0).floor() as i64;
    let (y, m, _) = super::clock::civil_from_days(day);
    match g {
        0 => day,
        1 => y * 12 + m - 1,
        _ => y,
    }
}

/// When the period `k` of granularity `g` ends, in clock seconds.
fn period_end(g: usize, k: i64) -> f64 {
    use super::clock::days_from_civil;
    let day = match g {
        0 => k + 1,
        1 => {
            let (y, m) = (k.div_euclid(12), k.rem_euclid(12) + 1);
            if m == 12 {
                days_from_civil(y + 1, 1, 1)
            } else {
                days_from_civil(y, m + 1, 1)
            }
        }
        _ => days_from_civil(k + 1, 1, 1),
    };
    day as f64 * 86_400.0
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
                "{} of its operating modes are active at once, and which one a timer uses is not documented. Set the other modes to 0 in the test.",
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
