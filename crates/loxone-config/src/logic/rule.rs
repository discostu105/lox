//! Automatic Designer rules from source.
//!
//! An `AutopilotRule` block keeps its whole rule as JSON in `APC=`
//! ([`crate::autopilot`]). A `rule <slug>` declaration owns what the rule
//! does: whether it is active, its conditions and its actions. Compiling
//! compares that with the payload *as read* ([`crate::autopilot::Rule`])
//! and writes only when the two differ — so a declaration lifted from a
//! config reproduces it byte for byte, raw spelling included.
//!
//! A write is the edit the app makes: an unchanged event or action keeps
//! its JSON object, a changed one keeps every field but the ones the
//! source states, and only a new one is built. `STA="1"` follows `active`
//! (set right after `APC=`, removed when inactive — the app's own toggle,
//! docs/loxone-format.md). A new action needs the action's `descID`, a
//! code per target type and command that ships only inside the app: it
//! is copied from an action with the same target type and command — in
//! this document, else in the corpus table [`OBSERVED_ACTIONS`] — and
//! refused when there is none. Rules with shapes outside the documented
//! ones are not written at all.
//!
//! A named rule (`rule <slug> "Name"`) is the source's own: [`create`]
//! finds it in the base (by its lock UUID, else by name) or builds it next
//! to the rules the document already has, and from there it is edited
//! like any other.

use super::ast::{
    CmpOp, ModeName, Module, RULE_BLOCK_TYPE, RuleDecl, RuleItem, RuleStmt, RuleStmtKind,
};
use crate::autopilot::{self, Action, CAPABILITIES, Capability, Event, EventCap, Kind, Rule};
use crate::doc::LoxoneDoc;
use crate::error::{Error, Result};
use crate::uuid::{LoxUuid, Minter};
use crate::xml::{Attr, Element, Node};
use serde_json::Value;
use std::collections::BTreeMap;

const DAYS: &[(&str, &[u8])] = &[
    ("mon", &[0]),
    ("tue", &[1]),
    ("wed", &[2]),
    ("thu", &[3]),
    ("fri", &[4]),
    ("sat", &[5]),
    ("sun", &[6]),
    ("weekdays", &[0, 1, 2, 3, 4]),
    ("weekend", &[5, 6]),
    ("daily", &[0, 1, 2, 3, 4, 5, 6]),
];

/// The weekday indices (`0` = Monday) a day keyword stands for.
pub(crate) fn weekdays(word: &str) -> Option<&'static [u8]> {
    DAYS.iter().find(|(k, _)| *k == word).map(|(_, d)| *d)
}

/// The shortest keywords for a set of weekdays: a whole group where the
/// set is one, single days otherwise.
fn day_words(days: &[u8]) -> Vec<String> {
    for (k, d) in &DAYS[7..] {
        if *d == days {
            return vec![(*k).to_string()];
        }
    }
    days.iter()
        .map(|&d| DAYS[d as usize].0.to_string())
        .collect()
}

/// The clock pulses a rule can wait for: keyword, code, `id`.
const SUN: &[(&str, i64, i64)] = &[
    ("sunrise", 294_001, 294),
    ("sunset", 295_001, 295),
    ("dawn", 298_001, 298),
    ("dusk", 299_001, 299),
];
const DAY: (i64, i64) = (269_001, 269);
const DAYLIGHT: (i64, i64) = (281_001, 281);
const SEVERITIES: &[(&str, u8)] = &[
    ("info", 1),
    ("warning", 2),
    ("important", 3),
    ("critical", 4),
];

/// A `then message` severity's code.
pub(crate) fn severity(word: &str) -> Option<u8> {
    SEVERITIES.iter().find(|(w, _)| *w == word).map(|(_, c)| *c)
}

/// `YYYY-MM-DD` as year, month, day.
pub(crate) fn parse_date(text: &str) -> Option<(i64, i64, i64)> {
    let mut it = text.split('-');
    let (y, m, d) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    let days_in = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => return None,
    };
    (1..=days_in).contains(&d).then_some((y, m, d))
}

/// A `then wait` duration in milliseconds, within the table's 0.1 s …
/// 86400 s.
pub(crate) fn wait_ms(text: &str) -> Option<i64> {
    let split = text.find(|c: char| c.is_ascii_alphabetic())?;
    let (n, unit) = text.split_at(split);
    let unit = super::ast::Unit::parse(unit)?;
    let secs = super::ast::scale_by_unit(n, unit)?;
    let (int, frac) = secs.split_once('.').unwrap_or((&secs, ""));
    if frac.len() > 3 {
        return None;
    }
    let ms: i64 = format!("{int}{frac:0<3}").parse().ok()?;
    (100..=86_400_000).contains(&ms).then_some(ms)
}

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day number (Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 {
            yoe + era * 400 + 1
        } else {
            yoe + era * 400
        },
        m,
        d,
    )
}

/// The time zones on Central European Time (UTC+1, summer time UTC+2
/// from the last Sunday of March to the last Sunday of October) — the
/// ones Lox Logic converts dates in.
const CET_ZONES: &[&str] = &[
    "Europe/Amsterdam",
    "Europe/Andorra",
    "Europe/Belgrade",
    "Europe/Berlin",
    "Europe/Bratislava",
    "Europe/Brussels",
    "Europe/Budapest",
    "Europe/Busingen",
    "Europe/Copenhagen",
    "Europe/Gibraltar",
    "Europe/Ljubljana",
    "Europe/Luxembourg",
    "Europe/Madrid",
    "Europe/Malta",
    "Europe/Monaco",
    "Europe/Oslo",
    "Europe/Paris",
    "Europe/Podgorica",
    "Europe/Prague",
    "Europe/Rome",
    "Europe/San_Marino",
    "Europe/Sarajevo",
    "Europe/Skopje",
    "Europe/Stockholm",
    "Europe/Tirane",
    "Europe/Vaduz",
    "Europe/Vatican",
    "Europe/Vienna",
    "Europe/Warsaw",
    "Europe/Zagreb",
    "Europe/Zurich",
];

/// The UTC offset in hours of local midnight on a day, in a CET zone.
fn cet_offset(y: i64, m: i64, d: i64) -> i64 {
    // The last Sunday of a month (1970-01-01 was a Thursday).
    let last_sunday = |m: i64| {
        let last = days_from_civil(y, m, 31);
        last - (last + 4).rem_euclid(7)
    };
    let day = days_from_civil(y, m, d);
    // Midnight is before the 01:00 UTC switch on both switching days.
    if day > last_sunday(3) && day <= last_sunday(10) {
        2
    } else {
        1
    }
}

/// The Unix time of a date's local midnight in the config's zone.
fn date_value(tz: &str, date: &str) -> Option<i64> {
    let (y, m, d) = parse_date(date)?;
    CET_ZONES.contains(&tz).then_some(())?;
    Some(days_from_civil(y, m, d) * 86_400 - cet_offset(y, m, d) * 3600)
}

/// The date whose local midnight a Unix time is, in the config's zone.
fn date_text(tz: &str, t: i64) -> Option<String> {
    CET_ZONES.contains(&tz).then_some(())?;
    // The day of the local time, then check it is that day's midnight.
    let (y, m, d) = civil_from_days((t + 7200).div_euclid(86_400));
    let text = format!("{y:04}-{m:02}-{d:02}");
    (date_value(tz, &text) == Some(t)).then_some(text)
}

/// A `then wait` duration as source text: whole seconds, else ms.
fn wait_text(ms: i64) -> String {
    if ms % 1000 == 0 {
        format!("{}s", ms / 1000)
    } else {
        format!("{ms}ms")
    }
}

/// The config's time zone (`Timezone=` on the Miniserver).
fn timezone(el: &Element) -> Option<String> {
    if let Some(tz) = el.attr("Timezone") {
        return Some(tz.to_string());
    }
    el.child_elements().find_map(timezone)
}

/// The connectors of an `AutopilotRule`, in index order (10/10 corpus
/// rules).
const RULE_PORTS: [&str; 7] = ["InputTrigger", "Reset", "On", "run", "Q", "A", "OutputAPI"];

/// The vertical gap between two rules stacked on their page (four of five
/// gaps in a real config).
const RULE_GAP: i64 = 168;

/// The rule UUIDs of the named rules, slug → UUID, creating the ones
/// the base lacks. `pinned` is the lock's map (slug → UUID); the result is
/// its new content. A pinned UUID the base carries is the rule — the
/// source owns its name, so a differing `Title=` is rewritten. Otherwise
/// the base's one rule of that name is adopted, and with none the rule is
/// created: under the pinned UUID when there is one (a base from before
/// the deployment), else a minted one.
pub(crate) fn create(
    doc: &mut LoxoneDoc,
    module: &Module,
    uuid_of: &BTreeMap<String, String>,
    pinned: &BTreeMap<String, String>,
    minter: &mut Minter,
) -> Result<BTreeMap<String, String>> {
    // The extern rules: a named rule must not be one of them.
    let mut out: BTreeMap<String, String> = module
        .rule_decls()
        .filter(|d| d.title.is_none())
        .filter_map(|d| Some((d.slug.clone(), uuid_of.get(&d.slug)?.clone())))
        .collect();
    let externs: Vec<String> = out.keys().cloned().collect();
    for decl in module.rule_decls() {
        let Some(title) = &decl.title else { continue };
        let ctx = format!("`rule {} {title:?}`", decl.slug);
        let objects = doc.objects();
        let rules: Vec<_> = objects
            .iter()
            .filter(|o| o.block_type == RULE_BLOCK_TYPE)
            .collect();
        let pin = pinned.get(&decl.slug);
        if let Some(o) = pin.and_then(|u| objects.iter().find(|o| &o.uuid == u))
            && o.block_type != RULE_BLOCK_TYPE
        {
            return Err(Error::Compile(format!(
                "{ctx}: the lockfile maps the rule to {}, which is a `{}` in this config. Remove \
                 the lockfile entry `rules.{}` to create the rule anew",
                o.uuid, o.block_type, decl.slug
            )));
        }
        let uuid = if let Some(o) = pin.and_then(|u| rules.iter().find(|o| &o.uuid == u)) {
            let el = doc.element_at_mut(&o.path).expect("path from objects()");
            if el.attr_decoded("Title").as_deref() != Some(title.as_str()) {
                rename(&ctx, el, title)?;
            }
            o.uuid.clone()
        } else {
            let named: Vec<_> = rules
                .iter()
                .filter(|o| o.title.as_deref() == Some(title.as_str()))
                .collect();
            match named.as_slice() {
                [o] => o.uuid.clone(),
                [] => build(&ctx, doc, title, pin, minter)?,
                _ => {
                    return Err(Error::Compile(format!(
                        "{ctx}: the config has {} rules of this name. Rename all but one \
                         in the app, or address one by its extern",
                        named.len()
                    )));
                }
            }
        };
        if let Some((other, _)) = out.iter().find(|(_, u)| **u == uuid) {
            return Err(Error::Compile(format!(
                "{ctx}: `rule {other}` already declares this rule"
            )));
        }
        out.insert(decl.slug.clone(), uuid);
    }
    for slug in externs {
        out.remove(&slug);
    }
    Ok(out)
}

/// Write `title` as the rule's name: `Title=` and the payload's `name`.
fn rename(ctx: &str, el: &mut Element, title: &str) -> Result<()> {
    let raw = el.attr_decoded("APC").unwrap_or_default().into_owned();
    let mut payload = autopilot::parse(&raw).ok_or_else(|| {
        Error::Compile(format!(
            "{ctx}: the rule has no readable payload (`APC=`) to rename"
        ))
    })?;
    payload["name"] = Value::String(title.to_string());
    el.set_attr("Title", title);
    el.set_attr("APC", &autopilot::to_text(&payload));
    Ok(())
}

/// A new, inactive rule without conditions or actions (the edit that
/// follows writes them), on the page of the document's rules, below the
/// last one. Everything not stated comes from that rule: the block
/// version, size, colour and flags, the `ref=` to the `AutoPilot`
/// container. A document without a rule is refused — where its rules live
/// is the app's choice (a special page in one real config, logic pages in
/// the use-case configs).
fn build(
    ctx: &str,
    doc: &mut LoxoneDoc,
    title: &str,
    pin: Option<&String>,
    minter: &mut Minter,
) -> Result<String> {
    let objects = doc.objects();
    let Some(last) = objects
        .iter()
        .filter(|o| o.block_type == RULE_BLOCK_TYPE)
        .max_by_key(|o| o.path.clone())
    else {
        return Err(Error::Compile(format!(
            "{ctx}: the config has no Automatic Designer rule yet. Create one in the app \
             once, then compile again"
        )));
    };
    let template = doc
        .element_at(&last.path)
        .expect("path from objects()")
        .clone();
    let page_path = &last.path[..last.path.len() - 1];
    // The lowest free spot in the column of the page's rules.
    let page = doc.element_at(page_path).expect("parent of an object");
    let int = |e: &Element, k: &str| e.attr(k).and_then(|v| v.parse::<i64>().ok());
    let column: Vec<&Element> = page
        .children
        .iter()
        .filter_map(|n| match n {
            Node::Element(e) if e.attr("Type") == Some(RULE_BLOCK_TYPE) => Some(e),
            _ => None,
        })
        .collect();
    let (Some(px), Some(py), Some(px2), Some(py2)) = (
        int(&template, "Px"),
        int(&template, "Py"),
        int(&template, "Px2"),
        int(&template, "Py2"),
    ) else {
        return Err(Error::Compile(format!(
            "{ctx}: the config's rule {} has no position (`Px`/`Py`/`Px2`/`Py2`) to place \
             a new one by",
            last.uuid
        )));
    };
    let top = column
        .iter()
        .filter_map(|e| int(e, "Py2"))
        .max()
        .unwrap_or(py2)
        + RULE_GAP;
    let (Some(container), Some(version)) = (template.attr("ref"), template.attr("V")) else {
        return Err(Error::Compile(format!(
            "{ctx}: the config's rule {} has no `ref=`/`V=` to copy",
            last.uuid
        )));
    };

    let uuid = match pin {
        Some(u) => u.clone(),
        None => {
            // The ports first, the rule last, as Loxone mints them.
            minter.seed(minter.counter() + RULE_PORTS.len() as u32);
            minter.mint_object().to_string()
        }
    };
    let ports = rule_ports(&uuid).ok_or_else(|| {
        Error::Compile(format!(
            "{ctx}: the lock pins the rule to a bad UUID {uuid}"
        ))
    })?;

    let payload = serde_json::json!({
        "type": 1, "id": uuid, "active": false, "activeIfValid": false,
        "name": title, "description": "", "operator": 3, "invalid": false,
        "eventGroups": [{"operator": 1, "events": []}], "actions": [],
    });
    let mut el = Element::new("C");
    el.set_attr("Type", RULE_BLOCK_TYPE);
    el.set_attr_raw("V", version);
    el.set_attr("U", &uuid);
    el.set_attr("Title", title);
    el.set_attr("Px", &px.to_string());
    el.set_attr("Py", &top.to_string());
    el.set_attr("Px2", &px2.to_string());
    el.set_attr("Py2", &(top + py2 - py).to_string());
    for k in ["Cl", "Nio", "WF"] {
        if let Some(v) = template.attr(k) {
            el.set_attr_raw(k, v);
        }
    }
    el.set_attr("APC", &autopilot::to_text(&payload));
    el.set_attr_raw("ref", container);
    for (k, u) in RULE_PORTS.iter().zip(&ports) {
        let mut co = Element::new("Co");
        co.set_attr("K", k);
        co.set_attr("U", u);
        el.push_child(co);
    }
    let mut io = Element::new("IoData");
    io.self_closing = false;
    el.push_child(io);

    let at = *last.path.last().expect("an object has a parent") + 1;
    let page = doc.element_at_mut(page_path).expect("parent of an object");
    page.children.insert(at, Node::Element(el));
    Ok(uuid)
}

/// A rule's connector UUIDs, derived from its own: Loxone mints the seven
/// ports right before the rule (5 of 5 in a real config), each with its index
/// and the rule's machine tail.
fn rule_ports(rule: &str) -> Option<Vec<String>> {
    let u = LoxUuid::parse(rule).ok()?;
    let counter = ((u.mid as u32) << 16 | u.seq as u32).checked_sub(RULE_PORTS.len() as u32)?;
    Some(
        (0..RULE_PORTS.len() as u32)
            .map(|i| {
                let c = counter + i;
                let mut tail = [0u8; 8];
                tail[0] = i as u8;
                tail[1] = 0xff;
                tail[2..].copy_from_slice(&u.tail[2..]);
                LoxUuid {
                    time: u.time,
                    mid: (c >> 16) as u16,
                    seq: c as u16,
                    tail,
                }
                .to_string()
            })
            .collect(),
    )
}

/// What a rule declaration needs from the document: the block types, the
/// app's view of the blocks (control type and states, for conditions and
/// action codes) and the operating modes.
struct Context {
    types: BTreeMap<String, String>,
    app: BTreeMap<String, (String, BTreeMap<String, String>)>,
    /// UUID, title, `Mode=` number.
    modes: Vec<(String, String, i64)>,
    /// The block conditions of the document's rules: control (lowercase),
    /// code, state UUID — evidence for a block whose app view Lox Logic does
    /// not derive.
    seen: Vec<(String, i64, String)>,
    /// The Miniserver's time zone, for dates.
    timezone: Option<String>,
}

impl Context {
    fn of(doc: &LoxoneDoc) -> Context {
        Context {
            types: doc
                .objects()
                .iter()
                .map(|o| (o.uuid.clone(), o.block_type.clone()))
                .collect(),
            app: crate::catalogue::app_controls(doc),
            modes: crate::doc::config_modes(doc),
            seen: doc
                .objects()
                .iter()
                .filter(|o| o.block_type == RULE_BLOCK_TYPE)
                .filter_map(|o| autopilot::parse(&doc.element_at(&o.path)?.attr_decoded("APC")?))
                .flat_map(|rule| {
                    let events: Vec<Value> = rule["eventGroups"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .flat_map(|g| g["events"].as_array().cloned().unwrap_or_default())
                        .collect();
                    events
                })
                .filter_map(|e| match autopilot::interpret_event(&e) {
                    Ok(Event::State {
                        control, desc, id, ..
                    }) => Some((control.to_ascii_lowercase(), desc, id)),
                    _ => None,
                })
                .collect(),
            timezone: timezone(&doc.xml.root),
        }
    }

    fn app(&self, uuid: &str) -> Option<&(String, BTreeMap<String, String>)> {
        self.app.get(&uuid.to_ascii_lowercase())
    }

    /// The capability row of a block's app control type — by its app view,
    /// else by a condition on it in the document's rules.
    fn capability(&self, uuid: &str) -> Option<&'static Capability> {
        match self.app(uuid) {
            Some((ty, _)) => CAPABILITIES
                .iter()
                .find(|c| c.control_types.contains(&ty.as_str())),
            None => {
                let lower = uuid.to_ascii_lowercase();
                let (_, desc, _) = self.seen.iter().find(|(c, _, _)| *c == lower)?;
                CAPABILITIES
                    .iter()
                    .find(|c| c.events.iter().any(|e| e.desc == *desc))
            }
        }
    }

    /// The UUID of a block's state: by its app view, else from a
    /// condition on that state in the document's rules.
    fn state_id(&self, uuid: &str, state: &str) -> Option<String> {
        if let Some((_, states)) = self.app(uuid) {
            return states.get(state).cloned();
        }
        let lower = uuid.to_ascii_lowercase();
        self.seen
            .iter()
            .find(|(c, d, _)| {
                *c == lower && autopilot::event_cap(*d).and_then(|e| e.state) == Some(state)
            })
            .map(|(_, _, id)| id.clone())
    }

    /// A block's app control type, for messages.
    fn type_name(&self, uuid: &str) -> String {
        match (self.app(uuid), self.capability(uuid)) {
            (Some((ty, _)), _) => ty.clone(),
            (None, Some(c)) => c.control_types[0].to_string(),
            (None, None) => self.types.get(uuid).cloned().unwrap_or_default(),
        }
    }

    /// The `Mode=` number of the mode the rule line `line` names. A title
    /// of a mode the module names by a slug is refused with the line as
    /// it should read.
    fn mode(
        &self,
        ctx: &str,
        line: &RuleStmtKind,
        uuid_of: &BTreeMap<String, String>,
    ) -> Result<i64> {
        let (RuleStmtKind::Mode { mode, .. } | RuleStmtKind::ThenMode { mode, .. }) = line else {
            unreachable!("a mode line")
        };
        let found: Vec<&(String, String, i64)> = match mode {
            ModeName::Slug(s) => {
                let uuid = uuid_of
                    .get(s)
                    .ok_or_else(|| Error::Compile(format!("{ctx}: `{s}` resolves to no object")))?;
                self.modes
                    .iter()
                    .filter(|(u, _, _)| u.eq_ignore_ascii_case(uuid))
                    .collect()
            }
            ModeName::Title(t) => self.modes.iter().filter(|(_, x, _)| x == t).collect(),
        };
        match found.as_slice() {
            [(uuid, _, n)] => {
                if let ModeName::Title(_) = mode
                    && let Some((slug, _)) =
                        uuid_of.iter().find(|(_, u)| u.eq_ignore_ascii_case(uuid))
                {
                    let mut new = line.clone();
                    if let RuleStmtKind::Mode { mode, .. } | RuleStmtKind::ThenMode { mode, .. } =
                        &mut new
                    {
                        *mode = ModeName::Slug(slug.clone());
                    }
                    return Err(Error::Compile(format!(
                        "{ctx}: a rule names a mode the module declares by its slug. \
                         Write `{new}` instead of `{line}`."
                    )));
                }
                Ok(*n)
            }
            [] => {
                let titles: Vec<String> = self
                    .modes
                    .iter()
                    .map(|(_, t, _)| format!("\"{t}\""))
                    .collect();
                Err(Error::Compile(format!(
                    "{ctx}: `{mode}` is no operating mode of the config. It has {}",
                    titles.join(", ")
                )))
            }
            _ => Err(Error::Compile(format!(
                "{ctx}: {} operating modes are titled {mode}. Rename one",
                found.len()
            ))),
        }
    }

    /// The source name of a mode number: the slug `slug_of` gives its
    /// object, else its title.
    fn mode_name(&self, n: i64, slug_of: &dyn Fn(&str) -> Option<String>) -> Option<ModeName> {
        let (uuid, title, _) = self.modes.iter().find(|(_, _, m)| *m == n)?;
        Some(slug_of(uuid).map_or_else(|| ModeName::Title(title.clone()), ModeName::Slug))
    }
}

pub(crate) fn apply(
    doc: &mut LoxoneDoc,
    module: &Module,
    uuid_of: &BTreeMap<String, String>,
) -> Result<()> {
    if module.rule_decls().next().is_none() {
        return Ok(());
    }
    let cx = Context::of(doc);
    let objects = doc.objects();
    let path_of: BTreeMap<&str, &[usize]> = objects
        .iter()
        .map(|o| (o.uuid.as_str(), o.path.as_slice()))
        .collect();

    // Every block action in the document, by target type and command: the
    // templates for new actions.
    let mut templates: BTreeMap<(String, String), Value> = BTreeMap::new();
    for o in objects.iter().filter(|o| o.block_type == RULE_BLOCK_TYPE) {
        let Some(rule) = doc
            .element_at(&o.path)
            .and_then(|el| el.attr_decoded("APC"))
            .and_then(|apc| autopilot::parse(&apc))
        else {
            continue;
        };
        for a in rule["actions"].as_array().into_iter().flatten() {
            if let Ok(Action::Control { target, command }) = autopilot::interpret_action(a)
                && let Some(ty) = cx.types.get(target.as_str())
            {
                templates
                    .entry((ty.to_string(), command))
                    .or_insert_with(|| a.clone());
            }
        }
    }

    let mut writes = Vec::new();
    for decl in module.rule_decls() {
        let ctx = format!("`rule {}`", decl.slug);
        let uuid = uuid_of[&decl.slug].as_str();
        let ty = cx.types.get(uuid).map_or("", String::as_str);
        if ty != RULE_BLOCK_TYPE {
            return Err(Error::Compile(format!(
                "{ctx}: the block ({uuid}) is a `{ty}`, not an Automatic Designer rule \
                 (`{RULE_BLOCK_TYPE}`)"
            )));
        }
        let el = doc.element_at(path_of[uuid]).expect("path from objects()");
        let raw = el.attr_decoded("APC").unwrap_or_default().into_owned();
        let old = autopilot::parse(&raw).ok_or_else(|| {
            Error::Compile(format!(
                "{ctx}: the rule has no readable payload (`APC=`). Create it in the app \
                 or Loxone Config first"
            ))
        })?;
        let have = autopilot::interpret(&old).map_err(|why| {
            Error::Compile(format!(
                "{ctx}: the rule has {why}, which Lox Logic cannot write. Keep \
                 this rule in the app and remove the declaration"
            ))
        })?;
        let want = wanted(&ctx, decl, uuid_of, &cx)?;
        if have == want {
            continue;
        }
        let new = rewrite(&ctx, &old, &want, &cx, &templates)?;
        writes.push((uuid.to_string(), autopilot::to_text(&new), want.active));
    }

    for (uuid, text, active) in writes {
        let el = doc
            .element_at_mut(path_of[uuid.as_str()])
            .expect("path from objects()");
        el.set_attr("APC", &text);
        match (active, el.attr("STA").is_some()) {
            (true, false) => {
                let at = el
                    .attrs
                    .iter()
                    .position(|a| a.name == "APC")
                    .map_or(el.attrs.len(), |i| i + 1);
                el.attrs.insert(
                    at,
                    Attr {
                        name: "STA".into(),
                        value: "1".into(),
                    },
                );
            }
            (false, true) => {
                el.remove_attr("STA");
            }
            _ => {}
        }
    }
    Ok(())
}

/// Every rule's payload, by rule UUID: what [`watch`] compares.
fn payloads(doc: &LoxoneDoc) -> BTreeMap<String, Option<String>> {
    doc.objects()
        .iter()
        .filter(|o| o.block_type == RULE_BLOCK_TYPE)
        .map(|o| {
            let apc = doc
                .element_at(&o.path)
                .and_then(|el| el.attr("APC"))
                .map(str::to_string);
            (o.uuid.clone(), apc)
        })
        .collect()
}

/// Re-derives the AutoPilot container's watch list ([`watch_inputs`])
/// when the compile changed the rules: rewrote a payload, created or
/// deleted a rule. Rules as the base has them leave the list as
/// the base has it — the list is what a save derives from the rules, and
/// a compile that changes none has nothing new to derive (the corpus has
/// a config whose rules have no list at all, #224).
pub(crate) fn watch(base: &LoxoneDoc, doc: &mut LoxoneDoc) {
    if payloads(base) != payloads(doc) {
        watch_inputs(doc);
    }
}

/// The AutoPilot container's `Input` wires. Loxone Config wires every
/// source a rule watches into it (`FLG="1"`):
///
/// | Condition | Source |
/// |---|---|
/// | weekday | the `DayOfWeek` block's `Q` |
/// | state | the state |
/// | mode | the mode's `Q` |
/// | weather | the `AQ` of the `WeatherData` block of its field (`WType=`) |
/// | day of month, daylight | the `Day` or `Daylight` block's `Q` |
/// | time, date | none |
///
/// A save lists them in reverse document order. Left alone when the set
/// is right, or when a rule has a condition whose wiring is not observed
/// (sun times) or whose block the config lacks.
fn watch_inputs(doc: &mut LoxoneDoc) {
    let objects = doc.objects();
    let output_of = |key: &str, pred: &dyn Fn(&crate::xml::Element) -> bool| -> Option<String> {
        objects.iter().find_map(|o| {
            let el = doc.element_at(&o.path)?;
            pred(el).then_some(())?;
            el.child_elements()
                .find(|c| c.name == "Co" && c.attr("K") == Some(key))
                .and_then(|c| c.attr("U"))
                .map(str::to_string)
        })
    };
    let q_of = |pred: &dyn Fn(&crate::xml::Element) -> bool| output_of("Q", pred);
    let of_type = |ty: &'static str| move |el: &crate::xml::Element| el.attr("Type") == Some(ty);
    let mut sources: Vec<String> = Vec::new();
    for o in objects.iter().filter(|o| o.block_type == RULE_BLOCK_TYPE) {
        let Some(rule) = doc
            .element_at(&o.path)
            .and_then(|el| el.attr_decoded("APC"))
            .and_then(|apc| autopilot::parse(&apc))
        else {
            return;
        };
        let events = rule["eventGroups"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|g| g["events"].as_array().into_iter().flatten());
        for e in events {
            let source = match e["type"].as_i64() {
                Some(4 | 5) => continue,
                Some(2) => e["id"].as_i64().and_then(|n| {
                    output_of("AQ", &|el| {
                        el.attr("Type") == Some("WeatherData")
                            && el.attr("WType").and_then(|w| w.parse().ok()) == Some(n)
                    })
                }),
                Some(3) => match e["descID"].as_i64() {
                    Some(270_001) => q_of(&of_type("DayOfWeek")),
                    Some(269_001) => q_of(&of_type("Day")),
                    Some(281_001) => q_of(&of_type("Daylight")),
                    _ => None,
                },
                Some(0) => e["id"].as_str().map(str::to_string),
                Some(1) => e["id"].as_i64().and_then(|n| {
                    q_of(&|el| {
                        el.attr("Type") == Some("Mode")
                            && el.attr("Mode").and_then(|m| m.parse().ok()).unwrap_or(0) == n
                    })
                }),
                _ => None,
            };
            let Some(source) = source else { return };
            if !sources.contains(&source) {
                sources.push(source);
            }
        }
    }
    sources.reverse();
    let Some(container) = objects.iter().find(|o| o.block_type == "AutoPilot") else {
        return;
    };
    let path = container.path.clone();
    let Some(co) = doc.element_at_mut(&path).and_then(|el| {
        el.child_elements_mut()
            .find(|c| c.name == "Co" && c.attr("K") == Some("Input"))
    }) else {
        return;
    };
    let have: std::collections::BTreeSet<&str> = co
        .child_elements()
        .filter(|c| c.name == "In")
        .filter_map(|c| c.attr("Input"))
        .collect();
    if have == sources.iter().map(String::as_str).collect() {
        return;
    }
    co.children
        .retain(|n| !matches!(n, crate::xml::Node::Element(e) if e.name == "In"));
    for s in &sources {
        let mut input = crate::xml::Element::new("In");
        input.set_attr("Input", s);
        input.set_attr("FLG", "1");
        co.push_child(input);
    }
    super::compile::sync_nc(co);
}

/// The app's operator code of a comparison.
fn op_code(op: CmpOp) -> u8 {
    let sym = op.symbol();
    autopilot::OPERATORS
        .iter()
        .find(|(_, s)| *s == sym)
        .map(|(c, _)| *c)
        .expect("every comparison has an app operator")
}

fn op_of(code: u8) -> Option<CmpOp> {
    [
        CmpOp::Eq,
        CmpOp::Ne,
        CmpOp::Gt,
        CmpOp::Lt,
        CmpOp::Ge,
        CmpOp::Le,
    ]
    .into_iter()
    .find(|o| op_code(*o) == code)
}

/// The condition `when <target>.<state> <op> <value>` stands for: the
/// capability-table event of the block's app control type that watches
/// `state` — the digital one waiting for exactly this value when the
/// comparison is `==`, else the one analog one.
fn state_event(
    ctx: &str,
    line: &str,
    control: &str,
    state: &str,
    op: CmpOp,
    value: &str,
    cx: &Context,
) -> Result<Event> {
    let fail = |m: String| Err(Error::Compile(format!("{ctx}: `{line}`: {m}")));
    let Some(cap) = cx.capability(control) else {
        return fail(if cx.app(control).is_some() {
            format!(
                "the app offers no conditions on a `{}` (the Miniserver's table lists none)",
                cx.type_name(control)
            )
        } else {
            "Lox Logic does not know this block's states in the app. Watch a block that \
             Lox Logic's catalogue covers or that a rule of the config already watches"
                .into()
        });
    };
    let ty = cx.type_name(control);
    let value = match value {
        "on" => "1",
        "off" => "0",
        v => v,
    };
    let number: serde_json::Number = match value.parse::<f64>() {
        Ok(f) if f.is_finite() => match serde_json::from_str(value) {
            Ok(n) => n,
            Err(_) => return fail(format!("`{value}` is not a number")),
        },
        _ => return fail(format!("`{value}` is not a number")),
    };
    let candidates: Vec<&EventCap> = cap
        .events
        .iter()
        .filter(|e| e.state == Some(state))
        .collect();
    let digital = candidates.iter().find(|e| {
        e.kind == Kind::Digital
            && e.value.is_some_and(|v| v.to_string() == number.to_string())
            && op == CmpOp::Eq
    });
    let analog: Vec<&&EventCap> = candidates
        .iter()
        .filter(|e| e.kind == Kind::Analog && e.value.is_none())
        .collect();
    let cap_event = match (digital, analog.as_slice()) {
        (Some(e), _) => *e,
        (None, [e]) => **e,
        (None, []) => {
            let offered: Vec<String> = cap
                .events
                .iter()
                .filter_map(|e| {
                    let s = e.state?;
                    Some(match (e.kind, e.value) {
                        (Kind::Digital, Some(v)) => format!("`{s} == {v}`"),
                        (Kind::Analog, None) => format!("`{s} <op> <number>`"),
                        _ => return None,
                    })
                })
                .collect();
            return fail(if offered.is_empty() {
                format!("the app offers no state conditions on a `{ty}`")
            } else {
                format!("the app offers on a `{ty}`: {}", offered.join(", "))
            });
        }
        (None, _) => {
            return fail(format!(
                "the app offers {} conditions on `{state}` of a `{ty}` that Lox Logic cannot tell \
                 apart. Keep this rule in the app",
                analog.len()
            ));
        }
    };
    let Some(id) = cx.state_id(control, state) else {
        return fail(format!(
            "Lox Logic does not know the `{state}` state's UUID of this `{ty}`"
        ));
    };
    Ok(Event::State {
        control: control.to_string(),
        desc: cap_event.desc,
        id,
        op: op_code(op),
        value: number.to_string(),
    })
}

/// The rule a declaration states, its blocks as UUIDs.
fn wanted(
    ctx: &str,
    decl: &RuleDecl,
    uuid_of: &BTreeMap<String, String>,
    cx: &Context,
) -> Result<Rule> {
    let mut rule = Rule {
        active: false,
        groups: vec![Vec::new()],
        actions: Vec::new(),
    };
    let resolve = |st: &RuleStmt, slug: &str| {
        uuid_of.get(slug).cloned().ok_or_else(|| {
            Error::Compile(format!(
                "{ctx}: `{}`: `{slug}` resolves to no block",
                st.kind
            ))
        })
    };
    for st in decl.stmts() {
        let group = rule.groups.last_mut().expect("one group at least");
        match &st.kind {
            RuleStmtKind::Active(a) => rule.active = *a,
            RuleStmtKind::Or => rule.groups.push(Vec::new()),
            RuleStmtKind::Time(m) => group.push(Event::Time(*m)),
            RuleStmtKind::Weekday(words) => {
                let mut days: Vec<u8> = words
                    .iter()
                    .flat_map(|w| weekdays(w).expect("checked by validate").iter().copied())
                    .collect();
                days.sort_unstable();
                group.push(Event::Weekdays(days));
            }
            RuleStmtKind::Mode { on, .. } => group.push(Event::Mode {
                mode: cx.mode(ctx, &st.kind, uuid_of)?,
                on: *on,
            }),
            RuleStmtKind::State {
                target,
                state,
                op,
                value,
            } => {
                let control = resolve(st, target)?;
                let line = st.kind.to_string();
                group.push(state_event(ctx, &line, &control, state, *op, value, cx)?);
            }
            RuleStmtKind::Then { target, command } => rule.actions.push(Action::Control {
                target: resolve(st, target)?,
                command: command.clone(),
            }),
            RuleStmtKind::ThenMode { on, .. } => rule.actions.push(Action::Mode {
                mode: cx.mode(ctx, &st.kind, uuid_of)?,
                on: *on,
            }),
            RuleStmtKind::Weather { field, op, value } => {
                let fail = |why: String| Error::Compile(format!("{ctx}: `{}`: {why}", st.kind));
                let Some(&(id, _, desc, digital)) =
                    autopilot::WEATHER_FIELDS.iter().find(|f| f.1 == field)
                else {
                    let names: Vec<&str> = autopilot::WEATHER_FIELDS.iter().map(|f| f.1).collect();
                    return Err(fail(format!(
                        "the app offers the weather fields {}",
                        names.join(", ")
                    )));
                };
                let value = match (digital, value.as_str()) {
                    (true, "on") if *op == CmpOp::Eq => "1".to_string(),
                    (true, "off") if *op == CmpOp::Eq => "0".to_string(),
                    (true, _) => return Err(fail(format!("`{field}` is `== on` or `== off`"))),
                    (false, "on" | "off") => {
                        return Err(fail(format!("`{field}` compares with a number")));
                    }
                    (false, v) => v.to_string(),
                };
                group.push(Event::Weather {
                    field: id,
                    desc,
                    op: op_code(*op),
                    value,
                });
            }
            RuleStmtKind::Day { op, value } => {
                let day = value.parse::<i64>().ok().filter(|d| (1..=31).contains(d));
                let Some(day) = day else {
                    return Err(Error::Compile(format!(
                        "{ctx}: `{}`: a day of the month is 1 … 31",
                        st.kind
                    )));
                };
                group.push(Event::Clock {
                    desc: DAY.0,
                    id: DAY.1,
                    op: op_code(*op),
                    value: day,
                });
            }
            RuleStmtKind::Daylight(on) => group.push(Event::Clock {
                desc: DAYLIGHT.0,
                id: DAYLIGHT.1,
                op: 1,
                value: i64::from(*on),
            }),
            RuleStmtKind::Sun(w) => {
                let &(_, desc, id) = SUN
                    .iter()
                    .find(|s| s.0 == w)
                    .expect("checked by the parser");
                group.push(Event::Clock {
                    desc,
                    id,
                    op: 1,
                    value: 1,
                });
            }
            RuleStmtKind::Date(d) => {
                let tz = cx.timezone.as_deref().unwrap_or("");
                let Some(t) = date_value(tz, d) else {
                    return Err(Error::Compile(format!(
                        "{ctx}: `{}`: Lox Logic converts dates only in the Central European \
                         time zones, the config's is {}",
                        st.kind,
                        if tz.is_empty() { "not stated" } else { tz }
                    )));
                };
                group.push(Event::Date(t));
            }
            RuleStmtKind::Notify { subject, text } => rule.actions.push(Action::Notify {
                subject: subject.clone(),
                text: text.clone(),
            }),
            RuleStmtKind::Mail { to, subject, text } => rule.actions.push(Action::Mail {
                to: to.clone(),
                subject: subject.clone(),
                text: text.clone(),
            }),
            RuleStmtKind::Wait(d) => {
                rule.actions
                    .push(Action::Pause(wait_ms(d).expect("checked by the parser")));
            }
            RuleStmtKind::Message {
                severity: sev,
                subject,
                text,
                admins,
            } => {
                if subject.contains('/') || text.contains('/') {
                    return Err(Error::Compile(format!(
                        "{ctx}: `{}`: a system message's subject and text go into a command \
                         path, so they cannot contain `/`",
                        st.kind
                    )));
                }
                rule.actions.push(Action::Message {
                    severity: severity(sev).expect("checked by the parser"),
                    subject: subject.clone(),
                    text: text.clone(),
                    visibility: if *admins { 4 } else { 2 },
                });
            }
        }
    }
    Ok(rule)
}

/// The action the capability table offers a block for a command: an
/// action whose command text is the command, or whose `<valN>`
/// placeholders the command fills with numbers (`setMode/4` for
/// `setMode/<val0>`) — those numbers its `arrValues`.
fn table_action(cap: &Capability, command: &str) -> Option<Value> {
    for a in cap.actions {
        let mut values = Vec::new();
        let mut rest = command;
        let mut ok = true;
        let parts: Vec<&str> = a.command.split('<').collect();
        for (i, part) in parts.iter().enumerate() {
            let lit = if i == 0 {
                *part
            } else {
                let Some((name, lit)) = part.split_once('>') else {
                    ok = false;
                    break;
                };
                if !name.starts_with("val") {
                    ok = false;
                    break;
                }
                // The number runs up to the next literal (or the end).
                let end = if lit.is_empty() {
                    rest.len()
                } else {
                    match rest.find(lit) {
                        Some(e) => e,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                };
                let Ok(n) = rest[..end].parse::<serde_json::Number>() else {
                    ok = false;
                    break;
                };
                values.push(Value::Number(n));
                rest = &rest[end..];
                lit
            };
            match rest.strip_prefix(lit) {
                Some(r) => rest = r,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok && rest.is_empty() {
            let mut v = serde_json::json!({"type": 0, "descID": a.desc, "command": command});
            if !values.is_empty() {
                v["arrValues"] = Value::Array(values);
            }
            return Some(v);
        }
    }
    None
}

/// `old` edited into `want`: unchanged parts keep their JSON, changed
/// ones keep every field the source does not state.
fn rewrite(
    ctx: &str,
    old: &Value,
    want: &Rule,
    cx: &Context,
    templates: &BTreeMap<(String, String), Value>,
) -> Result<Value> {
    let mut new = old.clone();
    // Both flags, as the app's activation writes them.
    new["active"] = Value::Bool(want.active);
    new["activeIfValid"] = Value::Bool(want.active);

    let old_groups = old["eventGroups"].as_array().cloned().unwrap_or_default();
    let groups: Vec<Value> = want
        .groups
        .iter()
        .enumerate()
        .map(|(g, events)| {
            let mut group = old_groups
                .get(g)
                .cloned()
                .unwrap_or_else(|| serde_json::json!({"operator": 1, "events": []}));
            let old_events = group["events"].as_array().cloned().unwrap_or_default();
            let events: Vec<Value> = events
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let read = old_events.get(i).map(autopilot::interpret_event);
                    match (e, read) {
                        (_, Some(Ok(o))) if &o == e => old_events[i].clone(),
                        (Event::Time(m), Some(Ok(Event::Time(_)))) => {
                            let mut o = old_events[i].clone();
                            o["value"] = (*m).into();
                            o
                        }
                        (Event::Weekdays(d), Some(Ok(Event::Weekdays(_)))) => {
                            let mut o = old_events[i].clone();
                            match (d.as_slice(), o.get("arrValues").is_some()) {
                                // The one-day spelling stays one-day.
                                ([one], false) => o["value"] = (*one).into(),
                                _ => {
                                    o["value"] = autopilot::WEEKDAY_SENTINEL.into();
                                    o["arrValues"] = d.clone().into();
                                }
                            }
                            o
                        }
                        _ => autopilot::new_event(e),
                    }
                })
                .collect();
            group["events"] = Value::Array(events);
            group
        })
        .collect();
    new["eventGroups"] = Value::Array(groups);

    let old_actions = old["actions"].as_array().cloned().unwrap_or_default();
    let mut actions = Vec::new();
    for (i, a) in want.actions.iter().enumerate() {
        let read = old_actions.get(i).map(autopilot::interpret_action);
        if let Some(Ok(o)) = &read
            && o == a
        {
            actions.push(old_actions[i].clone());
            continue;
        }
        let action = match a {
            Action::Control { target, command } => {
                let ty = cx.types.get(target.as_str()).map_or("", String::as_str);
                let found = templates
                    .get(&(ty.to_string(), command.clone()))
                    .cloned()
                    .or_else(|| table_action(cx.capability(target)?, command));
                let Some(mut action) = found else {
                    let offered = cx.capability(target).map(|c| {
                        c.actions
                            .iter()
                            .map(|a| format!("`{}`", a.command))
                            .collect::<Vec<_>>()
                            .join(", ")
                    });
                    return Err(Error::Compile(match offered {
                        Some(list) if !list.is_empty() => format!(
                            "{ctx}: the app has no action `{command}` for this `{ty}`. It \
                             offers {list} (`<val0>` a number)"
                        ),
                        _ => format!(
                            "{ctx}: Lox Logic knows no action code (`descID`) for `{command}` on a \
                             `{ty}`. Create one such action in the app once, then the \
                             compile can copy it"
                        ),
                    }));
                };
                action["id"] = Value::String(target.clone());
                action
            }
            other => autopilot::new_action(other).expect("an action without a block"),
        };
        actions.push(action);
    }
    new["actions"] = Value::Array(actions);
    Ok(new)
}

/// A rule's payload as a `rule` declaration, with `slug_of` naming the
/// blocks. `Err` says why it cannot be one.
pub(crate) fn lift(
    rule_slug: &str,
    apc: &str,
    doc: &LoxoneDoc,
    slug_of: &dyn Fn(&str) -> Option<String>,
) -> std::result::Result<RuleDecl, String> {
    let payload = autopilot::parse(apc).ok_or("the payload is not JSON")?;
    let rule = autopilot::interpret(&payload)?;
    if rule.groups.iter().any(Vec::is_empty) || rule.actions.is_empty() {
        return Err("the rule has no condition or no action".into());
    }
    let cx = Context::of(doc);
    let stmt = |kind| {
        RuleItem::Stmt(RuleStmt {
            kind,
            comment: None,
        })
    };
    let slug = |u: &str| slug_of(u).ok_or_else(|| format!("its block {u} is not in the config"));
    let mode = |n: i64| {
        cx.mode_name(n, slug_of)
            .ok_or_else(|| format!("its operating mode {n} is not in the config"))
    };
    let mut body = vec![stmt(RuleStmtKind::Active(rule.active))];
    for (g, events) in rule.groups.iter().enumerate() {
        if g > 0 {
            body.push(stmt(RuleStmtKind::Or));
        }
        for e in events {
            body.push(stmt(match e {
                Event::Time(m) => RuleStmtKind::Time(*m),
                Event::Weekdays(d) => RuleStmtKind::Weekday(day_words(d)),
                Event::Mode { mode: n, on } => RuleStmtKind::Mode {
                    mode: mode(*n)?,
                    on: *on,
                },
                Event::State {
                    control,
                    desc,
                    op,
                    value,
                    ..
                } => {
                    let cap = autopilot::event_cap(*desc).expect("interpret checked the code");
                    let state = cap
                        .state
                        .ok_or_else(|| format!("its condition {desc} watches no named state"))?;
                    let op = op_of(*op).expect("interpret checked the operator");
                    let value = match (cap.kind, value.as_str()) {
                        (Kind::Digital, "1") => "on".to_string(),
                        (Kind::Digital, "0") => "off".to_string(),
                        (_, v) => v.to_string(),
                    };
                    RuleStmtKind::State {
                        target: slug(control)?,
                        state: state.to_string(),
                        op,
                        value,
                    }
                }
                Event::Weather {
                    field, op, value, ..
                } => {
                    let &(_, name, _, digital) = autopilot::WEATHER_FIELDS
                        .iter()
                        .find(|f| f.0 == *field)
                        .expect("interpret checked the field");
                    let value = match (digital, value.as_str()) {
                        (true, "1") => "on".to_string(),
                        (true, "0") => "off".to_string(),
                        (true, v) => return Err(format!("weather `{name}` with value {v}")),
                        (false, v) => v.to_string(),
                    };
                    RuleStmtKind::Weather {
                        field: name.to_string(),
                        op: op_of(*op).expect("interpret checked the operator"),
                        value,
                    }
                }
                Event::Clock {
                    desc, op, value, ..
                } => match (*desc, *value) {
                    (d, v) if d == DAY.0 => RuleStmtKind::Day {
                        op: op_of(*op).expect("interpret checked the operator"),
                        value: v.to_string(),
                    },
                    (d, v @ (0 | 1)) if d == DAYLIGHT.0 => RuleStmtKind::Daylight(v == 1),
                    (d, 1) if SUN.iter().any(|s| s.1 == d) => RuleStmtKind::Sun(
                        SUN.iter()
                            .find(|s| s.1 == d)
                            .expect("just found")
                            .0
                            .to_string(),
                    ),
                    (d, v) => return Err(format!("clock condition {d} with value {v}")),
                },
                Event::Date(t) => {
                    let tz = cx.timezone.as_deref().unwrap_or("");
                    RuleStmtKind::Date(date_text(tz, *t).ok_or_else(|| {
                        format!("its date {t} is no midnight Lox Logic can convert in `{tz}`")
                    })?)
                }
            }));
        }
    }
    for a in &rule.actions {
        body.push(stmt(match a {
            Action::Control { target, command } => RuleStmtKind::Then {
                target: slug(target)?,
                command: command.clone(),
            },
            Action::Mode { mode: n, on } => RuleStmtKind::ThenMode {
                mode: mode(*n)?,
                on: *on,
            },
            Action::Notify { subject, text } => RuleStmtKind::Notify {
                subject: subject.clone(),
                text: text.clone(),
            },
            Action::Mail { to, subject, text } => RuleStmtKind::Mail {
                to: to.clone(),
                subject: subject.clone(),
                text: text.clone(),
            },
            Action::Pause(ms) => RuleStmtKind::Wait(wait_text(*ms)),
            Action::Message {
                severity: sev,
                subject,
                text,
                visibility,
            } => RuleStmtKind::Message {
                severity: SEVERITIES
                    .iter()
                    .find(|s| s.1 == *sev)
                    .expect("interpret checked the severity")
                    .0
                    .to_string(),
                subject: subject.clone(),
                text: text.clone(),
                admins: *visibility == 4,
            },
        }));
    }
    Ok(RuleDecl {
        slug: rule_slug.to_string(),
        title: None,
        body,
        comment: None,
        end_comment: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_words_prefer_groups() {
        assert_eq!(day_words(&[1]), ["tue"]);
        assert_eq!(day_words(&[0, 1, 2, 3, 4]), ["weekdays"]);
        assert_eq!(day_words(&[0, 6]), ["mon", "sun"]);
        assert_eq!(weekdays("weekend"), Some(&[5u8, 6][..]));
        assert_eq!(weekdays("monday"), None);
    }

    #[test]
    fn dates_are_local_midnights() {
        // Python's zoneinfo, Europe/Vienna: the summer-time switches.
        for (d, t) in [
            ("2026-01-01", 1_767_222_000),
            ("2026-03-29", 1_774_738_800),
            ("2026-03-30", 1_774_821_600),
            ("2026-07-01", 1_782_856_800),
            ("2026-10-25", 1_792_879_200),
            ("2026-10-26", 1_792_969_200),
        ] {
            assert_eq!(date_value("Europe/Vienna", d), Some(t), "{d}");
            assert_eq!(date_text("Europe/Vienna", t).as_deref(), Some(d));
        }
        assert_eq!(date_value("America/New_York", "2026-01-01"), None);
        assert_eq!(date_text("Europe/Vienna", 1_767_222_001), None);
        assert_eq!(wait_ms("2.5s"), Some(2500));
        assert_eq!(wait_ms("2min"), Some(120_000));
        assert_eq!(wait_ms("100ms"), Some(100));
        assert_eq!(wait_ms("99ms"), None);
        assert_eq!(wait_ms("25h"), None);
    }
}
