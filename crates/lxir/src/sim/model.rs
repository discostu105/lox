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
            | Model::Mode { .. } => &["Q"],
            Model::Arith(_) | Model::Add4 | Model::Formula(_) => &["AQ"],
            Model::Switch { .. } => &["Q", "Qon", "Qoff"],
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
