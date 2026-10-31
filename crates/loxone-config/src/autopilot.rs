//! The Automatic Designer rule payload.
//!
//! An `AutopilotRule` block carries its entire rule — conditions
//! ("events"), actions, activation flags — as JSON in the element's
//! `APC=` attribute ([loxone-format.md](../docs/loxone-format.md)). This
//! module gives a human summary for the decompile full view and a
//! field-path diff for `loxl diff`. It also reads a payload as a [`Rule`]
//! (the shapes seen in real configs) and writes one back in the app's key
//! order, for `rule` declarations (`logic::rule`).
//!
//! The payload is handled as untyped JSON deliberately: a typed struct
//! would silently drop fields a newer app version adds, and the diff
//! must see exactly what changed, known schema or not. Only the handful
//! of fields the summary renders are interpreted, and only as far as
//! corpus evidence supports (a `controlUUID`/action `id` is the UUID of
//! a control in the same document; `command` is verbatim app text).
//! Everything else is shown raw — refuse, never guess.

use serde_json::Value;

pub use crate::autopilot_caps::{CAPABILITIES, Capability, EventCap, Kind};

/// Parse a decoded `APC=` attribute value. `None` if it is not JSON —
/// callers fall back to showing the raw text.
pub fn parse(apc: &str) -> Option<Value> {
    serde_json::from_str(apc).ok()
}

/// The payload in canonical text form (compact, keys sorted — serde's
/// default map is ordered), for semantic comparison and fingerprinting.
/// The raw attribute text is not stable enough to compare: some
/// real rules carry a raw CRLF *inside* the JSON (the same
/// not-actually-XML deviation as `Code=`), so a writer reflowing the
/// payload would read as drift on every rule. `None` if not JSON —
/// comparisons fall back to the raw text.
pub fn canonical(apc: &str) -> Option<String> {
    parse(apc).map(|v| v.to_string())
}

/// One line per fact, for `#`-comment rendering in the decompile full
/// view. `resolve` maps an object UUID to a display name (the caller
/// knows the document); an unresolved UUID stays raw.
pub fn summary(rule: &Value, resolve: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut flags = Vec::new();
    if rule.get("active").and_then(Value::as_bool) == Some(false) {
        flags.push("inactive");
    }
    if rule.get("invalid").and_then(Value::as_bool) == Some(true) {
        flags.push("invalid");
    }
    if !flags.is_empty() {
        out.push(format!("rule is {}", flags.join(", ")));
    }
    let groups = rule
        .get("eventGroups")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for (gi, group) in groups.iter().enumerate() {
        let events = group
            .get("events")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for event in events {
            let group_tag = if groups.len() > 1 {
                format!(" (group {})", gi + 1)
            } else {
                String::new()
            };
            out.push(format!("when{group_tag} {}", event_line(event, resolve)));
        }
    }
    let actions = rule
        .get("actions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for action in actions {
        out.push(format!("then {}", action_line(action, resolve)));
    }
    out
}

/// A condition: a control event names its control (resolvable UUID);
/// anything else is shown by its raw type/descID/id — their semantics
/// are not publicly specified, and the view does not guess.
fn event_line(event: &Value, resolve: &dyn Fn(&str) -> Option<String>) -> String {
    let mut line = match event.get("controlUUID").and_then(Value::as_str) {
        Some(uuid) => format!(
            "{} ({})",
            display_name(uuid, resolve),
            brief(event, &["descID"])
        ),
        None => brief(event, &["type", "descID", "id"]),
    };
    if let Some(v) = event.get("value") {
        line.push_str(&format!(": value {v}"));
    }
    // The modal operator (1) is elided like a GUI-default parameter;
    // anything else is a fact worth showing.
    if let Some(op) = event.get("operator").and_then(Value::as_i64)
        && op != 1
    {
        line.push_str(&format!(", op {op}"));
    }
    if event.get("forceEdge").and_then(Value::as_bool) == Some(true) {
        line.push_str(", forceEdge");
    }
    line
}

/// An action: the target control (its `id` is an object UUID) plus the
/// verbatim command text.
fn action_line(action: &Value, resolve: &dyn Fn(&str) -> Option<String>) -> String {
    let target = match action.get("id").and_then(Value::as_str) {
        Some(uuid) => display_name(uuid, resolve),
        None => brief(action, &["type", "descID", "id"]),
    };
    match action.get("command").and_then(Value::as_str) {
        Some(cmd) => format!("{target}: {cmd}"),
        None => format!("{target}: {}", brief(action, &["descID"])),
    }
}

fn display_name(uuid: &str, resolve: &dyn Fn(&str) -> Option<String>) -> String {
    match resolve(uuid) {
        Some(name) => format!("\"{name}\""),
        None => uuid.to_string(),
    }
}

/// `key1 v1, key2 v2` over the listed keys that are present.
fn brief(obj: &Value, keys: &[&str]) -> String {
    keys.iter()
        .filter_map(|k| obj.get(k).map(|v| format!("{k} {v}")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Field-path changes between two rule payloads, one line each
/// (`eventGroups[0].events[1].value: 540 -> 600`). Structural and
/// schema-agnostic: fields this crate does not know still diff.
pub fn changes(old: &Value, new: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk("", old, new, &mut out);
    out
}

fn walk(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    if a == b {
        return;
    }
    match (a, b) {
        (Value::Object(ma), Value::Object(mb)) => {
            let keys: std::collections::BTreeSet<&String> = ma.keys().chain(mb.keys()).collect();
            for key in keys {
                let sub = join(path, key);
                match (ma.get(key.as_str()), mb.get(key.as_str())) {
                    (Some(va), Some(vb)) => walk(&sub, va, vb, out),
                    (Some(va), None) => out.push(format!("{sub}: {va} -> absent")),
                    (None, Some(vb)) => out.push(format!("{sub}: absent -> {vb}")),
                    (None, None) => unreachable!(),
                }
            }
        }
        (Value::Array(va), Value::Array(vb)) => {
            for (i, (ea, eb)) in va.iter().zip(vb).enumerate() {
                walk(&format!("{path}[{i}]"), ea, eb, out);
            }
            for (i, e) in va.iter().enumerate().skip(vb.len()) {
                out.push(format!("{path}[{i}]: {e} -> absent"));
            }
            for (i, e) in vb.iter().enumerate().skip(va.len()) {
                out.push(format!("{path}[{i}]: absent -> {e}"));
            }
        }
        _ => out.push(format!("{path}: {a} -> {b}")),
    }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

// --- The rule model -------------------------------------------

/// What a rule does: its condition groups — the rule runs when every
/// event of one group holds (the app's sections: group `operator` 1 under
/// rule `operator` 3, the only codes the app writes) — and the
/// actions it then sends. Two payloads with the same `Rule` do the same
/// thing, whatever their raw spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// `active` and `activeIfValid` both set: the rule runs.
    pub active: bool,
    pub groups: Vec<Vec<Event>>,
    pub actions: Vec<Action>,
}

impl Rule {
    /// The blocks the rule names: the ones its conditions watch, then its
    /// action targets.
    pub fn blocks(&self) -> Vec<&str> {
        let watched = self.groups.iter().flatten().filter_map(|e| match e {
            Event::State { control, .. } => Some(control.as_str()),
            _ => None,
        });
        let targets = self.actions.iter().filter_map(|a| match a {
            Action::Control { target, .. } => Some(target.as_str()),
            _ => None,
        });
        watched.chain(targets).collect()
    }
}

/// A condition of a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Time of day in minutes since midnight (`type` 5, `descID` 272001).
    Time(u16),
    /// The weekdays it holds on, `0` = Monday … `6` = Sunday (`type` 3,
    /// `descID` 270001). The app writes one day either as `value` (no
    /// `arrValues`) or as the sentinel `value` 9999999999 with the days
    /// in `arrValues`; both read the same.
    Weekdays(Vec<u8>),
    /// An operating mode is active (`on`) or not (`type` 1, `descID`
    /// 21001, `id` = the mode's `Mode=` number).
    Mode { mode: i64, on: bool },
    /// A block's state (`type` 0): the condition `desc` of the block
    /// `control` (its `uuidAction`), watching the state `id`, compared by
    /// `op` (the app's value operators, 1 `==` … 6 `<=`) with `value` (a
    /// JSON number, as text).
    State {
        control: String,
        desc: i64,
        id: String,
        op: u8,
        value: String,
    },
    /// A weather value (`type` 2): the condition `desc` the table offers
    /// for the field, `field` its `weatherFieldTypes` id, compared by `op`
    /// with `value` (a JSON number, as text; 0/1 for a digital field).
    Weather {
        field: i64,
        desc: i64,
        op: u8,
        value: String,
    },
    /// One of the clock's other values (`type` 3, `id` = the table's
    /// `intType`): the day of the month (269), daylight (281), the
    /// sunrise, sunset, dawn and dusk pulses (294, 295, 298, 299).
    Clock {
        desc: i64,
        id: i64,
        op: u8,
        value: i64,
    },
    /// A date (`type` 4, `descID` 233001): the Unix time of its local
    /// midnight, as the app's date picker stores it.
    Date(i64),
}

/// An action of a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The app command text sent to a block (an object UUID).
    Control { target: String, command: String },
    /// Switch an operating mode on or off (`type` 1, `descID` 21101).
    Mode { mode: i64, on: bool },
    /// A push notification (`type` 4, `descID` 96001).
    Notify { subject: String, text: String },
    /// A mail through the cloud mailer (`type` 2, `descID` 92001).
    Mail {
        to: String,
        subject: String,
        text: String,
    },
    /// Wait before the next action, in milliseconds (`type` 5, `descID`
    /// 341801: the app stores seconds times the table's factor 1000).
    Pause(i64),
    /// A system message (`type` 7, `descID` 98101): severity 1 info … 4
    /// critical, `visibility` 2 all users or 4 administrators.
    Message {
        severity: u8,
        subject: String,
        text: String,
        visibility: u8,
    },
}

/// The clock conditions besides time and weekday (`type` 3): code, `id`,
/// whether the value is digital.
pub const CLOCK_EVENTS: &[(i64, i64, bool)] = &[
    (269_001, 269, false),
    (281_001, 281, true),
    (294_001, 294, true),
    (295_001, 295, true),
    (298_001, 298, true),
    (299_001, 299, true),
];
/// The date condition.
pub const DATE: (i64, i64, i64) = (4, 233_001, 233);
/// The weather fields the app offers (its sections file): id, name, the
/// condition code (the capability table's entry for the id), digital.
pub const WEATHER_FIELDS: &[(i64, &str, i64, bool)] = &[
    (1, "temperature", 37_001, false),
    (2, "dewpoint", 37_001, false),
    (4, "wind", 37_002, false),
    (5, "wind_direction", 37_006, false),
    (6, "gusts", 37_002, false),
    (7, "irradiance", 37_007, false),
    (8, "relative_irradiance", 37_004, false),
    (9, "precipitation", 37_008, false),
    (11, "pressure", 37_009, false),
    (26, "felt_temperature", 37_001, false),
    (27, "solar_irradiance", 37_005, false),
    (34, "sunshine", 37_003, true),
];
pub const NOTIFY: i64 = 96_001;
pub const MAIL: i64 = 92_001;
pub const PAUSE: i64 = 341_801;
pub const MESSAGE: i64 = 98_101;

const TIME: (i64, i64, i64) = (5, 272_001, 272);
const WEEKDAY: (i64, i64, i64) = (3, 270_001, 270);
/// The operating mode's condition and action (the capability table's
/// one `type` 1 entry).
pub const MODE_EVENT: i64 = 21_001;
pub const MODE_ACTION: i64 = 21_101;
/// The `value` of a weekday event whose days are in `arrValues`.
pub const WEEKDAY_SENTINEL: i64 = 9_999_999_999;

/// The app's value operators (`AutomaticDesignerEnums.VALUE_OPERATORS`):
/// code and symbol. The text operators (7–12) take a text state, which no
/// condition Lox Logic writes watches.
pub const OPERATORS: &[(u8, &str)] = &[
    (1, "=="),
    (2, "!="),
    (3, ">"),
    (4, "<"),
    (5, ">="),
    (6, "<="),
];

/// Read a payload as a [`Rule`]. `Err` names the first part outside the
/// documented shapes — such a rule is not Lox Logic's to write.
pub fn interpret(rule: &Value) -> Result<Rule, String> {
    let int = |v: &Value, k: &str| v.get(k).and_then(Value::as_i64);
    if int(rule, "type") != Some(1) {
        return Err(format!("rule `type` {}", show(rule.get("type"))));
    }
    if int(rule, "operator") != Some(3) {
        return Err(format!(
            "rule `operator` {} (only 3 is documented)",
            show(rule.get("operator"))
        ));
    }
    // The engine runs a rule only when both flags are set: the app sets
    // both on activation, and rules with `active` alone never ran (house
    // config, live, 2026-10-30 — docs/loxone-format.md).
    let flag = |k: &str| {
        rule.get(k)
            .and_then(Value::as_bool)
            .ok_or(format!("no boolean `{k}`"))
    };
    let active = flag("active")? && flag("activeIfValid")?;
    let groups = rule
        .get("eventGroups")
        .and_then(Value::as_array)
        .ok_or("no `eventGroups` list")?
        .iter()
        .map(|group| {
            if int(group, "operator") != Some(1) {
                return Err(format!(
                    "event group `operator` {} (only 1 is documented)",
                    show(group.get("operator"))
                ));
            }
            group
                .get("events")
                .and_then(Value::as_array)
                .ok_or("no `events` list")?
                .iter()
                .map(interpret_event)
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    if groups.is_empty() {
        return Err("no event group".into());
    }
    let actions = rule
        .get("actions")
        .and_then(Value::as_array)
        .ok_or("no `actions` list")?
        .iter()
        .map(interpret_action)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Rule {
        active,
        groups,
        actions,
    })
}

fn show(v: Option<&Value>) -> String {
    v.map_or_else(|| "absent".into(), Value::to_string)
}

/// The capability-table condition with this code, if there is one.
pub fn event_cap(desc: i64) -> Option<&'static EventCap> {
    CAPABILITIES
        .iter()
        .flat_map(|c| c.events)
        .find(|e| e.desc == desc)
}

/// One event, if its shape is documented.
pub fn interpret_event(e: &Value) -> Result<Event, String> {
    let int = |k: &str| e.get(k).and_then(Value::as_i64);
    let what = || {
        format!(
            "event type {}, descID {}",
            show(e.get("type")),
            show(e.get("descID"))
        )
    };
    if e.get("forceEdge").and_then(Value::as_bool) != Some(false) {
        return Err(format!(
            "{} with forceEdge {} (only false is documented)",
            what(),
            show(e.get("forceEdge"))
        ));
    }
    let op = int("operator")
        .and_then(|o| u8::try_from(o).ok())
        .filter(|o| OPERATORS.iter().any(|(c, _)| c == o))
        .ok_or_else(|| format!("{} with operator {}", what(), show(e.get("operator"))))?;
    let (ty, desc) = (int("type").unwrap_or(-1), int("descID").unwrap_or(-1));
    if ty == 0 {
        let cap = event_cap(desc).ok_or_else(|| format!("{} (not in the app's table)", what()))?;
        if e.get("arrValues").is_some() || cap.kind == Kind::Text {
            return Err(format!("{} (a text or list condition)", what()));
        }
        let (Some(control), Some(id)) = (
            e.get("controlUUID").and_then(Value::as_str),
            e.get("id").and_then(Value::as_str),
        ) else {
            return Err(format!(
                "{} without a `controlUUID` and a state `id`",
                what()
            ));
        };
        let Some(Value::Number(value)) = e.get("value") else {
            return Err(format!("{} without a number `value`", what()));
        };
        return Ok(Event::State {
            control: control.to_string(),
            desc,
            id: id.to_string(),
            op,
            value: value.to_string(),
        });
    }
    if ty == 2 {
        let field = int("id").ok_or_else(|| format!("{} without a field `id`", what()))?;
        let Some(&(_, _, code, digital)) = WEATHER_FIELDS.iter().find(|f| f.0 == field) else {
            return Err(format!("{} on weather field {field}", what()));
        };
        let Some(Value::Number(value)) = e.get("value") else {
            return Err(format!("{} without a number `value`", what()));
        };
        if code != desc || e.get("arrValues").is_some() || (digital && op != 1) {
            return Err(format!("{} on weather field {field}", what()));
        }
        return Ok(Event::Weather {
            field,
            desc,
            op,
            value: value.to_string(),
        });
    }
    if ty == 3
        && let Some(&(_, id, digital)) = CLOCK_EVENTS.iter().find(|c| c.0 == desc)
    {
        let value = int("value").ok_or_else(|| format!("{} without a whole `value`", what()))?;
        if int("id") != Some(id) || e.get("arrValues").is_some() || (digital && op != 1) {
            return Err(format!("{} (not documented)", what()));
        }
        return Ok(Event::Clock {
            desc,
            id,
            op,
            value,
        });
    }
    if op != 1 {
        return Err(format!(
            "{} with operator {op} (only 1 is documented)",
            what()
        ));
    }
    let value = int("value").ok_or_else(|| format!("{} without a whole `value`", what()))?;
    if (ty, desc, int("id").unwrap_or(-1)) == DATE && e.get("arrValues").is_none() {
        return Ok(Event::Date(value));
    }
    if (ty, desc) == (1, MODE_EVENT) {
        let mode = int("id").ok_or_else(|| format!("{} without a mode number `id`", what()))?;
        return match value {
            0 | 1 => Ok(Event::Mode {
                mode,
                on: value == 1,
            }),
            _ => Err(format!("operating mode event with value {value}")),
        };
    }
    let key = (ty, desc, int("id").unwrap_or(-1));
    if key == TIME {
        return match u16::try_from(value) {
            Ok(m) if m < 1440 => Ok(Event::Time(m)),
            _ => Err(format!("time event with value {value}")),
        };
    }
    if key == WEEKDAY {
        let day = |d: i64| u8::try_from(d).ok().filter(|d| *d < 7);
        let days: Option<Vec<u8>> = match e.get("arrValues") {
            None => day(value).map(|d| vec![d]),
            Some(arr) if value == WEEKDAY_SENTINEL => arr
                .as_array()
                .and_then(|a| a.iter().map(|v| v.as_i64().and_then(day)).collect()),
            Some(_) => None,
        };
        return match days {
            Some(mut d) if !d.is_empty() => {
                d.sort_unstable();
                d.dedup();
                Ok(Event::Weekdays(d))
            }
            _ => Err(format!("weekday event with value {value}")),
        };
    }
    Err(format!("{} (not documented)", what()))
}

/// One action, if its shape is documented: `type` 0 with a target UUID
/// and the command text, or `type` 1 switching an operating mode.
pub fn interpret_action(a: &Value) -> Result<Action, String> {
    let ty = a.get("type").and_then(Value::as_i64);
    let desc = a.get("descID").and_then(Value::as_i64);
    if desc.is_none() {
        return Err("action without a `descID`".into());
    }
    let command = a.get("command").and_then(Value::as_str);
    match ty {
        Some(0) => match (a.get("id").and_then(Value::as_str), command) {
            (Some(id), Some(cmd)) => Ok(Action::Control {
                target: id.to_string(),
                command: cmd.to_string(),
            }),
            _ => Err("action without a target `id` and a `command`".into()),
        },
        Some(1) if desc == Some(MODE_ACTION) => {
            match (a.get("id").and_then(Value::as_i64), command) {
                (Some(mode), Some(c @ ("0" | "1"))) => Ok(Action::Mode { mode, on: c == "1" }),
                _ => Err(format!(
                    "operating mode action with id {} and command {}",
                    show(a.get("id")),
                    show(a.get("command"))
                )),
            }
        }
        _ => {
            let texts = |n: usize| -> Option<Vec<String>> {
                let arr = a.get("arrValues")?.as_array()?;
                (arr.len() == n)
                    .then(|| arr.iter().map(|v| v.as_str().map(str::to_string)).collect())
                    .flatten()
            };
            let bad = || {
                format!(
                    "action type {}, descID {}",
                    show(a.get("type")),
                    show(a.get("descID"))
                )
            };
            match (ty, desc) {
                (Some(4), Some(NOTIFY)) if command.is_none() => {
                    let [subject, text] =
                        <[String; 2]>::try_from(texts(2).ok_or_else(bad)?).map_err(|_| bad())?;
                    Ok(Action::Notify { subject, text })
                }
                (Some(2), Some(MAIL)) if command.is_none() => {
                    let [to, subject, text] =
                        <[String; 3]>::try_from(texts(3).ok_or_else(bad)?).map_err(|_| bad())?;
                    Ok(Action::Mail { to, subject, text })
                }
                (Some(5), Some(PAUSE)) => {
                    let ms = a
                        .get("arrValues")
                        .and_then(Value::as_array)
                        .and_then(|v| match v.as_slice() {
                            [n] => n.as_i64(),
                            _ => None,
                        })
                        .filter(|ms| command == Some(ms.to_string().as_str()))
                        .ok_or_else(bad)?;
                    Ok(Action::Pause(ms))
                }
                (Some(7), Some(MESSAGE)) => {
                    let arr = a
                        .get("arrValues")
                        .and_then(Value::as_array)
                        .ok_or_else(bad)?;
                    let num = |v: &Value| v.as_u64().and_then(|n| u8::try_from(n).ok());
                    let txt = |v: &Value| v.as_str().map(str::to_string);
                    let [sev, sub, txt_v, vis] = arr.as_slice() else {
                        return Err(bad());
                    };
                    let (Some(severity), Some(subject), Some(text), Some(visibility)) =
                        (num(sev), txt(sub), txt(txt_v), num(vis))
                    else {
                        return Err(bad());
                    };
                    let m = Action::Message {
                        severity,
                        subject,
                        text,
                        visibility,
                    };
                    if (1..=4).contains(&severity)
                        && matches!(visibility, 2 | 4)
                        && command == Some(message_command(&m).as_str())
                    {
                        Ok(m)
                    } else {
                        Err(bad())
                    }
                }
                _ => Err(bad()),
            }
        }
    }
}

/// A system message's command: the table's `autopilotMessage/<val0>/
/// <val1>/<val2>` filled as the app fills it (plain text, no escaping).
fn message_command(m: &Action) -> String {
    match m {
        Action::Message {
            severity,
            subject,
            text,
            ..
        } => format!("autopilotMessage/{severity}/{subject}/{text}"),
        _ => String::new(),
    }
}

/// A new time event, as the app writes it.
pub fn time_event(minutes: u16) -> Value {
    serde_json::json!({
        "type": TIME.0, "descID": TIME.1, "id": TIME.2, "value": minutes,
        "operator": 1, "forceEdge": false,
    })
}

/// A new weekday event: the days in `arrValues` under the sentinel
/// `value`, the form the app writes for days picked in its editor.
pub fn weekday_event(days: &[u8]) -> Value {
    serde_json::json!({
        "type": WEEKDAY.0, "descID": WEEKDAY.1, "id": WEEKDAY.2,
        "value": WEEKDAY_SENTINEL, "operator": 1, "forceEdge": false,
        "arrValues": days,
    })
}

/// A new event of the other kinds: the fields the app sends (its
/// `_applyEventToRule`), and `forceEdge`, which the Miniserver adds.
pub fn new_event(e: &Event) -> Value {
    match e {
        Event::Time(m) => time_event(*m),
        Event::Weekdays(d) => weekday_event(d),
        Event::Mode { mode, on } => serde_json::json!({
            "type": 1, "descID": MODE_EVENT, "id": mode, "value": u8::from(*on),
            "operator": 1, "forceEdge": false,
        }),
        Event::State {
            control,
            desc,
            id,
            op,
            value,
        } => serde_json::json!({
            "type": 0, "descID": desc, "id": id, "controlUUID": control,
            "value": serde_json::from_str::<Value>(value).unwrap_or(Value::Null),
            "operator": op, "forceEdge": false,
        }),
        Event::Weather {
            field,
            desc,
            op,
            value,
        } => serde_json::json!({
            "type": 2, "descID": desc, "id": field,
            "value": serde_json::from_str::<Value>(value).unwrap_or(Value::Null),
            "operator": op, "forceEdge": false,
        }),
        Event::Clock {
            desc,
            id,
            op,
            value,
        } => serde_json::json!({
            "type": 3, "descID": desc, "id": id, "value": value,
            "operator": op, "forceEdge": false,
        }),
        Event::Date(t) => serde_json::json!({
            "type": DATE.0, "descID": DATE.1, "id": DATE.2, "value": t,
            "operator": 1, "forceEdge": false,
        }),
    }
}

/// A new action of the kinds that name no block: the fields the app's
/// `_applyActionToRule` writes (`arrValues`, and `command` where the
/// table's entry has an `action` template).
pub fn new_action(a: &Action) -> Option<Value> {
    Some(match a {
        Action::Notify { subject, text } => serde_json::json!({
            "type": 4, "descID": NOTIFY, "arrValues": [subject, text],
        }),
        Action::Mail { to, subject, text } => serde_json::json!({
            "type": 2, "descID": MAIL, "arrValues": [to, subject, text],
        }),
        Action::Pause(ms) => serde_json::json!({
            "type": 5, "descID": PAUSE, "command": ms.to_string(), "arrValues": [ms],
        }),
        Action::Message {
            severity,
            subject,
            text,
            visibility,
        } => serde_json::json!({
            "type": 7, "descID": MESSAGE, "command": message_command(a),
            "arrValues": [severity, subject, text, visibility],
        }),
        Action::Mode { mode, on } => mode_action(*mode, *on),
        Action::Control { .. } => return None,
    })
}

/// A new operating mode action, as the app's `_applyActionToRule` builds
/// it from the table's entry (`action` `<val0>`, the picked value in
/// `arrValues`).
pub fn mode_action(mode: i64, on: bool) -> Value {
    let v = u8::from(on);
    serde_json::json!({
        "type": 1, "descID": MODE_ACTION, "id": mode, "command": v.to_string(),
        "arrValues": [v],
    })
}

/// The order the app writes keys in, at every level of the payload (rule,
/// group, event, action — observed in every corpus rule). Keys it does not
/// list follow, sorted.
const KEY_ORDER: &[&str] = &[
    "type",
    "descID",
    "id",
    "controlUUID",
    "active",
    "activeIfValid",
    "name",
    "description",
    "value",
    "operator",
    "invalid",
    "forceEdge",
    "command",
    "arrValues",
    "eventGroups",
    "events",
    "actions",
];

/// A payload as `APC=` text: compact, keys in the app's order.
pub fn to_text(v: &Value) -> String {
    let mut out = String::new();
    write_value(v, &mut out);
    out
}

fn write_value(v: &Value, out: &mut String) {
    match v {
        Value::Object(map) => {
            let rank = |k: &str| KEY_ORDER.iter().position(|o| *o == k).unwrap_or(usize::MAX);
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(k.clone()).to_string());
                out.push(':');
                write_value(&map[k.as_str()], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Corpus-shaped rule (synthetic UUIDs): one time event, one control
    /// event, one action.
    fn rule() -> Value {
        serde_json::from_str(
            r#"{
              "type": 1, "id": "30000009-0000-0090-ffff504f94112233",
              "active": true, "activeIfValid": true,
              "name": "Skimmer Ein", "description": "",
              "operator": 3, "invalid": false,
              "eventGroups": [{"operator": 1, "events": [
                {"type": 5, "descID": 272001, "id": 272, "value": 540,
                 "operator": 1, "forceEdge": false},
                {"type": 0, "descID": 42001,
                 "id": "30000001-0000-0010-ffff504f94112233",
                 "controlUUID": "30000001-0000-0010-ffff504f94112233",
                 "value": 65, "operator": 5, "forceEdge": false}
              ]}],
              "actions": [{"type": 0, "descID": 342101,
                "id": "30000005-0000-0050-ffff504f94112233",
                "command": "on"}]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn summary_resolves_controls_and_keeps_the_rest_raw() {
        let resolve = |uuid: &str| match uuid {
            "30000001-0000-0010-ffff504f94112233" => Some("Wassertemperatur".to_string()),
            "30000005-0000-0050-ffff504f94112233" => Some("Freigabe".to_string()),
            _ => None,
        };
        let lines = summary(&rule(), &resolve);
        assert_eq!(
            lines,
            vec![
                "when type 5, descID 272001, id 272: value 540",
                "when \"Wassertemperatur\" (descID 42001): value 65, op 5",
                "then \"Freigabe\": on",
            ]
        );
        // Flags only when notable.
        let mut inactive = rule();
        inactive["active"] = Value::Bool(false);
        assert_eq!(summary(&inactive, &resolve)[0], "rule is inactive");
    }

    #[test]
    fn changes_report_field_paths() {
        let old = rule();
        let mut new = rule();
        new["eventGroups"][0]["events"][0]["value"] = 600.into();
        new["active"] = Value::Bool(false);
        new["actions"][0]["command"] = "off".into();
        assert_eq!(
            changes(&old, &new),
            vec![
                "actions[0].command: \"on\" -> \"off\"",
                "active: true -> false",
                "eventGroups[0].events[0].value: 540 -> 600",
            ]
        );
        // Array growth is reported whole, element by element.
        let mut extra = rule();
        let arr = extra["actions"].as_array_mut().unwrap();
        arr.push(serde_json::json!({"command": "off"}));
        let c = changes(&old, &extra);
        assert_eq!(c, vec!["actions[1]: absent -> {\"command\":\"off\"}"]);
        assert!(changes(&old, &old).is_empty());
    }

    #[test]
    fn interpret_reads_the_documented_shapes() {
        let mut r = rule();
        assert_eq!(
            interpret(&r).unwrap(),
            Rule {
                active: true,
                groups: vec![vec![
                    Event::Time(540),
                    Event::State {
                        control: "30000001-0000-0010-ffff504f94112233".into(),
                        desc: 42001,
                        id: "30000001-0000-0010-ffff504f94112233".into(),
                        op: 5,
                        value: "65".into(),
                    },
                ]],
                actions: vec![Action::Control {
                    target: "30000005-0000-0050-ffff504f94112233".into(),
                    command: "on".into(),
                }],
            }
        );
        // A condition code the app's table does not have.
        r["eventGroups"][0]["events"][1]["descID"] = 1.into();
        let err = interpret(&r).unwrap_err();
        assert!(err.contains("descID 1 "), "{err}");
        // Both weekday spellings read the same.
        let one: Value = serde_json::from_str(
            r#"{"type":3,"descID":270001,"id":270,"value":1,"operator":1,"forceEdge":false}"#,
        )
        .unwrap();
        let arr = weekday_event(&[1]);
        assert_eq!(interpret_event(&one), interpret_event(&arr));
        assert_eq!(interpret_event(&arr), Ok(Event::Weekdays(vec![1])));
        let mut bad = arr.clone();
        bad["arrValues"] = serde_json::json!([7]);
        assert!(interpret_event(&bad).is_err());
        let mut op = time_event(60);
        op["operator"] = 2.into();
        assert!(interpret_event(&op).is_err());
    }

    #[test]
    fn modes_and_groups_read_as_the_app_writes_them() {
        // The corpus's operating mode condition (use-case config).
        let e: Value = serde_json::from_str(
            r#"{"type":1,"descID":21001,"id":-7,"value":1,"operator":1,"forceEdge":false}"#,
        )
        .unwrap();
        let mode = Event::Mode { mode: -7, on: true };
        assert_eq!(interpret_event(&e), Ok(mode.clone()));
        assert_eq!(new_event(&mode), e);
        let a = mode_action(-5, false);
        assert_eq!(
            to_text(&a),
            r#"{"type":1,"descID":21101,"id":-5,"command":"0","arrValues":[0]}"#
        );
        assert_eq!(
            interpret_action(&a),
            Ok(Action::Mode {
                mode: -5,
                on: false
            })
        );
        let mut r = rule();
        r["eventGroups"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"operator": 1, "events": [e]}));
        assert_eq!(interpret(&r).unwrap().groups.len(), 2);
        r["eventGroups"][1]["operator"] = 2.into();
        assert!(interpret(&r).is_err());
    }

    #[test]
    fn to_text_writes_the_app_key_order() {
        let text = r#"{"type":1,"id":"x","active":false,"activeIfValid":false,"name":"N","description":"","operator":3,"invalid":false,"eventGroups":[{"operator":1,"events":[{"type":3,"descID":270001,"id":270,"value":9999999999,"operator":1,"forceEdge":false,"arrValues":[1]},{"type":5,"descID":272001,"id":272,"value":1110,"operator":1,"forceEdge":false}]}],"actions":[{"type":0,"descID":342102,"id":"y","command":"off"}]}"#;
        assert_eq!(to_text(&parse(text).unwrap()), text);
        assert_eq!(
            to_text(&weekday_event(&[3])),
            r#"{"type":3,"descID":270001,"id":270,"value":9999999999,"operator":1,"forceEdge":false,"arrValues":[3]}"#
        );
        assert_eq!(
            to_text(&serde_json::json!({"zz": 1, "type": 0, "aa": "ü\"q"})),
            r#"{"type":0,"aa":"ü\"q","zz":1}"#
        );
    }
}
