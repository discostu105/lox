//! Virtual devices and Modbus from source: the objects of the
//! object tree a source can create and own — HTTP, UDP and text virtual
//! inputs, virtual outputs, Modbus servers and their devices — and the
//! I/Os that hang below them.
//!
//! Unlike a page block, such an object has no position and lives in a
//! folder (a device) or in its device (an I/O). What it does is in its
//! element attributes (`Address=`, `Check=`, `ModbusAddress=` …): those the
//! dialogs show are *settings*, bound in source like attribute parameters
//! (`Address: "http://…"`). Everything else on the element — its `IName=`
//! designation once minted, the app's `<IoData>`/`<Display>`, an
//! `<C Type="Online">` status input the device carries, statistics —
//! is carried verbatim from the base, so a rebuild changes only what the
//! source states.
//!
//! The shapes below are the ones in 134 real configs, and what a save in
//! Loxone Config 17.2.8 gives a minimal element. A fresh mint is written as
//! that save writes it, so the first save after a push is a no-op.

use crate::connectors::PortDir::{Input, Output};
use crate::connectors::PortSpec;

/// Where an object of a periphery type lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Home {
    /// A device: the base's one folder of this type.
    Folder(&'static str),
    /// An I/O (or a Modbus device): inside an object of one of these types,
    /// named with `device: <slug>`.
    Device(&'static [&'static str]),
}

/// One child element a fresh mint carries, as a save writes it: element
/// name, attributes, and whether it is written `<Name></Name>`.
pub type MintChild = (&'static str, &'static [(&'static str, &'static str)], bool);

#[derive(Debug)]
pub struct PeripheryType {
    pub block_type: &'static str,
    pub home: Home,
    pub ports: &'static [PortSpec],
    /// The `IName=` designation a fresh mint gets: this prefix and one past
    /// the highest number the document holds for it (the GUI numbers
    /// designations per prefix across the whole document; HTTP and UDP
    /// commands share `VCI`, Modbus analog sensors and actuators `AMQ`).
    pub iname: Option<&'static str>,
    /// The settings: attributes the source binds (`Address: "…"`).
    pub settings: &'static [&'static str],
    /// Settings a fresh mint cannot do without.
    pub required: &'static [&'static str],
    /// A fresh mint's attributes after `Title=` (settings the source does
    /// not bind take the default given here; `None` means "only when
    /// bound"), in the order Loxone Config writes them.
    pub mint_attrs: &'static [(&'static str, Option<&'static str>)],
    /// A fresh mint's child elements after its connectors.
    pub mint_children: &'static [MintChild],
}

const fn port(key: &'static str, dir: crate::connectors::PortDir) -> PortSpec {
    PortSpec { key, dir }
}

// An HTTP or UDP command: its value (`AQ`) and the digital form (`Q`),
// both wire sources on all 128 + 61 corpus instances that are wired (and
// the house's 7).
const IN_CMD: &[PortSpec] = &[port("AQ", Output), port("Q", Output)];
// A virtual output command is driven through `I` — a wire sink on 56
// corpus instances, never a source.
const OUT_CMD: &[PortSpec] = &[port("I", Input)];
// A virtual text input: `TQ`, a source on 5 corpus and 7 house wires.
const TEXT_IN: &[PortSpec] = &[port("TQ", Output)];
// A Modbus device's `AQ` is never wired in corpus or house; by the
// inert-port rule (connector-db.md) it counts as an input.
const MODBUS_DEV: &[PortSpec] = &[port("AQ", Input)];
// Modbus sensors: `Q` and `Qe`, both wire sources on 33 corpus and 32
// house instances of the analog one. The 16 digital ones (house only)
// carry the same keys and never a wire into them.
const MODBUS_SENSOR: &[PortSpec] = &[port("Q", Output), port("Qe", Output)];
// A Modbus actuator is written through `I` (2 corpus, 1 house sink).
const MODBUS_ACTOR: &[PortSpec] = &[port("I", Input)];
// An operating mode: `I` switches it from logic, `Q` reports it —
// the two connectors on every one of the house's 18 modes, built-in and
// user-defined alike.
const MODE: &[PortSpec] = &[port("I", Input), port("Q", Output)];

const CMD_IN_SETTINGS: &[&str] = &[
    "Check",
    "Signed",
    "Analog",
    "SourceValLow",
    "DestValLow",
    "SourceValHigh",
    "DestValHigh",
    "MinVal",
    "MaxVal",
];

/// IoData and Display as the save writes them on a fresh I/O.
const IODATA: MintChild = ("IoData", &[], true);

// A calendar entry (#211): `I` and `Q` like a mode's (`-00ff`/`-01ff`
// UUIDs), never wired in the corpus's 397 entries or the house's 2.
const CALENDAR: &[PortSpec] = &[port("I", Input), port("Q", Output)];

pub const PERIPHERY: &[PeripheryType] = &[
    // A calendar entry (#211): on which dates an operating mode is on.
    // `CalType=` is the mode's `Mode=` number (absent: 0, the holiday
    // mode); `Mode=` the kind — absent a fixed date (`Month`/`Day`), 1
    // relative to Easter (`EasterOffset` days; `Month`/`Day` the date in
    // the year of the save), 3 one period (`From`/`To`, days since
    // 2009-01-01), 4 every year from `Month`/`Day` to `Month2`/`Day2`.
    // Every one of the corpus's 397 entries carries `Month`, `Day`,
    // `Month2`, `Day2` and `WeekDayInMonth` (1 where unused), in this
    // order; `From`/`To` and `EasterOffset` only where they apply.
    PeripheryType {
        block_type: "CalendarEntry",
        home: Home::Folder("CalendarCaption"),
        ports: CALENDAR,
        iname: None,
        settings: &[
            "CalType",
            "Mode",
            "Month",
            "Day",
            "Month2",
            "Day2",
            "From",
            "To",
            "EasterOffset",
            "WeekDayInMonth",
        ],
        required: &[],
        mint_attrs: &[
            ("Nio", Some("2")),
            ("WF", Some("16384")),
            ("CalType", None),
            ("Mode", None),
            ("Month", Some("1")),
            ("Day", Some("1")),
            ("Month2", Some("1")),
            ("Day2", Some("1")),
            ("From", None),
            ("To", None),
            ("EasterOffset", None),
            ("WeekDayInMonth", Some("1")),
        ],
        mint_children: &[],
    },
    // A user-defined operating mode. Its `Mode=` number is minted
    // by the compiler: one below the lowest in the document (the GUI
    // numbers user-defined modes -1, -2, … in creation order).
    PeripheryType {
        block_type: "Mode",
        home: Home::Folder("ModeCaption"),
        ports: MODE,
        iname: None,
        settings: &[],
        required: &[],
        mint_attrs: &[("Nio", Some("2")), ("WF", Some("147456"))],
        mint_children: &[IODATA, ("Display", &[("StateOnly", "true")], false)],
    },
    PeripheryType {
        block_type: "VirtualHttpIn",
        home: Home::Folder("VirtualInCaption"),
        ports: &[],
        iname: Some("VHI"),
        settings: &["Address", "Poll", "Timeout", "Allowedtimeouts"],
        required: &["Address", "Poll"],
        mint_attrs: &[
            ("Cl", Some("0,0,0")),
            ("WF", Some("16384")),
            ("Address", None),
            ("Poll", None),
            ("Timeout", Some("4000")),
            ("Allowedtimeouts", None),
        ],
        mint_children: &[],
    },
    PeripheryType {
        block_type: "VirtualHttpInCmd",
        home: Home::Device(&["VirtualHttpIn"]),
        ports: IN_CMD,
        iname: Some("VCI"),
        settings: CMD_IN_SETTINGS,
        required: &["Check"],
        mint_attrs: &[
            ("Cl", Some("0,0,0")),
            ("Nio", Some("2")),
            ("WF", Some("16384")),
            ("Check", None),
            ("Signed", Some("true")),
            ("Analog", Some("true")),
            ("SourceValLow", None),
            ("DestValLow", None),
            ("SourceValHigh", Some("100")),
            ("DestValHigh", Some("100")),
            ("MinVal", Some("-10000")),
            ("MaxVal", Some("10000")),
        ],
        mint_children: &[IODATA, ("Display", &[("Unit", "<v.1>")], false)],
    },
    PeripheryType {
        block_type: "VirtualUdpIn",
        home: Home::Folder("VirtualInCaption"),
        ports: &[],
        iname: Some("VUI"),
        settings: &["Address", "Port"],
        required: &["Port"],
        mint_attrs: &[("WF", Some("16384")), ("Address", Some("")), ("Port", None)],
        mint_children: &[],
    },
    PeripheryType {
        block_type: "VirtualUdpInCmd",
        home: Home::Device(&["VirtualUdpIn"]),
        ports: IN_CMD,
        iname: Some("VCI"),
        settings: &[
            "Check",
            "Signed",
            "Analog",
            "SourceValLow",
            "DestValLow",
            "SourceValHigh",
            "DestValHigh",
            "MinVal",
            "MaxVal",
            "MinChange",
            "MinTime",
        ],
        required: &["Check"],
        mint_attrs: &[
            ("Nio", Some("2")),
            ("WF", Some("16384")),
            ("Check", None),
            ("Signed", Some("true")),
            ("Analog", Some("true")),
            ("SourceValLow", None),
            ("DestValLow", None),
            ("SourceValHigh", Some("100")),
            ("DestValHigh", Some("100")),
            ("MinVal", Some("-10000")),
            ("MaxVal", Some("10000")),
            ("MinChange", Some("0.25")),
            ("MinTime", Some("1000")),
        ],
        mint_children: &[
            IODATA,
            ("Display", &[("Type", "2"), ("Unit", "<v.1>")], false),
        ],
    },
    PeripheryType {
        block_type: "VirtualTextIn",
        home: Home::Folder("VirtualInCaption"),
        ports: TEXT_IN,
        iname: Some("VTI"),
        settings: &["Def"],
        required: &[],
        mint_attrs: &[("Nio", Some("1")), ("WF", Some("16384")), ("Def", None)],
        mint_children: &[IODATA, ("Display", &[("Unit", "<v.1>")], false)],
    },
    PeripheryType {
        block_type: "VirtualOut",
        home: Home::Folder("VirtualOutCaption"),
        ports: &[],
        iname: Some("VQ"),
        settings: &["Address", "CloseAfterSend", "CmdSep"],
        required: &["Address"],
        mint_attrs: &[
            ("WF", Some("16384")),
            ("Address", None),
            ("CloseAfterSend", Some("true")),
            ("CmdSep", Some(";")),
        ],
        mint_children: &[IODATA],
    },
    PeripheryType {
        block_type: "VirtualOutCmd",
        home: Home::Device(&["VirtualOut"]),
        ports: OUT_CMD,
        iname: Some("VQC"),
        settings: &[
            "CmdOn",
            "CmdOnMethod",
            "CmdOnHTTP",
            "CmdOnPost",
            "CmdOff",
            "CmdAnswer",
            "Analog",
            "SourceValHigh",
            "DestValHigh",
            "Repeat",
            "RepeatRate",
        ],
        required: &[],
        mint_attrs: &[
            ("Nio", Some("1")),
            ("WF", Some("16384")),
            ("Analog", None),
            ("CmdOn", None),
            ("CmdOnMethod", None),
            ("CmdOnHTTP", None),
            ("CmdOnPost", None),
            ("CmdOff", None),
            ("CmdAnswer", None),
            ("SourceValHigh", Some("100")),
            ("DestValHigh", Some("100")),
            ("Tx", Some("false")),
            ("Repeat", None),
            ("RepeatRate", None),
        ],
        mint_children: &[
            ("IoData", &[("St", "2")], false),
            (
                "Display",
                &[("Unit", "<v.1>"), ("StateOnly", "true")],
                false,
            ),
        ],
    },
    PeripheryType {
        block_type: "ModbusServer",
        home: Home::Folder("WeatherCaption"),
        ports: &[],
        iname: None,
        settings: &["Address", "Timeout"],
        required: &["Address"],
        mint_attrs: &[
            ("Cl", Some("0,0,0")),
            ("WF", Some("16384")),
            ("Address", None),
            ("Timeout", Some("2000")),
        ],
        mint_children: &[],
    },
    PeripheryType {
        block_type: "ModbusDev",
        home: Home::Device(&["ModbusServer"]),
        ports: MODBUS_DEV,
        iname: None,
        settings: &["Channel"],
        required: &[],
        mint_attrs: &[
            ("Nio", Some("1")),
            ("WF", Some("16384")),
            ("Channel", Some("1")),
            ("NumHeatCircuits", Some("1")),
            ("NumHeatBoilers", Some("1")),
            ("NumHeatBuffers", Some("1")),
        ],
        mint_children: &[IODATA],
    },
    PeripheryType {
        block_type: "ModbusASensor",
        home: Home::Device(&["ModbusDev"]),
        ports: MODBUS_SENSOR,
        iname: Some("AMQ"),
        settings: &[
            "ModbusAddress",
            "ModbusDataType",
            "ModbusCmd",
            "ModbusPollingCycle",
            "SourceValLow",
            "DestValLow",
            "SourceValHigh",
            "DestValHigh",
            "MinVal",
            "MaxVal",
            "MinChange",
            "MinTime",
            "MaxTime",
        ],
        required: &["ModbusAddress"],
        mint_attrs: &[
            ("Nio", Some("2")),
            ("WF", Some("16384")),
            ("ValOT", Some("1")),
            ("Analog", Some("true")),
            ("ModbusAddress", None),
            ("ModbusDataType", Some("1")),
            ("ModbusCmd", Some("4")),
            ("ModbusPollingCycle", Some("60")),
            ("SourceValLow", None),
            ("DestValLow", None),
            ("SourceValHigh", Some("100")),
            ("DestValHigh", Some("100")),
            ("MinVal", Some("-10000")),
            ("MaxVal", Some("10000")),
            ("MinChange", Some("0.25")),
            ("MinTime", Some("1000")),
            ("MaxTime", Some("300000")),
        ],
        mint_children: &[
            ("IoData", &[("St", "4")], false),
            (
                "Display",
                &[("Unit", "<v.1>"), ("StateOnly", "true")],
                false,
            ),
        ],
    },
    PeripheryType {
        block_type: "ModbusSensor",
        home: Home::Device(&["ModbusDev"]),
        ports: MODBUS_SENSOR,
        iname: Some("IM"),
        settings: &[
            "MinTime",
            "ModbusAddress",
            "ModbusCmd",
            "ModbusPollingCycle",
            "MaxTime",
        ],
        required: &["ModbusAddress"],
        mint_attrs: &[
            ("Nio", Some("2")),
            ("WF", Some("16384")),
            ("ValOT", Some("1")),
            ("MinTime", Some("10")),
            ("ModbusAddress", None),
            ("ModbusCmd", Some("4")),
            ("ModbusPollingCycle", Some("60")),
            ("MaxTime", Some("50000")),
        ],
        mint_children: &[
            ("IoData", &[("St", "2")], false),
            ("Display", &[("Unit", "<v>")], false),
        ],
    },
    PeripheryType {
        block_type: "ModbusAActor",
        home: Home::Device(&["ModbusDev"]),
        ports: MODBUS_ACTOR,
        iname: Some("AMQ"),
        settings: &[
            "Channel",
            "SourceValHigh",
            "DestValHigh",
            "DimmType",
            "ModbusAddress",
            "ModbusCmd",
            "ModbusCoilQuantity",
            "ModbusDataType",
            "RepeatRate",
        ],
        required: &["ModbusAddress"],
        mint_attrs: &[
            ("Nio", Some("1")),
            ("WF", Some("16384")),
            ("Channel", Some("1")),
            ("SourceValHigh", Some("100")),
            ("DestValHigh", Some("100")),
            ("DimmType", None),
            ("ModbusAddress", None),
            ("ModbusCmd", Some("6")),
            ("ModbusCoilQuantity", Some("1")),
            ("ModbusDataType", Some("1")),
            ("RepeatRate", None),
        ],
        mint_children: &[
            IODATA,
            (
                "Display",
                &[("Unit", "<v.1>"), ("StateOnly", "true")],
                false,
            ),
        ],
    },
];

/// The periphery type of this name, if it is one.
pub fn periphery(block_type: &str) -> Option<&'static PeripheryType> {
    PERIPHERY.iter().find(|p| p.block_type == block_type)
}

/// The device types an I/O of this type may hang below (`device:`).
pub fn device_types(block_type: &str) -> &'static [&'static str] {
    match periphery(block_type).map(|p| p.home) {
        Some(Home::Device(parents)) => parents,
        _ => &[],
    }
}

/// The folder a device of this type lives in.
pub fn folder(block_type: &str) -> Option<&'static str> {
    match periphery(block_type)?.home {
        Home::Folder(f) => Some(f),
        Home::Device(_) => None,
    }
}

/// Whether decompile states a missing setting as 0: a save leaves it out
/// at 0, and a fresh mint needs it or would fill in another default, so
/// lifting nothing would fail or change the value when the source is
/// minted elsewhere.
pub fn zero_when_missing(block_type: &str, name: &str) -> bool {
    let Some(p) = periphery(block_type) else {
        return false;
    };
    crate::connectors::omitted_when_zero(block_type, name)
        && (p.required.contains(&name)
            || p.mint_attrs
                .iter()
                .find(|(n, _)| *n == name)
                .and_then(|(_, d)| *d)
                .is_some_and(|d| !crate::connectors::is_zero(d)))
}

/// `device:` on a block names the device it hangs below — an
/// identity binding like `mirrors:`, not a value.
pub fn is_device_binding(block_type: &str, key: &str) -> bool {
    key == "device" && !device_types(block_type).is_empty()
}
