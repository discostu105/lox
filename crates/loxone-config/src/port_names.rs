//! The source names of the builtin ports.
//!
//! Loxone Config stores a connector under a key (`K="InputTrigger"`,
//! `K="Pf"`). Lox Logic source names the port by a name of its own:
//! the key where it reads well (`Reset`, `AutoShade`), a curated name
//! where it does not (`Trigger`, `Power`), and one name per family
//! (`In1`, `In2` on every two-input block). The lockfile, the compiled
//! config and the simulator keep the keys; everything that reads or
//! writes source goes through [`name_of`] and [`key_of`].
//!
//! A name is unique within its type and never equals the key of another
//! port of the type, so a lookup by "name or key" is unambiguous
//! (`tests` below check both for every builtin type).

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The source name of the port stored under `key` on `block_type`: the
/// curated name, else the key itself.
pub fn name_of(block_type: &str, key: &str) -> &'static str {
    if let Some(name) = explicit(block_type, key) {
        return name;
    }
    generic(block_type, key).unwrap_or_else(|| intern(key))
}

/// The connector key of the port a source names `name` on `block_type`,
/// when `name` is a port of the type: its name, or the key of a port
/// that has no other name. `None` for anything else, a renamed port's
/// key included (`InputTrigger` on a Monoflop is `Trigger` now).
pub fn key_of(block_type: &str, name: &str) -> Option<&'static str> {
    let specs = crate::connectors::builtin(block_type)?;
    specs.iter().find(|s| s.name == name).map(|s| s.key)
}

/// [`key_of`], else `name` itself: the key under which the config stores
/// a port the table does not know (an open-world extern port). On a type
/// outside the table the family names still read back to their keys
/// (`In` on a Tree actuator is its `I`).
pub fn key_or_raw<'a>(block_type: &str, name: &'a str) -> &'a str {
    if let Some(key) = key_of(block_type, name) {
        return key;
    }
    if crate::connectors::builtin(block_type).is_none()
        && let Some(key) = generic_key(name)
    {
        return key;
    }
    name
}

/// The key among `keys`, a block's connector keys as the config stores
/// them, of the port a source names `name`. This is the reverse of
/// [`name_of`] for any block type, including types outside the table,
/// whose keys differ (`In` is `I` on one type and `Input` on another).
/// `None` when no key reads as `name`, or more than one does.
pub fn key_among<'k>(block_type: &str, name: &str, keys: &[&'k str]) -> Option<&'k str> {
    let direct = key_or_raw(block_type, name);
    if let Some(key) = keys.iter().find(|k| **k == direct) {
        return Some(key);
    }
    let mut named = keys.iter().filter(|k| name_of(block_type, k) == name);
    let key = named.next()?;
    named.next().is_none().then_some(*key)
}

/// The key a family name reads back to on a type outside the table.
fn generic_key(name: &str) -> Option<&'static str> {
    Some(match name {
        "In" => "I",
        "Trigger" => "InputTrigger",
        "TriggerUp" => "InputTriggerUp",
        "TriggerDown" => "InputTriggerDown",
        "Enable" => "InputEnable",
        "Invert" => "InputInvert",
        "Disable" => "InputDisable",
        _ => return None,
    })
}

/// The source name of a renamed port whose key `name` is, if `name` is
/// such a key on `block_type` ("write `Trigger` instead of
/// `InputTrigger`").
pub fn renamed(block_type: &str, name: &str) -> Option<&'static str> {
    let specs = crate::connectors::builtin(block_type)?;
    specs
        .iter()
        .find(|s| s.key == name && s.name != name)
        .map(|s| s.name)
}

/// The output a bare block name stands for (`sonne` is `sonne.Q`), as a
/// source name: a type's only output, else the one listed here. `None`
/// for a type whose outputs are all equally meaningful.
pub fn default_output(block_type: &str) -> Option<&'static str> {
    if let Some(name) = DEFAULT_OUTPUTS
        .iter()
        .find(|(t, _)| *t == block_type)
        .map(|(_, o)| *o)
    {
        return Some(name);
    }
    let specs = crate::connectors::builtin(block_type)?;
    let mut outputs = specs
        .iter()
        .filter(|s| s.dir == crate::connectors::PortDir::Output);
    let first = outputs.next()?;
    outputs.next().is_none().then_some(first.name)
}

/// Types with several outputs, one of which is the one a reader means.
const DEFAULT_OUTPUTS: &[(&str, &str)] = &[
    ("Formula", "AQ"),
    ("AnalogThresholdTrigger", "Q"),
    ("PushButton", "Q"),
    ("Switch2Button", "Q"),
    ("DayTimer", "AQ"),
    ("EdgeDetection", "Edge"),
    ("UpDownCounter", "AQ"),
    ("Counter", "AQ"),
    ("HourCounter", "AQ"),
    ("VirtualIn", "Q"),
    ("VirtualHttpInCmd", "AQ"),
    ("VirtualUdpInCmd", "AQ"),
    ("ModbusASensor", "Q"),
    ("ModbusSensor", "Q"),
    ("OutputRef", "AQ"),
    ("OutputRefLM", "AQ"),
    ("TreeSensor", "Q"),
    ("TreeAsensor", "AQ"),
    ("LoxAIRsensor", "Q"),
    ("LoxAIRAsensor", "AQ"),
    ("DigitalIn", "Q"),
    ("VoltageIn", "AQ"),
    ("Lox1wireAsensor", "AQ"),
];

/// Ports whose key reads well enough but whose family has one name.
fn generic(block_type: &str, key: &str) -> Option<&'static str> {
    Some(match key {
        "I" | "Input" => "In",
        "InputTrigger" => "Trigger",
        "InputTriggerUp" => "TriggerUp",
        "InputTriggerDown" => "TriggerDown",
        "InputEnable" => "Enable",
        "InputInvert" => "Invert",
        "InputDisable" => "Disable",
        "InputS" => "Set",
        "InputR" => "Reset",
        "InputDir" => "Direction",
        "InputResetAll" => "ResetAll",
        "RtD" => "ResetToDefault",
        _ => {
            // `I<n>` and `Input<n>` are `In<n>`; a lighting controller's
            // `AQ<n>` circuits keep their key.
            let n = key
                .strip_prefix("Input")
                .or_else(|| key.strip_prefix('I'))?;
            if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let _ = block_type;
            return Some(intern(&format!("In{n}")));
        }
    })
}

/// Curated names per type, where the key says nothing a reader knows.
fn explicit(block_type: &str, key: &str) -> Option<&'static str> {
    let table: &[(&str, &str)] = match block_type {
        "PushButton" | "SRFlipFlop" => &[("InputTrigger", "Toggle")],
        "DayTimer" => &[
            ("InputTrigger", "Activate"),
            ("AQm", "ActiveMode"),
            ("AQmt", "RemainingTime"),
        ],
        "PulseGen" => &[("TimeHigh", "OnTime"), ("TimeLow", "OffTime")],
        "OnPulseDelay" => &[("Time", "PulseTime")],
        "StairwayLS" => &[
            ("TimeHigh", "Time"),
            ("TimeWarn", "WarnBefore"),
            ("WarnTime", "WarnDuration"),
        ],
        "WipingRelay" => &[("TimeOn", "Time")],
        "PulseBy" => &[("Time", "PulseTime")],
        "AMemory" => &[("Trigger", "Set")],
        "Avg" => &[("Max", "Count"), ("Time", "Cycle")],
        "Int" => &[("Mode", "Rounding")],
        "Counter" => &[("EndValue", "Limit")],
        "HourCounter" => &[
            ("Maintenance", "MaintenanceInterval"),
            ("AQl", "LastStart"),
            ("AQr", "RemainingTime"),
        ],
        "Code16" => &[
            ("TI1", "TextIn1"),
            ("TI2", "TextIn2"),
            ("TI3", "TextIn3"),
            ("AI1", "In1"),
            ("AI2", "In2"),
            ("AI3", "In3"),
            ("AI4", "In4"),
            ("AI5", "In5"),
            ("AI6", "In6"),
            ("AI7", "In7"),
            ("AI8", "In8"),
            ("AI9", "In9"),
            ("AI10", "In10"),
            ("AI11", "In11"),
            ("AI12", "In12"),
            ("AI13", "In13"),
            ("TQ1", "TextOut1"),
            ("TQ2", "TextOut2"),
            ("TQ3", "TextOut3"),
            ("AQ1", "Out1"),
            ("AQ2", "Out2"),
            ("AQ3", "Out3"),
            ("AQ4", "Out4"),
            ("AQ5", "Out5"),
            ("AQ6", "Out6"),
            ("AQ7", "Out7"),
            ("AQ8", "Out8"),
            ("AQ9", "Out9"),
            ("AQ10", "Out10"),
            ("AQ11", "Out11"),
            ("AQ12", "Out12"),
            ("AQ13", "Out13"),
            ("TeQ", "ErrorText"),
        ],
        "AutoJalousie" | "CentralShade" => &[
            ("InputTrigger", "Toggle"),
            ("EnAutoShade", "DisableAutoShade"),
            ("ReactAutoShade", "RestartAutoShade"),
            ("ManualPosition", "ManualPosition"),
            ("ManualLamelle", "ManualSlats"),
            ("Gesture", "T5"),
            ("SO", "SafetyPosition"),
            ("AutMode", "AutoShadeMode"),
            ("MinPulse", "LongClick"),
            ("DblClk", "DoubleClick"),
            ("DirTol", "DirToleranceIn"),
            ("DirTol2", "DirToleranceOut"),
            ("SRoff", "SunriseOffset"),
            ("SSoff", "SunsetOffset"),
            ("TimeEnd", "OpenTime"),
            ("TimeEndDown", "CloseTime"),
            ("TimeBlock", "MotorLock"),
            ("TurnOffset", "BacklashReverse"),
            ("Deadtime", "BacklashSame"),
            ("MinMove", "MinTravel"),
            ("Back", "ReturnTime"),
            ("Rdd", "ReferenceDown"),
            ("Width", "SlatWidth"),
            ("Space", "SlatSpacing"),
            ("OutputUp", "MotorUp"),
            ("OutputDown", "MotorDown"),
            ("OutputPos", "Position"),
            ("OutputLPos", "SlatPosition"),
            ("OutputAutoShade", "AutoShadeActive"),
            ("OutputSafety", "SafetyActive"),
            ("OutputLock", "Locked"),
            ("OutOpen", "OpenCount"),
            ("OutClose", "ClosedCount"),
        ],
        "LightController2" | "CentralLight" => &[
            ("InputTriggerUp", "NextMood"),
            ("InputTriggerDown", "PrevMood"),
            ("EnMove", "DisablePresence"),
            ("Move", "Motion"),
            ("AlarmClock", "Buzzer"),
            ("MasterBr", "MasterBrightness"),
            ("MoveOn", "MotionExtend"),
            ("MoveIgnore", "PresenceTimeout"),
            ("Steptime", "StepTime"),
            ("Wrap", "DimMode"),
            ("NoLast", "LastValue"),
            ("MoveScene", "PresenceMood"),
            ("MaxP", "DoubleClickTime"),
            ("MoveTimeout", "ManualTimeout"),
            ("RGBalt", "RgbAlternative"),
            ("SceneMixTime", "MoodMixTime"),
            ("AlarmClockPeriod", "BuzzerFade"),
            ("DayMinTemp", "MinColorTemp"),
            ("DayMaxTemp", "MaxColorTemp"),
            ("AlarmPeriod", "AlarmInterval"),
            ("Scene", "Mood"),
            ("OutputReset", "DoubleClick"),
            ("OutputResetAll", "TripleClick"),
            ("OutActive", "ActiveCount"),
        ],
        "HeatIRoomController2" => &[
            ("AMode", "Mode"),
            ("Input", "SetTemp"),
            ("Temp", "RoomTemp"),
            ("Save", "Eco"),
            ("Save2", "BuildingProtection"),
            ("Move", "Presence"),
            ("DisMv", "DisablePresence"),
            ("TempO", "OutdoorTemp"),
            ("InCo2", "Co2"),
            ("InHumid", "Humidity"),
            ("inFan", "Fan"),
            ("inAirDir", "AirDirection"),
            ("TComfort", "ComfortHeat"),
            ("TComfortC", "ComfortCool"),
            ("TComfortHC", "ComfortHeatCool"),
            ("TDiff", "ComfortDeviation"),
            ("TShadeHeat", "ShadeHeat"),
            ("TShadeCool", "ShadeCool"),
            ("TSaveL", "EcoHeat"),
            ("TSaveU", "EcoCool"),
            ("TSave", "EcoDeviation"),
            ("TDeepSleep", "FrostProtection"),
            ("TMax", "HeatProtection"),
            ("TimeMove", "ValveMoveDays"),
            ("TimeC", "ComfortExtend"),
            ("TimeS", "EcoExtend"),
            ("TimeMv", "PresenceExtend"),
            ("THCelvin", "HeatingSpeed"),
            ("TCCelvin", "CoolingSpeed"),
            ("TPWM", "PwmInterval"),
            ("TWin", "WindowDelay"),
            ("TExcess", "ExcessOffset"),
            ("AQh", "Heating"),
            ("AQc", "Cooling"),
            ("AQhc", "HeatingCooling"),
            ("AQh1", "Heating1"),
            ("AQc1", "Cooling1"),
            ("AQhc1", "HeatingCooling1"),
            ("AQh2", "Heating2"),
            ("AQc2", "Cooling2"),
            ("AQhc2", "HeatingCooling2"),
            ("AQh3", "Heating3"),
            ("AQc3", "Cooling3"),
            ("AQhc3", "HeatingCooling3"),
            ("Qs", "ShadeDemand"),
            ("AQs", "HeatCoolMode"),
            ("Qe", "Error"),
            ("Qa", "ErrorText"),
            ("AQt", "TargetTemp"),
            ("AQhm", "ScheduleMode"),
            ("AQtm", "TempMode"),
            ("Qb", "Boost"),
        ],
        "Irrigation" => &[
            ("A", "Activate"),
            ("AIv", "SelectValve"),
            ("Rf", "RainForecast"),
            ("Ra", "Rain"),
            ("MaxP", "MaxForecast"),
            ("MaxR", "MaxRain"),
            ("Tv1", "ValveTime1"),
            ("Tv2", "ValveTime2"),
            ("Tv3", "ValveTime3"),
            ("Tv4", "ValveTime4"),
            ("Tv5", "ValveTime5"),
            ("Tv6", "ValveTime6"),
            ("Tv7", "ValveTime7"),
            ("Tv8", "ValveTime8"),
            ("Qp", "Pump"),
            ("Qv1", "Valve1"),
            ("Qv2", "Valve2"),
            ("Qv3", "Valve3"),
            ("Qv4", "Valve4"),
            ("Qv5", "Valve5"),
            ("Qv6", "Valve6"),
            ("Qv7", "Valve7"),
            ("Qv8", "Valve8"),
            ("AQv", "ActiveValve"),
        ],
        "Alarm" => &[
            ("DisMv", "DisablePresence"),
            ("Delay", "ArmDelay"),
            ("MaxDurOut", "MaxDuration"),
            ("IInit", "ArmOpenContact"),
            ("TM", "TestMode"),
            ("TQ1", "SilentDelay"),
            ("TQ2", "AudibleDelay"),
            ("TQ3", "VisualDelay"),
            ("TQ4", "InternalDelay"),
            ("TQ5", "ExternalDelay"),
            ("TQ6", "RemoteDelay"),
            ("TI", "MinPulse"),
            ("WI2", "SecondSensorWindow"),
            ("Q1", "Silent"),
            ("Q2", "Audible"),
            ("Q3", "Visual"),
            ("Q4", "Internal"),
            ("Q5", "External"),
            ("Q6", "Remote"),
            ("AQ", "SensorCount"),
            ("QT", "Test"),
            ("AQr", "RemainingDelay"),
            ("TQ", "Cause"),
            ("OutTime", "LastAlarmTime"),
            ("Qo", "WindowOpen"),
            ("TTS", "OpenWindowsText"),
        ],
        "SmokeAlarm" => &[
            ("InputAlarm", "Smoke"),
            ("InputAlarmW", "Water"),
            ("InputAlarmS", "ArcFault"),
            ("InputTemp", "Temperature"),
            ("OutAlarm1", "PreAlarm"),
            ("OutAlarm2", "MainAlarm"),
            ("OutSilent", "PreAlarmSignal"),
            ("OutHorn", "MainAlarmSignal"),
            ("OutNumAlarms", "SensorCount"),
            ("OutAlarmTest", "Test"),
            ("OutText", "Cause"),
            ("OutTime", "LastAlarmTime"),
        ],
        "EFM" => &[
            ("R", "Reset"),
            ("Pre", "ExportPrice"),
            ("Pri", "ImportPrice"),
            ("OGpwr", "GridPower"),
            ("OPpwr", "ProductionPower"),
            ("OCpwr", "ConsumptionPower"),
            ("OSpwr", "StoragePower"),
            ("OEd", "ExportedToday"),
            ("OId", "ImportedToday"),
            ("OPd", "ProducedToday"),
            ("OCd", "ConsumedToday"),
            ("OScd", "SelfConsumedToday"),
            ("OCo2d", "Co2SavedToday"),
            ("OYd", "YieldToday"),
        ],
        "TpfController" => &[
            ("inpres", "Presence"),
            ("indnd", "DoNotDisturb"),
            ("inlbt", "LightByTouch"),
            ("inroomlight", "RoomLight"),
            ("indaylight", "Daylight"),
            ("inbrightness", "Brightness"),
            ("insetpoint", "Setpoint"),
            ("inoff", "Off"),
            ("indis", "Disable"),
            ("indisplay", "DisplayOn"),
            ("inbacklight", "BacklightOn"),
            ("inText", "Text"),
            ("maxbrightness", "BrightnessLimit"),
            ("parsetpoint1", "DarkPresence"),
            ("parsetpoint2", "DarkAbsence"),
            ("parsetpoint3", "LightPresence"),
            ("parsetpoint4", "LightAbsence"),
            ("parsetpoint5", "BrightPresence"),
            ("parsetpoint6", "BrightAbsence"),
            ("parsetpointFB", "DefaultBrightness"),
            ("outsetpoint", "Backlight"),
            ("outsetpointdisplay", "DisplayBrightness"),
            ("outpresence", "PresenceActive"),
            ("outroomlight", "RoomLightActive"),
            ("outdaylight", "Bright"),
            ("outbrightness", "OutBrightness"),
        ],
        "NfcCodeTouch" => &[
            ("Lr", "LedRed"),
            ("Lg", "LedGreen"),
            ("Lb", "LedBlue"),
            ("Lw", "LedWhite"),
            ("Au", "Authentication"),
            ("La", "NightLight"),
            ("Lon", "ButtonLight"),
            ("Bg", "BellGreen"),
            ("Lbr", "Brightness"),
            ("TQ", "LastAuth"),
            ("TQU", "LastUser"),
            ("TQU2", "LastUserName"),
            ("TQU3", "LastUserId"),
            ("TQo", "LastOutput"),
            ("TQt", "LastTime"),
            ("Qd", "Denied"),
            ("Qa", "Granted"),
            ("Qn", "OutputNumber"),
            ("Be", "Bell"),
            ("BeSel", "BellSelect"),
        ],
        "Intercom" => &[
            ("Ib", "RingBell"),
            ("Lon", "ButtonLight"),
            ("Lbr", "Brightness"),
            ("Tp", "PulseTime"),
            ("Qb", "Bell"),
            ("Qmute", "Muted"),
        ],
        "MeterDig" => &[
            ("A", "Active"),
            ("R", "Reset"),
            ("Ppwr", "Power"),
            ("Mro", "ReadingOffset"),
            ("OPf", "OutPower"),
            ("OMr", "Reading"),
            ("ORd", "ReadingToday"),
            ("ORld", "ReadingYesterday"),
            ("ORw", "ReadingWeek"),
            ("ORlw", "ReadingLastWeek"),
            ("ORm", "ReadingMonth"),
            ("ORlm", "ReadingLastMonth"),
            ("ORy", "ReadingYear"),
            ("ORly", "ReadingLastYear"),
        ],
        "MeterAbsUni" => &[
            ("Pf", "Power"),
            ("Mr", "ReadingIn"),
            ("R", "Reset"),
            ("Mro", "ReadingOffset"),
            ("OPf", "OutPower"),
            ("OMr", "Reading"),
            ("ORd", "ReadingToday"),
            ("ORld", "ReadingYesterday"),
            ("ORw", "ReadingWeek"),
            ("ORlw", "ReadingLastWeek"),
            ("ORm", "ReadingMonth"),
            ("ORlm", "ReadingLastMonth"),
            ("ORy", "ReadingYear"),
            ("ORly", "ReadingLastYear"),
        ],
        "MeterAbsBi" | "MeterAbsSt" => &[
            ("Pf", "Power"),
            ("Mr1", "ReadingIn"),
            ("Mr2", "ReadingOut"),
            ("Slvl", "StorageLevel"),
            ("R", "Reset"),
            ("Mro1", "ReadingOffsetIn"),
            ("Mro2", "ReadingOffsetOut"),
            ("OPf", "OutPower"),
            ("OMr1", "ReadingInOut"),
            ("ORd1", "ReadingInToday"),
            ("ORld1", "ReadingInYesterday"),
            ("ORw1", "ReadingInWeek"),
            ("ORlw1", "ReadingInLastWeek"),
            ("ORm1", "ReadingInMonth"),
            ("ORlm1", "ReadingInLastMonth"),
            ("ORy1", "ReadingInYear"),
            ("ORly1", "ReadingInLastYear"),
            ("OMr2", "ReadingOutOut"),
            ("ORd2", "ReadingOutToday"),
            ("ORld2", "ReadingOutYesterday"),
            ("ORw2", "ReadingOutWeek"),
            ("ORlw2", "ReadingOutLastWeek"),
            ("ORwl2", "ReadingOutLastWeek"),
            ("ORm2", "ReadingOutMonth"),
            ("ORlm2", "ReadingOutLastMonth"),
            ("ORy2", "ReadingOutYear"),
            ("ORly2", "ReadingOutLastYear"),
            ("OSlvl", "OutStorageLevel"),
        ],
        _ => &[],
    };
    table.iter().find(|(k, _)| *k == key).map(|(_, n)| *n)
}

/// A `'static` copy of a computed name (`In12`), made once.
fn intern(s: &str) -> &'static str {
    static POOL: OnceLock<std::sync::Mutex<BTreeMap<String, &'static str>>> = OnceLock::new();
    let pool = POOL.get_or_init(Default::default);
    let mut pool = pool.lock().expect("name pool");
    if let Some(v) = pool.get(s) {
        return v;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    pool.insert(s.to_string(), leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    // Types outside the connector table store the same port under
    // different keys; the source names it the same way on all of them.
    #[test]
    fn key_among_reverses_name_of_on_any_type() {
        let cases: &[(&str, &[&str], &str, &str)] = &[
            (
                "AnalogScaler",
                &["Input", "Src1", "Dst1", "AQ"],
                "In",
                "Input",
            ),
            (
                "RSFlipFlop",
                &["InputS", "InputTrigger", "InputR", "Q"],
                "Set",
                "InputS",
            ),
            (
                "RSFlipFlop",
                &["InputS", "InputTrigger", "InputR", "Q"],
                "Reset",
                "InputR",
            ),
            (
                "RSFlipFlop",
                &["InputS", "InputTrigger", "InputR", "Q"],
                "Trigger",
                "InputTrigger",
            ),
            ("SomeActuator", &["I", "Q"], "In", "I"),
            ("SomeLogic", &["Input1", "Input2", "Q"], "In2", "Input2"),
            // A key is its own name when it has no other.
            ("AnalogScaler", &["Input", "Src1"], "Src1", "Src1"),
        ];
        for (ty, keys, name, key) in cases {
            assert_eq!(key_among(ty, name, keys), Some(*key), "{ty}.{name}");
            assert_eq!(name_of(ty, key), *name, "{ty}.{key}");
        }
        assert_eq!(key_among("AnalogScaler", "Nope", &["Input"]), None);
        // Two keys that read the same: neither is picked.
        assert_eq!(key_among("Odd", "In", &["Input", "I0x"]), Some("Input"));
        assert_eq!(key_among("Odd", "In3", &["I3", "Input3"]), None);
    }
    use crate::connectors::{BUILTIN_TYPES, builtin};
    use std::collections::BTreeSet;

    fn every_type() -> Vec<&'static str> {
        let mut v: Vec<&str> = BUILTIN_TYPES.to_vec();
        v.extend(["InputRef", "OutputRef", "OutputRefLM"]);
        v
    }

    #[test]
    fn names_are_unique_and_clash_with_no_key() {
        for t in every_type() {
            let specs = builtin(t).unwrap();
            let names: BTreeSet<&str> = specs.iter().map(|s| s.name).collect();
            assert_eq!(names.len(), specs.len(), "{t}: duplicate source names");
            for s in specs {
                if let Some(other) = specs.iter().find(|o| o.key == s.name && o.key != s.key) {
                    panic!(
                        "{t}: `{}` names port `{}` but is the key of `{}`",
                        s.name, s.key, other.key
                    );
                }
            }
        }
    }

    #[test]
    fn names_round_trip_through_keys() {
        for t in every_type() {
            for s in builtin(t).unwrap() {
                assert_eq!(key_of(t, s.name), Some(s.key), "{t}.{}", s.name);
                assert_eq!(name_of(t, s.key), s.name);
            }
        }
    }

    #[test]
    fn families_and_curated_names() {
        assert_eq!(name_of("And", "I1"), "In1");
        assert_eq!(name_of("Equal", "Input2"), "In2");
        assert_eq!(name_of("Not", "I"), "In");
        assert_eq!(name_of("Monoflop", "InputTrigger"), "Trigger");
        assert_eq!(name_of("PushButton", "InputTrigger"), "Toggle");
        assert_eq!(name_of("MeterAbsBi", "Pf"), "Power");
        assert_eq!(name_of("Monoflop", "Reset"), "Reset");
        assert_eq!(name_of("LightController2", "AQ3"), "AQ3");
        assert_eq!(key_of("Monoflop", "Trigger"), Some("InputTrigger"));
        assert_eq!(key_of("Monoflop", "InputTrigger"), None);
        assert_eq!(renamed("Monoflop", "InputTrigger"), Some("Trigger"));
        assert_eq!(renamed("Monoflop", "Reset"), None);
    }

    #[test]
    fn default_outputs() {
        assert_eq!(default_output("And"), Some("Q"));
        assert_eq!(default_output("Monoflop"), Some("Q"));
        assert_eq!(default_output("Formula"), Some("AQ"));
        assert_eq!(default_output("Add"), Some("AQ"));
        assert_eq!(default_output("VirtualIn"), Some("Q"));
        assert_eq!(default_output("Memory"), None);
        assert_eq!(default_output("AutoJalousie"), None);
        assert_eq!(default_output("TreeAsensor"), Some("AQ"));
    }
}
