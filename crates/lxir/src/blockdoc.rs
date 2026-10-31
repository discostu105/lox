//! What each builtin block type is for: one line per type, for agents
//! choosing a type (`lxir types`, `lxir types --search`) and for the LSP
//! hover. Written for lxir, not copied from Loxone's documentation; the
//! knowledge-base page is named where it is documented in this repo
//! (docs/simulator.md), at `https://www.loxone.com/enen/kb/<page>/`.

/// The type's purpose, its main inputs and outputs, in one line.
pub fn summary(ty: &str) -> Option<&'static str> {
    Some(match ty {
        "And" => "Logic AND: Q on while every connected input is on.",
        "Or" => "Logic OR: Q on while any input is on.",
        "Not" => "Logic NOT: Q is the inverse of I.",
        "Equal" => "Comparator: Q on while Input1 equals Input2.",
        "NotEqual" => "Comparator: Q on while Input1 differs from Input2.",
        "Greater" => "Comparator: Q on while Input1 is above Input2 (a threshold, a sensor).",
        "GreaterEqual" => "Comparator: Q on while Input1 is at or above Input2.",
        "Less" => "Comparator: Q on while Input1 is below Input2 (darkness, frost).",
        "LessEqual" => "Comparator: Q on while Input1 is at or below Input2.",
        "Formula" => {
            "Formula: computes AQ from up to four analog inputs (I1–I4) with arithmetic, \
             comparisons and functions."
        }
        "Monoflop" => {
            "Monoflop: a rising edge at InputTrigger switches Q on for Time, a new edge \
             restarts it; Reset ends it."
        }
        "PulseGen" => {
            "Pulse generator: while enabled, Q is on for TimeHigh and off for TimeLow, \
             repeating (blinking, periodic tasks)."
        }
        "AnalogThresholdTrigger" => {
            "Threshold switch: Q switches on when the input reaches On and off when it \
             reaches Off — a hysteresis (heating, humidity)."
        }
        "Memory" => {
            "Memory flag: holds a value other blocks read; AQ is the input while wired, \
             Q on while it is non-zero."
        }
        "PushButton" => {
            "Switch: each pulse at InputTrigger toggles Q (a light on a push-button); \
             On and Off set and reset it."
        }
        "PButtonT" => {
            "Push-button: each pulse switches Q on for a set duration from the latest \
             pulse; with duration 0 Q follows the input."
        }
        "AutoJalousie" => {
            "Automatic shading: drives a blind or awning — up/down buttons, end \
             positions, automatic sun shading (AutoShade), safety (wind, rain), target \
             position."
        }
        "PulseAt" => "Pulse at: Q pulses at a configured point in time, for its duration.",
        "DayTimer" => {
            "Schedule: on/off or an analog value by time of day, per operating mode \
             (weekdays, holidays, user modes); write it with `schedule … end`."
        }
        "LightController2" => {
            "Lighting controller: a room's light circuits with moods, buttons, motion \
             and brightness inputs; moods with `moods … end`."
        }
        "Switch2Button" => {
            "Switch: InputTrigger toggles Q, On and Reset switch it on and off; Qon and \
             Qoff pulse on each change."
        }
        "CentralShade" => {
            "Central shading: one command (up, down, shade, safety) to many automatic \
             shading blocks."
        }
        "CentralLight" => {
            "Central lighting: moods, on and off to many lighting controllers at once \
             (all off when leaving)."
        }
        "Code16" => {
            "Program: a PicoC program with 13 analog and 3 text inputs and outputs, for \
             logic the blocks do not cover."
        }
        "OnPulseDelay" => {
            "Switch-on pulse delay: a rising edge at InputTrigger yields a pulse of \
             length Time after Delay."
        }
        "OffDelay" => {
            "Switch-off delay: Q on with the input, off Time after the input went off \
             (light after motion, run-on)."
        }
        "OnDelay" => {
            "Switch-on delay: Q on once the input has been on for Time without a break \
             (door open too long, CO2 high for minutes)."
        }
        "StairwayLS" => {
            "Stairwell light switch: each pulse switches on for a set time, with a \
             warning before it switches off."
        }
        "EdgeDetection" => {
            "Edge detection: pulses at every change of the input, on its rising and on \
             its falling edge."
        }
        "AnalogMultiplexer2" => "Analog multiplexer: AQ is Input1 or Input2, chosen by Select.",
        "SRFlipFlop" => {
            "SR flip-flop: S (InputS) sets Q, R (InputR) resets it, a pulse at the \
             toggle input toggles; S wins (latching alarms, hysteresis)."
        }
        "State" => {
            "Status: up to eight inputs map to a status text (TQ) and value (AQ) for \
             the app."
        }
        "VirtualIn" => {
            "Virtual input: a switch, button or value in the app or set over the API; \
             Q is its value."
        }
        "HeatIRoomController2" => {
            "Intelligent room controller: a room's heating and cooling by temperature, \
             comfort and eco setpoints, presence and schedule."
        }
        "Irrigation" => {
            "Irrigation: waters up to eight zones in turn for their durations; rain \
             stops it."
        }
        "Alarm" => {
            "Burglar alarm: arming, motion and contact inputs, delays, alarm levels \
             and outputs (siren, light)."
        }
        "SmokeAlarm" => {
            "Fire and water alarm: smoke, heat and water sensors raise an alarm with \
             acoustic and visual outputs."
        }
        "EFM" => {
            "Energy flow monitor: grid, production, storage and consumption of a \
             house, with the meters' statistics."
        }
        "TpfController" => {
            "Touch Pure Flex controller: presence, do-not-disturb, setpoints, \
             brightness and the display of a Touch Pure Flex."
        }
        "NfcCodeTouch" => {
            "NFC Code Touch: access by code or NFC tag per user and schedule; outputs \
             open the door and ring the bell."
        }
        "Intercom" => {
            "Intercom: doorbell, call and door-opener outputs of an intercom with \
             camera."
        }
        "MeterDig" => {
            "Meter, pulse input: counts pulses into power and energy, with day, week, \
             month and year totals."
        }
        "MeterAbsUni" => {
            "Meter, absolute reading: power and a meter reading in one direction \
             (consumption), with period totals."
        }
        "MeterAbsBi" => {
            "Meter, bidirectional: power and two meter readings (consumption and \
             feed-in), with period totals."
        }
        "MeterAbsSt" => {
            "Meter, storage: charge and discharge readings and the state of charge of \
             a battery, with period totals."
        }
        "VirtualHttpIn" => {
            "Virtual HTTP input: polls a URL; its commands (VirtualHttpInCmd) read \
             values from the response."
        }
        "VirtualHttpInCmd" => {
            "Virtual HTTP input command: one value parsed from the response of its \
             VirtualHttpIn."
        }
        "VirtualUdpIn" => {
            "Virtual UDP input: receives UDP packets on a port; its commands \
             (VirtualUdpInCmd) read values from them."
        }
        "VirtualUdpInCmd" => {
            "Virtual UDP input command: one value parsed from its VirtualUdpIn's packets."
        }
        "VirtualTextIn" => "Virtual text input: a text set in the app or over the API (TQ).",
        "VirtualOut" => {
            "Virtual output: the address of a device spoken to over HTTP or UDP; its \
             commands (VirtualOutCmd) send to it."
        }
        "VirtualOutCmd" => {
            "Virtual output command: sends CmdOn when I switches on and CmdOff when it \
             switches off, to its VirtualOut."
        }
        "ModbusServer" => {
            "Modbus server: a Modbus TCP device at an address; its devices hang in it."
        }
        "ModbusDev" => {
            "Modbus device: a unit address on a Modbus server; its sensors and actuators hang in it."
        }
        "ModbusASensor" => "Modbus analog sensor: reads a register of its Modbus device.",
        "ModbusSensor" => {
            "Modbus digital sensor: reads a coil or register bit of its Modbus device."
        }
        "ModbusAActor" => {
            "Modbus analog actuator: writes its input to a register of its Modbus device."
        }
        "Mode" => {
            "Operating mode: Q on while the mode is active; schedules name modes by \
             title. A `Mode(\"…\")` declaration creates a user-defined one."
        }
        _ => return None,
    })
}

/// The knowledge-base page of a type, where this repo documents it.
pub fn kb_page(ty: &str) -> Option<&'static str> {
    Some(match ty {
        "And" => "and",
        "Or" => "or",
        "Not" => "not",
        "Equal" | "NotEqual" | "Greater" | "GreaterEqual" | "Less" | "LessEqual" => "comparator",
        "Formula" => "formula",
        "Memory" => "memory-flags",
        "Monoflop" => "monoflop",
        "OnDelay" => "switch-on-delay",
        "OffDelay" => "switch-off-delay",
        "PushButton" => "switch",
        "PButtonT" => "push-button",
        "SRFlipFlop" => "flipflop-sr",
        "EdgeDetection" => "edge-detection",
        "AnalogThresholdTrigger" => "threshold-switch",
        "DayTimer" => "timerscheduler",
        _ => return None,
    })
}

/// The knowledge-base URL of a type, where known.
pub fn kb_url(ty: &str) -> Option<String> {
    kb_page(ty).map(|p| format!("https://www.loxone.com/enen/kb/{p}/"))
}

/// The builtin types whose name or summary contains `query`, ignoring case.
pub fn search(query: &str) -> Vec<&'static str> {
    let q = query.to_lowercase();
    crate::connectors::BUILTIN_TYPES
        .iter()
        .copied()
        .filter(|t| {
            t.to_lowercase().contains(&q)
                || summary(t).is_some_and(|s| s.to_lowercase().contains(&q))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_type_is_described() {
        for t in crate::connectors::BUILTIN_TYPES {
            let s = summary(t).unwrap_or_else(|| panic!("{t} has no summary"));
            assert!(s.ends_with('.') && s.len() < 200, "{t}: {s}");
        }
    }

    #[test]
    fn search_finds_by_purpose() {
        assert!(search("hysteresis").contains(&"AnalogThresholdTrigger"));
        assert!(search("run-on").contains(&"OffDelay"));
        assert_eq!(search("monoflop"), ["Monoflop"]);
        assert_eq!(
            kb_url("OnDelay").as_deref(),
            Some("https://www.loxone.com/enen/kb/switch-on-delay/")
        );
    }
}
