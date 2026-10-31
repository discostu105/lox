//! The Automatic Designer's codes for conditions and actions on blocks,
//! as the Miniserver serves them to the app (`jdev/sps/io/<AutoPilot
//! container>/capabilities/getraw`, version 20260521DEU, read from the
//! house's Miniserver, firmware 17.2, 2026-10-31). The app builds every
//! rule from this table: a condition on a block carries the `descID` of
//! one of its `events`, the block's `uuidAction` as `controlUUID` and the
//! UUID of the state named by `sourceState` as `id`; an action carries the
//! `descID` of one of its `actions` and the command text of `action`, its
//! `<valN>` placeholders filled in. Only the codes are kept, not the
//! localized texts. Generated; one row per app control type.

/// What the value of a condition or an action is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Digital,
    Analog,
    Text,
    Pulse,
}

/// A condition a block offers: its code, the state it watches and, for a
/// digital one, the value it waits for.
#[derive(Debug, Clone, Copy)]
pub struct EventCap {
    pub desc: i64,
    pub kind: Kind,
    pub state: Option<&'static str>,
    pub value: Option<i64>,
}

/// An action a block offers: its code and its command text.
#[derive(Debug, Clone, Copy)]
pub struct ActionCap {
    pub desc: i64,
    pub kind: Kind,
    pub command: &'static str,
}

/// The conditions and actions of the blocks of some app control types.
#[derive(Debug, Clone, Copy)]
pub struct Capability {
    pub control_types: &'static [&'static str],
    pub events: &'static [EventCap],
    pub actions: &'static [ActionCap],
}

pub const CAPABILITIES: &[Capability] = &[
    Capability {
        control_types: &["InfoOnlyDigital"],
        events: &[
            EventCap { desc: 41001, kind: Kind::Digital, state: Some("active"), value: None },
        ],
        actions: &[],
    },
    Capability {
        control_types: &["InfoOnlyAnalog"],
        events: &[
            EventCap { desc: 42001, kind: Kind::Analog, state: Some("value"), value: None },
        ],
        actions: &[],
    },
    Capability {
        control_types: &["LeftRightDigital"],
        events: &[],
        actions: &[
            ActionCap { desc: 44104, kind: Kind::Pulse, command: "pulseup" },
            ActionCap { desc: 44105, kind: Kind::Pulse, command: "pulsedown" },
        ],
    },
    Capability {
        control_types: &["UpDownDigital"],
        events: &[],
        actions: &[
            ActionCap { desc: 358101, kind: Kind::Pulse, command: "up" },
            ActionCap { desc: 358102, kind: Kind::Pulse, command: "down" },
            ActionCap { desc: 44102, kind: Kind::Pulse, command: "pulseup" },
            ActionCap { desc: 44103, kind: Kind::Pulse, command: "pulsedown" },
        ],
    },
    Capability {
        control_types: &["Slider", "LeftRightAnalog", "UpDownAnalog"],
        events: &[
            EventCap { desc: 44001, kind: Kind::Analog, state: Some("value"), value: None },
        ],
        actions: &[
            ActionCap { desc: 44101, kind: Kind::Analog, command: "<val0>" },
        ],
    },
    Capability {
        control_types: &["LightControllerV2"],
        events: &[
            EventCap { desc: 484001, kind: Kind::Analog, state: Some("activeMoodsNum"), value: None },
            EventCap { desc: 484002, kind: Kind::Digital, state: Some("activeMoodsNum"), value: Some(778) },
            EventCap { desc: 484003, kind: Kind::Digital, state: Some("activeMoodsNum"), value: Some(778) },
        ],
        actions: &[
            ActionCap { desc: 484101, kind: Kind::Analog, command: "changeTo/<val0>" },
            ActionCap { desc: 484102, kind: Kind::Analog, command: "addmood/<val0>" },
            ActionCap { desc: 484103, kind: Kind::Pulse, command: "on" },
            ActionCap { desc: 484104, kind: Kind::Pulse, command: "reset" },
        ],
    },
    Capability {
        control_types: &["CentralLightController"],
        events: &[
            EventCap { desc: 488001, kind: Kind::Digital, state: Some("events"), value: Some(8) },
            EventCap { desc: 488002, kind: Kind::Digital, state: Some("events"), value: Some(4) },
        ],
        actions: &[
            ActionCap { desc: 488101, kind: Kind::Pulse, command: "on" },
            ActionCap { desc: 488102, kind: Kind::Pulse, command: "reset" },
        ],
    },
    Capability {
        control_types: &["Jalousie", "Airmotor"],
        events: &[
            EventCap { desc: 348001, kind: Kind::Digital, state: Some("autoActive"), value: Some(1) },
            EventCap { desc: 348002, kind: Kind::Analog, state: Some("position"), value: None },
            EventCap { desc: 348003, kind: Kind::Digital, state: Some("position"), value: Some(0) },
            EventCap { desc: 348004, kind: Kind::Digital, state: Some("position"), value: Some(1) },
            EventCap { desc: 348005, kind: Kind::Digital, state: Some("safetyActive"), value: Some(1) },
            EventCap { desc: 348006, kind: Kind::Digital, state: Some("locked"), value: Some(1) },
        ],
        actions: &[
            ActionCap { desc: 348101, kind: Kind::Pulse, command: "fullup" },
            ActionCap { desc: 348102, kind: Kind::Pulse, command: "fulldown" },
            ActionCap { desc: 348103, kind: Kind::Pulse, command: "shade" },
            ActionCap { desc: 348104, kind: Kind::Pulse, command: "stop" },
            ActionCap { desc: 348105, kind: Kind::Pulse, command: "auto" },
            ActionCap { desc: 348106, kind: Kind::Pulse, command: "noauto" },
            ActionCap { desc: 348107, kind: Kind::Digital, command: "vsafetyon" },
            ActionCap { desc: 348108, kind: Kind::Digital, command: "vsafetyoff" },
            ActionCap { desc: 348109, kind: Kind::Analog, command: "manualPosition/<val0>/force" },
            ActionCap { desc: 348110, kind: Kind::Analog, command: "manualLamelle/<val0>/force" },
            ActionCap { desc: 348111, kind: Kind::Analog, command: "manualPosBlind/<val0>/<val1>" },
        ],
    },
    Capability {
        control_types: &["CentralJalousie"],
        events: &[
            EventCap { desc: 489001, kind: Kind::Digital, state: Some("events"), value: Some(1) },
            EventCap { desc: 489002, kind: Kind::Digital, state: Some("events"), value: Some(2) },
            EventCap { desc: 489003, kind: Kind::Digital, state: Some("events"), value: Some(64) },
            EventCap { desc: 489004, kind: Kind::Digital, state: Some("events"), value: Some(128) },
        ],
        actions: &[
            ActionCap { desc: 489101, kind: Kind::Pulse, command: "fullup" },
            ActionCap { desc: 489102, kind: Kind::Pulse, command: "fulldown" },
            ActionCap { desc: 489103, kind: Kind::Pulse, command: "shade" },
            ActionCap { desc: 489104, kind: Kind::Pulse, command: "stop" },
            ActionCap { desc: 489105, kind: Kind::Pulse, command: "auto" },
            ActionCap { desc: 489106, kind: Kind::Pulse, command: "noauto" },
        ],
    },
    Capability {
        control_types: &["AudioPlayerGroup"],
        events: &[],
        actions: &[
            ActionCap { desc: 508101, kind: Kind::Pulse, command: "play" },
            ActionCap { desc: 508102, kind: Kind::Pulse, command: "pause" },
            ActionCap { desc: 508103, kind: Kind::Pulse, command: "next" },
            ActionCap { desc: 508104, kind: Kind::Pulse, command: "prev" },
            ActionCap { desc: 508105, kind: Kind::Analog, command: "playZoneFav/<val0>/force" },
            ActionCap { desc: 508106, kind: Kind::Analog, command: "volume/<val0>/force" },
            ActionCap { desc: 508107, kind: Kind::Text, command: "tts/<val0>" },
            ActionCap { desc: 508108, kind: Kind::Analog, command: "volume/+<val0>" },
            ActionCap { desc: 508109, kind: Kind::Analog, command: "volume/-<val0>" },
        ],
    },
    Capability {
        control_types: &["AudioZoneV2"],
        events: &[
            EventCap { desc: 507001, kind: Kind::Digital, state: Some("power"), value: Some(1) },
            EventCap { desc: 507002, kind: Kind::Digital, state: Some("power"), value: Some(0) },
        ],
        actions: &[
            ActionCap { desc: 507101, kind: Kind::Pulse, command: "play" },
            ActionCap { desc: 507102, kind: Kind::Pulse, command: "pause" },
            ActionCap { desc: 507103, kind: Kind::Pulse, command: "next" },
            ActionCap { desc: 507104, kind: Kind::Pulse, command: "prev" },
            ActionCap { desc: 507105, kind: Kind::Analog, command: "playZoneFav/<val0>/force" },
            ActionCap { desc: 507106, kind: Kind::Analog, command: "volume/<val0>/force" },
            ActionCap { desc: 507107, kind: Kind::Text, command: "tts/<val0>" },
            ActionCap { desc: 507108, kind: Kind::Analog, command: "volume/+<val0>" },
            ActionCap { desc: 507109, kind: Kind::Analog, command: "volume/-<val0>" },
            ActionCap { desc: 507110, kind: Kind::Text, command: "grouptts/<val0>" },
            ActionCap { desc: 507111, kind: Kind::Analog, command: "lineIn/<val0>/force" },
        ],
    },
    Capability {
        control_types: &["MediaClient", "AudioZone"],
        events: &[],
        actions: &[
            ActionCap { desc: 463101, kind: Kind::Pulse, command: "play" },
            ActionCap { desc: 463102, kind: Kind::Pulse, command: "pause" },
            ActionCap { desc: 463103, kind: Kind::Pulse, command: "next" },
            ActionCap { desc: 463104, kind: Kind::Pulse, command: "prev" },
            ActionCap { desc: 463105, kind: Kind::Analog, command: "playZoneFav/<val0>" },
            ActionCap { desc: 463106, kind: Kind::Analog, command: "volume/<val0>" },
            ActionCap { desc: 463107, kind: Kind::Text, command: "tts/<val0>" },
            ActionCap { desc: 463108, kind: Kind::Analog, command: "volume/+<val0>" },
            ActionCap { desc: 463109, kind: Kind::Analog, command: "volume/-<val0>" },
        ],
    },
    Capability {
        control_types: &["CentralAudioZone"],
        events: &[
            EventCap { desc: 491001, kind: Kind::Digital, state: Some("events"), value: Some(1) },
            EventCap { desc: 491002, kind: Kind::Digital, state: Some("events"), value: Some(8) },
        ],
        actions: &[
            ActionCap { desc: 491101, kind: Kind::Pulse, command: "play" },
            ActionCap { desc: 491102, kind: Kind::Pulse, command: "pause" },
        ],
    },
    Capability {
        control_types: &["Alarm"],
        events: &[
            EventCap { desc: 373001, kind: Kind::Digital, state: Some("armed"), value: Some(0) },
            EventCap { desc: 373002, kind: Kind::Digital, state: Some("armed"), value: Some(0) },
            EventCap { desc: 373003, kind: Kind::Analog, state: Some("level"), value: Some(1) },
            EventCap { desc: 373004, kind: Kind::Digital, state: Some("level"), value: Some(0) },
            EventCap { desc: 373005, kind: Kind::Digital, state: Some("events"), value: Some(2) },
            EventCap { desc: 373006, kind: Kind::Digital, state: Some("events"), value: Some(1) },
            EventCap { desc: 373007, kind: Kind::Digital, state: Some("events"), value: Some(4) },
            EventCap { desc: 373008, kind: Kind::Digital, state: Some("events"), value: Some(8) },
        ],
        actions: &[
            ActionCap { desc: 373101, kind: Kind::Pulse, command: "on/1" },
            ActionCap { desc: 373102, kind: Kind::Pulse, command: "on/0" },
            ActionCap { desc: 373103, kind: Kind::Pulse, command: "delayedon/1" },
            ActionCap { desc: 373104, kind: Kind::Pulse, command: "delayedon/0" },
            ActionCap { desc: 373105, kind: Kind::Pulse, command: "startdrill" },
            ActionCap { desc: 373106, kind: Kind::Pulse, command: "off" },
            ActionCap { desc: 373107, kind: Kind::Pulse, command: "quit" },
        ],
    },
    Capability {
        control_types: &["CentralAlarm"],
        events: &[
            EventCap { desc: 492001, kind: Kind::Digital, state: Some("events"), value: Some(4) },
            EventCap { desc: 492002, kind: Kind::Digital, state: Some("events"), value: Some(8) },
        ],
        actions: &[
            ActionCap { desc: 492101, kind: Kind::Pulse, command: "on/1" },
            ActionCap { desc: 492102, kind: Kind::Pulse, command: "on/0" },
            ActionCap { desc: 492103, kind: Kind::Pulse, command: "delayedon/1" },
            ActionCap { desc: 492104, kind: Kind::Pulse, command: "delayedon/0" },
            ActionCap { desc: 492106, kind: Kind::Pulse, command: "off" },
            ActionCap { desc: 492107, kind: Kind::Pulse, command: "quit" },
        ],
    },
    Capability {
        control_types: &["ValueSelector"],
        events: &[],
        actions: &[
            ActionCap { desc: 345101, kind: Kind::Analog, command: "pos/<val0>/force" },
            ActionCap { desc: 345102, kind: Kind::Pulse, command: "up" },
            ActionCap { desc: 345103, kind: Kind::Pulse, command: "down" },
        ],
    },
    Capability {
        control_types: &["Switch"],
        events: &[
            EventCap { desc: 342001, kind: Kind::Digital, state: Some("active"), value: Some(1) },
            EventCap { desc: 342002, kind: Kind::Digital, state: Some("active"), value: Some(0) },
        ],
        actions: &[
            ActionCap { desc: 342101, kind: Kind::Pulse, command: "on" },
            ActionCap { desc: 342102, kind: Kind::Pulse, command: "off" },
        ],
    },
    Capability {
        control_types: &["TimedSwitch"],
        events: &[
            EventCap { desc: 340001, kind: Kind::Digital, state: None, value: Some(1) },
            EventCap { desc: 340002, kind: Kind::Digital, state: None, value: Some(0) },
        ],
        actions: &[
            ActionCap { desc: 340101, kind: Kind::Pulse, command: "pulse" },
            ActionCap { desc: 340102, kind: Kind::Pulse, command: "off" },
            ActionCap { desc: 340103, kind: Kind::Pulse, command: "pon" },
        ],
    },
    Capability {
        control_types: &["Pushbutton"],
        events: &[
            EventCap { desc: 493001, kind: Kind::Digital, state: Some("active"), value: Some(1) },
            EventCap { desc: 493002, kind: Kind::Digital, state: Some("active"), value: Some(0) },
        ],
        actions: &[
            ActionCap { desc: 493101, kind: Kind::Pulse, command: "on" },
            ActionCap { desc: 493102, kind: Kind::Pulse, command: "off" },
            ActionCap { desc: 493103, kind: Kind::Pulse, command: "pulse" },
            ActionCap { desc: 511101, kind: Kind::Pulse, command: "pulse" },
        ],
    },
    Capability {
        control_types: &["AalSmartAlarm"],
        events: &[
            EventCap { desc: 503001, kind: Kind::Digital, state: Some("alarmLevel"), value: Some(1) },
            EventCap { desc: 503002, kind: Kind::Digital, state: Some("alarmLevel"), value: Some(2) },
            EventCap { desc: 503003, kind: Kind::Digital, state: Some("isLeaveActive"), value: Some(1) },
            EventCap { desc: 503004, kind: Kind::Digital, state: Some("isLeaveActive"), value: Some(0) },
            EventCap { desc: 503005, kind: Kind::Digital, state: Some("isLocked"), value: Some(1) },
            EventCap { desc: 503006, kind: Kind::Digital, state: Some("isLocked"), value: Some(0) },
            EventCap { desc: 503007, kind: Kind::Digital, state: Some("alarmLevel"), value: Some(0) },
        ],
        actions: &[
            ActionCap { desc: 503101, kind: Kind::Pulse, command: "confirm" },
            ActionCap { desc: 503102, kind: Kind::Pulse, command: "startDrill" },
            ActionCap { desc: 503103, kind: Kind::Analog, command: "disable/<val0>" },
        ],
    },
    Capability {
        control_types: &["Hourcounter"],
        events: &[
            EventCap { desc: 380001, kind: Kind::Digital, state: Some("overdue"), value: Some(1) },
            EventCap { desc: 380002, kind: Kind::Analog, state: Some("totalTime"), value: None },
            EventCap { desc: 380003, kind: Kind::Analog, state: Some("totalTime"), value: None },
            EventCap { desc: 380004, kind: Kind::Analog, state: Some("totalTime"), value: None },
            EventCap { desc: 380005, kind: Kind::Analog, state: Some("totalTime"), value: None },
        ],
        actions: &[
            ActionCap { desc: 380101, kind: Kind::Pulse, command: "reset" },
            ActionCap { desc: 380102, kind: Kind::Pulse, command: "resetall" },
            ActionCap { desc: 380103, kind: Kind::Pulse, command: "enable" },
            ActionCap { desc: 380104, kind: Kind::Pulse, command: "disable" },
        ],
    },
    Capability {
        control_types: &["SmokeAlarm"],
        events: &[
            EventCap { desc: 475001, kind: Kind::Digital, state: Some("level"), value: Some(1) },
            EventCap { desc: 475002, kind: Kind::Digital, state: Some("level"), value: Some(2) },
        ],
        actions: &[
            ActionCap { desc: 475101, kind: Kind::Pulse, command: "mute" },
            ActionCap { desc: 475102, kind: Kind::Pulse, command: "confirm" },
            ActionCap { desc: 475103, kind: Kind::Pulse, command: "startDrill" },
        ],
    },
    Capability {
        control_types: &["Window"],
        events: &[
            EventCap { desc: 501001, kind: Kind::Analog, state: Some("position"), value: None },
            EventCap { desc: 501002, kind: Kind::Digital, state: Some("position"), value: Some(0) },
            EventCap { desc: 501003, kind: Kind::Digital, state: Some("position"), value: Some(1) },
        ],
        actions: &[
            ActionCap { desc: 501101, kind: Kind::Pulse, command: "fullopen" },
            ActionCap { desc: 501102, kind: Kind::Pulse, command: "fullclose" },
            ActionCap { desc: 501103, kind: Kind::Analog, command: "moveToPosition/<val0>/force" },
        ],
    },
    Capability {
        control_types: &["Dimmer", "EIBDimmer"],
        events: &[],
        actions: &[
            ActionCap { desc: 346101, kind: Kind::Analog, command: "<val0>/force" },
            ActionCap { desc: 346102, kind: Kind::Pulse, command: "reset" },
        ],
    },
    Capability {
        control_types: &["Fronius"],
        events: &[
            EventCap { desc: 465001, kind: Kind::Analog, state: Some("prodCurr"), value: None },
            EventCap { desc: 465002, kind: Kind::Analog, state: Some("consCurr"), value: None },
            EventCap { desc: 465003, kind: Kind::Analog, state: Some("gridCurr"), value: None },
            EventCap { desc: 465004, kind: Kind::Analog, state: Some("deliveryDay"), value: None },
            EventCap { desc: 465005, kind: Kind::Analog, state: Some("stateOfCharge"), value: None },
            EventCap { desc: 465006, kind: Kind::Analog, state: Some("batteryCurr"), value: None },
            EventCap { desc: 465007, kind: Kind::Analog, state: Some("prodTotal"), value: None },
            EventCap { desc: 465008, kind: Kind::Analog, state: Some("consTotal"), value: None },
            EventCap { desc: 465009, kind: Kind::Analog, state: Some("earningsTotal"), value: None },
        ],
        actions: &[],
    },
    Capability {
        control_types: &["Meter", "MeterV2"],
        events: &[
            EventCap { desc: 386001, kind: Kind::Analog, state: Some("total"), value: None },
            EventCap { desc: 386002, kind: Kind::Analog, state: Some("actual"), value: None },
            EventCap { desc: 518002, kind: Kind::Analog, state: Some("totalNeg"), value: None },
            EventCap { desc: 519001, kind: Kind::Analog, state: Some("total"), value: None },
            EventCap { desc: 519002, kind: Kind::Analog, state: Some("totalNeg"), value: None },
            EventCap { desc: 519004, kind: Kind::Analog, state: Some("storage"), value: None },
        ],
        actions: &[],
    },
    Capability {
        control_types: &["PvProductionForecast"],
        events: &[
            EventCap { desc: 543001, kind: Kind::Analog, state: Some("power"), value: None },
            EventCap { desc: 543002, kind: Kind::Analog, state: Some("period"), value: None },
            EventCap { desc: 543003, kind: Kind::Analog, state: Some("today"), value: None },
            EventCap { desc: 543004, kind: Kind::Analog, state: Some("tomorrow"), value: None },
            EventCap { desc: 543005, kind: Kind::Digital, state: Some("ready"), value: Some(1) },
        ],
        actions: &[],
    },
    Capability {
        control_types: &["WindowMonitor"],
        events: &[
            EventCap { desc: 482001, kind: Kind::Analog, state: Some("numOpen"), value: None },
            EventCap { desc: 482002, kind: Kind::Analog, state: Some("numClosed"), value: None },
            EventCap { desc: 482003, kind: Kind::Analog, state: Some("numTilted"), value: None },
            EventCap { desc: 482004, kind: Kind::Analog, state: Some("numLocked"), value: None },
            EventCap { desc: 482005, kind: Kind::Analog, state: Some("numUnlocked"), value: None },
            EventCap { desc: 482006, kind: Kind::Analog, state: Some("numOffline"), value: None },
            EventCap { desc: 482007, kind: Kind::Digital, state: Some("numOpen"), value: Some(1) },
            EventCap { desc: 482008, kind: Kind::Digital, state: Some("numOffline"), value: Some(1) },
        ],
        actions: &[],
    },
    Capability {
        control_types: &["IRoomcontroller"],
        events: &[
            EventCap { desc: 451001, kind: Kind::Analog, state: Some("tempActual"), value: None },
            EventCap { desc: 451002, kind: Kind::Digital, state: Some("openWindow"), value: Some(1) },
        ],
        actions: &[
            ActionCap { desc: 451101, kind: Kind::Analog, command: "mode/<val0>" },
            ActionCap { desc: 451102, kind: Kind::Analog, command: "override/<val0>/<val0>" },
            ActionCap { desc: 451103, kind: Kind::Pulse, command: "stoptimer" },
        ],
    },
    Capability {
        control_types: &["IRoomControllerV2", "IRoomControllerV2021"],
        events: &[
            EventCap { desc: 469001, kind: Kind::Analog, state: Some("tempActual"), value: None },
            EventCap { desc: 469002, kind: Kind::Digital, state: Some("openWindow"), value: Some(1) },
            EventCap { desc: 469003, kind: Kind::Digital, state: Some("activeMode"), value: Some(1) },
            EventCap { desc: 469004, kind: Kind::Digital, state: Some("activeMode"), value: Some(0) },
            EventCap { desc: 469005, kind: Kind::Digital, state: Some("activeMode"), value: Some(2) },
            EventCap { desc: 469007, kind: Kind::Digital, state: Some("prepareState"), value: Some(1) },
        ],
        actions: &[
            ActionCap { desc: 469101, kind: Kind::Analog, command: "setoperatingmode/<val0>" },
            ActionCap { desc: 469102, kind: Kind::Analog, command: "overrideRelative/1/<val0>" },
            ActionCap { desc: 469103, kind: Kind::Analog, command: "overrideRelative/0/<val0>" },
            ActionCap { desc: 469104, kind: Kind::Analog, command: "overrideRelative/2/<val0>" },
            ActionCap { desc: 469105, kind: Kind::Pulse, command: "stopoverride" },
            ActionCap { desc: 469106, kind: Kind::Analog, command: "overrideDuration/1/<val0>" },
            ActionCap { desc: 469107, kind: Kind::Analog, command: "overrideDuration/0/<val0>" },
            ActionCap { desc: 469108, kind: Kind::Analog, command: "overrideDuration/2/<val0>" },
        ],
    },
    Capability {
        control_types: &["Ventilation"],
        events: &[
            EventCap { desc: 496001, kind: Kind::Analog, state: Some("humidityIndoor"), value: None },
            EventCap { desc: 496002, kind: Kind::Analog, state: Some("airQualityIndoor"), value: None },
            EventCap { desc: 496003, kind: Kind::Analog, state: Some("temperatureOutdoor"), value: None },
            EventCap { desc: 496004, kind: Kind::Digital, state: Some("activeMode"), value: Some(5) },
            EventCap { desc: 496005, kind: Kind::Digital, state: Some("activeMode"), value: Some(9) },
            EventCap { desc: 496006, kind: Kind::Digital, state: Some("activeMode"), value: Some(6) },
            EventCap { desc: 496007, kind: Kind::Digital, state: Some("activeMode"), value: Some(8) },
        ],
        actions: &[
            ActionCap { desc: 496101, kind: Kind::Pulse, command: "ackfilterchange" },
            ActionCap { desc: 496102, kind: Kind::Analog, command: "settimerendtime/<val0>/0/2/0" },
            ActionCap { desc: 496103, kind: Kind::Analog, command: "settimerendtime/<val0>/100/2/1" },
            ActionCap { desc: 496104, kind: Kind::Analog, command: "settimerendtime/<val0>/100/0/2" },
            ActionCap { desc: 496105, kind: Kind::Pulse, command: "settimer/0" },
            ActionCap { desc: 496106, kind: Kind::Analog, command: "settimer/<val0>/0/2/0" },
            ActionCap { desc: 496107, kind: Kind::Analog, command: "settimer/<val0>/100/2/1" },
            ActionCap { desc: 496108, kind: Kind::Analog, command: "settimer/<val0>/100/0/2" },
        ],
    },
    Capability {
        control_types: &["ClimateControllerUS"],
        events: &[],
        actions: &[
            ActionCap { desc: 510101, kind: Kind::Analog, command: "setmode/<val0>" },
            ActionCap { desc: 487101, kind: Kind::Analog, command: "automode/<val0>" },
            ActionCap { desc: 510102, kind: Kind::Analog, command: "startventilationtimer/<val0>" },
            ActionCap { desc: 510103, kind: Kind::Digital, command: "usemergency/<val0>" },
            ActionCap { desc: 487102, kind: Kind::Digital, command: "boost/<val0>" },
            ActionCap { desc: 487103, kind: Kind::Digital, command: "ventilation/<val0>" },
            ActionCap { desc: 487104, kind: Kind::Digital, command: "secheat/<val0>" },
        ],
    },
    Capability {
        control_types: &["Remote"],
        events: &[
            EventCap { desc: 462001, kind: Kind::Analog, state: Some("mode"), value: None },
        ],
        actions: &[
            ActionCap { desc: 462101, kind: Kind::Analog, command: "mode/<val0>" },
            ActionCap { desc: 462102, kind: Kind::Analog, command: "volume" },
            ActionCap { desc: 462103, kind: Kind::Analog, command: "program/<val0>" },
            ActionCap { desc: 462104, kind: Kind::Pulse, command: "reset" },
        ],
    },
    Capability {
        control_types: &["AAlEmergency"],
        events: &[
            EventCap { desc: 506001, kind: Kind::Analog, state: Some("status"), value: Some(1) },
            EventCap { desc: 506002, kind: Kind::Analog, state: Some("status"), value: Some(0) },
            EventCap { desc: 506003, kind: Kind::Analog, state: Some("status"), value: Some(2) },
            EventCap { desc: 506004, kind: Kind::Analog, state: Some("status"), value: Some(3) },
        ],
        actions: &[
            ActionCap { desc: 506101, kind: Kind::Digital, command: "trigger" },
            ActionCap { desc: 506102, kind: Kind::Digital, command: "quit" },
            ActionCap { desc: 506103, kind: Kind::Analog, command: "disable/<val0>" },
            ActionCap { desc: 506104, kind: Kind::Analog, command: "disable/0" },
        ],
    },
    Capability {
        control_types: &["Ping"],
        events: &[
            EventCap { desc: 464001, kind: Kind::Digital, state: Some("active"), value: Some(1) },
            EventCap { desc: 464002, kind: Kind::Analog, state: Some("active"), value: Some(0) },
        ],
        actions: &[],
    },
    Capability {
        control_types: &["PoolController"],
        events: &[
            EventCap { desc: 470001, kind: Kind::Digital, state: Some("coverPosition"), value: Some(1) },
            EventCap { desc: 470002, kind: Kind::Digital, state: Some("coverPosition"), value: Some(0) },
            EventCap { desc: 470003, kind: Kind::Digital, state: Some("swimmingMachine"), value: Some(1) },
            EventCap { desc: 470004, kind: Kind::Analog, state: Some("currentCycle"), value: None },
            EventCap { desc: 470005, kind: Kind::Digital, state: Some("coverOpening"), value: None },
            EventCap { desc: 470006, kind: Kind::Digital, state: Some("coverClosing"), value: None },
            EventCap { desc: 470007, kind: Kind::Digital, state: Some("currentOpMode"), value: Some(0) },
        ],
        actions: &[
            ActionCap { desc: 470101, kind: Kind::Analog, command: "tempMode/<val0>" },
            ActionCap { desc: 470102, kind: Kind::Digital, command: "tempMode/1" },
            ActionCap { desc: 470103, kind: Kind::Digital, command: "tempMode/2" },
            ActionCap { desc: 470104, kind: Kind::Digital, command: "tempMode/3" },
            ActionCap { desc: 470105, kind: Kind::Digital, command: "tempMode/4" },
            ActionCap { desc: 470106, kind: Kind::Digital, command: "tempMode/5" },
            ActionCap { desc: 470107, kind: Kind::Digital, command: "tempMode/0" },
        ],
    },
    Capability {
        control_types: &["Radio"],
        events: &[
            EventCap { desc: 347001, kind: Kind::Analog, state: Some("activeOutput"), value: None },
        ],
        actions: &[
            ActionCap { desc: 347101, kind: Kind::Analog, command: "select/<val0>/force" },
        ],
    },
    Capability {
        control_types: &["Sauna"],
        events: &[
            EventCap { desc: 441001, kind: Kind::Analog, state: Some("tempActual"), value: None },
            EventCap { desc: 441002, kind: Kind::Analog, state: Some("tempBench"), value: None },
            EventCap { desc: 441003, kind: Kind::Digital, state: Some("ready"), value: Some(1) },
            EventCap { desc: 441004, kind: Kind::Analog, state: Some("timer"), value: Some(0) },
            EventCap { desc: 441005, kind: Kind::Digital, state: Some("doorClosed"), value: Some(0) },
            EventCap { desc: 441006, kind: Kind::Digital, state: Some("doorClosed"), value: Some(1) },
            EventCap { desc: 441007, kind: Kind::Digital, state: Some("fan"), value: Some(1) },
            EventCap { desc: 441008, kind: Kind::Digital, state: Some("drying"), value: Some(1) },
            EventCap { desc: 441009, kind: Kind::Digital, state: Some("active"), value: Some(1) },
        ],
        actions: &[
            ActionCap { desc: 441101, kind: Kind::Digital, command: "on" },
            ActionCap { desc: 441102, kind: Kind::Digital, command: "off" },
            ActionCap { desc: 441103, kind: Kind::Digital, command: "fanon" },
            ActionCap { desc: 441104, kind: Kind::Digital, command: "fanoff" },
            ActionCap { desc: 441105, kind: Kind::Digital, command: "starttimer" },
            ActionCap { desc: 441106, kind: Kind::Analog, command: "ontemp/<val0>" },
        ],
    },
    Capability {
        control_types: &["SaunaVapor"],
        events: &[
            EventCap { desc: 442001, kind: Kind::Analog, state: Some("tempActual"), value: None },
            EventCap { desc: 442002, kind: Kind::Analog, state: Some("tempBench"), value: None },
            EventCap { desc: 442010, kind: Kind::Analog, state: Some("humidityActual"), value: None },
            EventCap { desc: 442003, kind: Kind::Digital, state: Some("ready"), value: Some(1) },
            EventCap { desc: 442004, kind: Kind::Analog, state: Some("timer"), value: Some(0) },
            EventCap { desc: 442005, kind: Kind::Digital, state: Some("doorClosed"), value: Some(0) },
            EventCap { desc: 442006, kind: Kind::Digital, state: Some("doorClosed"), value: Some(1) },
            EventCap { desc: 442007, kind: Kind::Digital, state: Some("fan"), value: None },
            EventCap { desc: 442008, kind: Kind::Digital, state: Some("drying"), value: None },
            EventCap { desc: 442009, kind: Kind::Digital, state: Some("active"), value: None },
            EventCap { desc: 442011, kind: Kind::Digital, state: Some("lessWater"), value: None },
        ],
        actions: &[
            ActionCap { desc: 442101, kind: Kind::Analog, command: "mode/<val0>" },
            ActionCap { desc: 442102, kind: Kind::Digital, command: "off" },
            ActionCap { desc: 442103, kind: Kind::Digital, command: "fanon" },
            ActionCap { desc: 442104, kind: Kind::Digital, command: "fanoff" },
            ActionCap { desc: 442105, kind: Kind::Digital, command: "starttimer" },
            ActionCap { desc: 442106, kind: Kind::Analog, command: "ontemp/<val0>" },
            ActionCap { desc: 442107, kind: Kind::Analog, command: "ontemphum/<val0>/<val1>" },
        ],
    },
    Capability {
        control_types: &["Daytimer"],
        events: &[
            EventCap { desc: 368001, kind: Kind::Digital, state: Some("needsActivation"), value: None },
            EventCap { desc: 368002, kind: Kind::Analog, state: Some("value"), value: None },
            EventCap { desc: 368003, kind: Kind::Digital, state: Some("value"), value: Some(1) },
            EventCap { desc: 368004, kind: Kind::Digital, state: Some("value"), value: Some(0) },
        ],
        actions: &[
            ActionCap { desc: 368101, kind: Kind::Pulse, command: "pulse" },
            ActionCap { desc: 368102, kind: Kind::Analog, command: "startOverride/<val0>/<val1>" },
            ActionCap { desc: 368103, kind: Kind::Analog, command: "startOverride/1/<val0>" },
            ActionCap { desc: 368104, kind: Kind::Analog, command: "startOverride/0/<val0>" },
            ActionCap { desc: 368105, kind: Kind::Pulse, command: "stopOverride" },
        ],
    },
    Capability {
        control_types: &["SolarPumpController"],
        events: &[
            EventCap { desc: 447001, kind: Kind::Analog, state: Some("bufferTemp0"), value: None },
            EventCap { desc: 447002, kind: Kind::Analog, state: Some("collectorTemp"), value: None },
            EventCap { desc: 447003, kind: Kind::Digital, state: Some("heatOverload"), value: Some(1) },
            EventCap { desc: 447004, kind: Kind::Digital, state: Some("heatOverload"), value: Some(0) },
        ],
        actions: &[
            ActionCap { desc: 447101, kind: Kind::Analog, command: "settank/<val0>/force" },
            ActionCap { desc: 447102, kind: Kind::Digital, command: "manual/<val0>" },
        ],
    },
    Capability {
        control_types: &["Gate"],
        events: &[
            EventCap { desc: 352001, kind: Kind::Analog, state: Some("position"), value: None },
            EventCap { desc: 352002, kind: Kind::Digital, state: Some("position"), value: Some(0) },
            EventCap { desc: 352003, kind: Kind::Digital, state: Some("position"), value: Some(1) },
        ],
        actions: &[
            ActionCap { desc: 352101, kind: Kind::Pulse, command: "open" },
            ActionCap { desc: 352102, kind: Kind::Pulse, command: "close" },
            ActionCap { desc: 352103, kind: Kind::Pulse, command: "stop" },
            ActionCap { desc: 352104, kind: Kind::Pulse, command: "partiallyOpen" },
        ],
    },
    Capability {
        control_types: &["CentralGate"],
        events: &[
            EventCap { desc: 490001, kind: Kind::Digital, state: Some("events"), value: Some(8) },
            EventCap { desc: 490002, kind: Kind::Digital, state: Some("events"), value: Some(1) },
        ],
        actions: &[
            ActionCap { desc: 490101, kind: Kind::Pulse, command: "open" },
            ActionCap { desc: 490102, kind: Kind::Pulse, command: "close" },
            ActionCap { desc: 490103, kind: Kind::Pulse, command: "stop" },
        ],
    },
    Capability {
        control_types: &["Intercom"],
        events: &[
            EventCap { desc: 453001, kind: Kind::Digital, state: Some("bell"), value: Some(1) },
        ],
        actions: &[
            ActionCap { desc: 453101, kind: Kind::Pulse, command: "answer" },
            ActionCap { desc: 453102, kind: Kind::Pulse, command: "1/pulse" },
            ActionCap { desc: 453103, kind: Kind::Pulse, command: "2/pulse" },
            ActionCap { desc: 453104, kind: Kind::Pulse, command: "3/pulse" },
            ActionCap { desc: 453105, kind: Kind::Analog, command: "<val0>/pulse" },
        ],
    },
    Capability {
        control_types: &["SteakThermo"],
        events: &[
            EventCap { desc: 485001, kind: Kind::Digital, state: Some("yellowAlarmActive"), value: None },
            EventCap { desc: 485002, kind: Kind::Digital, state: Some("greenAlarmActive"), value: None },
            EventCap { desc: 485003, kind: Kind::Digital, state: Some("timerRemaining"), value: Some(0) },
            EventCap { desc: 485004, kind: Kind::Analog, state: Some("temperatureYellow"), value: None },
            EventCap { desc: 485005, kind: Kind::Analog, state: Some("temperatureGreen"), value: None },
            EventCap { desc: 485006, kind: Kind::Digital, state: Some("timerAlarmActive"), value: None },
        ],
        actions: &[
            ActionCap { desc: 485101, kind: Kind::Pulse, command: "quitAlarm" },
            ActionCap { desc: 485102, kind: Kind::Analog, command: "startTimerDuration/<val0>" },
            ActionCap { desc: 485103, kind: Kind::Pulse, command: "setactive/0" },
            ActionCap { desc: 485104, kind: Kind::Pulse, command: "setactive/1" },
        ],
    },
    Capability {
        control_types: &["CarCharger"],
        events: &[
            EventCap { desc: 473001, kind: Kind::Digital, state: Some("connected"), value: Some(1) },
            EventCap { desc: 473002, kind: Kind::Digital, state: Some("connected"), value: Some(0) },
            EventCap { desc: 473003, kind: Kind::Digital, state: Some("charging"), value: Some(1) },
            EventCap { desc: 473004, kind: Kind::Digital, state: Some("charging"), value: Some(0) },
            EventCap { desc: 473005, kind: Kind::Analog, state: Some("power"), value: None },
            EventCap { desc: 473006, kind: Kind::Analog, state: Some("chargeDuration"), value: None },
        ],
        actions: &[
            ActionCap { desc: 473101, kind: Kind::Pulse, command: "charge/on" },
            ActionCap { desc: 473102, kind: Kind::Pulse, command: "charge/off" },
            ActionCap { desc: 473103, kind: Kind::Pulse, command: "limitMode/2" },
            ActionCap { desc: 473104, kind: Kind::Pulse, command: "limitMode/0" },
            ActionCap { desc: 473105, kind: Kind::Analog, command: "limit/<val0>" },
        ],
    },
    Capability {
        control_types: &["AlarmClock"],
        events: &[
            EventCap { desc: 472001, kind: Kind::Digital, state: Some("isAlarmActive"), value: Some(1) },
            EventCap { desc: 472002, kind: Kind::Analog, state: Some("snoozeTime"), value: Some(0) },
        ],
        actions: &[
            ActionCap { desc: 472101, kind: Kind::Pulse, command: "Snooze" },
            ActionCap { desc: 472102, kind: Kind::Pulse, command: "dismiss" },
            ActionCap { desc: 472103, kind: Kind::Analog, command: "activateEntry/<val0>" },
            ActionCap { desc: 472104, kind: Kind::Analog, command: "deactivateEntry/<val0>" },
        ],
    },
    Capability {
        control_types: &["NfcCodeTouch"],
        events: &[
            EventCap { desc: 480001, kind: Kind::Digital, state: Some("events"), value: Some(4) },
            EventCap { desc: 480002, kind: Kind::Digital, state: Some("events"), value: Some(2) },
            EventCap { desc: 480003, kind: Kind::Digital, state: Some("events"), value: Some(1) },
            EventCap { desc: 480004, kind: Kind::Text, state: Some("lastid"), value: None },
            EventCap { desc: 480005, kind: Kind::Text, state: Some("lastuser"), value: None },
            EventCap { desc: 480006, kind: Kind::Text, state: Some("lasttag"), value: None },
            EventCap { desc: 480007, kind: Kind::Text, state: Some("lastcode"), value: None },
        ],
        actions: &[
            ActionCap { desc: 481001, kind: Kind::Analog, command: "outputpulse/<val0>" },
        ],
    },
    Capability {
        control_types: &["PresenceDetector"],
        events: &[
            EventCap { desc: 509001, kind: Kind::Digital, state: Some("events"), value: Some(0) },
            EventCap { desc: 509002, kind: Kind::Digital, state: Some("events"), value: Some(1) },
            EventCap { desc: 509003, kind: Kind::Digital, state: Some("events"), value: Some(2) },
            EventCap { desc: 509004, kind: Kind::Digital, state: Some("events"), value: Some(3) },
            EventCap { desc: 509005, kind: Kind::Digital, state: Some("events"), value: Some(5) },
        ],
        actions: &[
            ActionCap { desc: 509101, kind: Kind::Pulse, command: "pulse" },
            ActionCap { desc: 509102, kind: Kind::Analog, command: "presence/<val0>" },
            ActionCap { desc: 509103, kind: Kind::Analog, command: "deactivate/<val0>" },
        ],
    },
    Capability {
        control_types: &["IntercomV2"],
        events: &[
            EventCap { desc: 458001, kind: Kind::Digital, state: Some("bell"), value: Some(1) },
            EventCap { desc: 458002, kind: Kind::Digital, state: Some("muted"), value: Some(1) },
        ],
        actions: &[
            ActionCap { desc: 458101, kind: Kind::Text, command: "tts/<val0>" },
            ActionCap { desc: 458102, kind: Kind::Digital, command: "mute/<val0>" },
            ActionCap { desc: 458103, kind: Kind::Pulse, command: "answer" },
        ],
    },
    Capability {
        control_types: &["CentralWindow"],
        events: &[
            EventCap { desc: 512001, kind: Kind::Digital, state: Some("events"), value: Some(8) },
            EventCap { desc: 512002, kind: Kind::Digital, state: Some("events"), value: Some(1) },
        ],
        actions: &[
            ActionCap { desc: 512101, kind: Kind::Pulse, command: "fullopen" },
            ActionCap { desc: 512102, kind: Kind::Pulse, command: "fullclose" },
        ],
    },
    Capability {
        control_types: &["LoadManager"],
        events: &[
            EventCap { desc: 459001, kind: Kind::Digital, state: Some("maxPowerExceeded"), value: Some(1) },
            EventCap { desc: 459002, kind: Kind::Analog, state: Some("currentPower"), value: None },
            EventCap { desc: 459004, kind: Kind::Analog, state: Some("availablePower"), value: None },
            EventCap { desc: 459003, kind: Kind::Digital, state: Some("lockedLoads"), value: Some(0) },
        ],
        actions: &[],
    },
    Capability {
        control_types: &["EnergyManager"],
        events: &[
            EventCap { desc: 468001, kind: Kind::Analog, state: Some("currentPower"), value: None },
            EventCap { desc: 468002, kind: Kind::Analog, state: Some("currentBat"), value: None },
        ],
        actions: &[
            ActionCap { desc: 468003, kind: Kind::Analog, command: "trigger/<val0>" },
        ],
    },
    Capability {
        control_types: &["EnergyManager2"],
        events: &[
            EventCap { desc: 515001, kind: Kind::Analog, state: Some("Gpwr"), value: None },
            EventCap { desc: 515002, kind: Kind::Analog, state: Some("Spwr"), value: None },
            EventCap { desc: 515003, kind: Kind::Analog, state: Some("Ssoc"), value: None },
        ],
        actions: &[
            ActionCap { desc: 515004, kind: Kind::Analog, command: "activate/<val0>" },
            ActionCap { desc: 515005, kind: Kind::Analog, command: "deactivate/<val0>" },
        ],
    },
    Capability {
        control_types: &["StatusMonitor"],
        events: &[
            EventCap { desc: 528100, kind: Kind::Analog, state: Some("numState0"), value: None },
            EventCap { desc: 528101, kind: Kind::Analog, state: Some("numState1"), value: None },
            EventCap { desc: 528102, kind: Kind::Analog, state: Some("numState2"), value: None },
            EventCap { desc: 528103, kind: Kind::Analog, state: Some("numState3"), value: None },
            EventCap { desc: 528104, kind: Kind::Analog, state: Some("numState4"), value: None },
            EventCap { desc: 528105, kind: Kind::Analog, state: Some("numState5"), value: None },
            EventCap { desc: 528106, kind: Kind::Analog, state: Some("numState6"), value: None },
            EventCap { desc: 528107, kind: Kind::Analog, state: Some("numState7"), value: None },
            EventCap { desc: 528108, kind: Kind::Analog, state: Some("numState8"), value: None },
            EventCap { desc: 528109, kind: Kind::Analog, state: Some("numState9"), value: None },
            EventCap { desc: 528110, kind: Kind::Analog, state: Some("numDef"), value: None },
        ],
        actions: &[],
    },
    Capability {
        control_types: &["Irrigation"],
        events: &[
            EventCap { desc: 460001, kind: Kind::Digital, state: Some("currentZone"), value: Some(-1) },
            EventCap { desc: 460002, kind: Kind::Digital, state: Some("rainActive"), value: None },
            EventCap { desc: 460003, kind: Kind::Analog, state: Some("rainTime"), value: None },
            EventCap { desc: 460004, kind: Kind::Analog, state: Some("currentZone"), value: None },
        ],
        actions: &[
            ActionCap { desc: 460021, kind: Kind::Pulse, command: "startforce" },
            ActionCap { desc: 460022, kind: Kind::Pulse, command: "stop" },
        ],
    },
    Capability {
        control_types: &["PulseAt"],
        events: &[
            EventCap { desc: 377001, kind: Kind::Digital, state: Some("isActive"), value: None },
        ],
        actions: &[
            ActionCap { desc: 377020, kind: Kind::Pulse, command: "pulse" },
        ],
    },
    Capability {
        control_types: &["Wallbox2"],
        events: &[
            EventCap { desc: 525001, kind: Kind::Digital, state: Some("connected"), value: Some(1) },
            EventCap { desc: 525002, kind: Kind::Digital, state: Some("connected"), value: Some(0) },
            EventCap { desc: 525003, kind: Kind::Digital, state: Some("loadshed"), value: Some(1) },
            EventCap { desc: 525004, kind: Kind::Digital, state: Some("enabled"), value: None },
            EventCap { desc: 525005, kind: Kind::Analog, state: Some("actual"), value: None },
        ],
        actions: &[
            ActionCap { desc: 525101, kind: Kind::Pulse, command: "allow/on" },
            ActionCap { desc: 525102, kind: Kind::Pulse, command: "allow/off" },
            ActionCap { desc: 525103, kind: Kind::Pulse, command: "setmode/1" },
            ActionCap { desc: 525104, kind: Kind::Pulse, command: "setmode/2" },
            ActionCap { desc: 525105, kind: Kind::Pulse, command: "setmode/3" },
            ActionCap { desc: 525106, kind: Kind::Pulse, command: "setmode/4" },
            ActionCap { desc: 525107, kind: Kind::Pulse, command: "setmode/5" },
        ],
    },
    Capability {
        control_types: &["PowerUnit"],
        events: &[
            EventCap { desc: 514001, kind: Kind::Analog, state: Some("power"), value: None },
            EventCap { desc: 514002, kind: Kind::Digital, state: Some("fuse"), value: Some(0) },
            EventCap { desc: 514003, kind: Kind::Analog, state: Some("batteryStateOfCharge"), value: None },
            EventCap { desc: 514004, kind: Kind::Digital, state: Some("deviceInfo"), value: Some(1) },
            EventCap { desc: 514005, kind: Kind::Digital, state: Some("deviceInfo"), value: Some(8) },
            EventCap { desc: 514006, kind: Kind::Digital, state: Some("deviceInfo"), value: Some(2) },
        ],
        actions: &[],
    },
    Capability {
        control_types: &["AcControl"],
        events: &[
            EventCap { desc: 527001, kind: Kind::Digital, state: Some("status"), value: Some(1) },
            EventCap { desc: 527002, kind: Kind::Digital, state: Some("status"), value: Some(0) },
        ],
        actions: &[
            ActionCap { desc: 527101, kind: Kind::Pulse, command: "on" },
            ActionCap { desc: 527102, kind: Kind::Pulse, command: "off" },
            ActionCap { desc: 527103, kind: Kind::Analog, command: "setTarget/<val0>" },
            ActionCap { desc: 527104, kind: Kind::Analog, command: "setMode/<val0>" },
            ActionCap { desc: 527105, kind: Kind::Analog, command: "setFan/<val0>" },
            ActionCap { desc: 527106, kind: Kind::Analog, command: "setAirDir/<val0>" },
            ActionCap { desc: 527107, kind: Kind::Analog, command: "silentMode/<val0>" },
        ],
    },
    Capability {
        control_types: &["EFM"],
        events: &[
            EventCap { desc: 524001, kind: Kind::Analog, state: Some("Gpwr"), value: None },
            EventCap { desc: 524002, kind: Kind::Analog, state: Some("Ppwr"), value: None },
            EventCap { desc: 524003, kind: Kind::Analog, state: Some("Spwr"), value: None },
            EventCap { desc: 524004, kind: Kind::Analog, state: Some("selfConsumption"), value: None },
            EventCap { desc: 524005, kind: Kind::Analog, state: Some("Pri"), value: None },
            EventCap { desc: 524006, kind: Kind::Analog, state: Some("Pre"), value: None },
        ],
        actions: &[],
    },
    Capability {
        control_types: &["SpotPriceOptimizer", "SpotOpt"],
        events: &[
            EventCap { desc: 526001, kind: Kind::Digital, state: Some("veryHigh"), value: Some(1) },
            EventCap { desc: 526002, kind: Kind::Digital, state: Some("high"), value: Some(1) },
            EventCap { desc: 526003, kind: Kind::Digital, state: Some("low"), value: Some(1) },
            EventCap { desc: 526004, kind: Kind::Digital, state: Some("veryLow"), value: Some(1) },
            EventCap { desc: 526005, kind: Kind::Analog, state: Some("current"), value: None },
        ],
        actions: &[],
    },
    Capability {
        control_types: &["Sequential"],
        events: &[
            EventCap { desc: 513001, kind: Kind::Analog, state: Some("activeSequence"), value: None },
            EventCap { desc: 513002, kind: Kind::Digital, state: Some("finished"), value: None },
        ],
        actions: &[
            ActionCap { desc: 513101, kind: Kind::Analog, command: "triggerSequence/<val0>/force" },
        ],
    },
    Capability {
        control_types: &["LightsceneRGB"],
        events: &[],
        actions: &[
            ActionCap { desc: 355101, kind: Kind::Analog, command: "<val0>" },
            ActionCap { desc: 355102, kind: Kind::Pulse, command: "reset" },
        ],
    },
];
