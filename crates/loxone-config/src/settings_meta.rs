//! Documented units and ranges of the settings of virtual and Modbus
//! devices, which the TechDoc scrape behind `port_meta` does not cover.
//! Hand-written: every entry is read off a Loxone knowledge-base page or
//! off Loxone Config's property panel (an oracle session), cited per
//! type. Units and ranges are those of the saved attribute. The table is
//! in loxone-format.md, *Units of the settings*.
//!
//! A setting whose unit neither source shows has no entry, so its value
//! stays a plain number: the Modbus sensors' `MinTime`, which the
//! property panel does not show.

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
        // Oracle session 70, same panel: "Timeout [ms]", 10...8000, saved as is.
        ("ModbusServer", "Timeout") => m(Some("ms"), Some("10"), Some("8000")),
        // "Abfragezyklus [s]", 0...4095, saved as is.
        ("ModbusASensor" | "ModbusSensor", "ModbusPollingCycle") => {
            m(Some("s"), Some("0"), Some("4095"))
        }
        // "Zeitüberschreitung Empfang [s]", 0...unbounded, saved in ms
        // (46 s saved as 46000).
        ("ModbusASensor" | "ModbusSensor", "MaxTime") => m(Some("ms"), Some("0"), None),
        // "Minimaler Zeitabstand [s]", 0...unbounded, saved in ms (0.03 s
        // saved as 30).
        ("VirtualUdpInCmd", "MinTime") => m(Some("ms"), Some("0"), None),
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
    }

    #[test]
    fn modbus_and_udp_units() {
        let ms = Some(PortUnit::Milliseconds);
        assert_eq!(port_unit("ModbusServer", "Timeout"), ms);
        for t in ["ModbusASensor", "ModbusSensor"] {
            assert_eq!(port_unit(t, "ModbusPollingCycle"), Some(PortUnit::Seconds));
            assert_eq!(port_unit(t, "MaxTime"), ms);
            // The property panel does not show it.
            assert_eq!(port_unit(t, "MinTime"), None);
        }
        assert_eq!(port_unit("VirtualUdpInCmd", "MinTime"), ms);
        assert_eq!(port_unit("VirtualUdpInCmd", "MaxTime"), ms);
    }
}
