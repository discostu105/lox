//! Settings of physical inputs and outputs the source may own (D65): the
//! description, the display unit, the analog correction and the
//! transmission timing of an Air or Tree sensor, an extension's inputs
//! and outputs. `x.MinTime = 5` on an extern of such a type claims the
//! attribute the way a port assignment claims a `Def=`: the original is
//! kept in the lock and restored when the statement leaves the source.
//!
//! Only element attributes the corpus shows on these types are listed.
//! A setting the element lacks is added only where Loxone Config omits
//! it at its default (the description, the low ends of the correction,
//! `MinVal`, the reset and averaging times); the rest are rewritten,
//! never added — an analog correction on a digital input is not a thing.

/// The physical I/O types whose settings can be claimed.
pub const SETTING_TYPES: &[&str] = &[
    "LoxAIRsensor",
    "LoxAIRAsensor",
    "LoxAIRactor",
    "LoxAIRAactor",
    "TreeSensor",
    "TreeAsensor",
    "TreeActor",
    "TreeAactor",
    "Actor",
    "DigitalIn",
    "VoltageIn",
    "Lox1wireAsensor",
];

/// The claimable settings and whether a missing one may be added.
const SETTINGS: &[(&str, bool)] = &[
    ("Desc", true),
    ("Unit", false),
    ("SourceValLow", true),
    ("DestValLow", true),
    ("SourceValHigh", false),
    ("DestValHigh", false),
    ("MinVal", true),
    ("MaxVal", false),
    ("MinChange", false),
    ("MinTime", false),
    ("ResetTime", true),
    ("AverageTime", true),
    ("MaxTime", false),
];

/// The order Loxone Config writes the attributes in, after `Type`:
/// where an added one goes.
const ORDER: &[&str] = &[
    "Analog",
    "SourceValLow",
    "DestValLow",
    "SourceValHigh",
    "DestValHigh",
    "MinVal",
    "MaxVal",
    "MinChange",
    "MinTime",
    "ResetTime",
    "AverageTime",
    "MaxTime",
    "Idx",
];

/// `Some(addable)` when `name` is a claimable setting of `block_type`.
pub fn setting(block_type: &str, name: &str) -> Option<bool> {
    if !SETTING_TYPES.contains(&block_type) {
        return None;
    }
    SETTINGS.iter().find(|(n, _)| *n == name).map(|(_, a)| *a)
}

/// Whether `name` is a setting of some physical input or output.
pub fn is_setting(name: &str) -> bool {
    SETTINGS.iter().any(|(n, _)| *n == name)
}

/// The settings of a type, for messages.
pub fn names(block_type: &str) -> Vec<&'static str> {
    if SETTING_TYPES.contains(&block_type) {
        SETTINGS.iter().map(|(n, _)| *n).collect()
    } else {
        Vec::new()
    }
}

/// The lock key of a setting claim: it shares the port assignments'
/// records (`set_originals`, `removed_sets`), which key a connector UUID
/// — `#` never occurs in one.
pub fn key(object_uuid: &str, name: &str) -> String {
    format!("{object_uuid}#{name}")
}

/// `(object UUID, setting)` of a setting key, `None` for a connector UUID.
pub fn split_key(key: &str) -> Option<(&str, &str)> {
    key.split_once('#')
}

/// Where an added attribute goes: before the first present one that
/// Loxone Config writes after it; `Desc` right after `Type`.
pub fn insert_position(attrs: &[&str], name: &str) -> usize {
    if name == "Desc" {
        return attrs.iter().position(|a| *a == "Type").map_or(0, |p| p + 1);
    }
    let rank = |n: &str| ORDER.iter().position(|o| *o == n);
    let Some(mine) = rank(name) else {
        return attrs.len();
    };
    attrs
        .iter()
        .position(|a| rank(a).is_some_and(|r| r > mine))
        .unwrap_or(attrs.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_attributes_take_loxones_order() {
        let attrs = [
            "Type", "IName", "V", "U", "Title", "Nio", "WF", "Analog", "MaxVal", "MinTime",
        ];
        assert_eq!(insert_position(&attrs, "Desc"), 1);
        assert_eq!(insert_position(&attrs, "DestValLow"), 8);
        assert_eq!(insert_position(&attrs, "AverageTime"), 10);
        assert_eq!(setting("VoltageIn", "MinTime"), Some(false));
        assert_eq!(setting("VoltageIn", "Desc"), Some(true));
        assert_eq!(setting("Formula", "Desc"), None);
        assert_eq!(split_key(&key("u-1", "Desc")), Some(("u-1", "Desc")));
    }
}
