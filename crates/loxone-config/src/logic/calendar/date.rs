//! A calendar entry's date, stated as a date: `on: 12-31` (a day every
//! year), `easter: 1` (days after Easter Sunday), `from: 2026-07-05, to:
//! 2026-09-06` (one period) or `from: 10-01, to: 04-30` (every year). It
//! compiles to the stored `Mode=` and the attributes of that kind (see
//! [`EntryDate`] and loxone-format.md, "Calendar entries"). The stored
//! attributes are refused in source with the replacement; `loxl fmt`
//! writes it and adopt lifts it.

use super::BLOCK_TYPE;
use crate::error::{Error, Result};
use crate::logic::ast::{ArgItem, Binding, BindingKind, BlockDecl, Module, Value};
use crate::logic::rule::{civil_from_days, days_from_civil, parse_date};

/// The arguments that state an entry's date.
pub(crate) const ON: &str = "on";
pub(crate) const EASTER: &str = "easter";
pub(crate) const FROM: &str = "from";
pub(crate) const TO: &str = "to";
const DATE_KEYS: &[&str] = &[ON, EASTER, FROM, TO];

/// The stored attributes of the date, in the save's order: no source.
const RAW_DATE: &[&str] = &[
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

/// Whether `key` on a block of this type states the entry's date.
pub(crate) fn is_date_key(block_type: &str, key: &str) -> bool {
    block_type == BLOCK_TYPE && DATE_KEYS.contains(&key)
}

/// Whether `key` takes a date (`on:`, `from:`, `to:`) on a block of this
/// type: the parser reads `12-31` there as a date, not a subtraction.
pub(crate) fn takes_date(block_type: &str, key: &str) -> bool {
    block_type == BLOCK_TYPE && matches!(key, ON | FROM | TO)
}

/// Whether adopt leaves out a stored attribute of an entry: one of the
/// date's, which [`lift_date`] states.
pub(crate) fn is_raw_date(block_type: &str, name: &str) -> bool {
    block_type == BLOCK_TYPE && RAW_DATE.contains(&name)
}

/// Day 0 of a period's `From=`/`To=`: 2009-01-01.
fn epoch() -> i64 {
    days_from_civil(2009, 1, 1)
}

/// The days `month` has in some year (February 29).
fn days_in(month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 29,
        _ => 0,
    }
}

/// `MM-DD`, exactly, and a day of the year.
pub(crate) fn month_day(text: &str) -> Option<(i64, i64)> {
    let (m, d) = text.split_once('-')?;
    let digits = |p: &str| p.len() == 2 && p.bytes().all(|b| b.is_ascii_digit());
    if !digits(m) || !digits(d) {
        return None;
    }
    let (m, d): (i64, i64) = (m.parse().ok()?, d.parse().ok()?);
    (1..=days_in(m)).contains(&d).then_some((m, d))
}

/// `YYYY-MM-DD`, exactly, and a day of the calendar.
pub(crate) fn is_full_date(text: &str) -> bool {
    text.len() == 10 && parse_date(text).is_some()
}

/// The four kinds of date loxone-format.md documents ("Calendar
/// entries"); no other `Mode=` occurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryDate {
    /// No `Mode=`: `Month`/`Day`, every year.
    Fixed { month: i64, day: i64 },
    /// `Mode="1"`: `EasterOffset` days after Easter Sunday.
    Easter(i64),
    /// `Mode="3"`: `From` to `To`, days since 2009-01-01.
    Period { from: i64, to: i64 },
    /// `Mode="4"`: `Month`/`Day` to `Month2`/`Day2`, every year.
    Yearly {
        month: i64,
        day: i64,
        month2: i64,
        day2: i64,
    },
}

fn md(m: i64, d: i64) -> String {
    format!("{m:02}-{d:02}")
}

fn ymd(days: i64) -> String {
    let (y, m, d) = civil_from_days(days + epoch());
    format!("{y:04}-{m:02}-{d:02}")
}

/// How a calendar entry states its date, for errors.
const USAGE: &str = "`on: 12-31` (a day every year), `easter: 1` (days after Easter Sunday), \
                     `from: 2026-07-05, to: 2026-09-06` (one period) or `from: 10-01, to: \
                     04-30` (every year)";

impl EntryDate {
    /// The source arguments that state it.
    fn args(self) -> Vec<(&'static str, Value)> {
        match self {
            EntryDate::Fixed { month, day } => vec![(ON, Value::Date(md(month, day)))],
            EntryDate::Easter(n) => vec![(EASTER, Value::Number(n.to_string()))],
            EntryDate::Period { from, to } => {
                vec![(FROM, Value::Date(ymd(from))), (TO, Value::Date(ymd(to)))]
            }
            EntryDate::Yearly {
                month,
                day,
                month2,
                day2,
            } => vec![
                (FROM, Value::Date(md(month, day))),
                (TO, Value::Date(md(month2, day2))),
            ],
        }
    }

    /// The arguments as the source writes them (`from: 10-01, to: 04-30`).
    fn text(self) -> String {
        let args: Vec<String> = self
            .args()
            .into_iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
        args.join(", ")
    }

    /// The attributes it compiles to: `Mode=` (0: none) and those its
    /// kind reads. The others stay as they are, as in a Loxone Config
    /// save: the corpus has fixed dates with an `EasterOffset` and Easter
    /// entries with a date.
    fn attrs(self) -> Vec<(&'static str, String)> {
        let n = |v: i64| v.to_string();
        match self {
            EntryDate::Fixed { month, day } => {
                vec![("Mode", n(0)), ("Month", n(month)), ("Day", n(day))]
            }
            EntryDate::Easter(off) => vec![("Mode", n(1)), ("EasterOffset", n(off))],
            EntryDate::Period { from, to } => {
                vec![("Mode", n(3)), ("From", n(from)), ("To", n(to))]
            }
            EntryDate::Yearly {
                month,
                day,
                month2,
                day2,
            } => vec![
                ("Mode", n(4)),
                ("Month", n(month)),
                ("Day", n(day)),
                ("Month2", n(month2)),
                ("Day2", n(day2)),
            ],
        }
    }

    /// The date the stored attributes give; `get` reads one, `None` when
    /// absent. An absent `Mode`, `From`, `To` or `EasterOffset` is 0, as a
    /// save leaves it out. `Err` says why there is none: an undocumented
    /// kind, a missing or impossible day, a `WeekDayInMonth` other than 1.
    fn from_attrs(get: impl Fn(&str) -> Option<String>) -> std::result::Result<Self, String> {
        let int = |name: &str, absent: Option<i64>| -> std::result::Result<i64, String> {
            match get(name) {
                Some(v) => v
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| format!("`{name}` is {v}, no whole number")),
                None => absent.ok_or_else(|| format!("it has no `{name}`")),
            }
        };
        if let Some(w) = get("WeekDayInMonth")
            && w.trim() != "1"
        {
            return Err(format!(
                "`WeekDayInMonth` is {w}, and every entry of Loxone Config's has 1"
            ));
        }
        let day_of_year = |m: &str, d: &str| -> std::result::Result<(i64, i64), String> {
            let (month, day) = (int(m, None)?, int(d, None)?);
            if (1..=days_in(month)).contains(&day) {
                Ok((month, day))
            } else {
                Err(format!(
                    "`{m}` {month} and `{d}` {day} are no day of the year"
                ))
            }
        };
        Ok(match int("Mode", Some(0))? {
            0 => {
                let (month, day) = day_of_year("Month", "Day")?;
                EntryDate::Fixed { month, day }
            }
            1 => EntryDate::Easter(int("EasterOffset", Some(0))?),
            3 => {
                let (from, to) = (int("From", Some(0))?, int("To", Some(0))?);
                if from < 0 || to < 0 {
                    return Err("a day of its period is before 2009-01-01".into());
                }
                EntryDate::Period { from, to }
            }
            4 => {
                let (month, day) = day_of_year("Month", "Day")?;
                let (month2, day2) = day_of_year("Month2", "Day2")?;
                EntryDate::Yearly {
                    month,
                    day,
                    month2,
                    day2,
                }
            }
            m => {
                return Err(format!(
                    "`Mode` {m} is no kind of date Loxone Config writes (none, 1, 3 or 4)"
                ));
            }
        })
    }
}

/// The date an entry's `on:`, `easter:` or `from:`/`to:` state, `None`
/// without one. `Err` is the message, without the block.
fn source_date(module: &Module, b: &BlockDecl) -> std::result::Result<Option<EntryDate>, String> {
    let get = |k: &str| b.params().find(|(p, _)| *p == k).map(|(_, v)| v);
    let date = |k: &str, v: &Value| match v {
        Value::Date(d) => Ok(d.clone()),
        v => Err(format!("`{k}: {v}` is no date. Write {USAGE}.")),
    };
    let period_day = |d: &str| {
        let (y, m, dd) = parse_date(d)?;
        Some(days_from_civil(y, m, dd) - epoch())
    };
    Ok(Some(match (get(ON), get(EASTER), get(FROM), get(TO)) {
        (None, None, None, None) => return Ok(None),
        (Some(v), None, None, None) => {
            let d = date(ON, v)?;
            let Some((month, day)) = month_day(&d) else {
                return Err(format!(
                    "`{ON}:` is a day every year, without the year. For that one day, write \
                     `{FROM}: {d}, {TO}: {d}` instead of `{ON}: {d}`."
                ));
            };
            EntryDate::Fixed { month, day }
        }
        (None, Some(v), None, None) => {
            let n = match v {
                Value::Number(_) | Value::Ref(_) => module
                    .resolve_value(v)
                    .ok()
                    .and_then(|n| n.parse::<i64>().ok()),
                _ => None,
            };
            let Some(n) = n else {
                return Err(format!(
                    "`{EASTER}: {v}` is no number of days. Write the days after Easter Sunday, \
                     `{EASTER}: 1` (Easter Monday), or before it, `{EASTER}: -2`."
                ));
            };
            EntryDate::Easter(n)
        }
        (None, None, Some(f), Some(t)) => {
            let (f, t) = (date(FROM, f)?, date(TO, t)?);
            match (month_day(&f), month_day(&t), period_day(&f), period_day(&t)) {
                (Some((month, day)), Some((month2, day2)), _, _) => EntryDate::Yearly {
                    month,
                    day,
                    month2,
                    day2,
                },
                (_, _, Some(from), Some(to)) => {
                    if from < 0 || to < 0 {
                        return Err(format!(
                            "`{FROM}: {f}, {TO}: {t}`: a period lies on 2009-01-01 or later"
                        ));
                    }
                    EntryDate::Period { from, to }
                }
                _ => {
                    return Err(format!(
                        "`{FROM}: {f}, {TO}: {t}`: both are dates (one period: `{FROM}: \
                         2026-07-05, {TO}: 2026-09-06`) or both days of the year (every year: \
                         `{FROM}: 10-01, {TO}: 04-30`)"
                    ));
                }
            }
        }
        (None, None, Some(_), None) | (None, None, None, Some(_)) => {
            return Err(format!(
                "a range takes both `{FROM}:` and `{TO}:`. Write {USAGE}."
            ));
        }
        _ => {
            return Err(format!(
                "an entry has one date: `{ON}:`, `{EASTER}:`, or `{FROM}:` with `{TO}:`"
            ));
        }
    }))
}

/// A calendar entry's date, checked: the source states it with `on:`,
/// `easter:` or `from:`/`to:`. The stored attributes (`Mode: 4, Month:
/// 10, …`) are refused with the replacement.
pub(crate) fn check_date(module: &Module, b: &BlockDecl) -> Result<()> {
    let fail = |m: String| Err(Error::Compile(format!("block `{}`: {m}", b.slug)));
    if let Some(x) = b
        .bindings()
        .find(|x| DATE_KEYS.contains(&x.port.as_str()) && !matches!(x.kind, BindingKind::Param(_)))
    {
        return fail(format!(
            "`{}:` takes a date, not a signal. Write {USAGE}.",
            x.port
        ));
    }
    let raw: Vec<(&str, &Value)> = b.params().filter(|(k, _)| RAW_DATE.contains(k)).collect();
    if raw.is_empty() {
        return source_date(module, b).map(|_| ()).or_else(fail);
    }
    let old: Vec<String> = raw.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    let old = old.join(", ");
    if b.params().any(|(k, _)| DATE_KEYS.contains(&k)) {
        return fail(format!(
            "a calendar entry states its date once, with `{ON}:`, `{EASTER}:` or \
             `{FROM}:`/`{TO}:`. Leave out `{old}`."
        ));
    }
    if let [("WeekDayInMonth", v)] = raw.as_slice()
        && v.to_string() == "1"
    {
        return fail(
            "leave out `WeekDayInMonth: 1`: every calendar entry has it, and a new one gets it"
                .into(),
        );
    }
    let value = |name: &str| {
        raw.iter().find(|(k, _)| *k == name).map(|(_, v)| match v {
            Value::Ref(_) => module
                .resolve_value(v)
                .map_or_else(|_| v.to_string(), |c| c.into_owned()),
            v => v.to_string(),
        })
    };
    match EntryDate::from_attrs(value) {
        Ok(date) => fail(format!(
            "a calendar entry states its date as a date. Write `{}` instead of `{old}`.",
            date.text()
        )),
        Err(why) => fail(format!(
            "a calendar entry states its date as {USAGE}, not `{old}`: {why}."
        )),
    }
}

/// The stored attributes a checked entry's date compiles to, `None`
/// without a date: the entry keeps what it has (a new one: 1 January).
pub(crate) fn date_attrs(
    module: &Module,
    b: &BlockDecl,
) -> Result<Option<Vec<(&'static str, String)>>> {
    source_date(module, b)
        .map(|d| d.map(EntryDate::attrs))
        .map_err(|m| Error::Compile(format!("block `{}`: {m}", b.slug)))
}

/// The date arguments adopt writes for an entry whose attributes `get`
/// reads.
pub(crate) fn lift_date(
    get: impl Fn(&str) -> Option<String>,
) -> std::result::Result<Vec<(&'static str, Value)>, String> {
    EntryDate::from_attrs(get).map(EntryDate::args)
}

/// `loxl fmt`: an entry's stored attributes (`Mode: 4, Month: 10, …`)
/// become its date (`from: 10-01, to: 04-30`) where their numbers give
/// one, and `WeekDayInMonth: 1` leaves.
pub(crate) fn rewrite_raw_date(b: &mut BlockDecl) {
    if b.block_type != BLOCK_TYPE || b.params().any(|(k, _)| DATE_KEYS.contains(&k)) {
        return;
    }
    let numbers: Option<Vec<(String, String)>> = b
        .params()
        .filter(|(k, _)| RAW_DATE.contains(k))
        .map(|(k, v)| match v {
            Value::Number(n) => Some((k.to_string(), n.clone())),
            _ => None,
        })
        .collect();
    let new = match numbers.as_deref() {
        None | Some([]) => return,
        Some([(k, n)]) if k == "WeekDayInMonth" && n == "1" => Vec::new(),
        Some(numbers) => {
            let get = |name: &str| {
                numbers
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, n)| n.clone())
            };
            match EntryDate::from_attrs(get) {
                Ok(date) => date.args(),
                Err(_) => return,
            }
        }
    };
    let mut new = Some(new);
    let mut out = Vec::with_capacity(b.args.len());
    for arg in std::mem::take(&mut b.args) {
        let ArgItem::Binding(x) = &arg else {
            out.push(arg);
            continue;
        };
        if !matches!(x.kind, BindingKind::Param(_)) || !RAW_DATE.contains(&x.port.as_str()) {
            out.push(arg);
            continue;
        }
        // The date goes where its first attribute was, with its comment.
        if let Some(new) = new.take() {
            let mut comment = x.comment.clone();
            for (port, v) in new {
                out.push(ArgItem::Binding(Binding {
                    port: port.to_string(),
                    kind: BindingKind::Param(v),
                    comment: comment.take(),
                }));
            }
        }
    }
    b.args = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_period_counts_days_since_2009() {
        // loxone-format.md: `2375` is 4 July 2015.
        assert_eq!(ymd(2375), "2015-07-04");
        assert_eq!(ymd(0), "2009-01-01");
        let p = EntryDate::from_attrs(|n| match n {
            "Mode" => Some("3".into()),
            "From" => Some("2375".into()),
            "To" => Some("2438".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(p.text(), "from: 2015-07-04, to: 2015-09-05");
    }

    #[test]
    fn a_day_of_the_year_has_two_digits() {
        assert_eq!(month_day("02-29"), Some((2, 29)));
        assert_eq!(month_day("2-28"), None);
        assert_eq!(month_day("04-31"), None);
        assert_eq!(month_day("13-01"), None);
        assert!(is_full_date("2026-07-05"));
        assert!(!is_full_date("2026-02-29"));
    }

    #[test]
    fn only_the_documented_kinds_are_dates() {
        let kind = |mode: &str| {
            EntryDate::from_attrs(|n| match n {
                "Mode" => Some(mode.into()),
                "Month" | "Day" | "Month2" | "Day2" => Some("1".into()),
                _ => None,
            })
        };
        for m in ["0", "1", "3", "4"] {
            assert!(kind(m).is_ok(), "{m}");
        }
        for m in ["2", "5", "-1"] {
            assert!(kind(m).unwrap_err().contains("no kind of date"), "{m}");
        }
    }
}
