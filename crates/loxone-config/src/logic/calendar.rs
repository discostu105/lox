//! The operating mode a calendar entry switches on: `switches:` names it
//! the one way a mode is named elsewhere (a built-in mode by its day word,
//! a `Mode` the module declares or externs by its slug, any other by its
//! quoted title), and it compiles to the entry's `CalType=`, the mode's
//! `Mode=` number (absent for 0, the holiday mode; see loxone-format.md,
//! "Calendar entries").
//!
//! The raw number (`CalType: 10`) is refused with the replacement. A
//! built-in mode's number is fixed, so `loxl check` names it; a user
//! mode's number is only in the config, so `loxl compile` does.
//!
//! The entry's date is a date too ([`date`]).

mod date;

pub(crate) use date::{
    check_date, date_attrs, is_date_key, is_full_date, is_raw_date, lift_date, month_day,
    rewrite_raw_date, takes_date,
};

use super::ast::{Module, Value};
use super::schedule::{builtin_day, builtin_number, is_day};
use crate::doc::LoxoneDoc;
use crate::error::{Error, Result};
use crate::xml::Attr;
use std::collections::BTreeMap;

/// The argument that names the mode.
pub(crate) const SWITCHES: &str = "switches";
/// The stored attribute it compiles to.
pub(crate) const CAL_TYPE: &str = "CalType";
pub(crate) const BLOCK_TYPE: &str = "CalendarEntry";

/// The keys a calendar entry's source takes, as the language server
/// completes them.
pub(crate) const SOURCE_KEYS: &[(&str, &str)] = &[
    (
        SWITCHES,
        "the operating mode it switches on: `heating`, a slug, a title",
    ),
    (date::ON, "a day every year: `12-31`"),
    (date::EASTER, "days after Easter Sunday: `1`, `-2`"),
    (
        date::FROM,
        "start of a period (`2026-07-04`) or of a range every year (`10-01`)",
    ),
    (
        date::TO,
        "end of a period (`2026-09-05`) or of a range every year (`04-30`)",
    ),
];

/// Whether `key` on a block of this type is `switches:`.
pub(crate) fn is_switches(block_type: &str, key: &str) -> bool {
    block_type == BLOCK_TYPE && key == SWITCHES
}

/// The attributes a save writes after `CalType=`: a `CalType=` added to an
/// entry goes before the first of them.
const AFTER_CAL_TYPE: &[&str] = &[
    "Mode",
    "Month",
    "Day",
    "Month2",
    "Day2",
    "From",
    "To",
    "EasterOffset",
    "WeekDayInMonth",
];

/// The refusal of `CalType: <number>`, with the replacement `new`.
pub(crate) fn number_refused(slug: &str, number: &str, new: &Value) -> String {
    format!(
        "block `{slug}`: a calendar entry names its operating mode with `{SWITCHES}:`. \
         Write `{SWITCHES}: {new}` instead of `{CAL_TYPE}: {number}`."
    )
}

/// The `switches:` value of a built-in mode's `Mode=` number.
pub(crate) fn builtin_value(number: i64) -> Option<Value> {
    let n = u8::try_from(number.checked_add(1)?).ok()?;
    builtin_day(&super::schedule::mode_uuid(n)).map(|d| Value::Ref(d.to_string()))
}

/// How `switches:` names the mode `uuid` titled `title`: its day word if
/// it is built in, else the slug `uuid_of` gives it (one that is no day
/// word), else its title.
fn spelling(uuid: &str, title: &str, uuid_of: &BTreeMap<String, String>) -> Value {
    if let Some(day) = builtin_day(uuid) {
        return Value::Ref(day.to_string());
    }
    uuid_of
        .iter()
        .find(|(s, u)| u.eq_ignore_ascii_case(uuid) && !is_day(s))
        .map_or_else(
            || Value::Str(title.to_string()),
            |(s, _)| Value::Ref(s.clone()),
        )
}

/// The `switches:` value for an entry whose `CalType=` is `number`, as
/// adopt writes it. `slug_of` gives the module's slug for an object UUID.
/// None when the config has no such mode: a user mode deleted since, or
/// the heating or cooling mode of an older config (the other built-in
/// modes are in every one, as in [`apply`]).
pub(crate) fn lift(
    doc: &LoxoneDoc,
    number: i64,
    slug_of: &BTreeMap<String, String>,
) -> Option<Value> {
    let modes = crate::doc::config_modes(doc);
    let Some((uuid, title, _)) = modes.iter().find(|(_, _, n)| *n == number) else {
        return builtin_value(number).filter(|_| number < 10);
    };
    let uuid_of: BTreeMap<String, String> = slug_of
        .iter()
        .map(|(u, s)| (s.clone(), u.clone()))
        .collect();
    Some(spelling(uuid, title, &uuid_of))
}

/// The comment adopt writes in place of `switches:` on an entry whose
/// mode `number` the config no longer has (see [`lift`]).
pub(crate) fn gone_mode_comment(number: i64) -> String {
    format!(
        " Its operating mode ({CAL_TYPE} {number}) is not in the config. Without `{SWITCHES}:` \
         it stays; name a mode to switch that one."
    )
}

/// `CalType: <number>` on an entry, against the base: refused with the
/// mode's `switches:` spelling. `uuid_of` maps the module's slugs to
/// object UUIDs (the lock's managed objects, the resolved externs).
pub(crate) fn refuse_numbers(
    base: &LoxoneDoc,
    module: &Module,
    uuid_of: &BTreeMap<String, String>,
) -> Result<()> {
    let modes = crate::doc::config_modes(base);
    for b in module.blocks().filter(|b| b.block_type == BLOCK_TYPE) {
        let Some((_, v)) = b.params().find(|(k, _)| *k == CAL_TYPE) else {
            continue;
        };
        let raw = v.to_string();
        let new = match v {
            Value::Number(n) => n.parse::<i64>().ok().and_then(|n| {
                modes
                    .iter()
                    .find(|(_, _, m)| *m == n)
                    .map(|(u, t, _)| spelling(u, t, uuid_of))
                    .or_else(|| builtin_value(n))
            }),
            _ => None,
        };
        return Err(Error::Compile(match new {
            Some(new) => number_refused(&b.slug, &raw, &new),
            None => format!(
                "block `{}`: `{CAL_TYPE}: {raw}` names no operating mode of the config. \
                 Name the mode with `{SWITCHES}:`: a day word (`heating`), the slug of a \
                 `Mode`, or a mode's quoted title.",
                b.slug
            ),
        }));
    }
    Ok(())
}

/// Write `CalType=` on every managed entry that states `switches:`, once
/// the compiled document holds every mode (a fresh `Mode` has its number
/// then). An entry without `switches:` keeps what it has (a fresh one:
/// none, the holiday mode). Another spelling of the mode than its one is
/// refused with the replacement.
pub(crate) fn apply(
    doc: &mut LoxoneDoc,
    module: &Module,
    uuid_of: &BTreeMap<String, String>,
) -> Result<()> {
    let modes = crate::doc::config_modes(doc);
    let mut writes: Vec<(String, i64)> = Vec::new();
    for b in module.blocks().filter(|b| b.block_type == BLOCK_TYPE) {
        let Some((_, v)) = b.params().find(|(k, _)| *k == SWITCHES) else {
            continue;
        };
        let ctx = format!("block `{}`: `{SWITCHES}: {v}`", b.slug);
        let fail = |m: String| Err(Error::Compile(format!("{ctx}: {m}")));
        let (uuid, number) = match v {
            Value::Ref(w) if is_day(w) => {
                let Some(n) = builtin_number(w) else {
                    return fail("names several modes; a calendar entry switches one on".into());
                };
                let uuid = super::schedule::mode_uuid(u8::try_from(n + 1).expect("built in"));
                // The heating and cooling modes are missing from older
                // configs; the others are in every one (as in a schedule).
                if n >= 10 && !modes.iter().any(|(u, _, _)| u.eq_ignore_ascii_case(&uuid)) {
                    return fail(format!("the config has no operating mode `{w}` ({uuid})"));
                }
                (uuid, n)
            }
            Value::Ref(slug) => {
                let Some(uuid) = uuid_of.get(slug) else {
                    return fail(format!("`{slug}` resolves to no object"));
                };
                let Some((u, _, n)) = modes.iter().find(|(u, _, _)| u.eq_ignore_ascii_case(uuid))
                else {
                    return fail(format!("`{slug}` ({uuid}) is no operating mode"));
                };
                (u.clone(), *n)
            }
            Value::Str(title) => {
                let found: Vec<_> = modes.iter().filter(|(_, t, _)| t == title).collect();
                match found.as_slice() {
                    [(u, _, n)] => (u.clone(), *n),
                    [] => {
                        let titles: Vec<String> =
                            modes.iter().map(|(_, t, _)| format!("\"{t}\"")).collect();
                        return fail(format!(
                            "the config has no operating mode titled \"{title}\". It has {}.",
                            titles.join(", ")
                        ));
                    }
                    _ => {
                        return fail(format!(
                            "{} operating modes are titled \"{title}\". Rename one in Loxone \
                             Config.",
                            found.len()
                        ));
                    }
                }
            }
            _ => return fail("names no operating mode".into()),
        };
        let title = modes
            .iter()
            .find(|(u, _, _)| *u == uuid)
            .map_or("", |(_, t, _)| t.as_str());
        let canonical = spelling(&uuid, title, uuid_of);
        if canonical != *v {
            return Err(Error::Compile(format!(
                "block `{}`: a calendar entry names a built-in mode by its day word and a mode \
                 the module declares by its slug. Write `{SWITCHES}: {canonical}` instead of \
                 `{SWITCHES}: {v}`.",
                b.slug
            )));
        }
        let Some(entry) = uuid_of.get(&b.slug) else {
            continue;
        };
        writes.push((entry.clone(), number));
    }
    if writes.is_empty() {
        return Ok(());
    }
    let index = doc.index();
    for (entry, number) in writes {
        let Some(path) = index.by_uuid.get(&entry) else {
            continue;
        };
        let el = doc.element_at_mut(path).expect("indexed path");
        if number == 0 {
            el.remove_attr(CAL_TYPE);
        } else if el.attr(CAL_TYPE).is_some() {
            el.set_attr(CAL_TYPE, &number.to_string());
        } else {
            let pos = el
                .attrs
                .iter()
                .position(|a| AFTER_CAL_TYPE.contains(&a.name.as_str()))
                .unwrap_or(el.attrs.len());
            el.attrs.insert(
                pos,
                Attr {
                    name: CAL_TYPE.to_string(),
                    value: number.to_string(),
                },
            );
        }
    }
    Ok(())
}

/// Put the date attributes an adopted entry gains (its kind changed:
/// `From=`/`To=` for a period) where a save writes them: among the
/// entry's others, in [`AFTER_CAL_TYPE`] order. The compile adds a new
/// attribute in front of the base's; the attributes the base has keep
/// their places.
pub(crate) fn place_new_attrs(
    doc: &mut LoxoneDoc,
    base: &LoxoneDoc,
    module: &Module,
    uuid_of: &BTreeMap<String, String>,
) {
    let rank = |name: &str| AFTER_CAL_TYPE.iter().position(|n| *n == name);
    let (index, base_index) = (doc.index(), base.index());
    for b in module.blocks().filter(|b| b.block_type == BLOCK_TYPE) {
        let Some(uuid) = uuid_of.get(&b.slug) else {
            continue;
        };
        let (Some(path), Some(base_path)) = (index.by_uuid.get(uuid), base_index.by_uuid.get(uuid))
        else {
            continue;
        };
        let had: Vec<String> = base
            .element_at(base_path)
            .map(|e| e.attrs.iter().map(|a| a.name.clone()).collect())
            .unwrap_or_default();
        let el = doc.element_at_mut(path).expect("indexed path");
        let new: Vec<Attr> = AFTER_CAL_TYPE
            .iter()
            .filter(|n| !had.iter().any(|h| h == *n))
            .filter_map(|n| {
                let at = el.attrs.iter().position(|a| a.name == *n)?;
                Some(el.attrs.remove(at))
            })
            .collect();
        for attr in new {
            let mine = rank(&attr.name);
            let pos = el
                .attrs
                .iter()
                .position(|a| rank(&a.name) > mine)
                .or_else(|| {
                    el.attrs
                        .iter()
                        .rposition(|a| rank(&a.name).is_some() || a.name == CAL_TYPE)
                        .map(|p| p + 1)
                })
                .unwrap_or(el.attrs.len());
            el.attrs.insert(pos, attr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_numbers_are_day_words() {
        let word = |n| builtin_value(n).map(|v| v.to_string());
        assert_eq!(word(0).as_deref(), Some("holiday"));
        assert_eq!(word(1).as_deref(), Some("vacation"));
        assert_eq!(word(3).as_deref(), Some("mon"));
        assert_eq!(word(10).as_deref(), Some("heating"));
        assert_eq!(word(11).as_deref(), Some("cooling"));
        assert_eq!(word(12), None);
        assert_eq!(word(-1), None);
    }
}
