//! Port-direction knowledge.
//!
//! Two sources, kept deliberately separate:
//!
//! 1. A small **builtin table** of block types the IR compiler may mint.
//!    Every entry has been verified against real Miniserver configs and/or
//!    agrees across the existing connector databases (lox-cli's map and
//!    lox-sim's signatures — which contradict each other for many other
//!    types; that is exactly why this table only contains verified entries).
//!    The list order is the connector-index order used in port UUIDs.
//!
//! 2. **Evidence-based inference** ([`observe`]) over real configs: which
//!    keys occur per type, at which connector index, whether they carry
//!    incoming wires (input), are referenced as a wire source (output), or
//!    hold `Def=` values (parameter). This is the seed for a consolidated
//!    connector database — every imported real config is a test case.

use crate::doc::{LoxoneDoc, ports};
use crate::uuid::LoxUuid;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PortDir {
    Input,
    Output,
    Param,
    /// A bidirectional API connector: accepts wires in *and* serves as a
    /// wire source. Direct evidence on `AutoJalousie.OutputAPI` — the real
    /// house config wires into it on 4 instances and from it on 12, and
    /// the corpus shows 12 sink / 36 source uses. Never carries `Def=`.
    Api,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortSpec {
    pub key: &'static str,
    pub dir: PortDir,
}

const fn p(key: &'static str, dir: PortDir) -> PortSpec {
    PortSpec { key, dir }
}

use PortDir::{Api, Input, Output, Param};

// Verified live: Or{I1,I2,Q} at indexes 0,1,2 (And shares the shape).
const GATE: &[PortSpec] = &[p("I1", Input), p("I2", Input), p("Q", Output)];
// Verified live: Not{I,Q}.
const NOT: &[PortSpec] = &[p("I", Input), p("Q", Output)];
// Verified live: Equal{Input1,Input2,Q}; the whole comparator family
// (NotEqual, Greater, GreaterEqual, Less, LessEqual) shares the shape in
// both legacy connector databases.
const CMP: &[PortSpec] = &[p("Input1", Input), p("Input2", Input), p("Q", Output)];

// The entries below were admitted by the 2026-08-24 corpus consolidation
// run (6 real configs, evidence + legacy-db agreement — see
// docs/connector-db.md for the per-port rationale, including the inert-flag
// rule that classifies never-wired `Remanence` as Input).
const FORMULA: &[PortSpec] = &[
    p("Input1", Input),
    p("Input2", Input),
    p("Input3", Input),
    p("Input4", Input),
    p("AQ", Output),
    p("TQ", Output),
];
const MONOFLOP: &[PortSpec] = &[
    p("InputTrigger", Input),
    p("Reset", Input),
    p("Remanence", Input),
    p("Time", Param),
    p("Q", Output),
];
const PULSE_GEN: &[PortSpec] = &[
    p("InputEnable", Input),
    p("InputInvert", Input),
    p("Remanence", Input),
    p("TimeHigh", Param),
    p("TimeLow", Param),
    p("Q", Output),
];
const THRESHOLD: &[PortSpec] = &[
    p("Input", Input),
    p("Remanence", Input),
    p("On", Param),
    p("Off", Param),
    p("PulseTime", Param),
    p("Q", Output),
    p("RisingEdge", Output),
    p("FallingEdge", Output),
];

// Admitted 2026-08-25 after the mint-oracle run (docs/oracle-wine.md):
// real-instance evidence (7×/5×/8× in one config, zero index conflicts),
// legacy-db agreement on every overlapping port, and a minted minimal
// instance of each surviving a Loxone Config open+save. These blocks
// carry visualization children (`<IoData>`; PushButton also `<PSD>`,
// Memory also `<Display>` and `Tp=`) that the GUI adds on save — the
// compiler never authors them, but carries them forward verbatim on
// rebuilds and adoption accepts them (GUI-owned residue, D19).
const MEMORY: &[PortSpec] = &[p("Input", Input), p("AQ", Output), p("Q", Output)];
const PUSH_BUTTON: &[PortSpec] = &[
    p("InputTrigger", Input),
    p("On", Input),
    p("Reset", Input),
    p("InputDisable", Input),
    p("Remanence", Input),
    p("Q", Output),
    p("Qon", Output),
    p("Qoff", Output),
    // Bidirectionally evidenced by the 2026-08-25 web corpus (2 sink /
    // 2 source wires) — the counterexample that evicted the original
    // Output classification.
    p("OutputAPI", Api),
];
const PBUTTON_T: &[PortSpec] = &[
    p("InputTrigger", Input),
    p("Reset", Input),
    p("InputDisable", Input),
    p("Remanence", Input),
    p("Time", Param),
    p("Q", Output),
    // Inert across the whole corpus (20 occurrences, no wire, no Def,
    // absent from both legacy dbs) → Input by the inert-flag rule. The
    // original Output classification was a name-prior the rule never
    // supported; PushButton's flip to Api exposed it (docs/connector-db.md).
    p("OutputAPI", Input),
];

// Admitted 2026-08-25 from the strongest evidence base yet: 78 corpus
// occurrences (16 in the house config with an identical 49-key element
// order — the corpus-wide index conflicts come from older-generation
// configs and from connectors added later by schema migration, whose
// UUIDs reuse index bytes). Directions: EndUp/EndDown corpus-sink ×70;
// the 18 params carry Def= on every instance and both legacy dbs agree;
// the 9 Output*/TargetPos outputs agree across both legacy dbs with zero
// corpus contradiction; OutputAPI is bidirectionally evidenced (see
// [`PortDir::Api`]); the remaining never-touched keys classify Input by
// the inert-flag rule (docs/connector-db.md).
const AUTO_JALOUSIE: &[PortSpec] = &[
    p("InputTrigger", Input),
    p("InputTriggerUp", Input),
    p("InputTriggerDown", Input),
    p("EndUp", Input),
    p("EndDown", Input),
    p("Shade", Input),
    p("AutoShade", Input),
    p("EnAutoShade", Input),
    p("ReactAutoShade", Input),
    p("Safety", Input),
    p("Window", Input),
    p("Stop", Input),
    p("ManualPosition", Input),
    p("ManualLamelle", Input),
    p("Gesture", Input),
    p("InputDisable", Input),
    p("Type", Input),
    p("Dir", Param),
    p("TimeEnd", Param),
    p("TimeEndDown", Param),
    p("SO", Input),
    p("WP", Input),
    p("AutoShadeEnd", Param),
    p("AutMode", Param),
    p("MinPulse", Param),
    p("DblClk", Param),
    p("DirTol", Param),
    p("DirTol2", Param),
    p("AutoShadeTime", Param),
    p("SRoff", Param),
    p("SSoff", Param),
    p("Width", Param),
    p("Space", Param),
    p("Rdd", Input),
    p("TimeBlock", Param),
    p("TurnOffset", Param),
    p("Deadtime", Input),
    p("MinMove", Param),
    p("Back", Param),
    p("OutputUp", Output),
    p("OutputDown", Output),
    p("OutputPos", Output),
    p("OutputLPos", Output),
    p("OutputAutoShade", Output),
    p("OutputSafety", Output),
    p("OutputLock", Output),
    p("OutputCombined", Output),
    p("TargetPos", Output),
    p("OutputAPI", Api),
];

// Admitted 2026-08-25 from the web-grown corpus (149 configs — official
// Loxone samples, LoxWiki attachments, GitHub projects; see
// docs/connector-db.md). PulseAt: 16 modern instances (V254–V273,
// including the house) share this exact element order; Q corpus-source
// ×23, Time carries Def on every instance with connector-map agreeing
// P, the rest are inert-flag Inputs.
const PULSE_AT: &[PortSpec] = &[
    p("InputDisable", Input),
    p("Remanence", Input),
    p("Time", Param),
    p("Q", Output),
    p("OutputAPI", Input),
];
// DayTimer: 13 modern instances. The element order is the V259+ one —
// the V259 schema migration MOVED `PulseTime` from position 6 to 3
// while keeping its port UUID (index byte 06 at element position 3 in
// the house), the strongest confirmation yet that element order, not
// the UUID index byte, is canonical. Directions: InputTrigger/Reset
// corpus-sink; AQ/Qon/Qoff corpus-source; AQm/AQmt both legacy dbs
// Output with zero corpus contradiction; Manual/Mode/PulseTime
// connector-map Param (sim agrees input-like); RtD/Remanence/OutputAPI
// inert-flag Inputs. GUI-authored schedule entries (`<Entry>`) are
// D19 residue, carried forward verbatim.
const DAY_TIMER: &[PortSpec] = &[
    p("InputTrigger", Input),
    p("Reset", Input),
    p("RtD", Input),
    p("PulseTime", Param),
    p("Remanence", Input),
    p("Manual", Param),
    p("Mode", Param),
    p("AQ", Output),
    p("Qon", Output),
    p("Qoff", Output),
    p("AQm", Output),
    p("AQmt", Output),
    p("OutputAPI", Input),
];

// The 2026-08-25 admission batch (analysis rerun over the 145-file corpus
// plus the post-push house download; per-type rationale in
// docs/connector-db.md). Shared pattern: the house's Config-17 (V273)
// element order is canonical and uniform across every house instance;
// corpus variants are older generations whose divergence is explained by
// schema migration (appended/inserted keys, one param reorder), exactly
// the AutoJalousie/DayTimer story. Zero direction conflicts anywhere.

// LightController2: the largest admitted type — 75 connectors, 23 house
// instances with an identical element order (V259's 71-key order plus
// I5–I8 inserted; V227–254 additionally reordered params). 86 corpus
// occurrences. Directions: 17 corpus-sink inputs (Sel1 ×47, Brightness
// ×19, Move ×15 …); 16 Def=-on-every-instance params plus 5 more agreed
// P by connector-map with zero corpus contradiction; AQ1–AQ20 and
// Scene/OutputReset/OutputResetAll connector-map O (AQ1–AQ5 also
// corpus-source, ×131 on AQ1); `MoveOn` carries Def everywhere but has a
// sink wire — wire evidence precedes, and Input still admits the Def.
// `OutputAPI` has zero evidence in any direction and is absent from both
// legacy dbs → inert-flag Input (the PButtonT precedent; one observed
// counterexample re-classifies).
const LIGHT_CONTROLLER2: &[PortSpec] = &[
    p("I1", Input),
    p("I2", Input),
    p("I3", Input),
    p("I4", Input),
    p("I5", Input),
    p("I6", Input),
    p("I7", Input),
    p("I8", Input),
    p("InputTriggerUp", Input),
    p("InputTriggerDown", Input),
    p("Select", Input),
    p("Reset", Input),
    p("Sel1", Input),
    p("Sel2", Input),
    p("Sel3", Input),
    p("Sel4", Input),
    p("Sel5", Input),
    p("Sel6", Input),
    p("Sel7", Input),
    p("Sel8", Input),
    p("EnMove", Input),
    p("Move", Input),
    p("On", Input),
    p("Alarm", Input),
    p("AlarmClock", Input),
    p("Brightness", Input),
    p("InputDisable", Input),
    p("Presence", Input),
    p("RtD", Input),
    p("MasterBr", Input),
    p("BrightnessLimit", Param),
    p("Remanence", Input),
    p("MoveOn", Input),
    p("MoveIgnore", Param),
    p("Step", Param),
    p("Steptime", Param),
    p("Min", Param),
    p("Max", Param),
    p("Wrap", Param),
    p("NoLast", Param),
    p("MoveScene", Param),
    p("MaxP", Param),
    p("MoveTimeout", Param),
    p("RGBalt", Param),
    p("SceneMixTime", Param),
    p("FadingTime", Param),
    p("AlarmClockPeriod", Param),
    p("DayMinTemp", Param),
    p("DayMaxTemp", Param),
    p("AlarmPeriod", Param),
    p("AlarmBrightness", Param),
    p("AQ1", Output),
    p("AQ2", Output),
    p("AQ3", Output),
    p("AQ4", Output),
    p("AQ5", Output),
    p("AQ6", Output),
    p("AQ7", Output),
    p("AQ8", Output),
    p("AQ9", Output),
    p("AQ10", Output),
    p("AQ11", Output),
    p("AQ12", Output),
    p("AQ13", Output),
    p("AQ14", Output),
    p("AQ15", Output),
    p("AQ16", Output),
    p("AQ17", Output),
    p("AQ18", Output),
    p("AQ19", Output),
    p("AQ20", Output),
    p("Scene", Output),
    p("OutputReset", Output),
    p("OutputResetAll", Output),
    p("OutputAPI", Input),
];

// Switch2Button: 49 corpus occurrences; the modern (V273 house, ×5 per
// generation) order appends OutputAPI to the V74–162 shape and swaps
// On/Reset relative to it. InputTrigger corpus-sink on every instance;
// Time carries Def everywhere; Q/Qon/Qoff corpus-source (×16/×33/×15);
// On/Reset/InputDisable/OutputAPI inert-flag Inputs.
const SWITCH2_BUTTON: &[PortSpec] = &[
    p("InputTrigger", Input),
    p("On", Input),
    p("Reset", Input),
    p("InputDisable", Input),
    p("Time", Param),
    p("Q", Output),
    p("Qon", Output),
    p("Qoff", Output),
    p("OutputAPI", Input),
];

// CentralShade: 11 occurrences (3 modern house instances; the V254 corpus
// pair lacks the appended OutOpen/OutClose). EndUp/EndDown/AutoShade/
// Safety/Gesture corpus-sink; every other classic input agreed I by
// connector-map with zero contradiction; OutputAPI connector-map O, zero
// contradiction. OutOpen/OutClose had no wire evidence and were admitted
// as inert-flag Input; Loxone's documentation lists them as outputs, and
// oracle session 17 confirmed it: wires drawn *from* both survive a
// Loxone Config 17.2 save unchanged → Output (2026-10-29).
const CENTRAL_SHADE: &[PortSpec] = &[
    p("InputTrigger", Input),
    p("InputTriggerUp", Input),
    p("InputTriggerDown", Input),
    p("EndUp", Input),
    p("EndDown", Input),
    p("Shade", Input),
    p("AutoShade", Input),
    p("EnAutoShade", Input),
    p("ReactAutoShade", Input),
    p("Safety", Input),
    p("Stop", Input),
    p("ManualPosition", Input),
    p("ManualLamelle", Input),
    p("Gesture", Input),
    p("InputDisable", Input),
    p("OutOpen", Output),
    p("OutClose", Output),
    p("OutputAPI", Output),
];

// CentralLight: 9 occurrences (2 modern house instances; V254–259 corpus
// trio lacks the appended OutActive). Reset/On/Alarm corpus-sink; the
// rest connector-map I with zero contradiction; OutputAPI connector-map
// O. OutActive: inert-flag Input at admission; documented as output, and
// a wire drawn from it survives the save (oracle session 17) → Output.
const CENTRAL_LIGHT: &[PortSpec] = &[
    p("I1", Input),
    p("I2", Input),
    p("I3", Input),
    p("I4", Input),
    p("InputTriggerUp", Input),
    p("InputTriggerDown", Input),
    p("Select", Input),
    p("Reset", Input),
    p("Sel1", Input),
    p("Sel2", Input),
    p("Sel3", Input),
    p("Sel4", Input),
    p("Sel5", Input),
    p("Sel6", Input),
    p("Sel7", Input),
    p("Sel8", Input),
    p("EnMove", Input),
    p("On", Input),
    p("Alarm", Input),
    p("AlarmClock", Input),
    p("InputDisable", Input),
    p("RtD", Input),
    p("OutActive", Output),
    p("OutputAPI", Output),
];

// Code16: the most stable shape in the whole corpus — ONE identical
// 34-key element order across V99–V273 (12 instances, 8 files, house
// included). AI1–AI13 corpus-sink; TQ1/AQ1–AQ13/TeQ connector-map O
// (most also corpus-source); TI1–TI3 connector-map I; Remanence
// inert-flag Input.
const CODE16: &[PortSpec] = &[
    p("TI1", Input),
    p("TI2", Input),
    p("TI3", Input),
    p("AI1", Input),
    p("AI2", Input),
    p("AI3", Input),
    p("AI4", Input),
    p("AI5", Input),
    p("AI6", Input),
    p("AI7", Input),
    p("AI8", Input),
    p("AI9", Input),
    p("AI10", Input),
    p("AI11", Input),
    p("AI12", Input),
    p("AI13", Input),
    p("Remanence", Input),
    p("TQ1", Output),
    p("TQ2", Output),
    p("TQ3", Output),
    p("AQ1", Output),
    p("AQ2", Output),
    p("AQ3", Output),
    p("AQ4", Output),
    p("AQ5", Output),
    p("AQ6", Output),
    p("AQ7", Output),
    p("AQ8", Output),
    p("AQ9", Output),
    p("AQ10", Output),
    p("AQ11", Output),
    p("AQ12", Output),
    p("AQ13", Output),
    p("TeQ", Output),
];

// The timer/edge/state batch (2026-10-29), prioritized by demand: the
// block types an agent most often needs for everyday automations
// (lox-cli's 473-case eval set, counted by required block type) that the
// table could not mint yet. Shapes and directions from the 145-file web
// corpus plus the house config; per-type rationale in
// docs/connector-db.md. The corpus holds no Config-17 instance of most of
// them, so the save oracle (sessions 16/17, Loxone Config 17.2) decided
// each descriptor: every set below saves with identical connector keys,
// order and UUIDs. StairwayLS is the one the GUI rewrote from the corpus
// order (V151: Reset before On, no OutputAPI) — its table is the order
// the save writes, OutputAPI included (a wire drawn from it survives).
// Directions by corpus wires (sink/source counts), else inert-flag Input;
// Def-only ports without wire evidence are Param.
const ON_PULSE_DELAY: &[PortSpec] = &[
    p("InputTrigger", Input),
    p("Reset", Input),
    p("Remanence", Input),
    p("Delay", Input),
    p("Time", Input),
    p("Q", Output),
];
const OFF_DELAY: &[PortSpec] = &[
    p("InputTrigger", Input),
    p("Reset", Input),
    p("Remanence", Input),
    p("Time", Param),
    p("Q", Output),
];
const STAIRWAY_LS: &[PortSpec] = &[
    p("InputTrigger", Input),
    p("On", Param),
    p("Reset", Input),
    p("InputDisable", Input),
    p("TimeHigh", Input),
    p("TimeWarn", Param),
    p("Remanence", Param),
    p("WarnTime", Param),
    p("Q", Output),
    p("OutputAPI", Output),
];
const EDGE_DETECTION: &[PortSpec] = &[
    p("Input", Input),
    p("PulseTime", Param),
    p("Edge", Output),
    p("RisingEdge", Output),
    p("FallingEdge", Output),
];
const ANALOG_MULTIPLEXER2: &[PortSpec] = &[
    p("Select", Input),
    p("Input1", Input),
    p("Input2", Input),
    p("InputDisable", Input),
    p("AQ", Output),
];
const SR_FLIP_FLOP: &[PortSpec] = &[
    p("InputS", Input),
    p("InputTrigger", Input),
    p("InputR", Input),
    p("Remanence", Input),
    p("Q", Output),
];
const STATE: &[PortSpec] = &[
    p("I1", Input),
    p("I2", Input),
    p("I3", Input),
    p("I4", Input),
    p("I5", Input),
    p("I6", Input),
    p("I7", Input),
    p("I8", Input),
    p("TQ", Output),
    p("AQ", Output),
    p("OutputAPI", Input),
];

// The house-control batch (2026-10-30): the eight logic types the r50
// house used but lxir could only reference. Connector set and order are
// the Config 17.2 (V178) descriptor as the house's GUI-saved instances
// carry it — Loxone Config writes every descriptor connector (D8), and
// the corpus holds them in older generations only. Directions, per key
// (docs/connector-db.md, "The house-control batch"): corpus wires where
// they exist, else agreement of lox-cli's connector map with Loxone's
// documentation (zero corpus contradiction), else the inert-flag rule —
// a key that is never wired and never carries Def= anywhere is an Input
// (wiring *from* it is refused until evidence says otherwise). The save
// oracle cannot decide a direction: Config 17.2 keeps a wire sourced
// from an input and a wire into an output alike (session 25).
const HEAT_I_ROOM_CONTROLLER2: &[PortSpec] = &[
    p("AMode", Input),
    p("Input", Input),
    p("Temp", Input),
    p("Window", Input),
    p("Comfort", Input),
    p("Save", Input),
    p("Save2", Input),
    p("Move", Input),
    p("Reset", Input),
    p("DisMv", Input),
    p("RtD", Input),
    p("TempO", Input),
    p("InCo2", Input),
    p("InHumid", Input),
    p("inFan", Input),
    p("inAirDir", Input),
    p("TComfort", Param),
    p("TComfortC", Param),
    p("TComfortHC", Param),
    p("TDiff", Param),
    p("TShadeHeat", Param),
    p("TShadeCool", Param),
    p("TSaveL", Param),
    p("TSaveU", Param),
    p("TSave", Param),
    p("TDeepSleep", Param),
    p("TMax", Param),
    p("TimeMove", Param),
    p("TimeC", Param),
    p("TimeS", Param),
    p("TimeMv", Param),
    p("THCelvin", Param),
    p("TCCelvin", Param),
    p("TPWM", Param),
    p("TWin", Param),
    p("TExcess", Param),
    p("AQh", Output),
    p("AQc", Output),
    p("AQhc", Output),
    p("AQh1", Output),
    p("AQc1", Output),
    p("AQhc1", Output),
    p("AQh2", Output),
    p("AQc2", Output),
    p("AQhc2", Output),
    p("AQh3", Output),
    p("AQc3", Output),
    p("AQhc3", Output),
    p("Qs", Output),
    p("AQs", Output),
    p("Qe", Output),
    p("Qa", Output),
    p("AQt", Output),
    p("AQhm", Output),
    p("AQtm", Output),
    p("Qb", Output),
    p("OutputAPI", Input),
];
const IRRIGATION: &[PortSpec] = &[
    p("A", Input),
    p("AIv", Input),
    p("Rf", Input),
    p("Ra", Input),
    p("Reset", Input),
    p("MaxP", Param),
    p("MaxR", Param),
    p("Tv1", Param),
    p("Tv2", Param),
    p("Tv3", Param),
    p("Tv4", Param),
    p("Tv5", Param),
    p("Tv6", Param),
    p("Tv7", Param),
    p("Tv8", Param),
    p("Qp", Output),
    p("Qv1", Output),
    p("Qv2", Output),
    p("Qv3", Output),
    p("Qv4", Output),
    p("Qv5", Output),
    p("Qv6", Output),
    p("Qv7", Output),
    p("Qv8", Output),
    p("AQv", Output),
    p("OutputAPI", Output),
];
const ALARM: &[PortSpec] = &[
    p("I1", Input),
    p("I2", Input),
    p("I3", Input),
    p("I4", Input),
    p("I5", Input),
    p("ActiveOO", Input),
    p("ActiveOOP", Input),
    p("Active", Input),
    p("ActiveP", Input),
    p("ActiveDelay", Input),
    p("ActiveDelayP", Input),
    p("Inactive", Input),
    p("Confirm", Input),
    p("InputDisable", Input),
    p("DisMv", Input),
    p("HI1", Input),
    p("HI2", Input),
    p("HI3", Input),
    p("HI4", Input),
    p("Delay", Input),
    p("MaxDurOut", Param),
    p("Remanence", Input),
    p("IInit", Param),
    p("AutoConfirm", Param),
    p("TM", Param),
    p("TQ1", Param),
    p("TQ2", Param),
    p("TQ3", Param),
    p("TQ4", Param),
    p("TQ5", Param),
    p("TQ6", Param),
    p("TI", Param),
    p("WI2", Param),
    p("Q", Output),
    p("Q1", Output),
    p("Q2", Output),
    p("Q3", Output),
    p("Q4", Output),
    p("Q5", Output),
    p("Q6", Output),
    p("AQ", Output),
    p("QT", Output),
    p("AQr", Output),
    p("TQ", Output),
    p("OutTime", Output),
    p("Qo", Output),
    p("TTS", Output),
    p("OutputAPI", Input),
];
const SMOKE_ALARM: &[PortSpec] = &[
    p("Confirm", Input),
    p("Mute", Input),
    p("InputAlarm", Input),
    p("InputAlarmW", Input),
    p("InputAlarmS", Input),
    p("InputTemp", Input),
    p("InputAirDigitalS", Input),
    p("InputAirDigitalW", Input),
    p("InputAirAnalog", Input),
    p("Reset", Input),
    p("AlarmDelay", Param),
    p("MaxTemp", Param),
    p("MaxDuration", Param),
    p("Remanence", Param),
    p("Autoconfirm", Param),
    p("Servicemode", Param),
    p("OutActive", Output),
    p("OutAlarm1", Output),
    p("OutAlarm2", Output),
    p("OutSilent", Output),
    p("OutHorn", Output),
    p("OutNumAlarms", Output),
    p("OutAlarmTest", Output),
    p("OutText", Output),
    p("OutTime", Output),
    p("OutHornSpd", Input),
    p("OutputAPI", Output),
];
const EFM: &[PortSpec] = &[
    p("R", Input),
    p("Pre", Param),
    p("Pri", Param),
    p("CO2", Param),
    p("OGpwr", Output),
    p("OPpwr", Output),
    p("OCpwr", Input),
    p("OSpwr", Output),
    p("OEd", Output),
    p("OId", Output),
    p("OPd", Output),
    p("OCd", Output),
    p("OScd", Output),
    p("OCo2d", Output),
    p("OYd", Output),
    p("OSc", Output),
    p("OYt", Output),
    p("Rest", Output),
    p("OutputAPI", Output),
];
const TPF_CONTROLLER: &[PortSpec] = &[
    p("inpres", Input),
    p("indnd", Input),
    p("inlbt", Input),
    p("inroomlight", Input),
    p("indaylight", Input),
    p("inbrightness", Input),
    p("insetpoint", Input),
    p("inoff", Input),
    p("indis", Input),
    p("indisplay", Input),
    p("inbacklight", Input),
    p("inText", Input),
    p("maxbrightness", Param),
    p("parsetpoint1", Param),
    p("parsetpoint2", Param),
    p("parsetpoint3", Param),
    p("parsetpoint4", Param),
    p("parsetpoint5", Param),
    p("parsetpoint6", Param),
    p("parsetpointFB", Param),
    p("outsetpoint", Output),
    p("outsetpointdisplay", Output),
    p("outlbt", Input),
    p("outdnd", Input),
    p("outpresence", Output),
    p("outroomlight", Output),
    p("outdaylight", Output),
    p("outbrightness", Output),
    p("OutputAPI", Api),
];
const NFC_CODE_TOUCH: &[PortSpec] = &[
    p("Disable", Input),
    p("Lr", Input),
    p("Lg", Input),
    p("Lb", Input),
    p("Lw", Input),
    p("Bfb", Input),
    p("ExA", Input),
    p("ExARem", Input),
    p("Au", Param),
    p("La", Param),
    p("Lon", Input),
    p("Bg", Input),
    p("Lbr", Param),
    p("PulseTime", Input),
    p("Q1", Output),
    p("Q2", Output),
    p("Q3", Output),
    p("Q4", Output),
    p("Q5", Output),
    p("Q6", Output),
    p("TQ", Output),
    p("TQU", Output),
    p("TQU2", Output),
    p("TQU3", Output),
    p("TQo", Output),
    p("TQt", Output),
    p("Qd", Output),
    p("Qa", Output),
    p("Qn", Output),
    p("Be", Output),
    p("BeSel", Output),
    p("PUDel", Output),
    p("OutputAPI", Api),
];
const INTERCOM: &[PortSpec] = &[
    p("Ib", Input),
    p("Mute", Input),
    p("TTS", Input),
    p("Reset", Input),
    p("Lon", Param),
    p("Timeout", Param),
    p("Lbr", Param),
    p("Tp", Param),
    p("Qb", Output),
    p("Qmute", Output),
    p("Q1", Output),
    p("Q2", Output),
    p("Q3", Output),
    p("API", Api),
];

// The meters (2026-10-30): the house's 32 statistics meters — digital
// (MeterDig) and absolute uni-/bidirectional/storage (MeterAbsUni,
// MeterAbsBi, MeterAbsSt). Key order: every corpus instance (house V178,
// corpus V172/V178) materializes the full set in this order, matching the
// legacy db. Directions: corpus sinks (`A`, `Pf`, `Slvl`) and `Def` (`Ppwr`)
// where wired; everything else follows the legacy db with zero corpus
// contradiction — the reset and meter-reading inputs (`R`, `Mr*`) by the
// inert-flag rule, the readings `OR*` and `OMr*` as outputs. `OutputAPI` is
// wired both ways where the corpus wires it (Api), otherwise legacy's
// output (docs/connector-db.md, "The meters").
const METER_DIG: &[PortSpec] = &[
    p("A", Input),
    p("R", Input),
    p("Ppwr", Param),
    p("Mro", Param),
    p("OPf", Output),
    p("OMr", Output),
    p("ORd", Output),
    p("ORld", Output),
    p("ORw", Output),
    p("ORlw", Output),
    p("ORm", Output),
    p("ORlm", Output),
    p("ORy", Output),
    p("ORly", Output),
    p("OutputAPI", Output),
];
const METER_ABS_UNI: &[PortSpec] = &[
    p("Pf", Input),
    p("Mr", Input),
    p("R", Input),
    p("Mro", Param),
    p("OPf", Output),
    p("OMr", Output),
    p("ORd", Output),
    p("ORld", Output),
    p("ORw", Output),
    p("ORlw", Output),
    p("ORm", Output),
    p("ORlm", Output),
    p("ORy", Output),
    p("ORly", Output),
    p("OutputAPI", Api),
];
const METER_ABS_BI: &[PortSpec] = &[
    p("Pf", Input),
    p("Mr1", Input),
    p("Mr2", Input),
    p("R", Input),
    p("Mro1", Param),
    p("Mro2", Param),
    p("OPf", Output),
    p("OMr1", Output),
    p("ORd1", Output),
    p("ORld1", Output),
    p("ORw1", Output),
    p("ORlw1", Output),
    p("ORm1", Output),
    p("ORlm1", Output),
    p("ORy1", Output),
    p("ORly1", Output),
    p("OMr2", Output),
    p("ORd2", Output),
    p("ORld2", Output),
    p("ORw2", Output),
    p("ORlw2", Output),
    p("ORm2", Output),
    p("ORlm2", Output),
    p("ORy2", Output),
    p("ORly2", Output),
    p("OutputAPI", Api),
];
const METER_ABS_ST: &[PortSpec] = &[
    p("Pf", Input),
    p("Mr1", Input),
    p("Mr2", Input),
    p("Slvl", Input),
    p("R", Input),
    p("Mro1", Param),
    p("Mro2", Param),
    p("OPf", Output),
    p("OMr1", Output),
    p("ORd1", Output),
    p("ORld1", Output),
    p("ORw1", Output),
    p("ORlw1", Output),
    p("ORm1", Output),
    p("ORlm1", Output),
    p("ORy1", Output),
    p("ORly1", Output),
    p("OMr2", Output),
    p("ORd2", Output),
    p("ORld2", Output),
    p("ORw2", Output),
    p("ORwl2", Output),
    p("ORm2", Output),
    p("ORlm2", Output),
    p("ORy2", Output),
    p("ORly2", Output),
    p("OSlvl", Output),
    p("OutputAPI", Output),
];

// Mirror blocks: the on-page visual representation of an object living
// elsewhere (an I/O device, a system object, a block on another page).
// `AI`/`I` are fed by wires from the mirrored object's outputs; the
// page's logic consumes `AQ`/`Q`. Shape verified across the house
// config: 189 InputRef (all `Nio="4"`, AI always fed, I on 143),
// 154 OutputRef (all `Nio="2"`, AI fed on 151). The mirrored identity
// itself is not a connector — it lives in the `Ref=` element attribute
// (see `ref_link_type` and design.md D33).
const INPUT_REF: &[PortSpec] = &[
    p("AI", Input),
    p("I", Input),
    p("AQ", Output),
    p("Q", Output),
];
const OUTPUT_REF: &[PortSpec] = &[p("AI", Input), p("AQ", Output)];

/// The XML type a minted message ref (`OutputRefLM`, D51) may mirror: a
/// push notification of the *Messages* tree. Loxone Config draws the
/// notification on a page as an `OutputRefLM` with the `OutputRef` shape
/// plus a `<LoggerMailer>` child carrying the message (oracle session 15,
/// Config 17.2); it has no `LinkRefType=` and no value flag.
pub const MESSAGE_REF_TARGET: &str = "Notification";

/// What a ref says about its mirrored object's value: nothing for a
/// digital one, `Analog="true"` or `Text="true"` (D33, D48).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefValue {
    Digital,
    Analog,
    Text,
}
use RefValue::{Analog, Digital, Text};

// Virtual input (D54): an object of the "Virtual Inputs" folder, not a
// page block — the value a user sets in the app. `Q` and `Qm` are both
// sources on every one of the 113 corpus instances that carry
// connectors (175 instances overall, Config 3.x through 17).
const VIRTUAL_IN: &[PortSpec] = &[p("Q", Output), p("Qm", Output)];

/// `LinkRefType=` code and value flag a ref carries for a mirrored
/// object of the given XML type — Loxone Config's internal type-registry
/// number, learned from the house corpus (every code below observed on
/// real refs, deterministic per target type). A type not in this table
/// cannot be mirrored by a minted ref yet: extend the table from
/// corpus evidence rather than guessing the code.
///
/// `Text=` follows the target type as deterministically (D48): every
/// ref of an `ApiActor` (36) and of a `VirtualTextIn` (12) across the
/// corpus carries it, from Loxone Config 12.0 to 17.2.
///
/// `Memory` is the one target the corpus shows with *both* flags
/// (8× digital, 1× analog); minted mirrors of blocks default to digital
/// until evidence for a rule emerges.
pub fn ref_link_type(target_type: &str) -> Option<(u32, RefValue)> {
    REF_LINK_TYPES
        .iter()
        .find(|&&(t, ..)| t == target_type)
        .map(|&(_, code, value)| (code, value))
}

/// The table behind `ref_link_type`: target type, code, value flag.
const REF_LINK_TYPES: &[(&str, u32, RefValue)] = &[
    ("Online", 2, Digital),
    ("DigitalIn", 55, Digital),
    ("Actor", 63, Digital),
    ("VirtualIn", 71, Digital),
    ("VirtualUdpInCmd", 79, Digital),
    ("VirtualTextIn", 83, Text),
    ("VirtualHttpInCmd", 87, Analog),
    ("Lox1wireAsensor", 103, Analog),
    ("OvertempShutdown", 109, Digital),
    ("ApiActor", 136, Text),
    ("ModbusASensor", 153, Analog),
    ("ModbusAActor", 156, Analog),
    ("LoxAIRsensor", 172, Digital),
    ("LoxAIRAsensor", 173, Analog),
    ("LoxAIRactor", 174, Digital),
    ("LoxAIRAactor", 175, Analog),
    ("TreeSensor", 182, Digital),
    ("TreeAsensor", 183, Analog),
    ("TreeActor", 184, Digital),
    ("TreeAactor", 185, Analog),
    ("Daylight", 281, Digital),
    ("Memory", 320, Digital),
];

/// Every *logic* block type `builtin` knows. This is the default managed
/// set for decompile/adopt; `InputRef`/`OutputRef` are mintable too (see
/// `builtin`) but deliberately absent here — as plumbing they are matched
/// by externs or folded, never lifted into managed declarations.
pub const BUILTIN_TYPES: &[&str] = &[
    "And",
    "Or",
    "Not",
    "Equal",
    "NotEqual",
    "Greater",
    "GreaterEqual",
    "Less",
    "LessEqual",
    "Formula",
    "Monoflop",
    "PulseGen",
    "AnalogThresholdTrigger",
    "Memory",
    "PushButton",
    "PButtonT",
    "AutoJalousie",
    "PulseAt",
    "DayTimer",
    "LightController2",
    "Switch2Button",
    "CentralShade",
    "CentralLight",
    "Code16",
    "OnPulseDelay",
    "OffDelay",
    "OnDelay",
    "StairwayLS",
    "EdgeDetection",
    "AnalogMultiplexer2",
    "SRFlipFlop",
    "State",
    "VirtualIn",
    "HeatIRoomController2",
    "Irrigation",
    "Alarm",
    "SmokeAlarm",
    "EFM",
    "TpfController",
    "NfcCodeTouch",
    "Intercom",
    "MeterDig",
    "MeterAbsUni",
    "MeterAbsBi",
    "MeterAbsSt",
    // Virtual devices and Modbus (D64): objects of the object tree, see
    // `crate::periphery`.
    "VirtualHttpIn",
    "VirtualHttpInCmd",
    "VirtualUdpIn",
    "VirtualUdpInCmd",
    "VirtualTextIn",
    "VirtualOut",
    "VirtualOutCmd",
    "ModbusServer",
    "ModbusDev",
    "ModbusASensor",
    "ModbusSensor",
    "ModbusAActor",
];

/// Block versions (`V=` on `<C>`) the builtin table is verified for,
/// oldest first. Loxone Config writes every object of a document at its
/// release's block version on save (17.0 and 17.1 write 175, 17.2 writes
/// 178), and every builtin type keeps its connector keys and indexes across
/// these (D46). Compile emits the document's version when it is listed here.
pub const VERIFIED_BLOCK_VERSIONS: &[&str] = &["175", "178"];

/// Element-attribute parameters per block type: logic Loxone stores as an
/// attribute on the `<C>` element rather than as `Def=` on a connector. In
/// the IR they bind like any parameter (`Formula: "I1+I2"` in the argument
/// list). Every observed `Formula=` attribute is accompanied by
/// `Valid="false"` (the GUI revalidates on load); the compiler emits that
/// pair together.
///
/// An attribute parameter that is also GUI-owned (`Code=` on Code16, see
/// [`crate::doc::GUI_OWNED_ATTRS`]) is a *claim* (D40): carried verbatim
/// from the base until the source binds it, written from source while it
/// does. Decompile never lifts it, so adopting leaves it with the GUI.
pub fn attr_params(block_type: &str) -> &'static [&'static str] {
    match block_type {
        "Formula" => &["Formula"],
        "Code16" => &["Code"],
        "VirtualIn" => &["Analog", "MinVal", "MaxVal", "DefVal"],
        t => crate::periphery::periphery(t).map_or(&[], |p| p.settings),
    }
}

/// The container a block type lives in when it is not drawn on a page
/// (D54): a minted object of such a type is appended to the base's one
/// element of that type, never placed, and never governed by `page`.
pub fn container_type(block_type: &str) -> Option<&'static str> {
    match block_type {
        "VirtualIn" => Some("VirtualInCaption"),
        t => crate::periphery::folder(t),
    }
}

/// An object of the object tree rather than a page block: a virtual input
/// (D54), or a virtual device, Modbus device or one of their I/Os (D64).
/// Never placed, never governed by `page`.
pub fn object_tree(block_type: &str) -> bool {
    container_type(block_type).is_some() || crate::periphery::periphery(block_type).is_some()
}

/// Per-type GUI-owned attributes (D19) beyond the global
/// [`crate::doc::GUI_OWNED_ATTRS`]: re-emitted verbatim from the base so a
/// GUI save converges. A VirtualIn's input-dialog settings — `IName=` its
/// `VI<n>` designation (minted once, then the GUI's), `Desc=`, `ValOT=`,
/// `MinChange=`/`MinTime=` change filters, `EnVal=`, `DefOnFirstStart=`,
/// `Idx=` — and the value range the source may also bind (a binding is a
/// claim, as with `Code=` in D40).
pub fn carried_attrs(block_type: &str) -> &'static [&'static str] {
    match block_type {
        "VirtualIn" => &[
            "IName",
            "Desc",
            "ValOT",
            "MinVal",
            "MaxVal",
            "DefVal",
            "MinChange",
            "MinTime",
            "EnVal",
            "DefOnFirstStart",
            "Idx",
        ],
        // The house-control batch: every attribute the corpus and the
        // house show on these types beyond the global set — settings of
        // the GUI's block dialogs (history sizes, unit texts, output
        // names, answer texts), UUID lists the dialogs maintain
        // (`Objects=` the members of an alarm or a room controller,
        // `UUIDTimer=`/`SpCdUuid=` the ids of a child), and the device
        // binding of a block a device carries (`Ref=`, see
        // `adopt_only`). Carried, never authored: none of them changes
        // what a declared wire or parameter means.
        "HeatIRoomController2" => &[
            "UUIDTimer",
            "SourceCap",
            "ExT",
            "custRU",
            "custRI",
            "srcName",
            "Objects",
            "PrioH",
            "PrioC",
            "Cap",
            "linkC",
            "DefFan",
            "DefFanNP",
        ],
        "Irrigation" => &["TRK_NE", "TRK_MAX"],
        "Alarm" => &["Entries", "CentralOut", "WaitAfterRestart", "Objects"],
        "SmokeAlarm" => &["Entries", "CentralOut", "Objects", "ST"],
        "EFM" => &["UnSt", "Un1", "Un2"],
        "TpfController" => &["PRE"],
        "NfcCodeTouch" => &["Ref", "SpCdUuid", "OStr", "HSize", "AP", "NAUTT"],
        "Intercom" => &["Ref", "Objects", "ans", "OutName1", "OutName2", "OutName3"],
        // The meter dialog: units, recording and statistics settings, the
        // storage's level cap, an EFM member's `NOEFM`.
        "MeterDig" | "MeterAbsUni" | "MeterAbsBi" | "MeterAbsSt" => &[
            "PwrN",
            "Un1",
            "Un2",
            "UnSt",
            "Rec",
            "StatsType",
            "NOEFM",
            "MaxLvl",
        ],
        _ => &[],
    }
}

/// Per-type GUI-owned child elements (D19) beyond the global
/// [`crate::doc::GUI_OWNED_CHILDREN`], carried verbatim: the irrigation
/// zones (`<zone nm=…>`), the room controller's heating schedule
/// (`<Timer>` with its `<Entry>` rows), the energy flow monitor's node
/// tree (`<root>`/`<nd>`), statistics groups (`<statGrp>`) and the
/// meters it creates for itself (nested `<C>`), and an NFC Code Touch's
/// access codes (nested `<C Type="Keycode">`). A nested `<C>` keeps its
/// UUIDs: the rebuild re-emits the subtree as the base holds it.
pub fn carried_children(block_type: &str) -> &'static [&'static str] {
    match block_type {
        "HeatIRoomController2" => &["Timer"],
        "Irrigation" => &["zone"],
        "EFM" => &["root", "statGrp", "C"],
        "NfcCodeTouch" => &["C"],
        // The statistics groups the meter records into (Statistics V2).
        "MeterDig" | "MeterAbsUni" | "MeterAbsBi" | "MeterAbsSt" => &["statGrp"],
        _ => &[],
    }
}

/// What a Loxone Config save gives a block minted from source whose app
/// entry names it (D66, oracle session 28): the number of special states
/// (`SpStates=`, minted once and pinned in the lock) and the child
/// elements after the connectors, in the save's order. A block the base
/// carries keeps its own (D19).
pub fn save_defaults(block_type: &str) -> Option<SaveDefaults> {
    match block_type {
        "AutoJalousie" => Some((
            7,
            &[
                ("IoData", &[("Visu", "true")]),
                ("COHist", &[("TRK_NE", "20"), ("TRK_MAX", "100")]),
            ],
        )),
        _ => None,
    }
}

/// The special-state count and child elements of [`save_defaults`].
pub type SaveDefaults = (
    usize,
    &'static [(&'static str, &'static [(&'static str, &'static str)])],
);

/// Builtin types that are adopted but never minted: a block a device
/// carries — the Code Touch's and the Intercom's (bound by `Ref=`), and
/// the Touch Pure Flex's controller — exists because Loxone Config paired
/// the device. The source owns its parameters and wires once adopted;
/// creating one stays with Loxone Config.
pub fn adopt_only(block_type: &str) -> bool {
    matches!(block_type, "NfcCodeTouch" | "Intercom" | "TpfController")
}

/// Message parameters per block type (D51): the `<LoggerMailer>` child of
/// an `OutputRefLM` holds what a notification says when its input turns
/// on (`On=`) and off (`Off=`), and the least time between two messages
/// (`MinimumTime=`, milliseconds in the XML, seconds in the IR like every
/// duration). They bind like attribute parameters and never take a wire.
pub fn message_params(block_type: &str) -> &'static [&'static str] {
    match block_type {
        "OutputRefLM" => &["On", "Off", "MinimumTime"],
        _ => &[],
    }
}

/// Attribute parameters holding program text — the only bindings that
/// take a `file("…")` value (D40): a PicoC program is hundreds of lines,
/// which the line-oriented IR cannot hold inline.
pub fn file_params(block_type: &str) -> &'static [&'static str] {
    match block_type {
        "Code16" => &["Code"],
        _ => &[],
    }
}

/// The unit a port's value is in, as far as a suffixed value is concerned
/// (D52). Known only where Loxone's documentation states one
/// ([`crate::port_meta`]); an undocumented port keeps the D27 behavior
/// (time suffixes scale into seconds, `%`/`K` are annotations).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortUnit {
    Seconds,
    Minutes,
    Percent,
    Kelvin,
    /// A documented unit lxir has no suffix for (`°`, `mm`, `lux`, …).
    Other(&'static str),
}

impl PortUnit {
    pub fn describe(self) -> String {
        match self {
            PortUnit::Seconds => "seconds".into(),
            PortUnit::Minutes => "minutes".into(),
            PortUnit::Percent => "percent".into(),
            PortUnit::Kelvin => "kelvin".into(),
            PortUnit::Other(u) => format!("`{u}`"),
        }
    }
}

/// The documented unit of a block type's port, if any.
pub fn port_unit(block_type: &str, port: &str) -> Option<PortUnit> {
    let unit = crate::port_meta::port_meta(block_type, port)?.unit?;
    Some(match unit {
        "s" => PortUnit::Seconds,
        "min" => PortUnit::Minutes,
        "%" => PortUnit::Percent,
        "K" => PortUnit::Kelvin,
        other => PortUnit::Other(other),
    })
}

/// Verified port tables for the block types the compiler may create.
/// Order = connector-index order.
pub fn builtin(block_type: &str) -> Option<&'static [PortSpec]> {
    match block_type {
        "And" | "Or" => Some(GATE),
        "Not" => Some(NOT),
        "Equal" | "NotEqual" | "Greater" | "GreaterEqual" | "Less" | "LessEqual" => Some(CMP),
        "Formula" => Some(FORMULA),
        "Monoflop" => Some(MONOFLOP),
        "PulseGen" => Some(PULSE_GEN),
        "AnalogThresholdTrigger" => Some(THRESHOLD),
        "Memory" => Some(MEMORY),
        "PushButton" => Some(PUSH_BUTTON),
        "PButtonT" => Some(PBUTTON_T),
        "AutoJalousie" => Some(AUTO_JALOUSIE),
        "PulseAt" => Some(PULSE_AT),
        "DayTimer" => Some(DAY_TIMER),
        "LightController2" => Some(LIGHT_CONTROLLER2),
        "Switch2Button" => Some(SWITCH2_BUTTON),
        "CentralShade" => Some(CENTRAL_SHADE),
        "CentralLight" => Some(CENTRAL_LIGHT),
        "Code16" => Some(CODE16),
        "OnPulseDelay" => Some(ON_PULSE_DELAY),
        "OffDelay" | "OnDelay" => Some(OFF_DELAY),
        "StairwayLS" => Some(STAIRWAY_LS),
        "EdgeDetection" => Some(EDGE_DETECTION),
        "AnalogMultiplexer2" => Some(ANALOG_MULTIPLEXER2),
        "SRFlipFlop" => Some(SR_FLIP_FLOP),
        "State" => Some(STATE),
        "HeatIRoomController2" => Some(HEAT_I_ROOM_CONTROLLER2),
        "Irrigation" => Some(IRRIGATION),
        "Alarm" => Some(ALARM),
        "SmokeAlarm" => Some(SMOKE_ALARM),
        "EFM" => Some(EFM),
        "TpfController" => Some(TPF_CONTROLLER),
        "NfcCodeTouch" => Some(NFC_CODE_TOUCH),
        "Intercom" => Some(INTERCOM),
        "MeterDig" => Some(METER_DIG),
        "MeterAbsUni" => Some(METER_ABS_UNI),
        "MeterAbsBi" => Some(METER_ABS_BI),
        "MeterAbsSt" => Some(METER_ABS_ST),
        "InputRef" => Some(INPUT_REF),
        "OutputRef" | "OutputRefLM" => Some(OUTPUT_REF),
        "VirtualIn" => Some(VIRTUAL_IN),
        t => crate::periphery::periphery(t).map(|p| p.ports),
    }
}

/// Direction evidence for one port key of one block type, accumulated over
/// every occurrence in a document (or, after [`merge`], a whole corpus).
#[derive(Debug, Clone, Default, Serialize)]
pub struct ObservedPort {
    /// Connector index from the port UUID (when consistent across
    /// occurrences; `None` if never seen or conflicting — `index_conflict`
    /// distinguishes the two).
    pub index: Option<u8>,
    /// Two occurrences carried different connector indexes.
    pub index_conflict: bool,
    /// Occurrences where this port had incoming `<In>` wires → it is an input.
    pub wired_as_sink: u32,
    /// Occurrences where this port was referenced as a wire source → output.
    pub wired_as_source: u32,
    /// Occurrences carrying a `Def=` value → parameter-like.
    pub has_def: u32,
    pub occurrences: u32,
}

impl ObservedPort {
    /// Best direction guess from the evidence, if any.
    pub fn inferred_dir(&self) -> Option<PortDir> {
        match (self.wired_as_sink, self.wired_as_source) {
            (0, 0) if self.has_def > 0 => Some(PortDir::Param),
            (0, 0) => None,
            (s, o) if s >= o => Some(PortDir::Input),
            _ => Some(PortDir::Output),
        }
    }
}

/// Block type → port key → accumulated evidence.
pub type Observations = BTreeMap<String, BTreeMap<String, ObservedPort>>;

/// Scan a document and accumulate port-direction evidence.
pub fn observe(doc: &LoxoneDoc) -> Observations {
    let mut obs: Observations = BTreeMap::new();
    // Port UUID → (type, key), for resolving wire sources in pass 2.
    let mut owner: BTreeMap<String, (String, String)> = BTreeMap::new();

    for obj in doc.objects() {
        let el = doc.element_at(&obj.path).expect("path from objects()");
        for port in ports(el) {
            let entry = obs
                .entry(obj.block_type.clone())
                .or_default()
                .entry(port.key.clone())
                .or_default();
            entry.occurrences += 1;
            if !port.inputs.is_empty() {
                entry.wired_as_sink += 1;
            }
            if port.def.is_some() {
                entry.has_def += 1;
            }
            if let Ok(u) = LoxUuid::parse(&port.uuid)
                && let Some(idx) = u.connector_index()
            {
                match entry.index {
                    None if !entry.index_conflict => entry.index = Some(idx),
                    Some(prev) if prev != idx => {
                        entry.index = None;
                        entry.index_conflict = true;
                    }
                    _ => {}
                }
            }
            owner.insert(
                port.uuid.clone(),
                (obj.block_type.clone(), port.key.clone()),
            );
        }
    }

    for wire in doc.wires() {
        if let Some((ty, key)) = owner.get(&wire.from_port)
            && let Some(entry) = obs.get_mut(ty).and_then(|m| m.get_mut(key))
        {
            entry.wired_as_source += 1;
        }
    }
    obs
}

/// Merge evidence from another document (or corpus) into `into`.
///
/// Counters add; connector indexes stay only while every occurrence agrees
/// (a disagreement poisons the index permanently via `index_conflict`).
/// This is the aggregation step of the consolidated connector database:
/// `observe` each config separately (wire resolution is per-document),
/// then `merge` the results.
pub fn merge(into: &mut Observations, other: Observations) {
    for (ty, ports) in other {
        let target = into.entry(ty).or_default();
        for (key, o) in ports {
            let e = target.entry(key).or_default();
            e.wired_as_sink += o.wired_as_sink;
            e.wired_as_source += o.wired_as_source;
            e.has_def += o.has_def;
            e.occurrences += o.occurrences;
            e.index_conflict |= o.index_conflict;
            if let (Some(a), Some(b)) = (e.index, o.index)
                && a != b
            {
                e.index_conflict = true;
            }
            e.index = if e.index_conflict {
                None
            } else {
                e.index.or(o.index)
            };
        }
    }
}

/// One entry of a legacy connector database, in the `connector-map.json`
/// shape used by lox-cli: `c` = canonical connector order, `t` = port key →
/// `"I"` / `"O"`. (lox-sim's `block_signature` table converts into the same
/// shape: inputs+params as `"I"`, outputs as `"O"`.)
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct LegacyType {
    #[serde(default)]
    pub c: Vec<String>,
    #[serde(default)]
    pub t: BTreeMap<String, String>,
}

/// Block type → legacy entry.
pub type LegacyDb = BTreeMap<String, LegacyType>;

/// Read a cross-check database in either accepted shape. The
/// connector-map shape (`type -> {c, t}`) is taken as is. The documented
/// shape — Loxone's TechDoc as scraped into
/// `loxone-block-types-full.json` (`type -> {connectors: [{name, type:
/// "Input"|"Output"|"Parameter"}]}`) — is folded into it (D52): a
/// `%d`-patterned name (`AQ%d`) expands to the observed keys it matches,
/// and stays a literal pattern when none does, so it surfaces as
/// legacy-only.
pub fn legacy_db_from_json(
    value: &serde_json::Value,
    obs: &Observations,
) -> std::result::Result<LegacyDb, String> {
    let map = value
        .as_object()
        .ok_or("a cross-check database is a JSON object: type -> entry")?;
    let documented = map
        .values()
        .any(|e| e.get("connectors").is_some_and(|c| c.is_array()));
    if !documented {
        return serde_json::from_value(value.clone()).map_err(|e| e.to_string());
    }
    let mut db = LegacyDb::new();
    for (ty, entry) in map {
        let Some(conns) = entry.get("connectors").and_then(|c| c.as_array()) else {
            continue;
        };
        let observed: Vec<&String> = obs.get(ty).map(|p| p.keys().collect()).unwrap_or_default();
        let mut l = LegacyType::default();
        for c in conns {
            let Some(name) = c.get("name").and_then(|n| n.as_str()) else {
                continue;
            };
            let dir = match c.get("type").and_then(|t| t.as_str()) {
                Some("Output") => "O",
                Some("Parameter") => "P",
                _ => "I",
            };
            let keys: Vec<String> = match name.split_once("%d") {
                Some((pre, post)) => {
                    let hits: Vec<String> = observed
                        .iter()
                        .filter(|k| {
                            k.strip_prefix(pre)
                                .and_then(|r| r.strip_suffix(post))
                                .is_some_and(|d| {
                                    !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())
                                })
                        })
                        .map(|k| k.to_string())
                        .collect();
                    if hits.is_empty() {
                        vec![name.to_string()]
                    } else {
                        hits
                    }
                }
                None => vec![name.to_string()],
            };
            for k in keys {
                if !l.t.contains_key(&k) {
                    l.c.push(k.clone());
                    l.t.insert(k, dir.to_string());
                }
            }
        }
        db.insert(ty.clone(), l);
    }
    Ok(db)
}

/// Comparison of corpus evidence against one legacy entry, for one type.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TypeCheck {
    /// Ports whose inferred direction matches the legacy direction.
    pub dir_agreements: u32,
    /// `"key: corpus=Input legacy=O"` — evidence contradicts the legacy db.
    pub dir_conflicts: Vec<String>,
    /// Keys seen in real configs but absent from the legacy db.
    pub only_in_corpus: Vec<String>,
    /// Keys the legacy db lists but no config ever materialized.
    pub only_in_legacy: Vec<String>,
    /// Set when the legacy `c` order disagrees with the observed
    /// connector-index order (compared over the keys both sides know).
    pub order_mismatch: Option<String>,
}

/// Cross-check corpus observations against a legacy database, reporting per
/// type present in **both**. Types known to only one side are not errors —
/// enumerate them with set operations on the two maps.
pub fn crosscheck(obs: &Observations, legacy: &LegacyDb) -> BTreeMap<String, TypeCheck> {
    let mut out = BTreeMap::new();
    for (ty, ports) in obs {
        let Some(l) = legacy.get(ty) else { continue };
        let mut c = TypeCheck::default();

        for (key, o) in ports {
            match (o.inferred_dir(), l.t.get(key).map(String::as_str)) {
                (_, None) => c.only_in_corpus.push(key.clone()),
                (None, Some(_)) => {} // no direction evidence — nothing to compare
                (Some(dir), Some(ldir)) => {
                    // Directions compare on the input/output axis: params
                    // are input-like on both sides (corpus `Param` = Def
                    // evidence; legacy `"P"` = connector-map's param flag).
                    let ours = match dir {
                        PortDir::Output => "O",
                        PortDir::Input | PortDir::Param => "I",
                        // Inference never yields Api (bidirectional is a
                        // per-type judgment, not a corpus rule); nothing
                        // to compare on the input/output axis.
                        PortDir::Api => continue,
                    };
                    let ldir = if ldir == "P" { "I" } else { ldir };
                    if ours == ldir {
                        c.dir_agreements += 1;
                    } else {
                        c.dir_conflicts
                            .push(format!("{key}: corpus={dir:?} legacy={ldir}"));
                    }
                }
            }
        }
        for key in l.t.keys() {
            if !ports.contains_key(key) {
                c.only_in_legacy.push(key.clone());
            }
        }

        // Order: observed keys sorted by connector index, filtered to the
        // keys the legacy `c` list also names, must appear in the same order.
        let mut indexed: Vec<(&String, u8)> = ports
            .iter()
            .filter_map(|(k, o)| o.index.map(|i| (k, i)))
            .collect();
        indexed.sort_by_key(|&(_, i)| i);
        let observed_order: Vec<&String> = indexed
            .iter()
            .map(|&(k, _)| k)
            .filter(|k| l.c.contains(k))
            .collect();
        let legacy_order: Vec<&String> = l.c.iter().filter(|k| ports.contains_key(*k)).collect();
        if observed_order != legacy_order {
            c.order_mismatch = Some(format!(
                "observed {observed_order:?} vs legacy {legacy_order:?}"
            ));
        }

        out.insert(ty.clone(), c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_link_types_name_one_type_per_code() {
        let mut types = std::collections::BTreeSet::new();
        let mut codes = std::collections::BTreeSet::new();
        for &(t, code, _) in REF_LINK_TYPES {
            assert!(types.insert(t), "{t} listed twice");
            assert!(codes.insert(code), "LinkRefType {code} listed twice");
        }
        assert_eq!(ref_link_type("ApiActor"), Some((136, RefValue::Text)));
        assert_eq!(ref_link_type("Memory"), Some((320, RefValue::Digital)));
        assert_eq!(ref_link_type("And"), None);
    }

    #[test]
    fn builtin_shapes() {
        let and = builtin("And").unwrap();
        assert_eq!(and.len(), 3);
        assert_eq!(and[2].key, "Q");
        assert_eq!(and[2].dir, PortDir::Output);
        // AutoJalousie: 49 connectors in the uniform house element order;
        // OutputAPI is the first bidirectional (Api) port.
        let aj = builtin("AutoJalousie").unwrap();
        assert_eq!(aj.len(), 49);
        assert_eq!((aj[0].key, aj[0].dir), ("InputTrigger", PortDir::Input));
        assert_eq!((aj[6].key, aj[6].dir), ("AutoShade", PortDir::Input));
        assert_eq!((aj[17].key, aj[17].dir), ("Dir", PortDir::Param));
        assert_eq!((aj[39].key, aj[39].dir), ("OutputUp", PortDir::Output));
        assert_eq!((aj[47].key, aj[47].dir), ("TargetPos", PortDir::Output));
        assert_eq!((aj[48].key, aj[48].dir), ("OutputAPI", PortDir::Api));
        // Corpus-consolidated entries: index order matches the observed
        // connector indexes (docs/connector-db.md).
        let formula = builtin("Formula").unwrap();
        assert_eq!(formula.len(), 6);
        assert_eq!((formula[0].key, formula[0].dir), ("Input1", PortDir::Input));
        assert_eq!((formula[4].key, formula[4].dir), ("AQ", PortDir::Output));
        assert_eq!((formula[5].key, formula[5].dir), ("TQ", PortDir::Output));
        let mono = builtin("Monoflop").unwrap();
        assert_eq!((mono[1].key, mono[1].dir), ("Reset", PortDir::Input));
        assert_eq!((mono[3].key, mono[3].dir), ("Time", PortDir::Param));
        assert_eq!((mono[4].key, mono[4].dir), ("Q", PortDir::Output));
        assert_eq!(builtin("Less").unwrap().len(), 3);
        assert_eq!(builtin("AnalogThresholdTrigger").unwrap().len(), 8);
        // Admitted 2026-08-25 from the web-grown corpus: 16 uniform
        // modern instances; OutputAPI inert-flag Input.
        let pa = builtin("PulseAt").unwrap();
        assert_eq!(pa.len(), 5);
        assert_eq!((pa[2].key, pa[2].dir), ("Time", PortDir::Param));
        assert_eq!((pa[3].key, pa[3].dir), ("Q", PortDir::Output));
        assert_eq!((pa[4].key, pa[4].dir), ("OutputAPI", PortDir::Input));
        // DayTimer: the V259+ element order (PulseTime moved to index 3
        // by a schema migration that kept its port UUID).
        // The timer/edge/state batch (oracle-decided descriptors).
        let stair = builtin("StairwayLS").unwrap();
        assert_eq!(stair.len(), 10);
        assert_eq!((stair[1].key, stair[1].dir), ("On", PortDir::Param));
        assert_eq!((stair[2].key, stair[2].dir), ("Reset", PortDir::Input));
        assert_eq!((stair[9].key, stair[9].dir), ("OutputAPI", PortDir::Output));
        assert_eq!(builtin("OnDelay"), builtin("OffDelay"));
        assert_eq!(builtin("OnPulseDelay").unwrap().len(), 6);
        let mux = builtin("AnalogMultiplexer2").unwrap();
        assert_eq!((mux[0].key, mux[4].key), ("Select", "AQ"));
        let edge = builtin("EdgeDetection").unwrap();
        assert_eq!((edge[1].key, edge[1].dir), ("PulseTime", PortDir::Param));
        assert_eq!(builtin("SRFlipFlop").unwrap()[0].key, "InputS");
        let state = builtin("State").unwrap();
        assert_eq!(state.len(), 11);
        assert_eq!((state[9].key, state[9].dir), ("AQ", PortDir::Output));
        let dt = builtin("DayTimer").unwrap();
        assert_eq!(dt.len(), 13);
        assert_eq!((dt[3].key, dt[3].dir), ("PulseTime", PortDir::Param));
        assert_eq!((dt[7].key, dt[7].dir), ("AQ", PortDir::Output));
        assert_eq!((dt[11].key, dt[11].dir), ("AQmt", PortDir::Output));
        assert_eq!((dt[12].key, dt[12].dir), ("OutputAPI", PortDir::Input));
        // Mint-oracle verified 2026-08-25 (docs/oracle-wine.md): minted
        // minimal instances survived a GUI open+save; Memory's never-wired
        // `Q` proved an output by a compiled wire sourced from it surviving
        // the save (off-descriptor wires get deleted, D8).
        let mem = builtin("Memory").unwrap();
        assert_eq!(mem.len(), 3);
        assert_eq!((mem[1].key, mem[1].dir), ("AQ", PortDir::Output));
        assert_eq!((mem[2].key, mem[2].dir), ("Q", PortDir::Output));
        let pb = builtin("PushButton").unwrap();
        assert_eq!(pb.len(), 9);
        // Flipped Output -> Api by the 2026-08-25 web corpus (2 sink /
        // 2 source wires).
        assert_eq!((pb[8].key, pb[8].dir), ("OutputAPI", PortDir::Api));
        let pbt = builtin("PButtonT").unwrap();
        assert_eq!(pbt.len(), 7);
        assert_eq!((pbt[4].key, pbt[4].dir), ("Time", PortDir::Param));
        // Zero evidence anywhere -> inert-flag Input (corrected from the
        // unsupported Output name-prior).
        assert_eq!((pbt[6].key, pbt[6].dir), ("OutputAPI", PortDir::Input));
        // The 2026-08-25 batch (LightController2, Switch2Button,
        // CentralShade, CentralLight, Code16): modern house-V273 element
        // orders, directions per docs/connector-db.md.
        let lc = builtin("LightController2").unwrap();
        assert_eq!(lc.len(), 75);
        assert_eq!((lc[0].key, lc[0].dir), ("I1", PortDir::Input));
        // I5–I8 are the V273 insertion into the V259 order.
        assert_eq!((lc[4].key, lc[4].dir), ("I5", PortDir::Input));
        assert_eq!((lc[12].key, lc[12].dir), ("Sel1", PortDir::Input));
        assert_eq!(
            (lc[30].key, lc[30].dir),
            ("BrightnessLimit", PortDir::Param)
        );
        // Def= on every instance but also a corpus sink wire — wire
        // evidence precedes, Input still admits the Def.
        assert_eq!((lc[32].key, lc[32].dir), ("MoveOn", PortDir::Input));
        assert_eq!((lc[51].key, lc[51].dir), ("AQ1", PortDir::Output));
        assert_eq!((lc[72].key, lc[72].dir), ("OutputReset", PortDir::Output));
        assert_eq!((lc[74].key, lc[74].dir), ("OutputAPI", PortDir::Input));
        let s2 = builtin("Switch2Button").unwrap();
        assert_eq!(s2.len(), 9);
        // Modern order: On before Reset (swapped relative to V74–162).
        assert_eq!((s2[1].key, s2[1].dir), ("On", PortDir::Input));
        assert_eq!((s2[2].key, s2[2].dir), ("Reset", PortDir::Input));
        assert_eq!((s2[5].key, s2[5].dir), ("Q", PortDir::Output));
        assert_eq!((s2[8].key, s2[8].dir), ("OutputAPI", PortDir::Input));
        let cs = builtin("CentralShade").unwrap();
        assert_eq!(cs.len(), 18);
        assert_eq!((cs[3].key, cs[3].dir), ("EndUp", PortDir::Input));
        // Appended keys: inert-flag Input at admission, Output since
        // oracle session 17 (wires drawn from them survive the save).
        assert_eq!((cs[15].key, cs[15].dir), ("OutOpen", PortDir::Output));
        assert_eq!((cs[17].key, cs[17].dir), ("OutputAPI", PortDir::Output));
        let cl = builtin("CentralLight").unwrap();
        assert_eq!(cl.len(), 24);
        assert_eq!((cl[18].key, cl[18].dir), ("Alarm", PortDir::Input));
        assert_eq!((cl[22].key, cl[22].dir), ("OutActive", PortDir::Output));
        assert_eq!((cl[23].key, cl[23].dir), ("OutputAPI", PortDir::Output));
        let c16 = builtin("Code16").unwrap();
        assert_eq!(c16.len(), 34);
        assert_eq!((c16[3].key, c16[3].dir), ("AI1", PortDir::Input));
        assert_eq!((c16[17].key, c16[17].dir), ("TQ1", PortDir::Output));
        assert_eq!((c16[33].key, c16[33].dir), ("TeQ", PortDir::Output));
        for t in BUILTIN_TYPES {
            assert!(builtin(t).is_some(), "BUILTIN_TYPES lists `{t}`");
        }
    }

    fn port(index: Option<u8>, sink: u32, source: u32, def: u32) -> ObservedPort {
        ObservedPort {
            index,
            index_conflict: false,
            wired_as_sink: sink,
            wired_as_source: source,
            has_def: def,
            occurrences: 1,
        }
    }

    fn obs(ty: &str, ports: &[(&str, ObservedPort)]) -> Observations {
        let mut o = Observations::new();
        let m = o.entry(ty.into()).or_default();
        for (k, p) in ports {
            m.insert((*k).into(), p.clone());
        }
        o
    }

    #[test]
    fn merging() {
        let mut a = obs("T", &[("Q", port(Some(2), 0, 1, 0))]);
        merge(&mut a, obs("T", &[("Q", port(Some(2), 0, 3, 0))]));
        let q = &a["T"]["Q"];
        assert_eq!((q.index, q.wired_as_source, q.occurrences), (Some(2), 4, 2));
        assert!(!q.index_conflict);

        // Conflicting index poisons permanently, even against later agreement.
        merge(&mut a, obs("T", &[("Q", port(Some(5), 0, 1, 0))]));
        assert!(a["T"]["Q"].index_conflict);
        assert_eq!(a["T"]["Q"].index, None);
        merge(&mut a, obs("T", &[("Q", port(Some(2), 0, 1, 0))]));
        assert_eq!(a["T"]["Q"].index, None);

        // New types/keys appear.
        merge(&mut a, obs("U", &[("I", port(Some(0), 1, 0, 0))]));
        assert_eq!(a["U"]["I"].index, Some(0));
    }

    #[test]
    fn crosschecking() {
        let o = obs(
            "And",
            &[
                ("I1", port(Some(0), 2, 0, 0)),
                ("I2", port(Some(1), 1, 0, 1)),
                ("Q", port(Some(2), 0, 3, 0)),
                ("Mystery", port(Some(3), 0, 0, 2)), // Param, unknown to legacy
            ],
        );
        let legacy: LegacyDb = serde_json::from_str(
            r#"{"And": {"c": ["I1","I2","Q"],
                        "t": {"I1":"I","I2":"I","Q":"I","Ghost":"O"}},
                "OnlyLegacy": {"c": [], "t": {}}}"#,
        )
        .unwrap();
        let checks = crosscheck(&o, &legacy);
        let c = &checks["And"];
        assert_eq!(c.dir_agreements, 2, "I1, I2");
        assert_eq!(c.dir_conflicts, ["Q: corpus=Output legacy=I"]);
        assert_eq!(c.only_in_corpus, ["Mystery"]);
        assert_eq!(c.only_in_legacy, ["Ghost"]);
        assert!(c.order_mismatch.is_none());
        assert!(!checks.contains_key("OnlyLegacy"), "intersection only");
    }

    #[test]
    fn inference() {
        let mut o = ObservedPort::default();
        assert_eq!(o.inferred_dir(), None);
        o.has_def = 1;
        assert_eq!(o.inferred_dir(), Some(PortDir::Param));
        o.wired_as_source = 2;
        assert_eq!(o.inferred_dir(), Some(PortDir::Output));
        o.wired_as_sink = 3;
        assert_eq!(o.inferred_dir(), Some(PortDir::Input));
    }

    #[test]
    fn documented_shape_folds_into_a_legacy_entry() {
        let port = |sink, source| ObservedPort {
            index: None,
            index_conflict: false,
            wired_as_sink: sink,
            wired_as_source: source,
            has_def: 0,
            occurrences: 1,
        };
        let mut obs = Observations::new();
        let ports = obs.entry("Mux".into()).or_default();
        ports.insert("I1".into(), port(1, 0));
        ports.insert("I2".into(), port(1, 0));
        ports.insert("Q".into(), port(0, 1));
        let doc = serde_json::json!({ "Mux": { "connectors": [
            { "name": "I%d", "type": "Input" },
            { "name": "T", "type": "Parameter" },
            { "name": "Q", "type": "Output" },
            { "name": "AQ%d", "type": "Output" },
        ]}});
        let db = legacy_db_from_json(&doc, &obs).unwrap();
        assert_eq!(db["Mux"].c, ["I1", "I2", "T", "Q", "AQ%d"]);
        assert_eq!(db["Mux"].t["T"], "P");
        let check = &crosscheck(&obs, &db)["Mux"];
        assert_eq!(check.dir_agreements, 3);
        assert_eq!(check.only_in_legacy, ["AQ%d", "T"]);

        // The connector-map shape still reads as before.
        let map = serde_json::json!({ "Mux": { "c": ["I1"], "t": { "I1": "I" } } });
        assert_eq!(legacy_db_from_json(&map, &obs).unwrap()["Mux"].c, ["I1"]);
    }
}
