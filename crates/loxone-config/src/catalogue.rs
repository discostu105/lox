//! `LoxAPP3.json`, the app catalogue: the structure the app reads. Loxone
//! Config writes it into the program ZIP next to `sps0.LoxCC` when it saves
//! to the Miniserver; the Miniserver serves it as `/data/LoxApp3.json` as it
//! is and does not rebuild it (#155).
//!
//! [`update`] carries what a compile changes about the app's controls into
//! the catalogue of the config it compiled against: the name, room,
//! category and favourite flag of the controls it lists, the controls the
//! compile deletes or hides, and new entries for the block types whose
//! entries are known (switches, inputs and info displays, jalousies, central
//! blocks, meters: [`KNOWN`]). The rules and their evidence are in
//! docs/loxone-format.md (*LoxAPP3.json*).
//! What it cannot carry over it names, so that a save from Loxone Config
//! can.
//!
//! The file is edited as text: the rest of it stays byte for byte as Loxone
//! Config wrote it (its number formatting, its key order).

use crate::doc::LoxoneDoc;
use crate::xml::Element;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

/// The file name in the program ZIP.
pub const MEMBER: &str = "LoxAPP3.json";

const BOM: &str = "\u{feff}";

/// A control whose entry the compile changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub uuid: String,
    /// The control's name in the app (for one the catalogue does not list
    /// yet: the name it gets)
    pub name: String,
    /// What changes, e.g. `renamed "A" → "B"`
    pub what: String,
}

/// The catalogue after [`update`].
#[derive(Debug, Clone)]
pub struct Update {
    /// The new file (unchanged when nothing applied).
    pub text: String,
    /// What the new file carries over.
    pub applied: Vec<Change>,
    /// What it does not: these entries stay as they were until Loxone
    /// Config saves the project.
    pub left: Vec<Change>,
}

/// Carry the app-visible changes from `live` to `compiled` into
/// `catalogue`, the catalogue of `live`. When anything applies,
/// `lastModified` becomes `last_modified` (`YYYY-MM-DD HH:MM:SS`, the
/// Miniserver's local time, as Loxone Config writes it): the app reloads
/// the structure when it changes.
pub fn update(
    catalogue: &str,
    live: &LoxoneDoc,
    compiled: &LoxoneDoc,
    last_modified: &str,
) -> Result<Update, String> {
    let body = catalogue.strip_prefix(BOM).unwrap_or(catalogue);
    let root: Value =
        serde_json::from_str(body).map_err(|e| format!("{MEMBER} is not valid JSON: {e}"))?;
    let (top, _) = object(body, skip_ws(body, 0))?;
    let controls_member = top
        .iter()
        .find(|m| m.key == "controls")
        .ok_or_else(|| format!("{MEMBER} has no controls"))?;
    let (entries, controls_close) = object(body, controls_member.value.start)?;
    // Lowercase UUID → name.
    let keys_of = |section: &str| -> BTreeMap<String, String> {
        root.get(section)
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .map(|(k, v)| {
                        let name = v["name"].as_str().unwrap_or("").to_string();
                        (plain(k).to_ascii_lowercase(), name)
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let (rooms, cats) = (keys_of("rooms"), keys_of("cats"));

    let (was, now) = (Objects::of(live), Objects::of(compiled));
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut applied = Vec::new();
    let mut left = Vec::new();
    let mut listed = BTreeSet::new();
    let mut removed = BTreeSet::new();

    // The entries of the controls the catalogue lists.
    for (idx, entry) in entries.iter().enumerate() {
        let value = &root["controls"][&entry.key];
        let Some(uuid) = value.get("uuidAction").and_then(Value::as_str) else {
            continue;
        };
        let uuid = uuid.to_ascii_lowercase();
        listed.insert(uuid.clone());
        let app_name = value["name"].as_str().unwrap_or("").to_string();
        let change = |what: String| Change {
            uuid: uuid.clone(),
            name: app_name.clone(),
            what,
        };
        let shown = now.shown(&uuid);
        if shown.is_none() {
            let what = if now.all.contains(&uuid) {
                "hidden from the app"
            } else {
                "deleted"
            };
            if was.shown(&uuid).is_none() {
                // Not the compile's doing.
                continue;
            }
            // Another entry naming the control (a central block's list,
            // say) would dangle.
            let outside = [&body[..entry.span.start], &body[entry.span.end..]];
            if outside
                .iter()
                .any(|t| t.to_ascii_lowercase().contains(&uuid))
            {
                left.push(change(format!("{what}; other entries refer to it")));
            } else {
                removed.insert(idx);
                applied.push(change(what.to_string()));
            }
            continue;
        }
        let (Some(new), Some(old)) = (shown.map(view), was.shown(&uuid).map(view)) else {
            // Shown now, not before: listed already, nothing to add.
            continue;
        };
        let (fields, _) = object(body, entry.value.start)?;
        let field = |k: &str| fields.iter().find(|m| m.key == k);
        // A room or category by its name in the catalogue.
        let text = |v: &Option<String>, names: &BTreeMap<String, String>| match v {
            None => "none".to_string(),
            Some(v) => format!(
                "\"{}\"",
                names.get(v).filter(|n| !n.is_empty()).unwrap_or(v)
            ),
        };
        if new.name != old.name
            && let Some(m) = field("name")
        {
            edits.push((m.value.clone(), json_str(&new.name)));
            applied.push(change(format!(
                "renamed \"{}\" → \"{}\"",
                old.name, new.name
            )));
        }
        for (key, attr, set, from, to) in [
            ("room", "room", &rooms, &old.room, &new.room),
            ("cat", "category", &cats, &old.cat, &new.cat),
        ] {
            if from == to {
                continue;
            }
            let what = format!("{attr} {} → {}", text(from, set), text(to, set));
            match (to, field(key)) {
                (Some(to), Some(m)) if set.contains_key(to) => {
                    edits.push((m.value.clone(), json_str(to)));
                    applied.push(change(what));
                }
                (Some(_), Some(_)) => {
                    left.push(change(format!("{what}, which the catalogue does not list")))
                }
                _ => left.push(change(what)),
            }
        }
        // A display unit (D65) is the value format of the entry and of its
        // statistic outputs (`%` doubled in the entry only).
        let unit = |el: &Element| {
            el.child_elements()
                .find(|c| c.name == "Display")
                .and_then(|d| d.attr_decoded("Unit"))
                .map(|u| u.into_owned())
        };
        let (unit_was, unit_now) = (was.shown(&uuid).and_then(unit), shown.and_then(unit));
        if unit_was != unit_now {
            let what = format!(
                "unit \"{}\" → \"{}\"",
                unit_was.as_deref().unwrap_or(""),
                unit_now.as_deref().unwrap_or("")
            );
            let formats = unit_was
                .as_deref()
                .and_then(value_format)
                .zip(unit_now.as_deref().and_then(value_format));
            let spans = formats.as_ref().map_or_else(Vec::new, |(from, to)| {
                format_values(body, entry.value.clone())
                    .into_iter()
                    .filter_map(|r| {
                        let v: String = serde_json::from_str(&body[r.clone()]).ok()?;
                        let to = if v == from.0 {
                            &to.0
                        } else if v == from.1 {
                            &to.1
                        } else {
                            return None;
                        };
                        Some((r, json_str(to)))
                    })
                    .collect()
            });
            if spans.is_empty() {
                left.push(change(what));
            } else {
                edits.extend(spans);
                applied.push(change(what));
            }
        }
        if new.favorite != old.favorite
            && let Some(m) = field("isFavorite")
        {
            edits.push((m.value.clone(), new.favorite.to_string()));
            applied.push(change(format!(
                "{} the favourites",
                if new.favorite {
                    "added to"
                } else {
                    "removed from"
                }
            )));
        }
    }

    // New entries, for the block types whose entries Lox Logic reproduces.
    let trusted = trusted_types(body, &root, &entries, &was);
    let mut added = String::new();
    for (uuid, el) in &now.shown_in_order {
        if listed.contains(uuid) {
            continue;
        }
        let eligible = |objs: &Objects, el: &Element| {
            let v = view(el);
            let placed = el.attr("Type") != Some("VirtualIn") || objs.referenced.contains(uuid);
            placed
                && v.room.as_ref().is_some_and(|r| rooms.contains_key(r))
                && v.cat.as_ref().is_some_and(|c| cats.contains_key(c))
        };
        // Loxone Config does not list a virtual input no page uses, nor an
        // object without a room and a category.
        if was.shown(uuid).is_some_and(|w| eligible(&was, w)) {
            continue;
        }
        let ty = el.attr("Type").unwrap_or("");
        let name = view(el).name;
        let change = |what: &str| Change {
            uuid: uuid.clone(),
            name: name.clone(),
            what: what.to_string(),
        };
        if !eligible(&now, el) {
            // A new control of a known type: say why the app lacks it.
            let v = view(el);
            if was.shown(uuid).is_none()
                && KNOWN.contains(&ty)
                && ty != "VirtualIn"
                && (v.room.is_none() || v.cat.is_none())
            {
                left.push(change("not in the app: it needs a room and a category"));
            }
            continue;
        }
        match entry(el, &now) {
            Some(json) if trusted.contains(ty) => {
                added.push(',');
                added.push_str(&json_str(&format!("U:{}", el.attr("U").unwrap_or(uuid))));
                added.push(':');
                added.push_str(&json);
                applied.push(change("added to the app"));
            }
            Some(_) => left.push(change(&format!(
                "not added: Lox Logic does not reproduce the catalogue's {ty} entries"
            ))),
            None if el.child_elements().any(|c| c.attr("IconV").is_some()) => {
                left.push(change("not added: its icon"))
            }
            None if KNOWN.contains(&ty) => left.push(change(&format!(
                "not added: Lox Logic does not write this kind of {ty} entry"
            ))),
            None => left.push(change(&format!(
                "not added: Lox Logic does not write {ty} entries"
            ))),
        }
    }
    edits.extend(removal_spans(&entries, &removed).map(|r| (r, String::new())));
    if !added.is_empty() {
        let mut text = added;
        if removed.len() == entries.len() {
            text.remove(0);
        }
        edits.push((controls_close..controls_close, text));
    }

    if edits.is_empty() {
        return Ok(Update {
            text: catalogue.to_string(),
            applied,
            left,
        });
    }
    if let Some(m) = top.iter().find(|m| m.key == "lastModified") {
        edits.push((m.value.clone(), json_str(last_modified)));
    }
    edits.sort_by_key(|(r, _)| (r.start, r.end));
    let mut out = String::with_capacity(catalogue.len() + 512);
    if catalogue.starts_with(BOM) {
        out.push_str(BOM);
    }
    let mut at = 0;
    for (range, text) in edits {
        out.push_str(&body[at..range.start]);
        out.push_str(&text);
        at = range.end;
    }
    out.push_str(&body[at..]);
    serde_json::from_str::<Value>(out.strip_prefix(BOM).unwrap_or(&out))
        .map_err(|e| format!("the updated {MEMBER} is not valid JSON: {e}"))?;
    Ok(Update {
        text: out,
        applied,
        left,
    })
}

/// A control key without its `U:` prefix (Loxone Config 16 writes the
/// keys of `controls`, `modes`, `autopilot` and `messageCenter` with it).
fn plain(key: &str) -> &str {
    key.strip_prefix("U:").unwrap_or(key)
}

/// The block types whose entries [`entry`] writes exactly as the catalogue
/// has them: every listed control of the type (in `live`) reproduces its
/// `type`, `details` and `states` byte for byte. A type the catalogue does
/// not list yet is trusted on the evidence alone.
fn trusted_types(
    body: &str,
    root: &Value,
    entries: &[Member],
    live: &Objects,
) -> BTreeSet<&'static str> {
    let mut bad = BTreeSet::new();
    for e in entries {
        let Some(uuid) = root["controls"][&e.key]["uuidAction"].as_str() else {
            continue;
        };
        let Some(el) = live.shown(&uuid.to_ascii_lowercase()) else {
            continue;
        };
        let ty = el.attr("Type").unwrap_or("");
        if !KNOWN.contains(&ty) {
            continue;
        }
        // A variant `entry` does not write (an icon of its own, say) says
        // nothing about the others.
        let Some(json) = entry(el, live) else {
            continue;
        };
        let same = match (object(&json, 0), object(body, e.value.start)) {
            (Ok((ours, _)), Ok((theirs, _))) => SELF_CHECKED.iter().all(|k| {
                let a = ours
                    .iter()
                    .find(|m| m.key == *k)
                    .map(|m| &json[m.value.clone()]);
                let b = theirs
                    .iter()
                    .find(|m| m.key == *k)
                    .map(|m| &body[m.value.clone()]);
                a == b
            }),
            _ => false,
        };
        if !same {
            bad.insert(ty.to_string());
        }
    }
    KNOWN
        .iter()
        .copied()
        .filter(|t| !bad.contains(*t))
        .collect()
}

/// The block types `entry` knows.
pub const KNOWN: [&str; 22] = [
    "PButtonT",
    "PushButton",
    "VirtualIn",
    "Memory",
    "DigitalIn",
    "LoxAIRsensor",
    "TreeAsensor",
    "LoxAIRAsensor",
    "Lox1wireAsensor",
    "ModbusASensor",
    "ModbusAActor",
    "PulseAt",
    "DayTimer",
    "LoxAIRactor",
    "AutoJalousie",
    "CentralLight",
    "CentralShade",
    "MeterDig",
    "MeterAbsUni",
    "MeterAbsBi",
    "MeterAbsSt",
    "LightController2",
];

/// The members of an entry the self-check compares (the rest come from
/// [`view`]).
const SELF_CHECKED: [&str; 7] = [
    "type",
    "details",
    "states",
    "statistic",
    "statisticV2",
    "defaultIcon",
    "subControls",
];

/// Objects shown as a digital info display (`InfoOnlyDigital`): their
/// state is their own UUID.
const INFO_DIGITAL: [&str; 4] = ["Memory", "DigitalIn", "LoxAIRsensor", "LoxAIRactor"];

/// Objects shown as an analog info display (`InfoOnlyAnalog`).
const INFO_ANALOG: [&str; 5] = [
    "TreeAsensor",
    "LoxAIRAsensor",
    "Lox1wireAsensor",
    "ModbusASensor",
    "ModbusAActor",
];

const ON_OFF: &str = r##"{"jLockable":false,"text":{"off":"Aus","on":"Ein"},"color":{"off":"#FF0054","on":"#8DFF70"}}"##;

/// The catalogue entry Loxone Config writes for `el`, compact, in its key
/// order; `None` for a type (or variant) whose entry is not known, and for
/// an object with an icon of its own (`IconV=`), whose file name the
/// program does not carry.
fn entry(el: &Element, objs: &Objects) -> Option<String> {
    let io = el.child_elements().find(|c| c.name == "IoData")?;
    if io.attr("IconV").is_some() {
        return None;
    }
    entry_any_icon(el, objs)
}

/// The app's view of the document's blocks, as the catalogue derives it:
/// UUID (lowercase) → app control type and states (state name → UUID),
/// for each object shown in the app whose entry Lox Logic knows. Automatic
/// Designer conditions name a block's state by this map (D73).
pub(crate) fn app_controls(
    doc: &LoxoneDoc,
) -> BTreeMap<String, (String, BTreeMap<String, String>)> {
    let objs = Objects::of(doc);
    objs.shown_in_order
        .iter()
        .filter_map(|(uuid, el)| {
            let v: serde_json::Value = serde_json::from_str(&entry_any_icon(el, &objs)?).ok()?;
            let ty = v["type"].as_str()?.to_string();
            let states = v["states"]
                .as_object()?
                .iter()
                .filter_map(|(k, s)| Some((k.clone(), s.as_str()?.to_string())))
                .collect();
            Some((uuid.clone(), (ty, states)))
        })
        .collect()
}

/// [`entry`] for an object with or without an icon of its own.
fn entry_any_icon(el: &Element, objs: &Objects) -> Option<String> {
    let co = |k: &str| {
        el.child_elements()
            .find(|c| c.name == "Co" && c.attr("K") == Some(k))
            .and_then(|c| c.attr("U"))
            .map(str::to_string)
    };
    // The display settings Loxone Config writes last: a push button with
    // `Type="1"` is a switch in the app; a virtual input showing its state
    // only is an info display.
    let display = el.child_elements().filter(|c| c.name == "Display").last();
    let uuid = el.attr("U")?;
    // Most entries carry `"defaultIcon":null`; a jalousie's and a central
    // block's have no such key. What follows `states`: a statistic.
    let mut icon = true;
    let mut tail = String::new();
    let sp = || -> Vec<&str> {
        el.attr("SpStates")
            .map_or_else(Vec::new, |s| s.split(',').collect())
    };
    let (ty, details, states) = match el.attr("Type")? {
        "PButtonT" => (
            "Pushbutton",
            r#"{"jLockable":true}"#.to_string(),
            format!(
                r#"{{"jLocked":{},"active":{}}}"#,
                json_str(&locked(&co("Reset")?)?),
                json_str(&co("Q")?)
            ),
        ),
        "PushButton" => {
            let locked_on = el.attr("SpStates")?.split(',').nth(2)?.to_string();
            let switch = display.is_some_and(|d| d.attr("Type") == Some("1"));
            (
                if switch { "Switch" } else { "Pushbutton" },
                r#"{"jLockable":true}"#.to_string(),
                format!(
                    r#"{{"jLocked":{},"active":{},"lockedOn":{}}}"#,
                    json_str(&locked(&co("Reset")?)?),
                    json_str(&co("Q")?),
                    json_str(&locked_on)
                ),
            )
        }
        "VirtualIn" if el.attr("Analog") == Some("false") => {
            let info = display.is_some_and(|d| d.attr("StateOnly") == Some("true"));
            let details = if info {
                ON_OFF
            } else {
                r#"{"jLockable":false,"type":71}"#
            };
            (
                if info { "InfoOnlyDigital" } else { "Switch" },
                details.to_string(),
                format!(r#"{{"active":{}}}"#, json_str(uuid)),
            )
        }
        t if INFO_DIGITAL.contains(&t) && state_only(display) => (
            "InfoOnlyDigital",
            ON_OFF.to_string(),
            format!(r#"{{"active":{}}}"#, json_str(uuid)),
        ),
        t if INFO_ANALOG.contains(&t) && state_only(display) => {
            let (format, _) = value_format(&display?.attr_decoded("Unit")?)?;
            // The error output: `Qe`, or `Q` beside the value `AQ`.
            let error = co("Qe").or_else(|| co("AQ").and(co("Q")));
            (
                "InfoOnlyAnalog",
                format!(r#"{{"jLockable":false,"format":{}}}"#, json_str(&format)),
                match error {
                    Some(e) => {
                        format!(r#"{{"value":{},"error":{}}}"#, json_str(uuid), json_str(&e))
                    }
                    None => format!(r#"{{"value":{}}}"#, json_str(uuid)),
                },
            )
        }
        // The pulse at a time of day: locked by its disable input; start
        // time, one-time date and type are its special states.
        "PulseAt" => {
            let sp: Vec<&str> = el.attr("SpStates")?.split(',').collect();
            let [start, date, kind] = sp[..] else {
                return None;
            };
            (
                "PulseAt",
                r#"{"jLockable":true,"pulseDurationLocked":false}"#.to_string(),
                format!(
                    r#"{{"jLocked":{},"isActive":{},"pulseDuration":{},"startTime":{},"oneTimePulseDate":{},"type":{}}}"#,
                    json_str(&locked(&co("InputDisable")?)?),
                    json_str(&co("Q")?),
                    json_str(&co("Time")?),
                    json_str(start),
                    json_str(date),
                    json_str(kind),
                ),
            )
        }
        // A digital day timer with the default texts.
        "DayTimer"
            if el.attr("Analog").is_none()
                && el.attr("On") == Some("")
                && el.attr("Off") == Some("") =>
        {
            let sp: Vec<&str> = el.attr("SpStates")?.split(',').collect();
            let [needs_activation, modes] = sp[..] else {
                return None;
            };
            let reset = co("Reset")?;
            (
                "Daytimer",
                r#"{"jLockable":true,"analog":false,"text":{"on":"Aktiv","off":"Inaktiv"}}"#
                    .to_string(),
                format!(
                    r#"{{"jLocked":{},"mode":{},"override":{},"value":{},"needsActivation":{},"modeList":{},"entriesAndDefaultValue":{},"resetActive":{}}}"#,
                    json_str(&locked(&reset)?),
                    json_str(&co("AQm")?),
                    json_str(&co("AQmt")?),
                    json_str(&co("AQ")?),
                    json_str(needs_activation),
                    json_str(modes),
                    json_str(uuid),
                    json_str(&reset),
                ),
            )
        }
        // The automatic jalousie: its outputs and seven special states.
        "AutoJalousie" => {
            let sp = sp();
            let [
                info,
                auto_info,
                auto_state,
                device,
                target,
                target_lamelle,
                end_pos,
            ] = sp[..]
            else {
                return None;
            };
            let history = el
                .child_elements()
                .find(|c| c.name == "COHist")?
                .attr("TRK_NE")?
                .parse::<u32>()
                .ok()?;
            icon = false;
            (
                "Jalousie",
                format!(
                    r#"{{"jLockable":true,"controlHistory":true,"hasHistory":{history},"isAutomatic":true,"animation":0,"type":348}}"#
                ),
                format!(
                    r#"{{"jLocked":{},"infoText":{},"up":{},"down":{},"position":{},"shadePosition":{},"safetyActive":{},"autoAllowed":{},"autoActive":{},"locked":{},"autoInfoText":{},"autoState":{},"deviceState":{},"targetPosition":{},"targetPositionLamelle":{},"adjustingEndPos":{}}}"#,
                    json_str(&locked(&co("Stop")?)?),
                    json_str(info),
                    json_str(&co("OutputUp")?),
                    json_str(&co("OutputDown")?),
                    json_str(&co("OutputPos")?),
                    json_str(&co("OutputLPos")?),
                    json_str(&co("OutputSafety")?),
                    json_str(&co("EnAutoShade")?),
                    json_str(&co("OutputAutoShade")?),
                    json_str(&co("OutputLock")?),
                    json_str(auto_info),
                    json_str(auto_state),
                    json_str(device),
                    json_str(target),
                    json_str(target_lamelle),
                    json_str(end_pos),
                ),
            )
        }
        // A central block lists its members (`rec=`, in order); one
        // without members is not known.
        t @ ("CentralLight" | "CentralShade") => {
            let members: Vec<String> = el
                .attr("rec")
                .filter(|r| !r.is_empty())?
                .split(',')
                .enumerate()
                .map(|(i, u)| format!(r#"{{"id":{i},"uuid":{}}}"#, json_str(u)))
                .collect();
            let sp = sp();
            let details = format!(
                r#"{{"jLockable":true,"controls":[{}],"parameterConnections":0}}"#,
                members.join(",")
            );
            icon = false;
            match (t, &sp[..]) {
                ("CentralLight", [events]) => (
                    "CentralLightController",
                    details,
                    format!(
                        r#"{{"jLocked":{},"events":{}}}"#,
                        json_str(&locked(&co("Reset")?)?),
                        json_str(events)
                    ),
                ),
                ("CentralShade", [events, safety]) => (
                    "CentralJalousie",
                    details,
                    format!(
                        r#"{{"jLocked":{},"events":{},"safetyActive":{}}}"#,
                        json_str(&locked(&co("Stop")?)?),
                        json_str(events),
                        json_str(safety)
                    ),
                ),
                _ => return None,
            }
        }
        "LightController2" => {
            let (details, states, subs) = light_controller(el, &co, objs)?;
            icon = false;
            tail = format!(r#","subControls":{{{subs}}}"#);
            ("LightControllerV2", details, states)
        }
        t if t.starts_with("Meter") => {
            let (details, states, groups) = meter(el, &co)?;
            tail = format!(r#","statisticV2":{{"groups":[{groups}]}}"#);
            ("Meter", details, states)
        }
        _ => return None,
    };
    let v = view(el);
    // A meter's statistic is its `statisticV2`.
    let statistic = match el.attr("StatsType") {
        None => String::new(),
        Some(_) if ty == "Meter" => String::new(),
        Some(stats) => {
            // The first connector is the one recorded.
            let recorded = el.child_elements().find(|c| c.name == "Co")?.attr("U")?;
            let output = match (ty, stats) {
                ("InfoOnlyDigital", "7") => format!(
                    r#"{{"frequency":1,"outputs":[{{"id":0,"name":{},"uuid":{},"visuType":1}}]}}"#,
                    json_str(&v.name),
                    json_str(recorded)
                ),
                ("InfoOnlyAnalog", "11") => {
                    let (_, plain) = value_format(&display?.attr_decoded("Unit")?)?;
                    format!(
                        r#"{{"frequency":11,"outputs":[{{"id":0,"name":{},"format":{},"uuid":{},"visuType":0}}]}}"#,
                        json_str(&v.name),
                        json_str(&plain),
                        json_str(recorded)
                    )
                }
                _ => return None,
            };
            format!(r#","statistic":{output}"#)
        }
    };
    // Notes on the object (`NTXT=`, not the documentation `NDOC=`).
    let notes = if el.attr("NTXT").is_some_and(|t| !t.is_empty()) {
        r#""hasControlNotes":true,"#
    } else {
        ""
    };
    let icon = if icon { r#""defaultIcon":null,"# } else { "" };
    Some(format!(
        r#"{{"name":{},"type":"{ty}","uuidAction":{},"room":{},"cat":{},"defaultRating":{},"isFavorite":{},"isSecured":false,{notes}{icon}"restrictions":0,"details":{details},"states":{states}{statistic}{tail}}}"#,
        json_str(&v.name),
        json_str(uuid),
        json_str(v.room.as_deref()?),
        json_str(v.cat.as_deref()?),
        v.rating,
        v.favorite,
    ))
}

/// What drives a light controller's circuit, as its catalogue entry has it.
#[derive(Clone, Copy, PartialEq)]
enum Circuit {
    Switch,
    /// A colour picker: `Rgb` or `TunableWhite`, the actuator's range.
    Color(&'static str, (u32, u32)),
}

/// A light controller's details, states and sub-controls: a circuit per
/// named output (`NameAI<i>` is `AQ<i+1>`), its kind from the actuator's
/// capability Loxone Config copies onto the block (`CapAI<i>`, the
/// actuator's `CCap=`: `31` RGB, `80` tunable white) and the colour
/// range from the actuator itself (`TWMIN`/`TWMAX`, else 2200–6500 K);
/// without a capability a switch, when its moods only switch it (`0`/`1`)
/// and nothing analog takes the output. With two circuits or more a
/// master: a switch over switches, else a dimmer, and a master colour
/// when every circuit is a colour. Unknown and not written: other
/// capabilities, a circuit whose moods carry colour values without one (a
/// Lumitech output), a controller without its special states.
fn light_controller(
    el: &Element,
    co: &dyn Fn(&str) -> Option<String>,
    objs: &Objects,
) -> Option<(String, String, String)> {
    let uuid = el.attr("U")?;
    let sp: Vec<&str> = el.attr("SpStates")?.split(',').collect();
    let seq: Vec<&str> = el.attr("uuidSeqencing")?.split(',').collect();
    let seq_ix: Vec<&str> = el.attr("uuidSeqenceIx")?.split(',').collect();
    if sp.len() < 13 || seq.len() < 19 || seq_ix.len() < 19 {
        return None;
    }
    let moods: Vec<&Element> = el
        .child_elements()
        .find(|c| c.name == "LightscenesC")?
        .child_elements()
        .filter(|c| c.name == "LightsceneC")
        .collect();
    // The mood the movement switches on, by its place in the list.
    let movement = moods.iter().position(|m| m.attr("Move") == Some("true"))? + 1;
    let history = el
        .child_elements()
        .find(|c| c.name == "COHist")?
        .attr("TRK_NE")?
        .parse::<u32>()
        .ok()?;

    let mut circuits: Vec<(usize, String, Circuit)> = Vec::new();
    for i in 0..18 {
        let Some(name) = el.attr_decoded(&format!("NameAI{i}")) else {
            continue;
        };
        let output = co(&format!("AQ{}", i + 1))?;
        let range = objs.tw.get(&output.to_ascii_lowercase()).copied();
        let kind = match el.attr(&format!("CapAI{i}")) {
            Some("31") => Circuit::Color("Rgb", range.unwrap_or((2200, 6500))),
            Some("80") => Circuit::Color("TunableWhite", range.unwrap_or((2200, 6500))),
            Some(_) => return None,
            None => {
                let q = format!("Q{}", i + 1);
                let switched = moods
                    .iter()
                    .all(|m| m.attr(&q).is_none_or(|v| v == "0" || v == "1"));
                if !switched || objs.analog_sink.contains(&output.to_ascii_lowercase()) {
                    return None;
                }
                Circuit::Switch
            }
        };
        circuits.push((i, name.into_owned(), kind));
    }
    if circuits.is_empty() {
        return None;
    }

    let sub = |key: &str, name: &str, ty: &str, details: Option<String>, states: String| {
        let action = format!("{uuid}/{key}");
        let details = details.map_or_else(String::new, |d| format!(r#""details":{d},"#));
        format!(
            r#"{}:{{"name":{},"type":"{ty}","uuidAction":{},"defaultRating":0,"isFavorite":false,"isSecured":false,"restrictions":0,{details}"states":{states}}}"#,
            json_str(&action),
            json_str(name),
            json_str(&action),
        )
    };
    let picker = |kind: &str, (min, max): (u32, u32)| {
        format!(r#"{{"pickerType":"{kind}","TWMin":{min},"TWMax":{max}}}"#)
    };
    let color = |output: String, i: usize| {
        format!(
            r#"{{"color":{},"sequence":{},"sequenceColorIdx":{}}}"#,
            json_str(&output),
            json_str(seq[i]),
            json_str(seq_ix[i])
        )
    };

    let mut details = format!(
        r#"{{"jLockable":true,"controlHistory":true,"hasHistory":{history},"movementScene":{movement}"#
    );
    let mut subs = Vec::new();
    if circuits.len() > 1 {
        details.push_str(&format!(
            r#","masterValue":{}"#,
            json_str(&format!("{uuid}/masterValue"))
        ));
        let master = co("AQ20")?;
        if circuits.iter().all(|c| c.2 == Circuit::Switch) {
            subs.push(sub(
                "masterValue",
                "Master-Schalter",
                "Switch",
                None,
                format!(r#"{{"active":{}}}"#, json_str(&master)),
            ));
        } else {
            subs.push(sub(
                "masterValue",
                "Master-Helligkeit",
                "Dimmer",
                None,
                format!(
                    r#"{{"position":{},"min":{},"max":{},"step":{}}}"#,
                    json_str(&master),
                    json_str(sp[7]),
                    json_str(sp[8]),
                    json_str(&co("Step")?)
                ),
            ));
        }
        let ranges: Option<Vec<(u32, u32)>> = circuits
            .iter()
            .map(|c| match c.2 {
                Circuit::Color(_, r) => Some(r),
                Circuit::Switch => None,
            })
            .collect();
        if let Some(ranges) = ranges {
            details.push_str(&format!(
                r#","masterColor":{}"#,
                json_str(&format!("{uuid}/masterColor"))
            ));
            let range = (
                ranges.iter().map(|r| r.0).min()?,
                ranges.iter().map(|r| r.1).max()?,
            );
            subs.push(sub(
                "masterColor",
                "Master-Farbe",
                "ColorPickerV2",
                Some(picker("Rgb", range)),
                color(co("AQ19")?, 18),
            ));
        }
    }
    details.push('}');
    for (i, name, kind) in &circuits {
        let output = co(&format!("AQ{}", i + 1))?;
        let key = format!("AI{}", i + 1);
        subs.push(match *kind {
            Circuit::Switch => sub(
                &key,
                name,
                "Switch",
                None,
                format!(r#"{{"active":{}}}"#, json_str(&output)),
            ),
            Circuit::Color(kind, range) => sub(
                &key,
                name,
                "ColorPickerV2",
                Some(picker(kind, range)),
                color(output, *i),
            ),
        });
    }
    let states = format!(
        r#"{{"jLocked":{},"activeMoods":{},"moodList":{},"activeMoodsNum":{},"favoriteMoods":{},"additionalMoods":{},"circuitNames":{},"daylightConfig":{},"presence":{}}}"#,
        json_str(&locked(&co("Reset")?)?),
        json_str(sp[2]),
        json_str(sp[3]),
        json_str(sp[10]),
        json_str(sp[4]),
        json_str(sp[5]),
        json_str(sp[9]),
        json_str(sp[11]),
        json_str(sp[12]),
    );
    Some((details, states, subs.join(",")))
}

/// A meter's details, states and statistic groups. Known for the units
/// the corpus shows: a power unit (`Un1=`) and name (`PwrN=`), no unit of
/// its own for the total (`Un2=`), recorded (`StatsType="8"`).
fn meter(el: &Element, co: &dyn Fn(&str) -> Option<String>) -> Option<(String, String, String)> {
    if el.attr("Un2").is_some() || el.attr("StatsType") != Some("8") {
        return None;
    }
    let power = el.attr_decoded("PwrN")?;
    let (actual, actual_stat) = value_format(&el.attr_decoded("Un1")?)?;
    let kind = match el.attr("Type")? {
        "MeterDig" | "MeterAbsUni" => "unidirectional",
        "MeterAbsBi" => "bidirectional",
        "MeterAbsSt" => "storage",
        _ => return None,
    };
    let mut details = format!(
        r#"{{"jLockable":false,"type":"{kind}","displayType":0,"actualFormat":{},"totalFormat":"","powerName":{}"#,
        json_str(&actual),
        json_str(&power)
    );
    // State name → connector, in Loxone Config's order.
    let mut states: Vec<(&str, &str)> = vec![("jLocked", "R"), ("actual", "OPf")];
    let mut storage = None;
    if kind == "unidirectional" {
        states.extend([
            ("total", "OMr"),
            ("totalDay", "ORd"),
            ("totalWeek", "ORw"),
            ("totalMonth", "ORm"),
            ("totalYear", "ORy"),
        ]);
    } else {
        states.extend([("total", "OMr1"), ("totalNeg", "OMr2")]);
        if kind == "storage" {
            let (format, stat) = value_format(&el.attr_decoded("UnSt")?)?;
            let max: f64 = el.attr("MaxLvl")?.parse().ok()?;
            details.push_str(&format!(
                r#","storageFormat":{},"storageMax":{}"#,
                json_str(&format),
                serde_json::to_string(&max).ok()?
            ));
            states.push(("storage", "Slvl"));
            storage = Some(stat);
        }
        states.extend([
            ("totalDay", "ORd1"),
            ("totalWeek", "ORw1"),
            ("totalMonth", "ORm1"),
            ("totalYear", "ORy1"),
            ("totalNegDay", "ORd2"),
            ("totalNegWeek", "ORw2"),
            ("totalNegMonth", "ORm2"),
            ("totalNegYear", "ORy2"),
        ]);
    }
    details.push('}');
    let mut out = Vec::new();
    for (name, k) in states {
        let u = co(k)?;
        let u = if name == "jLocked" { locked(&u)? } else { u };
        out.push(format!("{}:{}", json_str(name), json_str(&u)));
    }
    // The groups the meter records into (`<statGrp id m acc>`): power,
    // the meter reading(s), the storage level.
    let point = |title: &str, format: &str, output: &str| {
        format!(
            r#"{{"title":{},"format":{},"output":"{output}"}}"#,
            json_str(title),
            json_str(format)
        )
    };
    let mut groups = Vec::new();
    for g in el.child_elements().filter(|c| c.name == "statGrp") {
        let id = g.attr("id")?;
        let mode: u32 = g.attr("m")?.parse().ok()?;
        let points = match id {
            "1" => point(&power, &stat_format(&actual_stat)?, "actual"),
            "2" if kind == "unidirectional" => point("Zählerstand", "", "total"),
            "2" => format!(
                "{},{}",
                point("Zählerstand", "", "total"),
                point("Zählerstand", "", "totalNeg")
            ),
            "3" => point(
                "Speicherstand",
                &stat_format(storage.as_deref()?)?,
                "storageLevel",
            ),
            _ => return None,
        };
        let acc = match g.attr("acc") {
            None => "",
            Some("true") => r#","accumulated":true"#,
            Some(_) => return None,
        };
        groups.push(format!(
            r#"{{"id":{},"mode":{mode}{acc},"dataPoints":[{points}]}}"#,
            json_str(id)
        ));
    }
    Some((details, format!("{{{}}}", out.join(",")), groups.join(",")))
}

/// A statistic's number format from a printf one: `%.3fkW` is `0.000kW`.
fn stat_format(printf: &str) -> Option<String> {
    let open = printf.find("%.")?;
    let close = open + printf[open..].find('f')?;
    let digits: usize = printf[open + 2..close].parse().ok()?;
    let number = if digits == 0 {
        "0".to_string()
    } else {
        format!("0.{}", "0".repeat(digits))
    };
    Some(format!(
        "{}{number}{}",
        &printf[..open],
        &printf[close + 1..]
    ))
}

/// The display shows the state only, with no settings beyond its type and
/// unit (texts or colours of its own are not known).
fn state_only(display: Option<&Element>) -> bool {
    display.is_some_and(|d| {
        d.attr("StateOnly") == Some("true")
            && d.attrs
                .iter()
                .all(|a| ["Type", "Unit", "StateOnly"].contains(&a.name.as_str()))
    })
}

/// The app's printf format of a display unit: `<v>` is `%.0f`, `<v.N>`
/// is `%.Nf`, the rest is literal. As the entry's `format` (`%` doubled)
/// and as the statistic's (`%` as it is).
fn value_format(unit: &str) -> Option<(String, String)> {
    let open = unit.find("<v")?;
    let close = open + unit[open..].find('>')?;
    let digits = match &unit[open + 2..close] {
        "" => "0",
        d => d
            .strip_prefix('.')
            .filter(|d| d.len() == 1 && d.bytes().all(|b| b.is_ascii_digit()))?,
    };
    let (before, after) = (&unit[..open], &unit[close + 1..]);
    if before.contains('<') || after.contains('<') {
        return None;
    }
    let value = format!("%.{digits}f");
    Some((
        format!(
            "{}{value}{}",
            before.replace('%', "%%"),
            after.replace('%', "%%")
        ),
        format!("{before}{value}{after}"),
    ))
}

/// The `jLocked` state of a block: its `Reset` connector with the top bit
/// of the first byte set.
fn locked(reset: &str) -> Option<String> {
    let first = u8::from_str_radix(reset.get(..2)?, 16).ok()?;
    Some(format!("{:02x}{}", first | 0x80, &reset[2..]))
}

/// What the catalogue takes from an object for every control type.
struct AppView {
    /// `Desc=` when set, else the title
    name: String,
    room: Option<String>,
    cat: Option<String>,
    favorite: bool,
    /// `<IoData Rating=…>`, the stars in the app
    rating: u32,
}

fn view(el: &Element) -> AppView {
    let io = el.child_elements().find(|c| c.name == "IoData");
    let io_attr = |k: &str| {
        io.and_then(|io| io.attr(k))
            .filter(|v| !v.is_empty())
            .map(str::to_ascii_lowercase)
    };
    let name = el
        .attr_decoded("Desc")
        .filter(|d| !d.is_empty())
        .or_else(|| el.attr_decoded("Title"))
        .unwrap_or_default()
        .into_owned();
    AppView {
        name,
        room: io_attr("Pr"),
        cat: io_attr("Cr"),
        favorite: io.and_then(|io| io.attr("UseInFav")) == Some("true"),
        rating: io
            .and_then(|io| io.attr("Rating"))
            .and_then(|r| r.parse().ok())
            .unwrap_or(0),
    }
}

/// The objects of a document as the catalogue sees them.
struct Objects<'d> {
    /// Lowercase UUID → the object shown in the app (`<IoData
    /// Visu="true">`), in document order. The first object with an
    /// `<IoData>` wins (an `InputRef` twin shares the UUID).
    shown_in_order: Vec<(String, &'d Element)>,
    shown: BTreeMap<String, usize>,
    /// Every object UUID.
    all: BTreeSet<String>,
    /// The UUIDs an `InputRef` names (`Ref=`): virtual inputs a page uses.
    referenced: BTreeSet<String>,
    /// Lowercase output UUID → the colour range (`TWMIN`, `TWMAX`) of the
    /// actuator an `OutputRef` on it drives.
    tw: BTreeMap<String, (u32, u32)>,
    /// The outputs an analog `OutputRef` takes.
    analog_sink: BTreeSet<String>,
}

impl<'d> Objects<'d> {
    fn of(doc: &'d LoxoneDoc) -> Self {
        let mut out = Objects {
            shown_in_order: Vec::new(),
            shown: BTreeMap::new(),
            all: BTreeSet::new(),
            referenced: BTreeSet::new(),
            tw: BTreeMap::new(),
            analog_sink: BTreeSet::new(),
        };
        let mut by_uuid: BTreeMap<String, &'d Element> = BTreeMap::new();
        let mut output_refs: Vec<&'d Element> = Vec::new();
        let mut seen = BTreeSet::new();
        for o in doc.objects() {
            let uuid = o.uuid.to_ascii_lowercase();
            out.all.insert(uuid.clone());
            let Some(el) = doc.element_at(&o.path) else {
                continue;
            };
            by_uuid.entry(uuid.clone()).or_insert(el);
            if o.block_type == "OutputRef" {
                output_refs.push(el);
            }
            if o.block_type == "InputRef"
                && let Some(r) = el.attr("Ref")
            {
                out.referenced.insert(r.to_ascii_lowercase());
            }
            let Some(io) = el.child_elements().find(|c| c.name == "IoData") else {
                continue;
            };
            if !seen.insert(uuid.clone()) {
                continue;
            }
            if io.attr("Visu") == Some("true") {
                out.shown.insert(uuid.clone(), out.shown_in_order.len());
                out.shown_in_order.push((uuid, el));
            }
        }
        for r in output_refs {
            let target = r
                .attr("Ref")
                .and_then(|t| by_uuid.get(&t.to_ascii_lowercase()));
            let range = target.and_then(|t| {
                Some((
                    t.attr("TWMIN")?.parse().ok()?,
                    t.attr("TWMAX")?.parse().ok()?,
                ))
            });
            let analog = r.attr("Analog") == Some("true");
            for input in r
                .child_elements()
                .filter(|c| c.name == "Co")
                .flat_map(|c| c.child_elements().filter(|i| i.name == "In"))
                .filter_map(|i| i.attr("Input"))
            {
                let input = input.to_ascii_lowercase();
                if let Some(range) = range {
                    out.tw.entry(input.clone()).or_insert(range);
                }
                if analog {
                    out.analog_sink.insert(input);
                }
            }
        }
        out
    }

    fn shown(&self, uuid: &str) -> Option<&'d Element> {
        self.shown.get(uuid).map(|&i| self.shown_in_order[i].1)
    }
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).expect("a string serializes")
}

/// A member of a JSON object in the text: its decoded key, the span from
/// the key's opening quote to the end of the value, and the value's span.
struct Member {
    key: String,
    span: Range<usize>,
    value: Range<usize>,
}

/// The members of the object whose `{` is at `open`, and the index of its
/// `}`.
fn object(s: &str, open: usize) -> Result<(Vec<Member>, usize), String> {
    let b = s.as_bytes();
    let bad = |at: usize| format!("{MEMBER}: unexpected input at byte {at}");
    if b.get(open) != Some(&b'{') {
        return Err(bad(open));
    }
    let mut out = Vec::new();
    let mut i = skip_ws(s, open + 1);
    if b.get(i) == Some(&b'}') {
        return Ok((out, i));
    }
    loop {
        let key_start = i;
        let key_end = value_end(s, i)?;
        let key: String = serde_json::from_str(&s[key_start..key_end]).map_err(|_| bad(i))?;
        i = skip_ws(s, key_end);
        if b.get(i) != Some(&b':') {
            return Err(bad(i));
        }
        let start = skip_ws(s, i + 1);
        let end = value_end(s, start)?;
        out.push(Member {
            key,
            span: key_start..end,
            value: start..end,
        });
        i = skip_ws(s, end);
        match b.get(i) {
            Some(b',') => i = skip_ws(s, i + 1),
            Some(b'}') => return Ok((out, i)),
            _ => return Err(bad(i)),
        }
    }
}

/// Where the JSON value starting at `i` ends.
fn value_end(s: &str, i: usize) -> Result<usize, String> {
    let b = s.as_bytes();
    let bad = |at: usize| format!("{MEMBER}: unexpected input at byte {at}");
    match b.get(i) {
        Some(b'"') => {
            let mut j = i + 1;
            while j < b.len() {
                match b[j] {
                    b'\\' => j += 2,
                    b'"' => return Ok(j + 1),
                    _ => j += 1,
                }
            }
            Err(bad(i))
        }
        Some(b'{') | Some(b'[') => {
            let mut depth = 0usize;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    b'"' => {
                        j = value_end(s, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Ok(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            Err(bad(i))
        }
        Some(_) => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']') && !b[j].is_ascii_whitespace()
            {
                j += 1;
            }
            if j == i { Err(bad(i)) } else { Ok(j) }
        }
        None => Err(bad(i)),
    }
}

/// The string values of the `"format"` members anywhere in `range`.
fn format_values(s: &str, range: Range<usize>) -> Vec<Range<usize>> {
    const KEY: &str = "\"format\"";
    let mut out = Vec::new();
    let mut from = range.start;
    while let Some(at) = s[from..range.end].find(KEY).map(|i| from + i) {
        from = at + KEY.len();
        // A key, not the tail of an escaped string.
        if s[..at].ends_with('\\') {
            continue;
        }
        let colon = skip_ws(s, from);
        if s.as_bytes().get(colon) != Some(&b':') {
            continue;
        }
        let start = skip_ws(s, colon + 1);
        if s.as_bytes().get(start) == Some(&b'"')
            && let Ok(end) = value_end(s, start)
            && end <= range.end
        {
            out.push(start..end);
            from = end;
        }
    }
    out
}

fn skip_ws(s: &str, mut i: usize) -> usize {
    let b = s.as_bytes();
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// The spans that remove the members `removed` (indexes) from their
/// object, commas included: a run at the start up to the first member that
/// stays, every other one with the comma before it.
fn removal_spans<'a>(
    all: &'a [Member],
    removed: &'a BTreeSet<usize>,
) -> impl Iterator<Item = Range<usize>> + 'a {
    let first_kept = (0..all.len()).find(|i| !removed.contains(i));
    let prefix = match first_kept {
        Some(0) => None,
        Some(k) => Some(all[0].span.start..all[k].span.start),
        None => all
            .first()
            .map(|f| f.span.start..all[all.len() - 1].span.end),
    };
    let rest = removed
        .iter()
        .filter(move |&&i| first_kept.is_some_and(|k| i > k))
        .map(move |&i| all[i - 1].span.end..all[i].span.end);
    prefix.into_iter().chain(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: &str = "0f000000-0000-00a1-ffff000000000000";
    const ROOM2: &str = "0f000000-0000-00a2-ffff000000000000";
    const CAT: &str = "0f000000-0000-00b1-ffff000000000000";
    const BUTTON: &str = "0f000000-0000-0002-ffff000000000000";
    const INPUT: &str = "0f000000-0000-0003-ffff000000000000";
    const NEW: &str = "0f000000-0000-0004-ffff000000000000";
    const NOW: &str = "2026-10-30 21:00:00";

    /// A push button (shown as a switch) and a virtual input; `extra` goes
    /// on the page.
    fn doc(title: &str, room: &str, extra: &str) -> LoxoneDoc {
        LoxoneDoc::parse(
            format!(
                r#"<?xml version="1.0" encoding="utf-8"?>
<ControlList>
	<C Type="Document" V="178" U="0f000000-0000-0000-ffff000000000000">
		<C Type="VirtualInCaption" U="0f000000-0000-0010-ffff000000000000">
			<C Type="VirtualIn" U="{INPUT}" Title="Eingang" Analog="false">
				<Co K="Q" U="0f000000-0000-0011-00ff000000000000"/>
				<IoData Visu="true" Pr="{ROOM}" Cr="{CAT}"/>
				<Display Type="1" Unit="&lt;v.1&gt;" StateOnly="true"/>
			</C>
		</C>
		<C Type="Page" U="0f000000-0000-0001-ffff000000000000" Title="Licht">
			<C Type="InputRef" U="0f000000-0000-0012-ffff000000000000" Ref="{INPUT}"/>
			<C Type="PushButton" U="{BUTTON}" Title="{title}" SpStates="0f000000-0000-0020-ffff000000000000,0f000000-0000-0021-ffff000000000000,0f000000-0000-0022-ffff000000000000">
				<Co K="Reset" U="0f000000-0000-0023-02ff000000000000"/>
				<Co K="Q" U="0f000000-0000-0024-05ff000000000000"/>
				<IoData Visu="true" Pr="{room}" Cr="{CAT}"/>
				<Display Type="1" Unit="&lt;v.1&gt;"/>
			</C>
			{extra}
		</C>
	</C>
</ControlList>
"#
            )
            .as_bytes(),
        )
        .unwrap()
    }

    /// What Loxone Config writes for `doc("Licht", ROOM, "")`.
    fn catalogue(also: &str) -> String {
        format!(
            "\u{feff}{{\"lastModified\":\"2026-10-29 17:15:28\",\"msInfo\":{{\"latitude\":48.343000}},\
             \"rooms\":{{\"{ROOM}\":{{\"name\":\"Küche\"}},\"{ROOM2}\":{{\"name\":\"Bad\"}}}},\"cats\":{{\"{CAT}\":{{}}}},\
             \"controls\":{{\"U:{INPUT}\":{{\"name\":\"Eingang\",\"type\":\"InfoOnlyDigital\",\
             \"uuidAction\":\"{INPUT}\",\"room\":\"{ROOM}\",\"cat\":\"{CAT}\",\"defaultRating\":0,\
             \"isFavorite\":false,\"isSecured\":false,\"defaultIcon\":null,\"restrictions\":0,\
             \"details\":{{\"jLockable\":false,\"text\":{{\"off\":\"Aus\",\"on\":\"Ein\"}},\
             \"color\":{{\"off\":\"#FF0054\",\"on\":\"#8DFF70\"}}}},\"states\":{{\"active\":\"{INPUT}\"}}}},\
             \"U:{BUTTON}\":{{\"name\":\"Licht\",\"type\":\"Switch\",\"uuidAction\":\"{BUTTON}\",\
             \"room\":\"{ROOM}\",\"cat\":\"{CAT}\",\"defaultRating\":0,\"isFavorite\":false,\
             \"isSecured\":false,\"defaultIcon\":null,\"restrictions\":0,\"details\":{{\"jLockable\":true}},\
             \"states\":{{\"jLocked\":\"8f000000-0000-0023-02ff000000000000\",\
             \"active\":\"0f000000-0000-0024-05ff000000000000\",\
             \"lockedOn\":\"0f000000-0000-0022-ffff000000000000\"}}}}{also}}}}}"
        )
    }

    fn whats(u: &Update) -> (Vec<&str>, Vec<&str>) {
        (
            u.applied.iter().map(|c| c.what.as_str()).collect(),
            u.left.iter().map(|c| c.what.as_str()).collect(),
        )
    }

    #[test]
    fn the_generated_entries_are_loxone_configs() {
        // Hidden, then shown again: the entries come back as they were.
        let shown = doc("Licht", ROOM, "");
        let hidden = LoxoneDoc::parse(
            &String::from_utf8(shown.to_bytes())
                .unwrap()
                .replace("Visu=\"true\"", "Visu=\"false\"")
                .into_bytes(),
        )
        .unwrap();
        let cat = catalogue("");
        let gone = update(&cat, &shown, &hidden, NOW).unwrap();
        assert_eq!(
            whats(&gone).0,
            ["hidden from the app", "hidden from the app"]
        );
        assert!(gone.text.contains("\"controls\":{}"), "{}", gone.text);
        let back = update(&gone.text, &hidden, &shown, NOW).unwrap();
        assert_eq!(whats(&back).0, ["added to the app", "added to the app"]);
        let cat_now = cat.replace("2026-10-29 17:15:28", NOW);
        let (a, b): (Value, Value) = (
            serde_json::from_str(&back.text[3..]).unwrap(),
            serde_json::from_str(&cat_now[3..]).unwrap(),
        );
        assert_eq!(a, b);
        // In document order, as text: the button's entry is Loxone Config's.
        let button = cat.find(&format!("\"U:{BUTTON}\"")).unwrap();
        assert!(back.text.ends_with(&cat[button..]), "{}", back.text);
    }

    /// The entry of the one object of type `ty` in a page holding `xml`.
    fn entry_of(ty: &str, xml: &str) -> Option<String> {
        let d = doc("Licht", ROOM, xml);
        let o = d.objects().into_iter().find(|o| o.block_type == ty)?;
        entry(d.element_at(&o.path)?, &Objects::of(&d))
    }

    /// The tail of an entry after `restrictions` (what the type decides).
    fn tail(e: &str) -> &str {
        &e[e.find("\"details\"").unwrap()..]
    }

    /// Connectors `K` with UUIDs `1f000000-0000-00NN-NNff000000000000`.
    fn cos(keys: &[&str]) -> String {
        keys.iter()
            .enumerate()
            .map(|(i, k)| {
                format!(r#"<Co K="{k}" U="1f000000-0000-00{i:02x}-{i:02x}ff000000000000"/>"#)
            })
            .collect()
    }

    #[test]
    fn meters_and_central_blocks_follow_loxone_config() {
        let io = format!(r#"<IoData Visu="true" Pr="{ROOM}" Cr="{CAT}" Rating="2"/>"#);
        let meter = |ty: &str, attrs: &str, keys: &[&str], groups: &str| {
            entry_of(
                ty,
                &format!(
                    r#"<C Type="{ty}" U="{NEW}" Title="Zähler" PwrN="Leistung" Un1="&lt;v.3&gt;kW"{attrs} StatsType="8">{}{io}{groups}</C>"#,
                    cos(keys)
                ),
            )
        };
        let uni = meter(
            "MeterDig",
            "",
            &["A", "R", "OPf", "OMr", "ORd", "ORw", "ORm", "ORy"],
            r#"<statGrp id="1" m="1"/><statGrp id="2" m="11" acc="true"/>"#,
        )
        .unwrap();
        assert!(uni.contains(r#""type":"Meter""#));
        assert!(uni.contains(r#""defaultRating":2,"#));
        assert!(uni.contains(r#""defaultIcon":null,"#));
        assert!(uni.ends_with(
            r#""details":{"jLockable":false,"type":"unidirectional","displayType":0,"actualFormat":"%.3fkW","totalFormat":"","powerName":"Leistung"},"states":{"jLocked":"9f000000-0000-0001-01ff000000000000","actual":"1f000000-0000-0002-02ff000000000000","total":"1f000000-0000-0003-03ff000000000000","totalDay":"1f000000-0000-0004-04ff000000000000","totalWeek":"1f000000-0000-0005-05ff000000000000","totalMonth":"1f000000-0000-0006-06ff000000000000","totalYear":"1f000000-0000-0007-07ff000000000000"},"statisticV2":{"groups":[{"id":"1","mode":1,"dataPoints":[{"title":"Leistung","format":"0.000kW","output":"actual"}]},{"id":"2","mode":11,"accumulated":true,"dataPoints":[{"title":"Zählerstand","format":"","output":"total"}]}]}}"#
        ));
        let bi_keys = [
            "R", "OPf", "OMr1", "OMr2", "Slvl", "ORd1", "ORw1", "ORm1", "ORy1", "ORd2", "ORw2",
            "ORm2", "ORy2",
        ];
        let st = meter(
            "MeterAbsSt",
            r#" UnSt="&lt;v&gt;%" MaxLvl="100""#,
            &bi_keys,
            r#"<statGrp id="1" m="12"/><statGrp id="2" m="11" acc="true"/><statGrp id="3" m="11"/>"#,
        )
        .unwrap();
        assert!(st.contains(r#""type":"storage","#));
        assert!(st.contains(r#""storageFormat":"%.0f%%","storageMax":100.0}"#));
        assert!(st.contains(r#""storage":"1f000000-0000-0004-04ff000000000000","totalDay""#));
        assert!(st.contains(r#"{"title":"Zählerstand","format":"","output":"totalNeg"}"#));
        assert!(st.contains(
            r#"{"id":"3","mode":11,"dataPoints":[{"title":"Speicherstand","format":"0%","output":"storageLevel"}]}"#
        ));
        let bi = meter(
            "MeterAbsBi",
            "",
            &bi_keys,
            r#"<statGrp id="1" m="12"/><statGrp id="2" m="11" acc="true"/>"#,
        )
        .unwrap();
        assert!(bi.contains(r#""type":"bidirectional","#));
        assert!(bi.contains(r#""totalNegYear":"1f000000-0000-000c-0cff000000000000""#));
        // A unit of its own for the total is not known.
        assert_eq!(
            meter("MeterDig", r#" Un2="&lt;v&gt;kWh""#, &["R"], ""),
            None
        );
        assert_eq!(stat_format("%.0f%"), Some("0%".to_string()));

        // A central block lists its members; one without is not known.
        let central = |ty: &str, rec: &str, sp: &str, keys: &[&str]| {
            entry_of(
                ty,
                &format!(
                    r#"<C Type="{ty}" U="{NEW}" Title="Zentral"{rec} SpStates="{sp}">{}{io}</C>"#,
                    cos(keys)
                ),
            )
        };
        let shade = central(
            "CentralShade",
            r#" rec="aa000000-0000-0000-ffff000000000000,bb000000-0000-0000-ffff000000000000""#,
            "e0000000-0000-0000-ffff000000000000,e1000000-0000-0000-ffff000000000000",
            &["Stop"],
        )
        .unwrap();
        assert!(!shade.contains("defaultIcon"));
        assert!(shade.ends_with(
            r#""details":{"jLockable":true,"controls":[{"id":0,"uuid":"aa000000-0000-0000-ffff000000000000"},{"id":1,"uuid":"bb000000-0000-0000-ffff000000000000"}],"parameterConnections":0},"states":{"jLocked":"9f000000-0000-0000-00ff000000000000","events":"e0000000-0000-0000-ffff000000000000","safetyActive":"e1000000-0000-0000-ffff000000000000"}}"#
        ));
        let light = central(
            "CentralLight",
            r#" rec="aa000000-0000-0000-ffff000000000000""#,
            "e0000000-0000-0000-ffff000000000000",
            &["Reset"],
        )
        .unwrap();
        assert!(light.contains(r#""type":"CentralLightController""#));
        assert_eq!(
            central(
                "CentralLight",
                "",
                "e0000000-0000-0000-ffff000000000000",
                &["Reset"]
            ),
            None
        );
    }

    #[test]
    fn light_controllers_list_their_circuits_and_masters() {
        let io = format!(r#"<IoData Visu="true" Pr="{ROOM}" Cr="{CAT}"/>"#);
        let list = |tag: &str, n: usize| {
            (0..n)
                .map(|i| format!("{tag}0000000-0000-00{i:02x}-ffff000000000000"))
                .collect::<Vec<_>>()
                .join(",")
        };
        // An RGB circuit on a tunable white actuator and a switched one;
        // the movement switches on the second mood.
        let lc = |names: &str, q2: &str, extra: &str| {
            entry_of(
                "LightController2",
                &format!(
                    r#"<C Type="LightController2" U="{NEW}" Title="Licht" {names} SpStates="{}" uuidSeqencing="{}" uuidSeqenceIx="{}">{}<COHist TRK_NE="20"/><LightscenesC><LightsceneC Title="Aus" Q1="0" Q2="0"/><LightsceneC Title="Bewegung" Move="true" Q1="0" Q2="{q2}"/></LightscenesC>{io}</C>
                    <C Type="TreeAactor" U="2f000000-0000-0000-ffff000000000000" TWMIN="2000" TWMAX="6000" CCap="31"/>
                    <C Type="OutputRef" U="2f000000-0000-0001-ffff000000000000" Ref="2f000000-0000-0000-ffff000000000000"><Co K="AI" U="2f000000-0000-0002-00ff000000000000"><In Input="1f000000-0000-0002-02ff000000000000"/></Co></C>{extra}"#,
                    list("e", 13),
                    list("a", 20),
                    list("b", 20),
                    cos(&["Reset", "Step", "AQ1", "AQ2", "AQ19", "AQ20"]),
                ),
            )
        };
        let both = lc(r#"NameAI0="Farbe" CapAI0="31" NameAI1="Spots""#, "1", "").unwrap();
        assert!(both.contains(r#""type":"LightControllerV2""#));
        assert!(!both.contains("defaultIcon"));
        assert!(both.contains(
            r#""details":{"jLockable":true,"controlHistory":true,"hasHistory":20,"movementScene":2,"masterValue":"0f000000-0000-0004-ffff000000000000/masterValue"},"states":{"jLocked":"9f000000-0000-0000-00ff000000000000","activeMoods":"e0000000-0000-0002-ffff000000000000","moodList":"e0000000-0000-0003-ffff000000000000","activeMoodsNum":"e0000000-0000-000a-ffff000000000000","#
        ));
        assert!(both.contains(
            r#""0f000000-0000-0004-ffff000000000000/masterValue":{"name":"Master-Helligkeit","type":"Dimmer","uuidAction":"0f000000-0000-0004-ffff000000000000/masterValue","defaultRating":0,"isFavorite":false,"isSecured":false,"restrictions":0,"states":{"position":"1f000000-0000-0005-05ff000000000000","min":"e0000000-0000-0007-ffff000000000000","max":"e0000000-0000-0008-ffff000000000000","step":"1f000000-0000-0001-01ff000000000000"}}"#
        ));
        assert!(both.contains(
            r#""details":{"pickerType":"Rgb","TWMin":2000,"TWMax":6000},"states":{"color":"1f000000-0000-0002-02ff000000000000","sequence":"a0000000-0000-0000-ffff000000000000","sequenceColorIdx":"b0000000-0000-0000-ffff000000000000"}}"#
        ));
        assert!(both.ends_with(
            r#""0f000000-0000-0004-ffff000000000000/AI2":{"name":"Spots","type":"Switch","uuidAction":"0f000000-0000-0004-ffff000000000000/AI2","defaultRating":0,"isFavorite":false,"isSecured":false,"restrictions":0,"states":{"active":"1f000000-0000-0003-03ff000000000000"}}}}"#
        ));
        assert!(!both.contains("masterColor"));

        // Two colours: a master colour over their range too (2200 K without
        // an actuator); two switches: a master switch.
        let colours = lc(
            r#"NameAI0="A" CapAI0="31" NameAI1="B" CapAI1="80""#,
            "1",
            "",
        )
        .unwrap();
        assert!(colours.contains(
            r#""name":"Master-Farbe","type":"ColorPickerV2","uuidAction":"0f000000-0000-0004-ffff000000000000/masterColor","defaultRating":0,"isFavorite":false,"isSecured":false,"restrictions":0,"details":{"pickerType":"Rgb","TWMin":2000,"TWMax":6500},"states":{"color":"1f000000-0000-0004-04ff000000000000","sequence":"a0000000-0000-0012-ffff000000000000""#
        ));
        assert!(colours.contains(r#""pickerType":"TunableWhite","TWMin":2200,"TWMax":6500"#));
        let switches = lc(r#"NameAI0="A" NameAI1="B""#, "1", "").unwrap();
        assert!(switches.contains(r#""name":"Master-Schalter","type":"Switch""#));
        // One circuit: no master.
        let one = lc(r#"NameAI1="B""#, "1", "").unwrap();
        assert!(!one.contains("master"));

        // Not known: a mood giving a switched output a colour (Lumitech),
        // an analog output on a switched circuit, another capability.
        assert_eq!(lc(r#"NameAI1="B""#, "50", ""), None);
        assert_eq!(
            lc(
                r#"NameAI1="B""#,
                "1",
                r#"<C Type="OutputRef" U="2f000000-0000-0003-ffff000000000000" Analog="true"><Co K="AI" U="2f000000-0000-0004-00ff000000000000"><In Input="1f000000-0000-0003-03ff000000000000"/></Co></C>"#
            ),
            None
        );
        assert_eq!(lc(r#"NameAI0="A" CapAI0="7""#, "1", ""), None);
    }

    #[test]
    fn info_displays_pulses_and_day_timers_follow_loxone_config() {
        // A memory flag with statistics and notes: an info display; the
        // first connector is recorded.
        let memory = entry_of(
            "Memory",
            r#"<C Type="Memory" U="0f000000-0000-0030-ffff000000000000" Title="Anwesend" StatsType="7" StatsAutoDel="1" NTXT="Notiz">
					<Co K="Input" U="0f000000-0000-0031-00ff000000000000"/>
					<Co K="AQ" U="0f000000-0000-0032-01ff000000000000"/>
					<Co K="Q" U="0f000000-0000-0033-02ff000000000000"/>
					<IoData Visu="true" Cr="0f000000-0000-00b1-ffff000000000000" Pr="0f000000-0000-00a1-ffff000000000000"/>
					<Display Unit="&lt;v.1&gt;" StateOnly="true"/>
				</C>"#,
        )
        .unwrap();
        assert!(memory.contains(r#""isSecured":false,"hasControlNotes":true,"defaultIcon""#));
        assert_eq!(
            tail(&memory),
            r##""details":{"jLockable":false,"text":{"off":"Aus","on":"Ein"},"color":{"off":"#FF0054","on":"#8DFF70"}},"states":{"active":"0f000000-0000-0030-ffff000000000000"},"statistic":{"frequency":1,"outputs":[{"id":0,"name":"Anwesend","uuid":"0f000000-0000-0031-00ff000000000000","visuType":1}]}}"##
        );

        // An analog sensor: the unit becomes a printf format (`%` doubled
        // in the entry, not in the statistic), `Q` beside `AQ` is the error.
        let sensor = entry_of(
            "LoxAIRAsensor",
            r#"<C Type="LoxAIRAsensor" U="0f000000-0000-0040-ffff000000000000" Title="Feuchte" Analog="true" StatsType="11" StatsAutoDel="1">
					<Co K="AQ" U="0f000000-0000-0041-00ff000000000000"/>
					<Co K="Q" U="0f000000-0000-0042-01ff000000000000"/>
					<IoData Visu="true" Cr="0f000000-0000-00b1-ffff000000000000" Pr="0f000000-0000-00a1-ffff000000000000" St="4"/>
					<Display Type="2" Unit="&lt;v&gt;%" StateOnly="true"/>
				</C>"#,
        )
        .unwrap();
        assert_eq!(
            tail(&sensor),
            r#""details":{"jLockable":false,"format":"%.0f%%"},"states":{"value":"0f000000-0000-0040-ffff000000000000","error":"0f000000-0000-0042-01ff000000000000"},"statistic":{"frequency":11,"outputs":[{"id":0,"name":"Feuchte","format":"%.0f%","uuid":"0f000000-0000-0041-00ff000000000000","visuType":0}]}}"#
        );
        // Display texts of its own, or a statistics type not seen: no entry.
        assert!(entry_of("LoxAIRAsensor", r#"<C Type="LoxAIRAsensor" U="0f000000-0000-0040-ffff000000000000" Title="F" Analog="true"><Co K="AQ" U="0f000000-0000-0041-00ff000000000000"/><IoData Visu="true" Cr="0f000000-0000-00b1-ffff000000000000" Pr="0f000000-0000-00a1-ffff000000000000"/><Display Unit="&lt;v&gt;" StateOnly="true" TextOn="x"/></C>"#).is_none());
        assert!(entry_of("Memory", r#"<C Type="Memory" U="0f000000-0000-0030-ffff000000000000" Title="A" StatsType="3"><Co K="Input" U="0f000000-0000-0031-00ff000000000000"/><IoData Visu="true" Cr="0f000000-0000-00b1-ffff000000000000" Pr="0f000000-0000-00a1-ffff000000000000"/><Display StateOnly="true"/></C>"#).is_none());

        let pulse = entry_of(
            "PulseAt",
            r#"<C Type="PulseAt" U="0f000000-0000-0050-ffff000000000000" Title="Impuls" SpStates="0f000000-0000-0056-ffff000000000000,0f000000-0000-0057-ffff000000000000,0f000000-0000-0058-ffff000000000000" Typ="0">
					<Co K="InputDisable" U="0f000000-0000-0051-00ff000000000000"/>
					<Co K="Time" Def="1" U="0f000000-0000-0053-02ff000000000000"/>
					<Co K="Q" U="0f000000-0000-0054-03ff000000000000"/>
					<IoData Visu="true" Cr="0f000000-0000-00b1-ffff000000000000" Pr="0f000000-0000-00a1-ffff000000000000"/>
				</C>"#,
        )
        .unwrap();
        assert_eq!(
            tail(&pulse),
            r#""details":{"jLockable":true,"pulseDurationLocked":false},"states":{"jLocked":"8f000000-0000-0051-00ff000000000000","isActive":"0f000000-0000-0054-03ff000000000000","pulseDuration":"0f000000-0000-0053-02ff000000000000","startTime":"0f000000-0000-0056-ffff000000000000","oneTimePulseDate":"0f000000-0000-0057-ffff000000000000","type":"0f000000-0000-0058-ffff000000000000"}}"#
        );

        let timer = entry_of(
            "DayTimer",
            r#"<C Type="DayTimer" U="0f000000-0000-0060-ffff000000000000" Title="Schaltuhr" On="" Off="" SpStates="0f000000-0000-0068-ffff000000000000,0f000000-0000-0069-ffff000000000000">
					<Co K="Reset" U="0f000000-0000-0061-01ff000000000000"/>
					<Co K="AQ" U="0f000000-0000-0062-07ff000000000000"/>
					<Co K="AQm" U="0f000000-0000-0063-0aff000000000000"/>
					<Co K="AQmt" U="0f000000-0000-0064-0bff000000000000"/>
					<IoData Visu="true" Cr="0f000000-0000-00b1-ffff000000000000" Pr="0f000000-0000-00a1-ffff000000000000"/>
				</C>"#,
        )
        .unwrap();
        assert!(timer.contains(r#""type":"Daytimer""#));
        assert_eq!(
            tail(&timer),
            r#""details":{"jLockable":true,"analog":false,"text":{"on":"Aktiv","off":"Inaktiv"}},"states":{"jLocked":"8f000000-0000-0061-01ff000000000000","mode":"0f000000-0000-0063-0aff000000000000","override":"0f000000-0000-0064-0bff000000000000","value":"0f000000-0000-0062-07ff000000000000","needsActivation":"0f000000-0000-0068-ffff000000000000","modeList":"0f000000-0000-0069-ffff000000000000","entriesAndDefaultValue":"0f000000-0000-0060-ffff000000000000","resetActive":"0f000000-0000-0061-01ff000000000000"}}"#
        );
    }

    #[test]
    fn units_become_printf_formats() {
        assert_eq!(
            value_format("<v.1> °C"),
            Some(("%.1f °C".into(), "%.1f °C".into()))
        );
        assert_eq!(
            value_format("<v>%"),
            Some(("%.0f%%".into(), "%.0f%".into()))
        );
        assert_eq!(
            value_format("<v>ppm"),
            Some(("%.0fppm".into(), "%.0fppm".into()))
        );
        assert_eq!(value_format("<v.12>"), None);
        assert_eq!(value_format("<v.1> <v.2>"), None);
        assert_eq!(value_format("kein Wert"), None);
    }

    #[test]
    fn renames_and_moves_edit_the_entry_in_place() {
        let cat = catalogue("");
        let u = update(
            &cat,
            &doc("Licht", ROOM, ""),
            &doc("Deckenlicht", ROOM2, ""),
            NOW,
        )
        .unwrap();
        assert_eq!(
            whats(&u).0,
            [
                "renamed \"Licht\" → \"Deckenlicht\"",
                "room \"Küche\" → \"Bad\"",
            ]
        );
        let want = cat
            .replace("\"name\":\"Licht\"", "\"name\":\"Deckenlicht\"")
            .replace(
                &format!("\"room\":\"{ROOM}\",\"cat\":\"{CAT}\",\"defaultRating\":0,\"isFavorite\":false,\"isSecured\":false,\"defaultIcon\":null,\"restrictions\":0,\"details\":{{\"jLockable\":true}}"),
                &format!("\"room\":\"{ROOM2}\",\"cat\":\"{CAT}\",\"defaultRating\":0,\"isFavorite\":false,\"isSecured\":false,\"defaultIcon\":null,\"restrictions\":0,\"details\":{{\"jLockable\":true}}"),
            )
            .replace("2026-10-29 17:15:28", NOW);
        assert_eq!(u.text, want);
        // The rest stays as Loxone Config wrote it.
        assert!(u.text.contains("48.343000"));

        // A room the catalogue does not know stays for a save.
        let elsewhere = "0f000000-0000-00a9-ffff000000000000";
        let u = update(
            &cat,
            &doc("Licht", ROOM, ""),
            &doc("Licht", elsewhere, ""),
            NOW,
        )
        .unwrap();
        assert!(u.applied.is_empty());
        assert_eq!(u.text, cat);
        assert!(whats(&u).1[0].ends_with("which the catalogue does not list"));
    }

    #[test]
    fn a_new_unit_rewrites_the_formats() {
        let sensor = |unit: &str| {
            format!(
                r#"<C Type="LoxAIRAsensor" U="{NEW}" Title="Feuchte" Analog="true" StatsType="11" StatsAutoDel="1">
					<Co K="AQ" U="0f000000-0000-0041-00ff000000000000"/>
					<Co K="Q" U="0f000000-0000-0042-01ff000000000000"/>
					<IoData Visu="true" Cr="{CAT}" Pr="{ROOM}" St="4"/>
					<Display Type="2" Unit="{unit}" StateOnly="true"/>
				</C>"#
            )
        };
        let (was, now) = (
            doc("Licht", ROOM, &sensor("&lt;v&gt;%")),
            doc("Licht", ROOM, &sensor("&lt;v.1&gt; % rF")),
        );
        let el = |d: &LoxoneDoc| {
            let o = d.objects().into_iter().find(|o| o.uuid == NEW).unwrap();
            entry(d.element_at(&o.path).unwrap(), &Objects::of(d)).unwrap()
        };
        let cat = catalogue(&format!(",\"U:{NEW}\":{}", el(&was)));
        let u = update(&cat, &was, &now, NOW).unwrap();
        assert_eq!(whats(&u).0, ["unit \"<v>%\" → \"<v.1> % rF\""]);
        let want =
            catalogue(&format!(",\"U:{NEW}\":{}", el(&now))).replace("2026-10-29 17:15:28", NOW);
        assert_eq!(u.text, want);

        // A unit the app has no format for stays for a save.
        let odd = doc("Licht", ROOM, &sensor("&lt;v.1&gt; &lt;b&gt;"));
        let u = update(&cat, &was, &odd, NOW).unwrap();
        assert!(u.applied.is_empty());
        assert_eq!(whats(&u).1, ["unit \"<v>%\" → \"<v.1> <b>\""]);
    }

    #[test]
    fn a_deleted_control_goes_unless_another_entry_names_it() {
        let live = doc("Licht", ROOM, "");
        let mut compiled = live.clone();
        compiled.remove_by_uuid(BUTTON);
        let u = update(&catalogue(""), &live, &compiled, NOW).unwrap();
        assert_eq!(whats(&u).0, ["deleted"]);
        assert!(!u.text.contains(BUTTON));
        assert!(u.text.contains(&format!("\"U:{INPUT}\"")));

        // A central block listing it.
        let central = format!(
            ",\"U:0f000000-0000-0099-ffff000000000000\":{{\"name\":\"Zentral\",\
             \"uuidAction\":\"0f000000-0000-0099-ffff000000000000\",\"details\":{{\"controls\":[{{\"uuid\":\"{BUTTON}\"}}]}}}}"
        );
        let cat = catalogue(&central);
        let u = update(&cat, &live, &compiled, NOW).unwrap();
        assert_eq!(whats(&u).1, ["deleted; other entries refer to it"]);
        assert_eq!(u.text, cat);
    }

    #[test]
    fn new_controls_get_an_entry_when_loxone_config_would_list_them() {
        let button = |u: &str, io: &str| {
            format!(
                r#"<C Type="PButtonT" U="{u}" Title="Taster"><Co K="Reset" U="1f000000-0000-0030-01ff000000000000"/><Co K="Q" U="1f000000-0000-0031-05ff000000000000"/>{io}</C>"#
            )
        };
        let live = doc("Licht", ROOM, "");
        let with_room = doc(
            "Licht",
            ROOM,
            &button(
                NEW,
                &format!(r#"<IoData Visu="true" Pr="{ROOM}" Cr="{CAT}"/>"#),
            ),
        );
        let u = update(&catalogue(""), &live, &with_room, NOW).unwrap();
        assert_eq!(whats(&u).0, ["added to the app"]);
        assert!(u.text.contains(&format!(
            ",\"U:{NEW}\":{{\"name\":\"Taster\",\"type\":\"Pushbutton\",\"uuidAction\":\"{NEW}\",\
             \"room\":\"{ROOM}\",\"cat\":\"{CAT}\",\"defaultRating\":0,\"isFavorite\":false,\
             \"isSecured\":false,\"defaultIcon\":null,\"restrictions\":0,\"details\":{{\"jLockable\":true}},\
             \"states\":{{\"jLocked\":\"9f000000-0000-0030-01ff000000000000\",\
             \"active\":\"1f000000-0000-0031-05ff000000000000\"}}}}}}}}"
        )));
        assert!(u.text.contains(&format!("\"lastModified\":\"{NOW}\"")));

        // Without a room, Loxone Config does not list it: nothing to
        // write, and the push says why the app lacks it.
        let no_room = doc("Licht", ROOM, &button(NEW, r#"<IoData Visu="true"/>"#));
        let u = update(&catalogue(""), &live, &no_room, NOW).unwrap();
        assert_eq!(u.text, catalogue(""));
        assert_eq!(
            whats(&u).1,
            ["not in the app: it needs a room and a category"]
        );

        // A block type Lox Logic does not write an entry for is named.
        let memory = format!(
            r#"<C Type="Memory" U="{NEW}" Title="Merker"><IoData Visu="true" Pr="{ROOM}" Cr="{CAT}"/></C>"#
        );
        let u = update(&catalogue(""), &live, &doc("Licht", ROOM, &memory), NOW).unwrap();
        assert_eq!(
            whats(&u).1,
            ["not added: Lox Logic does not write this kind of Memory entry"]
        );
    }

    #[test]
    fn an_entry_loxl_does_not_reproduce_stops_new_ones_of_its_type() {
        // The live button's entry differs from what Lox Logic would write.
        let cat = catalogue("").replace("\"lockedOn\"", "\"lockedOff\"");
        let other = r#"<C Type="PushButton" U="0f000000-0000-0005-ffff000000000000" Title="Zwei" SpStates="a,b,0f000000-0000-0052-ffff000000000000"><Co K="Reset" U="0f000000-0000-0053-02ff000000000000"/><Co K="Q" U="0f000000-0000-0054-05ff000000000000"/>"#.to_string()
            + &format!(r#"<IoData Visu="true" Pr="{ROOM}" Cr="{CAT}"/></C>"#);
        let u = update(
            &cat,
            &doc("Licht", ROOM, ""),
            &doc("Licht", ROOM, &other),
            NOW,
        )
        .unwrap();
        assert_eq!(
            whats(&u).1,
            ["not added: Lox Logic does not reproduce the catalogue's PushButton entries"]
        );
        assert_eq!(u.text, cat);
    }

    #[test]
    fn removing_runs_of_entries_keeps_the_json_valid() {
        let cat = r#"{"o":{"a":1,"b":2,"c":3,"d":4}}"#;
        let (top, _) = object(cat, 0).unwrap();
        let (all, _) = object(cat, top[0].value.start).unwrap();
        for set in [
            vec![0],
            vec![0, 1],
            vec![1, 3],
            vec![0, 2, 3],
            vec![0, 1, 2, 3],
        ] {
            let removed: BTreeSet<usize> = set.iter().copied().collect();
            let mut spans: Vec<_> = removal_spans(&all, &removed).collect();
            spans.sort_by_key(|r| r.start);
            let mut out = String::new();
            let mut at = 0;
            for r in spans {
                out.push_str(&cat[at..r.start]);
                at = r.end;
            }
            out.push_str(&cat[at..]);
            let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{out}: {e}"));
            assert_eq!(v["o"].as_object().unwrap().len(), 4 - set.len(), "{out}");
        }
    }
}
