//! Hardware the source can own: Tree, Air and extension devices that
//! Loxone Config paired, and the inputs and outputs below them. A
//! declaration takes over an existing element. It is matched by its
//! serial (a device) or its designation inside its device (an I/O), and
//! the source owns its title, room, category and settings
//! ([`crate::settings`]). Pairing stays in Loxone Config.

/// The device types a declaration can take over, with the I/O types that
/// hang below each.
const DEVICES: &[(&str, &[&str])] = &[
    (
        "TreeDevice",
        &["TreeSensor", "TreeAsensor", "TreeActor", "TreeAactor"],
    ),
    (
        "LoxAIRDevice",
        &[
            "LoxAIRsensor",
            "LoxAIRAsensor",
            "LoxAIRactor",
            "LoxAIRAactor",
        ],
    ),
    // The Tree and Air extensions: their devices are declared on their own.
    ("LoxTree", &[]),
    ("LoxAIR", &[]),
];

/// Whether a declaration of this type takes over a hardware device.
pub fn is_device(block_type: &str) -> bool {
    DEVICES.iter().any(|(d, _)| *d == block_type)
}

/// The device types an I/O of this type hangs below; empty for any other
/// type.
pub fn parents(block_type: &str) -> Vec<&'static str> {
    DEVICES
        .iter()
        .filter(|(_, ios)| ios.contains(&block_type))
        .map(|(d, _)| *d)
        .collect()
}

/// Whether a declaration of this type takes over a hardware I/O.
pub fn is_io(block_type: &str) -> bool {
    DEVICES.iter().any(|(_, ios)| ios.contains(&block_type))
}

/// Whether a declaration of this type takes over hardware.
pub fn is_hardware(block_type: &str) -> bool {
    is_device(block_type) || is_io(block_type)
}

/// The I/O types below a device type.
pub fn ios(device_type: &str) -> &'static [&'static str] {
    DEVICES
        .iter()
        .find(|(d, _)| *d == device_type)
        .map_or(&[], |(_, ios)| ios)
}

/// The statistics setting: `Statistics: on` writes the `StatsType=` the
/// type records with, `off` removes it with its `StatsAutoDel=`. A save
/// keeps either.
pub const STATISTICS: &str = "Statistics";

/// The `StatsType` a type records with: 7 on a digital input or output, 11
/// on an analog sensor. Every hardware I/O of these types with statistics
/// in real configs carries this value, and the app catalogue knows both.
pub fn stats_value(block_type: &str) -> Option<&'static str> {
    match block_type {
        "TreeSensor" | "TreeActor" | "LoxAIRsensor" | "LoxAIRactor" => Some("7"),
        "TreeAsensor" | "LoxAIRAsensor" => Some("11"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_and_their_ios() {
        assert!(is_device("TreeDevice"));
        assert!(is_io("LoxAIRAsensor"));
        assert_eq!(parents("TreeAactor"), ["TreeDevice"]);
        assert!(parents("Formula").is_empty());
        assert!(!is_hardware("VirtualIn"));
        assert_eq!(stats_value("TreeAsensor"), Some("11"));
        assert_eq!(stats_value("TreeAactor"), None);
    }
}
