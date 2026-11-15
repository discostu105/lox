//! Settings of physical inputs and outputs the source may own: the
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

/// The Air and Tree devices and extensions whose own settings can be
/// claimed: the description, the installation place, and the switch board
/// with its row and position.
///
/// Not claimable: `PWMFrequency`, `UseDimmer` and `UseRGBDimmer`, since what
/// a value other than `123`/`true` means is unknown (no real config carries
/// one). Pairing and firmware stay in Loxone Config.
pub const DEVICE_TYPES: &[&str] = &[
    // Always carries the place and the board (103 of 103 in real configs).
    "LoxAIRDevice",
    // Always carries the place. The board only where it is mounted in one
    // (31 of 111). Loxone Config drops a board added to one without, so a
    // board is rewritten, never added.
    "TreeDevice",
    // The Tree and Air extensions: the place and the board.
    "LoxTree",
    "LoxAIR",
];

/// The claimable device settings and whether a missing one may be added.
fn device_settings(block_type: &str) -> &'static [(&'static str, bool)] {
    match block_type {
        "LoxAIRDevice" => &[
            ("Desc", true),
            ("Installation", false),
            ("SwitchBoard", false),
            ("SwitchBoardRow", false),
            ("SwitchBoardPos", false),
        ],
        "TreeDevice" => &[
            ("Desc", true),
            ("Installation", false),
            ("SwitchBoard", false),
            ("SwitchBoardRow", false),
            ("SwitchBoardPos", false),
        ],
        "LoxTree" | "LoxAIR" => &[
            ("Installation", false),
            ("SwitchBoard", false),
            ("SwitchBoardRow", false),
            ("SwitchBoardPos", false),
        ],
        _ => &[],
    }
}

/// The project (`Document`) and the Miniserver (`LoxLIVE`): the settings
/// of the house that live outside the pages, as the project's and the
/// Miniserver's properties panes in Loxone Config 17.3 show them (oracle
/// session 94).
///
/// Not claimable: the identity and bookkeeping (`U`, `ConfigVersion`,
/// `Date`, `NumO`, `APPKEY`, …), the program cycle (`SPSfreq`), the display
/// units (codes whose words are unknown), `DeviceTelemetry` (no pane shows
/// it) and the addresses Loxone Config connects to (`IntAddr`, `ExtAddr`),
/// which say nothing to the Miniserver.
pub const SYSTEM_TYPES: &[&str] = &["Document", "LoxLIVE"];

/// How a project or Miniserver setting is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemKind {
    /// The value as written, in one attribute every save writes.
    Text,
    /// A number a save writes only once it is set (`SL=`).
    AddableText,
    /// `on`/`off`: the attribute's value for each, `None` for no attribute.
    Switch {
        on: Option<&'static str>,
        off: Option<&'static str>,
    },
    /// A time of day (`02:00`), stored as minutes after midnight.
    MinuteOfDay,
    /// The automatic updates: `AUE=` and `AUM=` together.
    Updates,
}

/// One claimable project or Miniserver setting.
#[derive(Debug, Clone, Copy)]
pub struct SystemSetting {
    /// The name in source: the attribute's where that is the plain word,
    /// the pane's word where the attribute is a code.
    pub name: &'static str,
    /// The attribute it is stored in (`Updates` also writes `AUE=`).
    pub attr: &'static str,
    pub kind: SystemKind,
}

const fn text(name: &'static str, attr: &'static str) -> SystemSetting {
    SystemSetting {
        name,
        attr,
        kind: SystemKind::Text,
    }
}

const fn switch(
    name: &'static str,
    attr: &'static str,
    on: Option<&'static str>,
    off: Option<&'static str>,
) -> SystemSetting {
    SystemSetting {
        name,
        attr,
        kind: SystemKind::Switch { on, off },
    }
}

const PROJECT: &[SystemSetting] = &[
    // Where the house stands: the sun times, the weather and the
    // holidays follow from it.
    text("Street", "Street"),
    text("Town", "Town"),
    text("PostCode", "PostCode"),
    text("Country", "Ctry"),
    text("Latitude", "Latitude"),
    text("Longitude", "Longitude"),
    SystemSetting {
        name: "Altitude",
        attr: "SL",
        kind: SystemKind::AddableText,
    },
    text("Timezone", "Timezone"),
    // Empty: Loxone's own time server.
    text("Timeserver", "Timeserver"),
    text("Currency", "Currency"),
    // The automatic updates. Without them there is no Remote Connect.
    SystemSetting {
        name: "Updates",
        attr: "AUM",
        kind: SystemKind::Updates,
    },
    SystemSetting {
        name: "UpdateTime",
        attr: "UpdTime",
        kind: SystemKind::MinuteOfDay,
    },
    // Security.
    switch("AppFirewall", "FwMode", None, Some("0")),
    switch("PasswordReset", "NoPwReset", None, Some("true")),
    switch("ExtendedLogin", "ExtVal", Some("true"), None),
    // Crash and diagnostic reports to Loxone ("LOXONE Log").
    switch("LoxoneLog", "CrashL", Some("true"), None),
];

const MINISERVER: &[SystemSetting] = &[
    text("Installation", "Installation"),
    text("SwitchBoard", "SwitchBoard"),
    text("SwitchBoardRow", "SwitchBoardRow"),
    text("SwitchBoardPos", "SwitchBoardPos"),
];

/// The update modes: the word, `AUE=` and `AUM=`. A 17.2 save writes no
/// `AUM=`; Loxone Config 17.3 reads `AUE="true"` alone as automatic.
pub const UPDATE_MODES: &[(&str, Option<&str>, &str)] = &[
    ("auto", Some("true"), "1"),
    ("confirm", Some("true"), "2"),
    ("never", None, "0"),
];

/// The settings of a project or Miniserver.
pub fn system_settings(block_type: &str) -> &'static [SystemSetting] {
    match block_type {
        "Document" => PROJECT,
        "LoxLIVE" => MINISERVER,
        _ => &[],
    }
}

/// A project or Miniserver setting by name.
pub fn system_setting(block_type: &str, name: &str) -> Option<SystemSetting> {
    system_settings(block_type)
        .iter()
        .find(|s| s.name == name)
        .copied()
}

/// The order a save writes the project's attributes in (Loxone Config 17.3):
/// where an added one goes.
const PROJECT_ORDER: &[&str] = &[
    "Latitude",
    "Longitude",
    "SL",
    "Timezone",
    "Timeserver",
    "Tel",
    "EMail",
    "Web",
    "Logo",
    "Cust",
    "uuidNotifications",
    "uuidFavSeq",
    "uuidFavCol",
    "FC",
    "uuidTime",
    "12hTF",
    "dtfmt",
    "Currency",
    "UpdTime",
    "ExtVal",
    "AUE",
    "AUM",
    "DeviceTelemetry",
    "CrashL",
    "FwMode",
    "NoPwReset",
    "Geo",
    "NumO",
    "NumPresets",
    "APPKEY",
    "APPID",
];

/// The element attribute a setting is stored in: the setting's own name,
/// except where the attribute is a code (`Country` is `Ctry=`).
pub fn attr<'a>(block_type: &str, name: &'a str) -> &'a str {
    system_setting(block_type, name).map_or(name, |s| s.attr)
}

fn settings_of(block_type: &str) -> Vec<(&'static str, bool)> {
    if SETTING_TYPES.contains(&block_type) {
        SETTINGS.to_vec()
    } else if SYSTEM_TYPES.contains(&block_type) {
        system_settings(block_type)
            .iter()
            .map(|s| (s.name, s.kind != SystemKind::Text))
            .collect()
    } else {
        device_settings(block_type).to_vec()
    }
}

/// `Some(addable)` when `name` is a claimable setting of `block_type`.
pub fn setting(block_type: &str, name: &str) -> Option<bool> {
    settings_of(block_type)
        .into_iter()
        .find(|(n, _)| *n == name)
        .map(|(_, a)| a)
}

/// Whether `name` is a setting of some physical input, output or device.
pub fn is_setting(name: &str) -> bool {
    SETTINGS
        .iter()
        .chain(DEVICE_TYPES.iter().flat_map(|t| device_settings(t)))
        .any(|(n, _)| *n == name)
        || SYSTEM_TYPES
            .iter()
            .flat_map(|t| system_settings(t))
            .any(|s| s.name == name)
}

/// The settings of a type, for messages.
pub fn names(block_type: &str) -> Vec<&'static str> {
    settings_of(block_type)
        .into_iter()
        .map(|(n, _)| n)
        .collect()
}

/// The attributes of a type's settings, for the comparison.
pub fn attrs(block_type: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = settings_of(block_type)
        .into_iter()
        .map(|(n, _)| attr(block_type, n))
        .collect();
    if system_setting(block_type, "Updates").is_some() {
        out.push("AUE");
    }
    out
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
    insert_position_in(ORDER, attrs, name)
}

/// [`insert_position`] on an element of `block_type`: the project has an
/// order of its own.
pub fn insert_position_for(block_type: &str, attrs: &[&str], name: &str) -> usize {
    if block_type == "Document" {
        insert_position_in(PROJECT_ORDER, attrs, name)
    } else {
        insert_position(attrs, name)
    }
}

fn insert_position_in(order: &[&str], attrs: &[&str], name: &str) -> usize {
    if name == "Desc" {
        return attrs.iter().position(|a| *a == "Type").map_or(0, |p| p + 1);
    }
    let rank = |n: &str| order.iter().position(|o| *o == n);
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

    #[test]
    fn device_settings_are_rewritten_but_the_description() {
        assert_eq!(setting("TreeDevice", "Desc"), Some(true));
        assert_eq!(setting("TreeDevice", "SwitchBoard"), Some(false));
        assert_eq!(setting("LoxAIRDevice", "SwitchBoard"), Some(false));
        assert_eq!(setting("LoxAIRDevice", "PWMFrequency"), None);
        assert_eq!(setting("LoxAIRDevice", "MinTime"), None);
        assert!(is_setting("Installation"));
        assert_eq!(names("TreeDevice").len(), 5);
    }

    #[test]
    fn project_and_miniserver_settings_name_their_codes() {
        assert_eq!(setting("Document", "Country"), Some(false));
        assert_eq!(setting("Document", "Ctry"), None);
        assert_eq!(attr("Document", "Country"), "Ctry");
        assert_eq!(attr("Document", "Altitude"), "SL");
        assert_eq!(attr("Document", "Updates"), "AUM");
        assert_eq!(attr("TreeDevice", "Desc"), "Desc");
        assert_eq!(setting("Document", "SPSfreq"), None);
        assert_eq!(setting("LoxLIVE", "IntAddr"), None);
        assert!(is_setting("Timezone"));
        assert!(attrs("Document").contains(&"CrashL"));
        assert!(attrs("Document").contains(&"AUE"));
        assert_eq!(setting("Document", "DeviceTelemetry"), None);
        assert_eq!(setting("Document", "Altitude"), Some(true));
        assert_eq!(setting("Document", "Timezone"), Some(false));
    }

    #[test]
    fn added_project_attributes_take_the_save_order() {
        let attrs = [
            "Type",
            "Longitude",
            "Timezone",
            "Currency",
            "UpdTime",
            "AUE",
            "CrashL",
            "Geo",
        ];
        assert_eq!(insert_position_for("Document", &attrs, "SL"), 2);
        assert_eq!(insert_position_for("Document", &attrs, "AUM"), 6);
        assert_eq!(insert_position_for("Document", &attrs, "ExtVal"), 5);
        assert_eq!(insert_position_for("Document", &attrs, "FwMode"), 7);
    }
}
