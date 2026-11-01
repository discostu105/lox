//! The value Loxone Config gives a block's connector when it places the
//! block: its GUI default. A save writes it for every connector a block
//! lacks, so oracle session 92 (Loxone Config 17.2.8.28) read the table
//! off a save of one block of each builtin page type whose connectors had
//! all been removed. It agrees with the documented default of every port
//! the documentation gives one for.
//!
//! Only defaults other than 0 are listed: a save writes no `Def=` at 0. A
//! port that is not listed is 0 when a block is placed.
//!
//! A block the compiler creates gets these values on the ports the source
//! leaves out, as Loxone Config does. A block that already existed keeps a
//! missing `Def=`, which is 0 (`loxl lint` reports it as `implicit-zero`).

/// The GUI default of a builtin block's connector, as the exact `Def=`
/// string, or `None` when it is 0.
pub fn gui_default(block_type: &str, port: &str) -> Option<&'static str> {
    TABLE
        .iter()
        .find(|(t, _)| *t == block_type)
        .and_then(|(_, ports)| ports.iter().find(|(k, _)| *k == port))
        .map(|(_, v)| *v)
}

/// The connectors of `block_type` with a GUI default other than 0, in
/// connector order.
pub fn gui_defaults(block_type: &str) -> &'static [(&'static str, &'static str)] {
    TABLE
        .iter()
        .find(|(t, _)| *t == block_type)
        .map_or(&[], |(_, ports)| ports)
}

type Ports = &'static [(&'static str, &'static str)];

/// The types of oracle session 92 that got no `Def=` at all.
#[cfg(test)]
const ALL_ZERO: &[&str] = &[
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
    "Memory",
    "PushButton",
    "DayTimer",
    "CentralShade",
    "CentralLight",
    "Code16",
    "AnalogMultiplexer2",
    "SRFlipFlop",
    "State",
    "StateV",
    "Mult",
    "Add",
    "Sub",
    "Div",
    "Int",
    "HourCounter",
    "AMemory",
    "MeterAbsUni",
    "MeterAbsBi",
    "MeterAbsSt",
];

/// Oracle session 92, in the order the save wrote the connectors.
const TABLE: &[(&str, Ports)] = &[
    (
        "Alarm",
        &[
            ("Delay", "600"),
            ("MaxDurOut", "900"),
            ("TQ2", "20"),
            ("TQ3", "40"),
            ("TQ4", "90"),
            ("TQ5", "150"),
            ("TQ6", "300"),
            ("WI2", "900"),
        ],
    ),
    (
        "AnalogThresholdTrigger",
        &[("On", "5"), ("Off", "1"), ("PulseTime", "1")],
    ),
    (
        "AutoJalousie",
        &[
            ("Dir", "-1"),
            ("TimeEnd", "75"),
            ("TimeEndDown", "70"),
            ("AutoShadeEnd", "1"),
            ("AutMode", "1"),
            ("MinPulse", "3"),
            ("DblClk", "0.3"),
            ("DirTol", "85"),
            ("DirTol2", "85"),
            ("AutoShadeTime", "120"),
            ("SRoff", "30"),
            ("SSoff", "-30"),
            ("Width", "70"),
            ("Space", "60"),
            ("TimeBlock", "0.5"),
            ("TurnOffset", "0.15"),
            ("MinMove", "0.4"),
            ("Back", "0.8"),
        ],
    ),
    ("Avg", &[("Time", "60"), ("Max", "60")]),
    ("Counter", &[("EndValue", "1000")]),
    ("EFM", &[("Pre", "0.2"), ("Pri", "0.2"), ("CO2", "0.42")]),
    ("EdgeDetection", &[("PulseTime", "1")]),
    (
        "HeatIRoomController2",
        &[
            ("inFan", "2"),
            ("inAirDir", "1"),
            ("TComfort", "22.5"),
            ("TComfortC", "24.5"),
            ("TComfortHC", "22"),
            ("TDiff", "1.5"),
            ("TShadeHeat", "27.5"),
            ("TShadeCool", "23.5"),
            ("TSaveL", "3"),
            ("TSaveU", "3"),
            ("TSave", "2"),
            ("TDeepSleep", "5"),
            ("TMax", "28"),
            ("TimeMove", "14"),
            ("TimeC", "3600"),
            ("TimeS", "3600"),
            ("TimeMv", "1800"),
            ("THCelvin", "120"),
            ("TCCelvin", "60"),
            ("TWin", "300"),
            ("TExcess", "1"),
        ],
    ),
    (
        "Irrigation",
        &[
            ("MaxP", "2"),
            ("MaxR", "1800"),
            ("Tv1", "600"),
            ("Tv2", "600"),
            ("Tv3", "600"),
            ("Tv4", "600"),
            ("Tv5", "600"),
            ("Tv6", "600"),
            ("Tv7", "600"),
            ("Tv8", "600"),
        ],
    ),
    (
        "LightController2",
        &[
            ("BrightnessLimit", "30"),
            ("MoveOn", "900"),
            ("MoveIgnore", "300"),
            ("Step", "2"),
            ("Steptime", "0.2"),
            ("Max", "100"),
            ("MaxP", "0.35"),
            ("MoveTimeout", "3600"),
            ("SceneMixTime", "1"),
            ("FadingTime", "1"),
            ("AlarmClockPeriod", "3"),
            ("DayMinTemp", "2700"),
            ("DayMaxTemp", "6500"),
            ("AlarmPeriod", "4"),
            ("AlarmBrightness", "50"),
        ],
    ),
    ("MeterDig", &[("Ppwr", "1")]),
    ("Monoflop", &[("Time", "2")]),
    ("OffDelay", &[("Time", "1")]),
    ("OnDelay", &[("Time", "1")]),
    ("OnPulseDelay", &[("Delay", "5"), ("Time", "0.5")]),
    ("PButtonT", &[("Time", "0.3")]),
    ("PulseAt", &[("Time", "1")]),
    ("PulseBy", &[("Time", "1")]),
    ("PulseGen", &[("TimeHigh", "1"), ("TimeLow", "1")]),
    (
        "SmokeAlarm",
        &[
            ("AlarmDelay", "120"),
            ("MaxTemp", "43"),
            ("MaxDuration", "300"),
        ],
    ),
    (
        "StairwayLS",
        &[("TimeHigh", "180"), ("TimeWarn", "15"), ("WarnTime", "0.5")],
    ),
    ("Switch2Button", &[("Time", "0.02")]),
    ("UpDownCounter", &[("OnValue", "10"), ("OffValue", "5")]),
    ("WipingRelay", &[("TimeOn", "1")]),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::builtin;

    #[test]
    fn every_entry_is_a_port_in_connector_order_and_not_zero() {
        for (ty, ports) in TABLE {
            let specs = builtin(ty).unwrap_or_else(|| panic!("{ty} is not builtin"));
            let mut last = 0;
            for (key, value) in *ports {
                let at = specs
                    .iter()
                    .position(|s| s.key == *key)
                    .unwrap_or_else(|| panic!("{ty}.{key} is no port"));
                assert!(at >= last, "{ty}.{key} out of connector order");
                last = at;
                assert!(crate::connectors::def_written(value), "{ty}.{key}");
            }
        }
    }

    #[test]
    fn agrees_with_the_documented_defaults() {
        let types = TABLE
            .iter()
            .map(|(t, _)| *t)
            .chain(ALL_ZERO.iter().copied());
        for ty in types {
            for spec in builtin(ty).unwrap_or_else(|| panic!("{ty} is not builtin")) {
                let documented = crate::port_meta::port_meta(ty, spec.key).and_then(|m| m.default);
                let Some(doc) = documented else { continue };
                let gui = gui_default(ty, spec.key).unwrap_or("0");
                assert_eq!(
                    doc.parse::<f64>().ok(),
                    gui.parse::<f64>().ok(),
                    "{ty}.{}: documented {doc}, GUI {gui}",
                    spec.key
                );
            }
        }
    }
}
