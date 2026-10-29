//! DayTimer schedules from source (D53).
//!
//! A DayTimer keeps its schedule as GUI-owned content (D19): the
//! `Modes=` attribute lists the operating modes it has a column for, and
//! one `<Entry>` child per interval names its mode by index into that
//! list (`Ix=`, absent = 0), its span in minutes of the day (`From=`,
//! absent = 0; `To=`, `1440` = midnight), its value (`V=`) and the
//! needs-activation flag (`NeedActivate="true"`). `N=` counts the
//! entries, `UserModes=` the modes beyond the seven weekdays. A
//! `<timer>.schedule = <set>` claim rewrites exactly these — the rest of
//! the element stays the GUI's.
//!
//! Only the ten built-in operating modes are addressable: their UUIDs
//! are fixed in every config (`00000000-0000-000N-1500000000000000`,
//! corpus-wide). User-defined modes are objects of each config, with
//! their own UUIDs; resolving them by title is not built yet, so they
//! are refused rather than guessed.

use super::ast::{Interval, Module, ScheduleDecl};
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
];

pub(crate) fn is_day(word: &str) -> bool {
    DAYS.iter().any(|(k, _)| *k == word)
}

pub(crate) fn day_keywords() -> impl Iterator<Item = &'static str> {
    DAYS.iter().map(|(k, _)| *k)
}

fn mode_uuid(n: u8) -> String {
    format!("00000000-0000-{n:04x}-1500000000000000")
}

/// A schedule resolved to modes: mode number → its intervals, sorted by
/// start. A mode with no intervals is listed (the timer is off all day).
/// The seven weekdays are always listed: Loxone Config fills a missing
/// weekday column with an all-day entry on load (oracle session 18), so
/// a weekday the schedule does not name is written as off, explicitly.
pub(crate) type Program<'a> = BTreeMap<u8, Vec<&'a Interval>>;

/// Resolve the day keywords and check the intervals: inside the day,
/// non-empty, and non-overlapping per mode — the GUI's schedule editor
/// cannot represent two overlapping entries in one column.
pub(crate) fn program(decl: &ScheduleDecl) -> std::result::Result<Program<'_>, String> {
    let hm = |m: u16| format!("{:02}:{:02}", m / 60, m % 60);
    let mut out: Program = BTreeMap::new();
    for line in decl.lines() {
        for day in &line.days {
            let modes = DAYS
                .iter()
                .find(|(k, _)| k == day)
                .map(|(_, m)| *m)
                .ok_or_else(|| format!("unknown day `{day}`"))?;
            for &m in modes {
                out.entry(m).or_default().extend(line.intervals.iter());
            }
        }
        for iv in &line.intervals {
            if iv.from >= iv.to {
                return Err(format!(
                    "`{iv}` ends before it starts — split an interval across midnight \
                     into two (`22:00-24:00` and `00:00-06:00` on the next day)"
                ));
            }
        }
    }
    for weekday in 4..=10 {
        out.entry(weekday).or_default();
    }
    for (mode, ivs) in &mut out {
        ivs.sort_by_key(|iv| iv.from);
        for pair in ivs.windows(2) {
            if pair[1].from < pair[0].to {
                let day = DAYS
                    .iter()
                    .find(|(_, m)| m == &[*mode])
                    .map_or("?", |(k, _)| *k);
                return Err(format!(
                    "`{}` and `{}` overlap on {day} ({}–{})",
                    pair[0],
                    pair[1],
                    hm(pair[1].from),
                    hm(pair[0].to.min(pair[1].to))
                ));
            }
        }
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
    let mut claimed: BTreeMap<&str, &str> = BTreeMap::new();
    for c in module.schedule_claims() {
        let uuid = uuid_of[&c.slug].as_str();
        if let Some(other) = claimed.insert(uuid, &c.slug) {
            return fail(format!(
                "`{c}` and `{other}.schedule = …` claim the schedule of the same timer \
                 ({uuid}) — keep one"
            ));
        }
        let decl = module
            .schedules()
            .find(|s| s.name == c.schedule)
            .expect("validate admitted only declared schedules");
        let program = program(decl).map_err(|m| Error::Compile(format!("`{c}`: {m}")))?;
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
                "`{c}`: the timer is analog — `{iv}` needs a value (`{iv} = <value>`)"
            ));
        }
        if !analog
            && let Some(iv) = program
                .values()
                .flatten()
                .find(|iv| iv.value.as_deref().is_some_and(|v| v != "1"))
        {
            return fail(format!(
                "`{c}`: the timer is digital — `{iv}` can only be on (drop the value); \
                 values belong to an analog DayTimer"
            ));
        }
        write_schedule(el, &program);
    }
    Ok(())
}

/// Rewrite `Modes=`, `UserModes=`, `N=` and the `<Entry>` children. The
/// entries take the place of the old ones (after the connectors when
/// there were none), in the order Loxone writes them: by mode index, then
/// start. `UserModes=` counts the modes beyond the seven weekdays and is
/// absent when there are none (corpus-wide, and how a save rewrites it).
fn write_schedule(el: &mut Element, program: &Program) {
    let modes: Vec<String> = program.keys().map(|&m| mode_uuid(m)).collect();
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
    let extra = program.keys().filter(|&&m| m < 4).count();
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
        let m = Module::parse(&format!("schedule s\n{src}end\n")).unwrap();
        m.schedules().next().unwrap().clone()
    }

    #[test]
    fn day_groups_expand_and_merge_per_mode() {
        let d = decl("\tweekdays 17:00-22:00\n\tmon 06:00-07:00\n");
        let p = program(&d).unwrap();
        assert_eq!(
            p.keys().copied().collect::<Vec<_>>(),
            [4, 5, 6, 7, 8, 9, 10]
        );
        assert!(p[&9].is_empty(), "an unnamed weekday is off");
        let mon: Vec<(u16, u16)> = p[&4].iter().map(|iv| (iv.from, iv.to)).collect();
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
}
