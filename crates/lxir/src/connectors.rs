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
// contradiction; OutOpen/OutClose zero-evidence → inert-flag Input.
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
    p("OutOpen", Input),
    p("OutClose", Input),
    p("OutputAPI", Output),
];

// CentralLight: 9 occurrences (2 modern house instances; V254–259 corpus
// trio lacks the appended OutActive). Reset/On/Alarm corpus-sink; the
// rest connector-map I with zero contradiction; OutputAPI connector-map
// O; OutActive zero-evidence → inert-flag Input.
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
    p("OutActive", Input),
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

/// `LinkRefType=` code and `Analog=` flag a ref carries for a mirrored
/// object of the given XML type — Loxone Config's internal type-registry
/// number, learned from the house corpus (every code below observed on
/// real refs, deterministic per target type). A type not in this table
/// cannot be mirrored by a minted ref yet: extend the table from
/// corpus evidence rather than guessing the code.
///
/// `Memory` is the one target the corpus shows with *both* flags
/// (8× digital, 1× analog); minted mirrors of blocks default to digital
/// until evidence for a rule emerges.
pub fn ref_link_type(target_type: &str) -> Option<(u32, bool)> {
    Some(match target_type {
        "Online" => (2, false),
        "DigitalIn" => (55, false),
        "Actor" => (63, false),
        "VirtualIn" => (71, false),
        "VirtualUdpInCmd" => (79, false),
        "VirtualTextIn" => (83, false),
        "VirtualHttpInCmd" => (87, true),
        "Lox1wireAsensor" => (103, true),
        "OvertempShutdown" => (109, false),
        "ApiActor" => (136, false),
        "ModbusASensor" => (153, true),
        "ModbusAActor" => (156, true),
        "LoxAIRsensor" => (172, false),
        "LoxAIRAsensor" => (173, true),
        "LoxAIRactor" => (174, false),
        "LoxAIRAactor" => (175, true),
        "TreeSensor" => (182, false),
        "TreeAsensor" => (183, true),
        "TreeActor" => (184, false),
        "TreeAactor" => (185, true),
        "Daylight" => (281, false),
        "Memory" => (320, false),
        _ => return None,
    })
}

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
];

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
        "InputRef" => Some(INPUT_REF),
        "OutputRef" => Some(OUTPUT_REF),
        _ => None,
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
#[derive(Debug, Clone, serde::Deserialize)]
pub struct LegacyType {
    #[serde(default)]
    pub c: Vec<String>,
    #[serde(default)]
    pub t: BTreeMap<String, String>,
}

/// Block type → legacy entry.
pub type LegacyDb = BTreeMap<String, LegacyType>;

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
        // Zero-evidence appended keys -> inert-flag Input.
        assert_eq!((cs[15].key, cs[15].dir), ("OutOpen", PortDir::Input));
        assert_eq!((cs[17].key, cs[17].dir), ("OutputAPI", PortDir::Output));
        let cl = builtin("CentralLight").unwrap();
        assert_eq!(cl.len(), 24);
        assert_eq!((cl[18].key, cl[18].dir), ("Alarm", PortDir::Input));
        assert_eq!((cl[22].key, cl[22].dir), ("OutActive", PortDir::Input));
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
}
