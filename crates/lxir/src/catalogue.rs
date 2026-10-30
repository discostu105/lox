//! `LoxAPP3.json`, the app catalogue: the structure the app reads. Loxone
//! Config writes it into the program ZIP next to `sps0.LoxCC` when it saves
//! to the Miniserver; the Miniserver serves it as `/data/LoxApp3.json` as it
//! is and does not rebuild it (#155).
//!
//! [`update`] carries what a compile changes about the app's controls into
//! the catalogue of the config it compiled against: the name, room,
//! category and favourite flag of the controls it lists, the controls the
//! compile deletes or hides, and new entries for the block types whose
//! entries are known (push buttons, switches, digital virtual inputs). The
//! rules and their evidence are in docs/loxone-format.md (*LoxAPP3.json*).
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

    // New entries, for the block types whose entries lxir reproduces.
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
        if !eligible(&now, el) || was.shown(uuid).is_some_and(|w| eligible(&was, w)) {
            continue;
        }
        let ty = el.attr("Type").unwrap_or("");
        let name = view(el).name;
        let change = |what: &str| Change {
            uuid: uuid.clone(),
            name: name.clone(),
            what: what.to_string(),
        };
        match entry(el) {
            Some(json) if trusted.contains(ty) => {
                added.push(',');
                added.push_str(&json_str(&format!("U:{}", el.attr("U").unwrap_or(uuid))));
                added.push(':');
                added.push_str(&json);
                applied.push(change("added to the app"));
            }
            Some(_) => left.push(change(&format!(
                "not added: lxir does not reproduce the catalogue's {ty} entries"
            ))),
            None if el.child_elements().any(|c| c.attr("IconV").is_some()) => {
                left.push(change("not added: its icon"))
            }
            None => left.push(change(&format!(
                "not added: lxir does not write {ty} entries"
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
        let Some(json) = entry(el) else {
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

/// The block types [`entry`] knows.
pub const KNOWN: [&str; 14] = [
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
];

/// The members of an entry the self-check compares (the rest come from
/// [`view`]).
const SELF_CHECKED: [&str; 4] = ["type", "details", "states", "statistic"];

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
fn entry(el: &Element) -> Option<String> {
    let io = el.child_elements().find(|c| c.name == "IoData")?;
    if io.attr("IconV").is_some() {
        return None;
    }
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
        _ => return None,
    };
    let v = view(el);
    let statistic = match el.attr("StatsType") {
        None => String::new(),
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
    Some(format!(
        r#"{{"name":{},"type":"{ty}","uuidAction":{},"room":{},"cat":{},"defaultRating":0,"isFavorite":{},"isSecured":false,{notes}"defaultIcon":null,"restrictions":0,"details":{details},"states":{states}{statistic}}}"#,
        json_str(&v.name),
        json_str(uuid),
        json_str(v.room.as_deref()?),
        json_str(v.cat.as_deref()?),
        v.favorite,
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
}

impl<'d> Objects<'d> {
    fn of(doc: &'d LoxoneDoc) -> Self {
        let mut out = Objects {
            shown_in_order: Vec::new(),
            shown: BTreeMap::new(),
            all: BTreeSet::new(),
            referenced: BTreeSet::new(),
        };
        let mut seen = BTreeSet::new();
        for o in doc.objects() {
            let uuid = o.uuid.to_ascii_lowercase();
            out.all.insert(uuid.clone());
            let Some(el) = doc.element_at(&o.path) else {
                continue;
            };
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
        entry(d.element_at(&o.path)?)
    }

    /// The tail of an entry after `restrictions` (what the type decides).
    fn tail(e: &str) -> &str {
        &e[e.find("\"details\"").unwrap()..]
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
            entry(d.element_at(&o.path).unwrap()).unwrap()
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

        // Without a room, Loxone Config does not list it: nothing to do.
        let no_room = doc("Licht", ROOM, &button(NEW, r#"<IoData Visu="true"/>"#));
        let u = update(&catalogue(""), &live, &no_room, NOW).unwrap();
        assert_eq!((u.applied.len(), u.left.len()), (0, 0));

        // A block type lxir does not write an entry for is named.
        let memory = format!(
            r#"<C Type="Memory" U="{NEW}" Title="Merker"><IoData Visu="true" Pr="{ROOM}" Cr="{CAT}"/></C>"#
        );
        let u = update(&catalogue(""), &live, &doc("Licht", ROOM, &memory), NOW).unwrap();
        assert_eq!(
            whats(&u).1,
            ["not added: lxir does not write Memory entries"]
        );
    }

    #[test]
    fn an_entry_lxir_does_not_reproduce_stops_new_ones_of_its_type() {
        // The live button's entry differs from what lxir would write.
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
            ["not added: lxir does not reproduce the catalogue's PushButton entries"]
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
