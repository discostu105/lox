//! Canvas labels of input keys that the TechDoc scrape behind
//! `port_meta` lacks, read off Loxone Config's canvas. The table is in
//! loxone-format.md, *Canvas labels of input keys*. Hand-written: every
//! entry is verified in Loxone Config.

/// The label Loxone Config shows for a block's input key.
pub fn label(block_type: &str, key: &str) -> Option<&'static str> {
    let table: &[(&str, &'static str)] = match block_type {
        "Switch2Button" => &[
            ("InputTrigger", "Tr"),
            ("On", "On"),
            ("Reset", "Off"),
            ("InputDisable", "DisPc"),
        ],
        "PulseAt" => &[("InputDisable", "Off"), ("Remanence", "Rem")],
        "MeterDig" => &[("A", "S"), ("R", "R")],
        "WipingRelay" => &[("InputTrigger", "Tr")],
        "Counter" => &[("Reset", "Off")],
        "HourCounter" => &[("InputResetAll", "Off")],
        "MeterAbsUni" => &[("Pf", "Pf"), ("Mr", "Mr"), ("R", "R")],
        "MeterAbsBi" => &[("Pf", "Pf"), ("Mr1", "Mrc"), ("Mr2", "Mrd"), ("R", "R")],
        "AutoJalousie" | "CentralShade" => &[
            ("InputTrigger", "Tg"),
            ("InputTriggerUp", "Po"),
            ("InputTriggerDown", "Pc"),
            ("EndUp", "Co"),
            ("EndDown", "Cc"),
            ("Shade", "So"),
            ("AutoShade", "Sps"),
            ("EnAutoShade", "DisSp"),
            ("ReactAutoShade", "Spr"),
            ("Safety", "Wa"),
            ("Window", "Dwc"),
            ("Stop", "Off"),
            ("ManualPosition", "Pos"),
            ("ManualLamelle", "Slat"),
            ("Gesture", "T5"),
            ("InputDisable", "DisPc"),
            ("Type", "Type"),
            ("SO", "Wap"),
            ("Rdd", "Rdd"),
            ("Deadtime", "Bld"),
        ],
        "LightController2" => &[
            ("I1", "Lc1"),
            ("I2", "Lc2"),
            ("I3", "Lc3"),
            ("I4", "Lc4"),
            ("I5", "Lc5"),
            ("I6", "Lc6"),
            ("I7", "Lc7"),
            ("I8", "Lc8"),
            ("InputTriggerUp", "M+"),
            ("InputTriggerDown", "M-"),
            ("Select", "Mood"),
            ("Reset", "Off"),
            ("Sel1", "T5/1"),
            ("Sel2", "T5/2"),
            ("Sel3", "T5/3"),
            ("Sel4", "T5/4"),
            ("Sel5", "T5/5"),
            ("Sel6", "T5/6"),
            ("Sel7", "T5/7"),
            ("Sel8", "T5/8"),
            ("EnMove", "DisP"),
            ("Move", "Mo"),
            ("On", "On"),
            ("Alarm", "Alarm"),
            ("AlarmClock", "Buzzer"),
            ("Brightness", "Br"),
            ("InputDisable", "DisPc"),
            ("Presence", "P"),
            ("RtD", "Rtd"),
            ("MasterBr", "MBr"),
            ("Remanence", "Rem"),
            ("MoveOn", "Moet"),
        ],
        _ => return None,
    };
    if block_type == "CentralShade" && key == "Window" {
        return None;
    }
    table.iter().find(|(k, _)| *k == key).map(|(_, l)| *l)
}
