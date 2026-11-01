//! What each port of a builtin block is for: one line per port, for
//! `loxl types` (and its JSON) and the LSP hover and completion.
//!
//! Source: the type's page in Loxone's knowledge base (reference
//! `kb-block-pages` in docs/references.json, the page `blockdoc::kb_page`
//! names). The page lists every input, output and parameter by its
//! abbreviation (`Tr`, `Lc1-18`) with a summary and a description. Each
//! entry here gives the port's abbreviation on that page and a line written
//! in our words from it. The key maps to the abbreviation by the canvas
//! label (`port_meta`), else by the label in Loxone's TechDoc (reference
//! `lox-cli-block-types`), else where the key spells it (`Lr`, `Mro`, `RtD`,
//! `OPf` for `Pf`, `Remanence` for `Rem`, `OutputAPI` for `API`), else by
//! the mapping the simulator models from the same page (`Reset` for `Off`,
//! docs/simulator.md).
//! `tools/check-port-docs.py` checks every abbreviation against the pages.
//!
//! A port the page does not document, or whose abbreviation cannot be tied
//! to its key, has no entry and no description.

/// A port key, the abbreviation on the knowledge-base page, the line.
///
/// A numbered family is one entry: `I1-8` with `Lc1-8` covers `I1`…`I8`,
/// and `#` in the line stands for the number.
type Entry = (&'static str, &'static str, &'static str);

const REM: &str = "Remanence: when on, the block keeps its state across a Miniserver reboot.";
const API: &str = "API connector: links the block to devices and other blocks.";
const OFF: &str = "Off: a short pulse switches the output off, a longer one locks the block.";
const RTD: &str = "Reset to default: resets the block's parameters and settings to its preset.";

/// The documented meaning of `port` on a builtin `block_type`, one line.
pub fn describe(block_type: &str, port: &str) -> Option<String> {
    entries(block_type)?.iter().find_map(|(key, _, text)| {
        let n = matches(key, port)?;
        Some(match n {
            Some(n) => text.replace('#', &n.to_string()),
            None => text.to_string(),
        })
    })
}

/// `Some(None)` for an exact key, `Some(Some(n))` for member `n` of a
/// family, `None` when `key` does not cover `port`.
fn matches(key: &str, port: &str) -> Option<Option<u32>> {
    if key == port {
        return Some(None);
    }
    let (prefix, range) = family(key)?;
    let n: u32 = port.strip_prefix(prefix)?.parse().ok()?;
    range.contains(&n).then_some(Some(n))
}

/// `I1-8` → (`I`, 1..=8).
fn family(key: &str) -> Option<(&str, std::ops::RangeInclusive<u32>)> {
    let (head, hi) = key.split_once('-')?;
    let digits = head.len() - head.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (prefix, lo) = head.split_at(head.len() - digits);
    Some((prefix, lo.parse().ok()?..=hi.parse().ok()?))
}

/// Every entry of a type, for the checks.
pub(crate) fn entries(block_type: &str) -> Option<&'static [Entry]> {
    Some(match block_type {
        "And" => &[
            (
                "I1",
                "I1",
                "Input 1: Q is on while every connected input is on.",
            ),
            (
                "I2",
                "I2",
                "Input 2: Q is on while every connected input is on.",
            ),
            ("Q", "O", "Output: on while all connected inputs are on."),
        ],
        "Or" => &[
            ("I1", "I1", "Input 1: Q is on while any input is on."),
            ("I2", "I2", "Input 2: Q is on while any input is on."),
            ("Q", "O", "Output: on while any input is on."),
        ],
        "Not" => &[
            ("I", "I", "Input."),
            ("Q", "O", "Output: the inverse of the input."),
        ],
        "Equal" => &[
            ("Input1", "V1", "Value 1, compared with value 2."),
            ("Input2", "V2", "Value 2, compared with value 1."),
            ("Q", "E", "On while value 1 equals value 2."),
        ],
        "NotEqual" => &[
            ("Input1", "V1", "Value 1, compared with value 2."),
            ("Input2", "V2", "Value 2, compared with value 1."),
            ("Q", "U", "On while value 1 differs from value 2."),
        ],
        "GreaterEqual" => &[
            ("Input1", "V1", "Value 1, compared with value 2."),
            ("Input2", "V2", "Value 2, compared with value 1."),
            (
                "Q",
                "Ge",
                "On while value 1 is greater than or equal to value 2.",
            ),
        ],
        "LessEqual" => &[
            ("Input1", "V1", "Value 1, compared with value 2."),
            ("Input2", "V2", "Value 2, compared with value 1."),
            (
                "Q",
                "Le",
                "On while value 1 is less than or equal to value 2.",
            ),
        ],
        "Formula" => &[
            (
                "Input1-4",
                "I1-4",
                "Value #, used in the formula as I#; a constant when unwired.",
            ),
            ("AQ", "R", "Result of the formula."),
            (
                "TQ",
                "E",
                "Error, such as an illegal mathematical operation.",
            ),
        ],
        "Monoflop" => &[
            (
                "InputTrigger",
                "Tr",
                "Trigger: a pulse switches Q on for Time.",
            ),
            ("Reset", "Off", OFF),
            ("Remanence", "Rem", REM),
            ("Time", "D", "Duration of the output pulse."),
            ("Q", "O", "Output."),
        ],
        "PulseGen" => &[
            ("InputInvert", "Inv", "Invert: inverts the pulse output."),
            ("Remanence", "Rem", REM),
            ("TimeHigh", "Don", "Duration on of each pulse."),
            ("TimeLow", "Doff", "Duration off between pulses."),
            ("Q", "P", "Pulse output."),
        ],
        "AnalogThresholdTrigger" => &[
            ("Input", "V", "Value compared with the thresholds."),
            ("Remanence", "Rem", REM),
            ("On", "Von", "Value at which the output turns on."),
            ("Off", "Voff", "Value at which the output turns off."),
            ("PulseTime", "Pd", "Pulse duration of the edge outputs."),
            ("Q", "O", "Output."),
            ("RisingEdge", "On", "Pulse when the output turns on."),
            ("FallingEdge", "Off", "Pulse when the output turns off."),
        ],
        "PushButton" => &[
            (
                "InputTrigger",
                "Tg",
                "Toggle: switches the output on or off.",
            ),
            ("On", "On", "Switches the output on."),
            ("Reset", "Off", OFF),
            (
                "InputDisable",
                "DisPc",
                "Disable periphery control: locks every input; the app still works.",
            ),
            ("Remanence", "Rem", REM),
            ("Q", "O", "Output."),
            ("Qon", "On", "Pulse when the output switches on."),
            ("Qoff", "Off", "Pulse when the output switches off."),
            ("OutputAPI", "API", API),
        ],
        "PButtonT" => &[
            (
                "InputTrigger",
                "Tr",
                "Trigger: switches Q on for Time; another pulse extends it.",
            ),
            ("Reset", "Off", OFF),
            (
                "InputDisable",
                "DisPc",
                "Disable periphery control: locks InputTrigger and Reset; the app still works.",
            ),
            ("Remanence", "Rem", REM),
            (
                "Time",
                "Don",
                "On-duration of the output; 0 keeps it on while the button is held.",
            ),
            ("Q", "O", "Output: on for Time."),
            ("OutputAPI", "API", API),
        ],
        "AutoJalousie" => AUTO_JALOUSIE,
        "PulseAt" => &[
            (
                "InputDisable",
                "Off",
                "Off: a pulse resets the output, held on it locks the block.",
            ),
            ("Remanence", "Rem", REM),
            ("Time", "Don", "On-duration of the output."),
            ("Q", "O", "Output: on at the set time, for Time."),
            ("OutputAPI", "API", API),
        ],
        "DayTimer" => &[
            (
                "InputTrigger",
                "Act",
                "Activate: starts an entry that needs activation, while it is active.",
            ),
            ("RtD", "Rtd", RTD),
            (
                "PulseTime",
                "Don",
                "On-duration of the output for entries that need activation; 0 = no limit.",
            ),
            ("Remanence", "Rem", REM),
            (
                "Manual",
                "Am",
                "Automatic mode: 0 = automatic, 1 = manual through Mode.",
            ),
            ("Mode", "Mm", "Manual mode: the operating mode set by hand."),
            (
                "AQ",
                "O",
                "Output: 0 or 1, or the analog value of the active entry.",
            ),
            ("Qon", "On", "Pulse when the output switches on."),
            ("Qoff", "Off", "Pulse when the output switches off."),
            ("AQm", "Om", "Number of the active operating mode."),
            (
                "AQmt",
                "Rt",
                "Remaining time of a timer started in the app.",
            ),
            ("OutputAPI", "API", API),
        ],
        "LightController2" => LIGHT_CONTROLLER2,
        "Switch2Button" => &[
            (
                "InputTrigger",
                "Tr",
                "Trigger: a pulse switches the output on for Time.",
            ),
            ("On", "On", "Switches the output on."),
            (
                "Reset",
                "Off",
                "Off: switches the output off; held on it locks the block.",
            ),
            (
                "InputDisable",
                "DisPc",
                "Disable periphery control: locks InputTrigger, Reset and On.",
            ),
            ("OutputAPI", "API", API),
        ],
        "CentralShade" => CENTRAL_SHADE,
        "CentralLight" => &[
            (
                "On",
                "On",
                "All on: activates mood 99, or every light at full alarm brightness.",
            ),
            ("Alarm", "Alarm", "Alarm: every light flashes while on."),
            ("RtD", "Rtd", RTD),
            ("OutActive", "Na", "Number of active lights."),
            ("OutputAPI", "API", API),
        ],
        "Code16" => &[
            ("TI1-3", "T1-3", "Text input #."),
            ("AI1-13", "I1-13", "Input #."),
            ("Remanence", "Rem", REM),
            ("TQ1-3", "Txt1-3", "Text output #, at most 4096 bytes."),
            ("AQ1-13", "O1-13", "Output #."),
            ("TeQ", "Etxt", "Error text of the program."),
        ],
        "OnPulseDelay" => &[
            ("InputTrigger", "P", "Pulse: starts the delay."),
            ("Remanence", "Rem", REM),
            (
                "Delay",
                "Dd",
                "Duration of the delay before the output pulse.",
            ),
            ("Time", "Dp", "Duration of the output pulse."),
            ("Q", "P", "Pulse output, after the delay."),
        ],
        "OnDelay" => &[
            (
                "InputTrigger",
                "Tr",
                "Trigger: the output follows it, switching on after the delay.",
            ),
            ("Reset", "Off", OFF),
            ("Remanence", "Rem", REM),
            (
                "Time",
                "Don",
                "Delay before the output switches on while the input is on.",
            ),
            ("Q", "O", "Output."),
        ],
        "StairwayLS" => &[
            ("InputTrigger", "Tr", "Trigger: switches Q on for TimeHigh."),
            ("On", "On", "Switches the output on."),
            ("Remanence", "Rem", REM),
            (
                "TimeHigh",
                "Don",
                "On-duration of Q when triggered by InputTrigger.",
            ),
            (
                "TimeWarn",
                "Tw",
                "Switch-off warning: how long before switching off it warns.",
            ),
            ("WarnTime", "Dw", "Switch-off warning duration."),
            ("Q", "O", "Output."),
            ("OutputAPI", "API", API),
        ],
        "EdgeDetection" => &[
            ("Input", "I", "Input whose edges are detected."),
            (
                "PulseTime",
                "Pd",
                "Pulse duration of the outputs when an edge is detected.",
            ),
            ("Edge", "P", "Pulse at every edge."),
            ("RisingEdge", "On", "Pulse at a rising edge."),
            ("FallingEdge", "Off", "Pulse at a falling edge."),
        ],
        "AnalogMultiplexer2" => &[
            ("Select", "Sel", "Select: which value goes to the output."),
            ("Input1", "V1", "Value 1."),
            ("Input2", "V2", "Value 2."),
            ("AQ", "V", "Value: the selected input."),
        ],
        "SRFlipFlop" => &[
            (
                "InputS",
                "S",
                "Set: a pulse switches the output on; it wins over Reset.",
            ),
            ("InputTrigger", "Tg", "Toggle: a pulse toggles the output."),
            ("InputR", "R", "Reset: a pulse switches the output off."),
            ("Remanence", "Rem", REM),
            ("Q", "O", "Output."),
        ],
        "State" => &[
            ("I1-8", "I1-8", "Input #, used by the status conditions."),
            (
                "TQ",
                "Txt",
                "Current status text, from the conditions set in the block.",
            ),
            (
                "AQ",
                "Val",
                "Current status value, from the conditions set in the block.",
            ),
            ("OutputAPI", "API", API),
        ],
        "HeatIRoomController2" => HEAT_I_ROOM_CONTROLLER2,
        "Irrigation" => &[
            (
                "A",
                "Act",
                "Activation: starts irrigation unless enough rain fell or is forecast.",
            ),
            (
                "AIv",
                "Sel",
                "Select valve: 1–8 turns on that valve, 0 all off, 9 all on.",
            ),
            (
                "Rf",
                "Raf",
                "Rain forecast: expected precipitation in the next hours.",
            ),
            (
                "Ra",
                "Ra",
                "Rain: a rain sensor, used to measure how long it rained.",
            ),
            (
                "MaxP",
                "MaxR",
                "Maximum forecast precipitation above which A does not start.",
            ),
            (
                "MaxR",
                "MaxRa",
                "Maximum rain duration in the last 24 hours above which A does not start.",
            ),
            (
                "Tv1-8",
                "Tv1-8",
                "Valve time #: how long valve # runs before the next one.",
            ),
            ("Qp", "P", "Pump output."),
            ("Qv1-8", "V1-8", "Valve # output."),
            ("AQv", "Av", "Active valve: 0 all off, 9 all on."),
            ("OutputAPI", "API", API),
        ],
        "Alarm" => ALARM,
        "SmokeAlarm" => SMOKE_ALARM,
        "EFM" => &[
            ("Pre", "Pre", "Price per kWh exported."),
            ("Pri", "Pri", "Price per kWh imported."),
            ("CO2", "CO2", "Kilograms of CO2 saved per kWh produced."),
            ("OGpwr", "Gpwr", "Grid power."),
            ("OPpwr", "Ppwr", "Production power."),
            ("OCpwr", "Cpwr", "Consumption power."),
            ("OSpwr", "Spwr", "Energy storage power."),
            ("OEd", "Ed", "Energy exported today."),
            ("OId", "Id", "Energy imported today."),
            ("OPd", "Pd", "Energy produced today."),
            ("OCd", "Cd", "Energy consumed today."),
            ("OScd", "Scd", "Self-consumption today."),
            ("OCo2d", "Co2d", "CO2 saved today: OPd × CO2."),
            (
                "OYd",
                "Yd",
                "Yield today: production used at the import price plus export at the export price.",
            ),
            ("Rest", "Rest", "Remaining power of the top group."),
            ("OutputAPI", "API", API),
        ],
        "TpfController" => TPF_CONTROLLER,
        "NfcCodeTouch" => NFC_CODE_TOUCH,
        "Intercom" => &[
            (
                "Ib",
                "Bell",
                "Activate bell: rings the bell output and the assigned audio players.",
            ),
            (
                "Mute",
                "Mute",
                "Mute bell: silences the bell output and audio players while on.",
            ),
            (
                "TTS",
                "TTS",
                "Text to speech: a message played over the intercom speakers.",
            ),
            ("Lon", "Bbl", "Turns the bell button light on."),
            (
                "Timeout",
                "maxB",
                "Maximum bell duration of the Qb and Qmute outputs.",
            ),
            ("Lbr", "Bbr", "Brightness of the bell button light."),
            ("Tp", "Qon", "On-duration of the custom outputs Q1–Q3."),
            ("Qb", "Bell", "Bell output, for a doorbell."),
            (
                "Qmute",
                "Mute",
                "Bell muted: on when the bell rings while muted, for an alternative signal.",
            ),
            (
                "Q1-3",
                "O1-3",
                "Custom output #, named in the settings and triggered from the app.",
            ),
            ("API", "API", API),
        ],
        "MeterDig" => &[
            (
                "A",
                "S",
                "State: while 1, the meter runs at the power Ppwr.",
            ),
            (
                "R",
                "R",
                "Reset: a pulse resets the readings; held on it locks the block.",
            ),
            (
                "Ppwr",
                "Pf",
                "Power rating or nominal flow, used while A is on.",
            ),
            (
                "Mro",
                "Mro",
                "Meter reading offset, added to the reading OMr.",
            ),
            ("OPf", "Pf", "Current power or flow."),
            ("OMr", "Mr", METER_MR),
            ("ORd", "Rd", "Reading today."),
            ("ORld", "Rld", "Reading yesterday."),
            ("ORm", "Rm", "Reading this month."),
            ("ORlm", "Rlm", "Reading last month."),
            ("ORy", "Ry", "Reading this year."),
            ("ORly", "Rly", "Reading last year."),
            ("OutputAPI", "API", API),
        ],
        "MeterAbsUni" => &[
            (
                "Pf",
                "Pf",
                "Power or flow; alone, the reading is computed from it.",
            ),
            (
                "Mr",
                "Mr",
                "Meter reading, for meters that send their reading as a value.",
            ),
            (
                "R",
                "R",
                "Reset: a pulse resets the readings; held on it locks the block.",
            ),
            (
                "Mro",
                "Mro",
                "Meter reading offset, added to the reading OMr.",
            ),
            ("OPf", "Pf", "Current power or flow."),
            ("OMr", "Mr", METER_MR),
            ("ORd", "Rd", "Reading today."),
            ("ORld", "Rld", "Reading yesterday."),
            ("ORm", "Rm", "Reading this month."),
            ("ORlm", "Rlm", "Reading last month."),
            ("ORy", "Ry", "Reading this year."),
            ("ORly", "Rly", "Reading last year."),
            ("OutputAPI", "API", API),
        ],
        "MeterAbsBi" => &[
            (
                "Pf",
                "Pf",
                "Power or flow; alone, the readings are computed from it.",
            ),
            (
                "Mr1",
                "Mrc",
                "Meter reading of consumption, for meters that send their reading.",
            ),
            (
                "Mr2",
                "Mrd",
                "Meter reading of delivery, for meters that send their reading.",
            ),
            (
                "R",
                "R",
                "Reset: a pulse resets the readings; held on it locks the block.",
            ),
            ("OPf", "Pf", "Current power or flow."),
            ("OutputAPI", "API", API),
        ],
        "MeterAbsSt" => &[
            (
                "Pf",
                "Pf",
                "Power or flow; alone, the readings are computed from it.",
            ),
            ("Slvl", "Slvl", "Storage level or state of charge."),
            (
                "R",
                "R",
                "Reset: a pulse resets the readings; held on it locks the block.",
            ),
            ("OPf", "Pf", "Current power or flow."),
            ("OutputAPI", "API", API),
        ],
        _ => return None,
    })
}

const METER_MR: &str = "Meter reading.";

const AUTO_JALOUSIE: &[Entry] = &[
    (
        "InputTrigger",
        "Tg",
        "Toggle: open, stop, close in turn, for a single button.",
    ),
    (
        "InputTriggerUp",
        "Po",
        "Partial open while held; a long or double click opens fully.",
    ),
    (
        "InputTriggerDown",
        "Pc",
        "Partial close while held; a long or double click closes fully.",
    ),
    (
        "EndUp",
        "Co",
        "Complete open; stops when the shade is moving.",
    ),
    (
        "EndDown",
        "Cc",
        "Complete close; stops when the shade is moving.",
    ),
    (
        "Shade",
        "So",
        "Slightly open: closes, then sets the slats or position by Back.",
    ),
    ("AutoShade", "Sps", "Starts the sun position automatic."),
    (
        "EnAutoShade",
        "DisSp",
        "Disables the sun position automatic while on.",
    ),
    (
        "ReactAutoShade",
        "Spr",
        "Restarts the sun position automatic.",
    ),
    (
        "Safety",
        "Wa",
        "Wind alarm: moves to the wind alarm position SO and locks the block.",
    ),
    (
        "Window",
        "Dwc",
        "Door/window contact: open (1) opens the shade fully and locks it.",
    ),
    (
        "Stop",
        "Off",
        "Off: a short pulse stops, a longer one locks the block.",
    ),
    ("ManualPosition", "Pos", "Moves the shade to this position."),
    ("ManualLamelle", "Slat", "Moves the slats to this position."),
    (
        "Gesture",
        "T5",
        "T5 control: button 1 opens, button 4 closes.",
    ),
    (
        "InputDisable",
        "DisPc",
        "Disable periphery control: locks the push-button inputs; the app still works.",
    ),
    (
        "Type",
        "Type",
        "Shading type: 0 venetian blinds, 1 roller blinds, 2–5 curtains, 6 awning.",
    ),
    (
        "Dir",
        "Dir",
        "Compass direction of the window: 0 north, 90 east, 180 south; −1 not set.",
    ),
    ("TimeEnd", "Opd", "Opening duration."),
    ("TimeEndDown", "Cld", "Closing duration."),
    (
        "SO",
        "Wap",
        "Wind alarm position: 0 completely open, 1 completely closed.",
    ),
    (
        "AutoShadeEnd",
        "Spe",
        "End action of the sun position automatic: 0 none, 1 open, 2 close, 3 slats level.",
    ),
    (
        "AutMode",
        "Spm",
        "Sun position automatic mode, such as optimal brightness or cooling.",
    ),
    (
        "MinPulse",
        "Tlc",
        "Long-click time on InputTriggerUp/Down for a complete open or close.",
    ),
    (
        "DblClk",
        "Tdc",
        "Double-click time on InputTriggerUp/Down for a complete open or close; 0 = off.",
    ),
    (
        "DirTol",
        "Dts",
        "Direction tolerance when the sun enters the shading area.",
    ),
    (
        "DirTol2",
        "Dte",
        "Direction tolerance when the sun leaves the shading area.",
    ),
    (
        "AutoShadeTime",
        "Spi",
        "How often the slats may adjust during the sun position automatic.",
    ),
    (
        "SRoff",
        "Spos",
        "Start offset of the sun position automatic, relative to sunrise.",
    ),
    (
        "SSoff",
        "Spoe",
        "End offset of the sun position automatic, relative to sunset.",
    ),
    ("Width", "Sw", "Slat width."),
    ("Space", "Sd", "Distance between two slats."),
    (
        "Rdd",
        "Rdd",
        "Reference drive down: every close command drives the full closing duration.",
    ),
    (
        "TimeBlock",
        "Mld",
        "Motor lock duration between direction changes.",
    ),
    (
        "TurnOffset",
        "Bldo",
        "Backlash duration when moving in the opposite direction.",
    ),
    (
        "Deadtime",
        "Bld",
        "Backlash duration when moving in the same direction.",
    ),
    (
        "MinMove",
        "minTd",
        "Minimum travel time on a pulse at InputTriggerUp or InputTriggerDown.",
    ),
    (
        "Back",
        "Rd",
        "Return duration until the slats are level; for other shades the Shade position.",
    ),
    ("OutputUp", "Op", "Open: drives the motor up."),
    ("OutputDown", "Cl", "Close: drives the motor down."),
    (
        "OutputPos",
        "Pos",
        "Position of the shade: 0 open, 1 closed.",
    ),
    (
        "OutputLPos",
        "Slat",
        "Position of the slats: 0 horizontal, 1 vertical.",
    ),
    (
        "OutputAutoShade",
        "Sp",
        "On while the sun position automatic is active.",
    ),
    (
        "OutputSafety",
        "Wds",
        "On while the wind alarm or the door/window contact is active.",
    ),
    ("OutputLock", "Off", "On while Stop is on."),
    (
        "OutputCombined",
        "AQpp",
        "Command output for specific devices: command, position and slat angle.",
    ),
    ("TargetPos", "TPos", "Target position of the shade."),
    ("OutputAPI", "API", API),
];

const CENTRAL_SHADE: &[Entry] = &[
    (
        "InputTrigger",
        "Tg",
        "Toggle: open, stop, close in turn, for a single button.",
    ),
    ("InputTriggerUp", "Po", "Partial open while held."),
    ("InputTriggerDown", "Pc", "Partial close while held."),
    ("EndUp", "Co", "Complete open; cannot be stopped."),
    ("EndDown", "Cc", "Complete close; cannot be stopped."),
    (
        "Shade",
        "So",
        "Slightly open: closes, then sets the slats or position.",
    ),
    ("AutoShade", "Sps", "Starts the sun position automatic."),
    (
        "EnAutoShade",
        "DisSp",
        "Disables the sun position automatic while on.",
    ),
    (
        "ReactAutoShade",
        "Spr",
        "Restarts the sun position automatic.",
    ),
    (
        "Safety",
        "Wa",
        "Wind alarm: moves to the wind alarm position and locks the shades.",
    ),
    (
        "Stop",
        "Off",
        "Off: a short pulse stops, a longer one locks the block.",
    ),
    (
        "ManualPosition",
        "Pos",
        "Moves the shades to this position.",
    ),
    ("ManualLamelle", "Slat", "Moves the slats to this position."),
    (
        "Gesture",
        "T5",
        "T5 control: button 1 opens, button 4 closes.",
    ),
    (
        "InputDisable",
        "DisPc",
        "Disable periphery control: locks the push-button inputs; the app still works.",
    ),
    ("OutOpen", "No", "Number of open shades."),
    ("OutClose", "Nc", "Number of closed shades."),
    ("OutputAPI", "API", API),
];

const LIGHT_CONTROLLER2: &[Entry] = &[
    (
        "I1-8",
        "Lc1-8",
        "Light circuit #: a pulse switches it, a long click dims it.",
    ),
    (
        "InputTriggerUp",
        "M+",
        "Next mood; a double click switches off and pulses OutputReset.",
    ),
    (
        "InputTriggerDown",
        "M-",
        "Previous mood; a double click switches off and pulses OutputReset.",
    ),
    ("Select", "Mood", "Selects a mood by its ID."),
    (
        "Reset",
        "Off",
        "Off: a short pulse switches off, a longer one locks the block.",
    ),
    (
        "Sel1-8",
        "T5/1-8",
        "T5 control #: selects the assigned mood, further pulses the next ones.",
    ),
    ("EnMove", "DisP", "Disables Presence and Move while on."),
    (
        "Move",
        "Mo",
        "Motion: activates the presence mood; switches off MoveOn after it falls.",
    ),
    ("On", "On", "All on: activates mood 99, or every light."),
    ("Alarm", "Alarm", "Alarm: every light flashes while on."),
    (
        "AlarmClock",
        "Buzzer",
        "Buzzer: activates the alarm clock mood (98).",
    ),
    (
        "Brightness",
        "Br",
        "Current brightness; above BrightnessLimit, presence and motion are ignored.",
    ),
    (
        "InputDisable",
        "DisPc",
        "Disable periphery control: locks the push-button inputs; the app still works.",
    ),
    (
        "Presence",
        "P",
        "Presence: activates the presence mood while on.",
    ),
    ("RtD", "Rtd", RTD),
    (
        "MasterBr",
        "MBr",
        "Master brightness: scales every light circuit by this value.",
    ),
    ("Remanence", "Rem", REM),
    (
        "BrightnessLimit",
        "Brt",
        "Brightness threshold above which presence and motion are ignored.",
    ),
    (
        "MoveOn",
        "Moet",
        "Motion extend time after the motion input falls.",
    ),
    (
        "MoveIgnore",
        "Pto",
        "Presence timeout: presence is ignored this long after switching off by hand.",
    ),
    ("Step", "Sts", "Step size of the brightness when dimming."),
    (
        "Steptime",
        "Str",
        "Step rate when dimming: one Step every Steptime seconds.",
    ),
    (
        "Min",
        "MinBr",
        "Minimum brightness when dimming a circuit directly (0–50%).",
    ),
    (
        "Max",
        "MaxBr",
        "Maximum brightness when dimming a circuit directly (50–100%).",
    ),
    (
        "Wrap",
        "Dm",
        "Dim mode: on, a long click dims back and forth between Min and Max.",
    ),
    (
        "NoLast",
        "Lv",
        "Last value: off, circuits 1–4 switch on at their last brightness, on at Max.",
    ),
    (
        "MoveScene",
        "Pm",
        "Presence mood: the mood ID that Presence and Move start; 0 = the automatic's.",
    ),
    ("MaxP", "Tdc", "Time for a double click."),
    (
        "MoveTimeout",
        "Met",
        "Manual operation extend time: lights switch off after it, even if set by hand.",
    ),
    (
        "RGBalt",
        "Ao",
        "Alternative operation of circuits 1–4: a pulse cycles the RGB primary colors.",
    ),
    (
        "SceneMixTime",
        "Mmd",
        "Long-click time on Sel1–Sel8 to mix in a mood; 0 = no mixing.",
    ),
    ("FadingTime", "Ft", "Fading time when moods change."),
    (
        "AlarmClockPeriod",
        "Fbu",
        "Fading time of the alarm clock mood.",
    ),
    (
        "DayMinTemp",
        "MinCt",
        "Minimum (warm) color temperature for daylight control.",
    ),
    (
        "DayMaxTemp",
        "MaxCt",
        "Maximum (cool) color temperature for daylight control.",
    ),
    (
        "AlarmPeriod",
        "Afi",
        "Flashing interval of the lights during an alarm.",
    ),
    (
        "AlarmBrightness",
        "MaxAbr",
        "Maximum brightness of the lights during an alarm or all on.",
    ),
    ("AQ1-18", "Lc1-18", "Light circuit # output."),
    (
        "Scene",
        "M",
        "Current mood ID: 0 off, 98 buzzer, 99 all on, −1 user-defined.",
    ),
    (
        "OutputReset",
        "2C",
        "Pulse on a double or triple click, or on Reset.",
    ),
    ("OutputResetAll", "3C", "Pulse on a triple click."),
    ("OutputAPI", "API", API),
];

const HEAT_I_ROOM_CONTROLLER2: &[Entry] = &[
    (
        "AMode",
        "Mode",
        "Mode: −1 off, 0–2 automatic by schedule, 3–5 fixed setpoint (Input).",
    ),
    ("Input", "ϑt", "Target temperature in fixed setpoint mode."),
    ("Temp", "ϑc", "Current room temperature."),
    ("Window", "Dwc", "Door/window contact: 0 closed, 1 open."),
    (
        "Comfort",
        "C",
        "Comfort: starts comfort mode; when it falls, TimeC keeps it.",
    ),
    (
        "Save",
        "E",
        "Eco: starts eco mode; when it falls, TimeS keeps it.",
    ),
    (
        "Save2",
        "Bp",
        "Building protection: starts it; when it falls, TimeS keeps it.",
    ),
    (
        "Move",
        "P",
        "Presence: extends comfort mode while on, then for TimeMv.",
    ),
    (
        "Reset",
        "Off",
        "Off: a short pulse cancels the input timers, a longer one locks the block.",
    ),
    ("DisMv", "DisP", "Disables the presence input while on."),
    ("RtD", "Rtd", RTD),
    (
        "TempO",
        "ϑo",
        "Outdoor temperature, used with the door/window contact.",
    ),
    (
        "InCo2",
        "CO2",
        "CO2 level, passed on to the app and targets; not used for control.",
    ),
    (
        "InHumid",
        "H",
        "Relative humidity, passed on to the app and targets; not used for control.",
    ),
    (
        "inFan",
        "Fan",
        "Fan speed: 0 off, 1 auto, 2–7 silent to very high.",
    ),
    (
        "inAirDir",
        "ADir",
        "Airflow direction: 1 auto, 2–6 positions, 7 swing, 8 no swing.",
    ),
    ("TComfort", "ϑch", "Comfort temperature when heating."),
    ("TComfortC", "ϑcc", "Comfort temperature when cooling."),
    (
        "TComfortHC",
        "ϑchc",
        "Comfort temperature for heating and cooling.",
    ),
    (
        "TDiff",
        "ϑd",
        "Allowed deviation from the target in comfort mode.",
    ),
    (
        "TShadeHeat",
        "ϑsh",
        "Temperature above which shading starts while heating.",
    ),
    (
        "TShadeCool",
        "ϑsc",
        "Temperature above which shading starts while cooling.",
    ),
    (
        "TSaveL",
        "ϑeh",
        "Eco heating temperature, below the comfort heating temperature.",
    ),
    (
        "TSaveU",
        "ϑec",
        "Eco cooling temperature, above the comfort cooling temperature.",
    ),
    ("TSave", "ϑe", "Allowed deviation from comfort in eco mode."),
    ("TDeepSleep", "ϑfp", "Frost protection temperature."),
    ("TMax", "ϑhp", "Heat protection temperature."),
    (
        "TimeMove",
        "Vs",
        "Valve standstill: days after which an unused valve is moved.",
    ),
    ("TimeC", "Cet", "Comfort extend time after Comfort falls."),
    (
        "TimeS",
        "EBpet",
        "Eco and building protection extend time after Save or Save2 falls.",
    ),
    ("TimeMv", "Pet", "Presence extend time after Move falls."),
    (
        "THCelvin",
        "Hs",
        "Heating-up speed: time to raise the room by 1 °C; 0 = learned.",
    ),
    (
        "TCCelvin",
        "Cs",
        "Cooling-down speed: time to lower the room by 1 °C; 0 = learned.",
    ),
    (
        "TPWM",
        "Pwm",
        "PWM interval of outputs configured as PWM; 0 = automatic.",
    ),
    (
        "TWin",
        "Ddwc",
        "Delay before building protection after a door or window opens.",
    ),
    (
        "TExcess",
        "ϑExc",
        "Target offset while surplus heating or cooling is signalled.",
    ),
    (
        "AQh",
        "H",
        "Heating: output for valves or actuators that can only heat.",
    ),
    (
        "AQc",
        "C",
        "Cooling: output for valves or actuators that can only cool.",
    ),
    (
        "AQhc",
        "HC",
        "Heating/cooling: output for valves or actuators that do both.",
    ),
    (
        "AQh1-3",
        "H1-3",
        "Heating source # output, for a heating and cooling controller's source.",
    ),
    (
        "AQc1-3",
        "C1-3",
        "Cooling source # output, for a heating and cooling controller's source.",
    ),
    (
        "AQhc1-3",
        "HC1-3",
        "Heating/cooling source # output, for a heating and cooling controller's source.",
    ),
    (
        "Qs",
        "Shd",
        "Shading demand, for the AutoShade input of automatic shading.",
    ),
    (
        "AQs",
        "HCm",
        "Heating/cooling mode: 1 heating, −1 cooling, 0 off.",
    ),
    (
        "Qe",
        "Error",
        "Error: past frost or heat protection, or far from the target after heating.",
    ),
    ("Qa", "TxErr", "Error text."),
    ("AQt", "ϑt", "Target temperature."),
    ("AQhm", "Om", "Current operating mode ID of the schedule."),
    (
        "AQtm",
        "Os",
        "Temperature mode: −1 off, 0 eco, 1 comfort, 2 building protection, 3–4 manual.",
    ),
    (
        "Qb",
        "Boost",
        "Boost: on while preheating or far from the target temperature.",
    ),
    ("OutputAPI", "API", API),
];

const ALARM: &[Entry] = &[
    ("Remanence", "Rem", REM),
    ("Delay", "Ard", "Arming delay."),
    ("TQ1", "Sad", "Silent alarm delay."),
    ("TQ2", "Aad", "Audible alarm delay."),
    ("TQ3", "Vad", "Visual alarm delay."),
    ("TQ4", "Iad", "Internal alarm delay."),
    ("TQ5", "Ead", "External alarm delay."),
    ("TQ6", "Rad", "Remote alarm delay."),
    (
        "TI",
        "Eip",
        "Minimum time an alarm input pulse stays active, for counting sensors.",
    ),
    (
        "WI2",
        "Spt",
        "Time window in which a second presence sensor must trigger.",
    ),
    (
        "TM",
        "Atm",
        "Alarm test mode: 1 = only the test output QT fires.",
    ),
    (
        "MaxDurOut",
        "MaxA",
        "Maximum alarm duration, then a silent alarm; 0 = no limit.",
    ),
    (
        "AutoConfirm",
        "Sac",
        "Silent alarm confirmation: 1 confirms it when MaxDurOut is reached.",
    ),
    (
        "IInit",
        "Aoc",
        "Arm open contact: 1 = an open window only alarms when its state changes.",
    ),
    (
        "Q",
        "S",
        "Status: 0 disarmed, 1 armed with, 2 armed without motion sensors.",
    ),
    ("Q1", "Sa", "Silent alarm."),
    ("Q2", "Aa", "Audible alarm."),
    ("Q3", "Va", "Visual alarm."),
    ("Q4", "Ia", "Internal alarm."),
    ("Q5", "Ea", "External alarm."),
    ("Q6", "Ra", "Remote alarm."),
    ("AQ", "N", "Number of active sensors."),
    ("QT", "At", "Alarm test: used when TM is 1."),
    ("AQr", "Rtad", "Remaining time of the arming delay."),
    ("TQ", "Ca", "Cause of the last alarm."),
    ("OutTime", "Ta", "Date and time of the last alarm."),
    (
        "Qo",
        "WDs",
        "Window/door state: on while any window or door is open.",
    ),
    (
        "TTS",
        "WDot",
        "Names of the windows and doors open while arming, for text to speech.",
    ),
    ("OutputAPI", "API", API),
];

const SMOKE_ALARM: &[Entry] = &[
    ("Confirm", "Ca", "Confirms the alarm."),
    (
        "Mute",
        "Cs",
        "Confirms the alarm signals: OutSilent and OutHorn go off, the alarms stay.",
    ),
    (
        "InputAlarm",
        "S",
        "Alarm input for an external smoke detector.",
    ),
    (
        "InputAlarmW",
        "W",
        "Alarm input for an external water detector.",
    ),
    (
        "InputAlarmS",
        "F",
        "Input for an arc-fault circuit interrupter (AFCI).",
    ),
    ("InputTemp", "T", "Input for external temperature sensors."),
    ("AlarmDelay", "Mad", "Main alarm delay after the pre-alarm."),
    (
        "MaxTemp",
        "Maxϑ",
        "Maximum temperature: T at or above it raises the alarm.",
    ),
    (
        "MaxDuration",
        "MaxA",
        "Maximum alarm duration, then the main alarm is confirmed; 0 = no limit.",
    ),
    ("Remanence", "Rem", REM),
    (
        "Autoconfirm",
        "Pac",
        "Pre-alarm confirmation: 1 confirms the pre-alarm at MaxDuration too.",
    ),
    (
        "Servicemode",
        "Sm",
        "Service mode: suppresses the alarm, only OutAlarmTest fires; 1 always, above 1 for that time.",
    ),
    ("OutAlarm1", "Pa", "Pre-alarm."),
    ("OutAlarm2", "Ma", "Main alarm."),
    ("OutSilent", "Pas", "Pre-alarm signal."),
    ("OutHorn", "Mas", "Main alarm signal."),
    ("OutNumAlarms", "N", "Number of active sensors."),
    ("OutAlarmTest", "At", "Alarm test."),
    ("OutText", "Ca", "Cause of the last alarm."),
    ("OutTime", "Ta", "Date and time of the last alarm."),
    ("OutputAPI", "API", API),
];

const TPF_CONTROLLER: &[Entry] = &[
    ("inpres", "P", "Presence: activates the display."),
    (
        "indnd",
        "DnD",
        "Do not disturb: display and backlight stay off when idle.",
    ),
    (
        "inlbt",
        "LbT",
        "Light by touch: the first touch only wakes the display.",
    ),
    (
        "inroomlight",
        "L",
        "Lighting active: the room is lit artificially.",
    ),
    ("inbrightness", "Br", "Current room brightness."),
    (
        "insetpoint",
        "Set",
        "Sets the display brightness until presence or light changes.",
    ),
    ("inoff", "Off", "Off: locks the block."),
    (
        "indisplay",
        "Don",
        "Display on, at the brightness outsetpointdisplay.",
    ),
    ("inbacklight", "Bl", "Backlight on."),
    ("inText", "Txt", "Custom display text."),
    (
        "maxbrightness",
        "Brt",
        "Brightness threshold above which the daylight settings apply.",
    ),
    (
        "parsetpoint1",
        "BrDP",
        "Brightness when dark, with presence.",
    ),
    (
        "parsetpoint2",
        "BrDnP",
        "Brightness when dark, without presence.",
    ),
    (
        "parsetpoint3",
        "BrLP",
        "Brightness with artificial light below maxbrightness, with presence.",
    ),
    (
        "parsetpoint4",
        "BrLnP",
        "Brightness with artificial light below maxbrightness, without presence.",
    ),
    (
        "parsetpoint5",
        "BrBP",
        "Brightness when the room is above maxbrightness, with presence.",
    ),
    (
        "parsetpoint6",
        "BrBnP",
        "Brightness when the room is above maxbrightness, without presence.",
    ),
    (
        "parsetpointFB",
        "BrDef",
        "Default brightness, used when the calculated one is 0.",
    ),
    (
        "outsetpoint",
        "CBrB",
        "Current backlight brightness for connected devices.",
    ),
    (
        "outsetpointdisplay",
        "BrD",
        "Brightness of the display and status LEDs.",
    ),
    ("outpresence", "P", "Presence is active."),
    ("outroomlight", "L", "Lighting is active."),
    (
        "outdaylight",
        "B",
        "Bright: on while the room is above the brightness threshold.",
    ),
    ("outbrightness", "Br", "Current room brightness."),
    ("OutputAPI", "API", API),
];

const NFC_CODE_TOUCH: &[Entry] = &[
    ("Lr", "Lr", "Turns the status LEDs red."),
    ("Lg", "Lg", "Turns the status LEDs green."),
    ("Lb", "Lb", "Turns the status LEDs blue."),
    ("Lw", "Lw", "Turns the status LEDs white."),
    (
        "Au",
        "Au",
        "Authentication: 0 two-factor, 1 code or NFC, 2 NFC, 3 code, 4 OCPP or NFC.",
    ),
    (
        "La",
        "Blan",
        "Turns the button light on at night (Gen. 1 devices).",
    ),
    (
        "Lon",
        "Bl",
        "Turns the button light on permanently (not on battery devices).",
    ),
    (
        "Bg",
        "Bbl",
        "Turns the bell button's green light on (not on battery devices).",
    ),
    (
        "Lbr",
        "Bbr",
        "Brightness of the keypad light (not on battery devices).",
    ),
    (
        "PulseTime",
        "Don",
        "On-duration of the outputs Q1–Q6, Qd and Qa.",
    ),
    (
        "Q1-6",
        "O1-6",
        "Output #: on for PulseTime when an authentication for it succeeds.",
    ),
    (
        "TQ",
        "Cla",
        "Last authorization: date, time, user and output.",
    ),
    (
        "TQU",
        "Ula",
        "User of the last authorization: the NFC Code Touch ID, tag or code name.",
    ),
    ("TQU2", "Unla", "User name of the last authorization."),
    ("TQU3", "Uidla", "User ID of the last authorization."),
    ("TQo", "Nlo", "Name or number of the last output."),
    ("TQt", "Tla", "Date and time of the last authorization."),
    ("Qd", "Ad", "Authentication denied: on for PulseTime."),
    ("Qa", "As", "Authentication successful: on for PulseTime."),
    ("Qn", "Nco", "Number of the current output; −1 = denied."),
    ("Be", "Bell", "Doorbell output."),
    (
        "BeSel",
        "Bsel",
        "Doorbell pre-select: a number entered before ringing; −1 = none.",
    ),
    ("OutputAPI", "API", API),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::{BUILTIN_TYPES, builtin};

    #[test]
    fn every_entry_names_a_real_port_and_cites_a_page() {
        for t in BUILTIN_TYPES {
            let Some(entries) = entries(t) else { continue };
            assert!(
                crate::blockdoc::kb_page(t).is_some(),
                "{t}: no knowledge-base page"
            );
            let ports = builtin(t).unwrap();
            for (key, abbr, text) in entries {
                assert!(
                    ports.iter().any(|p| matches(key, p.key).is_some()),
                    "{t}.{key} is not a port"
                );
                assert!(text.ends_with('.') && text.len() < 110, "{t}.{key}: {text}");
                assert_eq!(
                    family(key).is_some(),
                    family(abbr).is_some(),
                    "{t}.{key}: {abbr}"
                );
            }
        }
    }

    /// Where the canvas label is known, the entry's abbreviation is that label.
    #[test]
    fn abbreviations_agree_with_canvas_labels() {
        for t in BUILTIN_TYPES {
            let Some(entries) = entries(t) else { continue };
            for spec in builtin(t).unwrap() {
                let Some(label) = crate::port_meta::port_meta(t, spec.key).and_then(|m| m.label)
                else {
                    continue;
                };
                let Some((key, abbr, _)) = entries
                    .iter()
                    .find(|(k, _, _)| matches(k, spec.key).is_some())
                else {
                    continue;
                };
                let abbr = match matches(key, spec.key).unwrap() {
                    Some(n) => format!("{}{n}", family(abbr).unwrap().0),
                    None => abbr.to_string(),
                };
                let label = label.replace('−', "-");
                assert_eq!(abbr, label, "{t}.{}", spec.key);
            }
        }
    }

    #[test]
    fn families_expand() {
        assert_eq!(
            describe("LightController2", "I3").as_deref(),
            Some("Light circuit 3: a pulse switches it, a long click dims it.")
        );
        assert!(describe("LightController2", "AQ18").is_some());
        assert_eq!(describe("LightController2", "AQ19"), None);
        assert_eq!(
            describe("NfcCodeTouch", "Lr").as_deref(),
            Some("Turns the status LEDs red.")
        );
        assert_eq!(describe("PulseGen", "InputEnable"), None);
    }
}
