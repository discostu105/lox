//! DayTimer schedules from source.
//!
//! A DayTimer keeps its schedule as GUI-owned content: the
//! `Modes=` attribute lists the operating modes it has a column for, and
//! one `<Entry>` child per interval names its mode by index into that
//! list (`Ix=`, absent = 0), its span in minutes of the day (`From=`,
//! absent = 0; `To=`, `1440` = midnight), its value (`V=`) and the
//! needs-activation flag (`NeedActivate="true"`). `N=` counts the
//! entries, `UserModes=` the columns before the seven weekdays. A
//! `<timer>.schedule = <set>` claim rewrites exactly these — the rest of
//! the element stays the GUI's.
//!
//! The built-in operating modes have fixed UUIDs in every config
//! (`00000000-0000-000N-1500000000000000`, corpus-wide) and are named by
//! keyword. A `Mode` the module declares or externs is named by its slug,
//! any other mode of the config by its title: the title resolves against
//! the compiled document's `Mode` objects. Each mode has that one
//! spelling; another is refused with the line as it should read. A column's place in `Modes=` follows the
//! modes' `Mode=` numbers, ascending — how every observed timer lists
//! them (a user-defined mode, `Mode="-3"`, before Monday, `Mode="3"`).

use super::ast::{Day, Interval, Module, ScheduleDecl};
use crate::doc::LoxoneDoc;
use crate::error::{Error, Result};
use crate::xml::{Attr, Element, Node};
use std::collections::BTreeMap;

/// Day keywords → built-in operating-mode numbers (the `N` of the fixed
/// UUID; Loxone's own `Mode=` attribute is `N - 1`).
const DAYS: &[(&str, &[u8])] = &[
    ("holiday", &[1]),
    ("vacation", &[2]),
    ("dayoff", &[3]),
    ("mon", &[4]),
    ("tue", &[5]),
    ("wed", &[6]),
    ("thu", &[7]),
    ("fri", &[8]),
    ("sat", &[9]),
    ("sun", &[10]),
    ("weekdays", &[4, 5, 6, 7, 8]),
    ("weekend", &[9, 10]),
    ("daily", &[4, 5, 6, 7, 8, 9, 10]),
    ("heating", &[11]),
    ("cooling", &[12]),
];

pub(crate) fn is_day(word: &str) -> bool {
    DAYS.iter().any(|(k, _)| *k == word)
}

pub(crate) fn day_keywords() -> impl Iterator<Item = &'static str> {
    DAYS.iter().map(|(k, _)| *k)
}

/// The error for a schedule day that is no day word and no `Mode` of the
/// module (`modes`: the module's `Mode` slugs), with the nearest name.
pub(crate) fn unknown_day<'a>(word: &str, modes: impl IntoIterator<Item = &'a str>) -> String {
    let modes: Vec<&str> = modes.into_iter().collect();
    let words = day_keywords().map(|k| -> &str { k });
    let mut hint = super::validate::suggest(word, words.chain(modes.iter().copied()));
    if hint.is_empty()
        && let Some(short) = word.get(..3).filter(|s| is_day(s))
    {
        hint = format!("; did you mean `{short}`?");
    }
    format!(
        "unknown day `{word}`{hint}. Days are {}, the slug of a `Mode` the module declares, \
         or the title of another operating mode (`\"Party\"`).",
        day_keywords().collect::<Vec<_>>().join(", ")
    )
}

fn mode_uuid(n: u8) -> String {
    format!("00000000-0000-{n:04x}-1500000000000000")
}

/// The day word of a built-in operating mode, by its fixed UUID.
pub(crate) fn builtin_day(uuid: &str) -> Option<&'static str> {
    DAYS.iter()
        .find(|(_, m)| m.len() == 1 && mode_uuid(m[0]).eq_ignore_ascii_case(uuid))
        .map(|(k, _)| *k)
}

/// One column of a schedule before the document is known: a built-in
/// mode by number, a `Mode` of the module by slug, or a mode of the
/// config by title.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Column {
    Builtin(u8),
    Slug(String),
    Titled(String),
}

impl Column {
    fn name(&self) -> String {
        match self {
            Column::Builtin(n) => DAYS
                .iter()
                .find(|(_, m)| m == &[*n])
                .map_or_else(|| format!("mode {n}"), |(k, _)| (*k).to_string()),
            Column::Slug(s) => s.clone(),
            Column::Titled(t) => format!("\"{t}\""),
        }
    }
}

/// A schedule resolved to columns: column → its intervals, sorted by
/// start. A column with no intervals is listed (the timer is off all
/// day). The seven weekdays are always listed: Loxone Config fills a
/// missing weekday column with an all-day entry on load, so a weekday the schedule does not name is written as off,
/// explicitly.
pub(crate) type Program<'a> = BTreeMap<Column, Vec<&'a Interval>>;

/// Resolve the day keywords and check the intervals: inside the day,
/// non-empty, and non-overlapping per column — the GUI's schedule editor
/// cannot represent two overlapping entries in one column.
pub(crate) fn program(decl: &ScheduleDecl) -> std::result::Result<Program<'_>, String> {
    let mut out: Program = BTreeMap::new();
    for line in decl.lines() {
        for day in &line.days {
            let columns: Vec<Column> = match day {
                Day::Keyword(k) => DAYS
                    .iter()
                    .find(|(d, _)| d == k)
                    .map(|(_, m)| m.iter().map(|&n| Column::Builtin(n)).collect())
                    .ok_or_else(|| format!("unknown day `{k}`"))?,
                Day::Slug(slug) => vec![Column::Slug(slug.clone())],
                Day::Title(title) => vec![Column::Titled(title.clone())],
            };
            for c in columns {
                out.entry(c).or_default().extend(line.intervals.iter());
            }
        }
        for iv in &line.intervals {
            if iv.from >= iv.to {
                return Err(format!(
                    "`{iv}` ends before it starts. Split an interval across midnight into two (`22:00-24:00` and `00:00-06:00` on the next day)."
                ));
            }
        }
    }
    for weekday in 4..=10 {
        out.entry(Column::Builtin(weekday)).or_default();
    }
    for (column, ivs) in &mut out {
        sort_and_check(&column.name(), ivs)?;
    }
    Ok(out)
}

fn sort_and_check(day: &str, ivs: &mut [&Interval]) -> std::result::Result<(), String> {
    let hm = |m: u16| format!("{:02}:{:02}", m / 60, m % 60);
    ivs.sort_by_key(|iv| iv.from);
    for pair in ivs.windows(2) {
        if pair[1].from < pair[0].to {
            return Err(format!(
                "`{}` and `{}` overlap on {day} ({}–{})",
                pair[0],
                pair[1],
                hm(pair[1].from),
                hm(pair[0].to.min(pair[1].to))
            ));
        }
    }
    Ok(())
}

/// A mode the timer has a column for: its `Mode=` number (the column
/// order) and its UUID.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ModeColumn {
    number: i64,
    uuid: String,
}

/// The one spelling of each day of the schedule's lines, now that the
/// document tells which mode a title or slug names: a built-in mode by
/// its day word, a mode the module names by its slug (unless that slug is
/// a day word), any other by its title. A line that spells one otherwise
/// is refused with the line as it should read.
fn check_spelling(
    decl: &ScheduleDecl,
    modes: &[(String, String, i64)],
    uuid_of: &BTreeMap<String, String>,
) -> std::result::Result<(), String> {
    let slug_of = |uuid: &str| {
        uuid_of
            .iter()
            .find(|(s, u)| u.eq_ignore_ascii_case(uuid) && !is_day(s))
            .map(|(s, _)| s.clone())
    };
    for line in decl.lines() {
        let mut canonical = line.clone();
        for day in &mut canonical.days {
            let uuid = match day {
                Day::Keyword(_) => continue,
                Day::Slug(s) => uuid_of.get(s.as_str()).cloned(),
                Day::Title(t) => match modes
                    .iter()
                    .filter(|(_, x, _)| x == t)
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    [(u, _, _)] => Some(u.clone()),
                    _ => None,
                },
            };
            let Some(uuid) = uuid else { continue };
            if let Some(word) = builtin_day(&uuid) {
                *day = Day::Keyword(word.to_string());
            } else if let Day::Title(_) = day
                && let Some(s) = slug_of(&uuid)
            {
                *day = Day::Slug(s);
            }
        }
        if canonical.days != line.days {
            let (mut new, mut old) = (canonical, line.clone());
            new.comment = None;
            old.comment = None;
            return Err(format!(
                "a schedule names a built-in mode by its day word and a mode the module \
                 declares by its slug. Write `{new}` instead of `{old}`."
            ));
        }
    }
    Ok(())
}

/// Resolve each column against the document's modes, merging columns
/// that turn out to be the same mode.
fn resolve<'a>(
    program: Program<'a>,
    modes: &[(String, String, i64)],
    uuid_of: &BTreeMap<String, String>,
) -> std::result::Result<BTreeMap<ModeColumn, Vec<&'a Interval>>, String> {
    let mut out: BTreeMap<ModeColumn, Vec<&Interval>> = BTreeMap::new();
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    for (column, ivs) in program {
        let mode = match &column {
            Column::Slug(slug) => {
                let uuid = uuid_of
                    .get(slug)
                    .ok_or_else(|| format!("`{slug}` resolves to no object"))?;
                let Some((uuid, _, number)) =
                    modes.iter().find(|(u, _, _)| u.eq_ignore_ascii_case(uuid))
                else {
                    return Err(format!("`{slug}` ({uuid}) is no operating mode"));
                };
                ModeColumn {
                    number: *number,
                    uuid: uuid.clone(),
                }
            }
            Column::Builtin(n) => {
                let uuid = mode_uuid(*n);
                if *n > 10 && !modes.iter().any(|(u, _, _)| *u == uuid) {
                    return Err(format!(
                        "the config has no operating mode `{}` ({uuid})",
                        column.name()
                    ));
                }
                ModeColumn {
                    number: i64::from(*n) - 1,
                    uuid,
                }
            }
            Column::Titled(title) => {
                let found: Vec<_> = modes.iter().filter(|(_, t, _)| t == title).collect();
                match found.as_slice() {
                    [(uuid, _, number)] => ModeColumn {
                        number: *number,
                        uuid: uuid.clone(),
                    },
                    [] => {
                        let titles: Vec<String> =
                            modes.iter().map(|(_, t, _)| format!("\"{t}\"")).collect();
                        return Err(format!(
                            "the config has no operating mode titled \"{title}\". It has {}.",
                            titles.join(", ")
                        ));
                    }
                    _ => {
                        return Err(format!(
                            "{} operating modes are titled \"{title}\". Rename one in Loxone Config.",
                            found.len()
                        ));
                    }
                }
            }
        };
        names
            .entry(mode.uuid.clone())
            .or_insert_with(|| column.name());
        out.entry(mode).or_default().extend(ivs);
    }
    for (mode, ivs) in &mut out {
        sort_and_check(&names[&mode.uuid], ivs)?;
    }
    Ok(out)
}

/// Write every claimed schedule onto its DayTimer in the compiled doc.
/// `uuid_of` maps extern and managed slugs to their object UUIDs.
pub(crate) fn apply(
    doc: &mut LoxoneDoc,
    module: &Module,
    uuid_of: &BTreeMap<String, String>,
) -> Result<()> {
    if module.schedule_claims().next().is_none() {
        return Ok(());
    }
    let fail = |m: String| Err(Error::Compile(m));
    let path_of: BTreeMap<String, Vec<usize>> = doc
        .objects()
        .iter()
        .map(|o| (o.uuid.clone(), o.path.clone()))
        .collect();
    let modes = crate::doc::config_modes(doc);
    let mut claimed: BTreeMap<&str, &str> = BTreeMap::new();
    for c in module.schedule_claims() {
        let uuid = uuid_of[&c.slug].as_str();
        if let Some(other) = claimed.insert(uuid, &c.slug) {
            return fail(format!(
                "`{c}` and `{other}.schedule = …` claim the schedule of the same timer \
                 ({uuid}). Keep one."
            ));
        }
        let decl = module
            .schedules()
            .find(|s| s.name == c.schedule)
            .expect("validate admitted only declared schedules");
        let program = check_spelling(decl, &modes, uuid_of)
            .and_then(|()| program(decl))
            .and_then(|p| resolve(p, &modes, uuid_of))
            .map_err(|m| Error::Compile(format!("`{c}`: {m}")))?;
        let el = doc
            .element_at_mut(&path_of[uuid])
            .expect("object path from objects()");
        if el.attr("Type") != Some("DayTimer") {
            return fail(format!(
                "`{c}`: {uuid} is a {:?}, not a DayTimer",
                el.attr("Type")
            ));
        }
        let analog = el.attr("Analog") == Some("true");
        if analog && let Some(iv) = program.values().flatten().find(|iv| iv.value.is_none()) {
            return fail(format!(
                "`{c}`: the timer is analog, so `{iv}` needs a value (`{iv} = <value>`)"
            ));
        }
        if !analog
            && let Some(iv) = program
                .values()
                .flatten()
                .find(|iv| iv.value.as_deref().is_some_and(|v| v != "1"))
        {
            return fail(format!(
                "`{c}`: the timer is digital, so `{iv}` can only be on. Drop the value, or use an analog DayTimer."
            ));
        }
        write_schedule(el, &program);
    }
    Ok(())
}

/// Rewrite `Modes=`, `UserModes=`, `N=` and the `<Entry>` children. The
/// entries take the place of the old ones (after the connectors when
/// there were none), in the order Loxone writes them: by mode index, then
/// start. `UserModes=` counts the columns before the seven weekdays and is
/// absent when there are none (corpus-wide, and how a save rewrites it).
fn write_schedule(el: &mut Element, program: &BTreeMap<ModeColumn, Vec<&Interval>>) {
    let modes: Vec<String> = program.keys().map(|m| m.uuid.clone()).collect();
    let mut entries: Vec<Node> = Vec::new();
    for (ix, ivs) in program.values().enumerate() {
        for iv in ivs {
            let mut attrs = Vec::new();
            let mut push = |name: &str, value: String| {
                attrs.push(Attr {
                    name: name.into(),
                    value,
                })
            };
            if iv.from != 0 {
                push("From", iv.from.to_string());
            }
            push("To", iv.to.to_string());
            if ix != 0 {
                push("Ix", ix.to_string());
            }
            if iv.activate {
                push("NeedActivate", "true".into());
            }
            push("V", iv.value.clone().unwrap_or_else(|| "1".into()));
            entries.push(Node::Element(Element {
                name: "Entry".into(),
                attrs,
                children: Vec::new(),
                self_closing: true,
            }));
        }
    }
    el.set_attr("N", &entries.len().to_string());
    el.set_attr("Modes", &modes.join(","));
    el.remove_attr("UserModes");
    // The columns before Monday (`Mode="3"`): a heating or cooling column
    // after Sunday does not count (oracle session 29).
    let extra = program.keys().filter(|m| m.number < 3).count();
    if extra > 0 {
        let at = el
            .attrs
            .iter()
            .position(|a| a.name == "N")
            .expect("N was just set");
        el.attrs.insert(
            at,
            Attr {
                name: "UserModes".into(),
                value: extra.to_string(),
            },
        );
    }
    let is_entry = |n: &Node| matches!(n, Node::Element(e) if e.name == "Entry");
    let at = el.children.iter().position(is_entry).unwrap_or_else(|| {
        el.children
            .iter()
            .rposition(|n| matches!(n, Node::Element(e) if e.name == "Co"))
            .map_or(0, |i| i + 1)
    });
    el.children.retain(|n| !is_entry(n));
    let at = at.min(el.children.len());
    el.children.splice(at..at, entries);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(src: &str) -> ScheduleDecl {
        let m = Module::parse_fragment(&format!("schedule s\n{src}end\n")).unwrap();
        m.schedules().next().unwrap().clone()
    }

    #[test]
    fn day_groups_expand_and_merge_per_mode() {
        let d = decl("\tweekdays 17:00-22:00\n\tmon 06:00-07:00\n");
        let p = program(&d).unwrap();
        assert_eq!(
            p.keys().cloned().collect::<Vec<_>>(),
            (4..=10).map(Column::Builtin).collect::<Vec<_>>()
        );
        assert!(
            p[&Column::Builtin(9)].is_empty(),
            "an unnamed weekday is off"
        );
        let mon: Vec<(u16, u16)> = p[&Column::Builtin(4)]
            .iter()
            .map(|iv| (iv.from, iv.to))
            .collect();
        assert_eq!(mon, [(360, 420), (1020, 1320)]);
        assert_eq!(mode_uuid(10), "00000000-0000-000a-1500000000000000");
    }

    #[test]
    fn touching_intervals_are_fine_overlaps_are_not() {
        assert!(program(&decl("\tsun 00:00-12:00, 12:00-24:00\n")).is_ok());
        let err = Module::parse("schedule s\n\tdaily 00:00-12:00\n\tsun 11:00-13:00\nend\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("overlap on sun (11:00–12:00)"), "{err}");
    }

    fn modes() -> Vec<(String, String, i64)> {
        vec![
            (mode_uuid(4), "Montag".into(), 3),
            ("u-party".into(), "Party".into(), -2),
            ("u-tief".into(), "Tiefschlaf".into(), -1),
        ]
    }

    #[test]
    fn modes_by_title_order_by_their_number() {
        let d = decl("\t\"Tiefschlaf\" off\n\t\"Party\" 18:00-23:00\n\tmon 06:00-07:00\n");
        let none = BTreeMap::new();
        let r = resolve(program(&d).unwrap(), &modes(), &none).unwrap();
        let uuids: Vec<&str> = r.keys().map(|m| m.uuid.as_str()).take(3).collect();
        assert_eq!(uuids, ["u-party", "u-tief", mode_uuid(4).as_str()]);
    }

    #[test]
    fn each_mode_has_one_spelling() {
        let party: BTreeMap<String, String> = [
            ("party".to_string(), "u-party".to_string()),
            ("montag".to_string(), mode_uuid(4)),
        ]
        .into();
        let spelling = |src: &str| check_spelling(&decl(src), &modes(), &party);
        assert!(spelling("\tmon 06:00-07:00\n\tparty off\n\t\"Tiefschlaf\" off\n").is_ok());
        // A built-in mode by title or slug: its day word.
        let err = spelling("\tsat \"Montag\" 06:30-08:00\n").unwrap_err();
        assert!(
            err.contains("Write `sat mon 06:30-08:00` instead of `sat \"Montag\" 06:30-08:00`"),
            "{err}"
        );
        let err = spelling("\tmontag off\n").unwrap_err();
        assert!(
            err.contains("Write `mon off` instead of `montag off`"),
            "{err}"
        );
        // A mode the module names: its slug.
        let err = spelling("\t\"Party\" 18:00-23:00 # Fest\n").unwrap_err();
        assert!(
            err.contains("Write `party 18:00-23:00` instead of `\"Party\" 18:00-23:00`"),
            "{err}"
        );
        // A slug columns its mode by number.
        let d = decl("\tparty off\n");
        let r = resolve(program(&d).unwrap(), &modes(), &party).unwrap();
        assert!(r.keys().any(|m| m.uuid == "u-party" && m.number == -2));
    }

    #[test]
    fn an_unknown_title_lists_the_modes_and_heating_needs_its_object() {
        let d = decl("\t\"Partymodus\" 18:00-23:00\n");
        let err = resolve(program(&d).unwrap(), &modes(), &BTreeMap::new()).unwrap_err();
        assert!(
            err.contains("no operating mode titled \"Partymodus\". It has \"Montag\", \"Party\""),
            "{err}"
        );
        let d = decl("\theating 00:00-24:00\n");
        let err = resolve(program(&d).unwrap(), &modes(), &BTreeMap::new()).unwrap_err();
        assert!(err.contains("no operating mode `heating`"), "{err}");
    }
}
