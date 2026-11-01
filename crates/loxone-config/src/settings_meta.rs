//! Documented units and ranges of the settings of virtual and Modbus
//! devices, which the TechDoc scrape behind `port_meta` does not cover.
//! Hand-written: every entry is read off a Loxone knowledge-base page,
//! cited per type. The table is in loxone-format.md, *Virtual and Modbus
//! devices*.
//!
//! A setting whose unit no Loxone page states has no entry, so its value
//! stays a plain number: the Modbus server's `Timeout`, the Modbus
//! sensors' `ModbusPollingCycle`, `MinTime` and `MaxTime`, and the
//! virtual UDP command's `MinTime` (reference `kb-modbus-tcp` names the
//! timeout and the polling cycle without a unit).

use crate::port_meta::PortMeta;

const fn m(
    unit: Option<&'static str>,
    min: Option<&'static str>,
    max: Option<&'static str>,
) -> PortMeta {
    PortMeta {
        unit,
        label: None,
        min,
        max,
        default: None,
    }
}

/// The documented unit and range of a device setting, if any.
pub fn documented(block_type: &str, setting: &str) -> Option<PortMeta> {
    Some(match (block_type, setting) {
        // Reference `kb-virtual-http-input`, *Properties*: "Polling cycle
        // in seconds (Minimum 10s)", s, 10...604800; "Timeout", ms,
        // 10...8000; "Number of permitted timeouts", 0...100.
        ("VirtualHttpIn", "Poll") => m(Some("s"), Some("10"), Some("604800")),
        ("VirtualHttpIn", "Timeout") => m(Some("ms"), Some("10"), Some("8000")),
        ("VirtualHttpIn", "Allowedtimeouts") => m(None, Some("0"), Some("100")),
        // Oracle session 84, Loxone Config 17.2 property panel:
        // "Zeitüberschreitung Empfang [s]", saved in ms (7 s saved as
        // 7000), absent at 0.
        ("VirtualUdpInCmd", "MaxTime") => m(Some("ms"), Some("0"), None),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crate::connectors::{PortUnit, port_unit};

    #[test]
    fn virtual_http_input_units() {
        assert_eq!(port_unit("VirtualHttpIn", "Poll"), Some(PortUnit::Seconds));
        assert_eq!(
            port_unit("VirtualHttpIn", "Timeout"),
            Some(PortUnit::Milliseconds)
        );
        assert_eq!(port_unit("VirtualHttpIn", "Allowedtimeouts"), None);
        // No Loxone page states these units.
        assert_eq!(port_unit("ModbusServer", "Timeout"), None);
        assert_eq!(port_unit("ModbusASensor", "MaxTime"), None);
        assert_eq!(
            port_unit("VirtualUdpInCmd", "MaxTime"),
            Some(PortUnit::Milliseconds)
        );
    }
}
