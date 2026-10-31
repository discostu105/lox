//! How much of a config the source governs: every object of the config,
//! by type, against the lockfile — managed, referenced as an extern, a
//! rule (D60), or untouched — and what Lox Logic can do with its type.
//! `loxl coverage` prints it; the goal it measures is a config whose
//! logic is all source.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::connectors::{adopt_only, builtin, container_type};
use crate::doc::LoxoneDoc;
use crate::lock::Lockfile;

/// What Lox Logic can do with objects of a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Support {
    /// Verified builtin: declared, minted and rebuilt from source.
    Managed,
    /// Verified builtin that Loxone Config creates when it pairs a device
    /// (Code Touch, Intercom, Touch Pure Flex): adopted, never minted.
    AdoptOnly,
    /// An Autopilot rule (D60): one the source names is its own, one made
    /// in the app is referenced (`rule <slug>`).
    Rule,
    /// Not in the verified table: the source references it as an extern
    /// (wires, port values, title/room/category claims) but cannot
    /// rebuild it.
    Reference,
}

impl Support {
    pub fn of(block_type: &str) -> Support {
        if block_type == "AutopilotRule" {
            Support::Rule
        } else if builtin(block_type).is_none() {
            Support::Reference
        } else if adopt_only(block_type) {
            Support::AdoptOnly
        } else {
            Support::Managed
        }
    }

    fn label(self) -> &'static str {
        match self {
            Support::Managed => "managed",
            Support::AdoptOnly => "adopt-only",
            Support::Rule => "rule",
            Support::Reference => "reference",
        }
    }
}

/// One type's objects in one part of the config.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TypeCoverage {
    pub block_type: String,
    pub support: Option<Support>,
    pub total: usize,
    pub managed: usize,
    pub externs: usize,
    pub rules: usize,
    /// Objects the source does not mention at all.
    pub untouched: usize,
    /// Titles of manageable objects the source does not manage yet — the
    /// work left (`loxl adopt --uuid …`, which also promotes an extern).
    pub adoptable: Vec<Adoptable>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Adoptable {
    pub uuid: String,
    pub title: Option<String>,
    /// The extern slug the source already uses for it, if any.
    pub extern_slug: Option<String>,
}

/// The coverage report: logic drawn on pages, and the periphery (devices,
/// virtual inputs and outputs, system objects with connectors).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Coverage {
    pub pages: Vec<TypeCoverage>,
    pub periphery: Vec<TypeCoverage>,
}

/// Page furniture that is not logic of its own: the page, the mirrors
/// (refs) the GUI draws for wires across pages, and comment boxes.
fn is_plumbing(block_type: &str) -> bool {
    matches!(
        block_type,
        "Page" | "InputRef" | "OutputRef" | "OutputRefLM" | "Text"
    )
}

pub fn coverage(doc: &LoxoneDoc, lock: &Lockfile) -> Coverage {
    let managed: BTreeSet<&str> = lock.objects.values().map(|o| o.uuid.as_str()).collect();
    let externs: BTreeMap<&str, &str> = lock
        .externals
        .iter()
        .map(|(s, e)| (e.uuid.as_str(), s.as_str()))
        .collect();
    let rules: BTreeSet<&str> = lock.rules.values().map(String::as_str).collect();
    let mut pages: BTreeMap<String, TypeCoverage> = BTreeMap::new();
    let mut periphery: BTreeMap<String, TypeCoverage> = BTreeMap::new();
    for o in doc.objects() {
        if is_plumbing(&o.block_type) {
            continue;
        }
        let on_page = (1..o.path.len()).any(|n| {
            doc.element_at(&o.path[..n])
                .is_some_and(|el| el.name == "C" && el.attr("Type") == Some("Page"))
        });
        // A block's own child objects (a Code Touch's keypad, the meters
        // of an energy flow monitor) go with their parent.
        let parent = doc.element_at(&o.path[..o.path.len() - 1]);
        if on_page && parent.is_some_and(|p| p.name == "C" && p.attr("Type") != Some("Page")) {
            continue;
        }
        // Off the pages, only objects a wire can reach count: rooms,
        // categories, users and folders have no connectors.
        let el = doc.element_at(&o.path).expect("path from objects()");
        let device = crate::periphery::periphery(&o.block_type).is_some();
        if !on_page && !device && !el.child_elements().any(|c| c.name == "Co") {
            continue;
        }
        let part = if on_page && container_type(&o.block_type).is_none() {
            &mut pages
        } else {
            &mut periphery
        };
        let t = part
            .entry(o.block_type.clone())
            .or_insert_with(|| TypeCoverage {
                block_type: o.block_type.clone(),
                support: Some(Support::of(&o.block_type)),
                ..TypeCoverage::default()
            });
        t.total += 1;
        let ext = externs.get(o.uuid.as_str()).copied();
        if managed.contains(o.uuid.as_str()) {
            t.managed += 1;
        } else if rules.contains(o.uuid.as_str()) {
            t.rules += 1;
        } else {
            match ext {
                Some(_) => t.externs += 1,
                None => t.untouched += 1,
            }
            // A virtual or Modbus device is adopted with its I/O (D64): the
            // topmost one the source does not manage is the work left.
            let topmost = device
                && !(1..o.path.len()).any(|n| {
                    doc.element_at(&o.path[..n]).is_some_and(|a| {
                        a.name == "C"
                            && a.attr("Type")
                                .is_some_and(|t| crate::periphery::periphery(t).is_some())
                            && a.attr("U").is_some_and(|u| !managed.contains(u))
                    })
                });
            if (on_page || topmost)
                && matches!(t.support, Some(Support::Managed | Support::AdoptOnly))
            {
                t.adoptable.push(Adoptable {
                    uuid: o.uuid.clone(),
                    title: o.title.clone(),
                    extern_slug: ext.map(str::to_string),
                });
            }
        }
    }
    let ordered = |m: BTreeMap<String, TypeCoverage>| {
        let mut v: Vec<TypeCoverage> = m.into_values().collect();
        v.sort_by(|a, b| {
            a.support
                .cmp(&b.support)
                .then(b.total.cmp(&a.total))
                .then(a.block_type.cmp(&b.block_type))
        });
        v
    };
    Coverage {
        pages: ordered(pages),
        periphery: ordered(periphery),
    }
}

impl Coverage {
    /// The plain-text report `loxl coverage` prints.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        let table = |s: &mut String, title: &str, rows: &[TypeCoverage]| {
            s.push_str(&format!(
                "{title}\n  {:<28} {:>5} {:>7} {:>7} {:>9}  support\n",
                "type", "total", "managed", "extern", "untouched"
            ));
            for r in rows {
                let support = r.support.map_or("", Support::label);
                s.push_str(&format!(
                    "  {:<28} {:>5} {:>7} {:>7} {:>9}  {support}{}\n",
                    r.block_type,
                    r.total,
                    r.managed + r.rules,
                    r.externs,
                    r.untouched,
                    if r.rules > 0 { " (rules)" } else { "" }
                ));
            }
        };
        table(&mut s, "logic on pages", &self.pages);
        s.push('\n');
        table(
            &mut s,
            "periphery (devices, virtual I/O, system)",
            &self.periphery,
        );

        let sum = |f: fn(&TypeCoverage) -> usize| self.pages.iter().map(f).sum::<usize>();
        let total = sum(|r| r.total);
        let managed = sum(|r| r.managed + r.rules);
        let manageable = sum(|r| match r.support {
            Some(Support::Reference) => 0,
            // An app-made rule stays the app's until it is rewritten in
            // source; the ones the source names count.
            Some(Support::Rule) => r.rules,
            _ => r.total,
        });
        s.push_str(&format!(
            "\npage logic: {managed} of {total} blocks managed; {manageable} of {total} are \
             of a type Lox Logic can manage\n"
        ));
        let todo: Vec<&Adoptable> = self.pages.iter().flat_map(|r| &r.adoptable).collect();
        if !todo.is_empty() {
            s.push_str(&format!(
                "not managed yet ({}) — `loxl adopt <cfg> --uuid <uuid> --as <slug> …` \
                 (an extern keeps its slug):\n",
                todo.len()
            ));
            for a in todo {
                s.push_str(&format!(
                    "  {} {:?}{}\n",
                    a.uuid,
                    a.title.as_deref().unwrap_or(""),
                    a.extern_slug
                        .as_deref()
                        .map(|x| format!(" (extern `{x}`)"))
                        .unwrap_or_default()
                ));
            }
        }
        let devices: Vec<&Adoptable> = self.periphery.iter().flat_map(|r| &r.adoptable).collect();
        if !devices.is_empty() {
            s.push_str(&format!(
                "virtual and Modbus devices not managed yet ({}) — `loxl adopt <cfg> \
                 --uuid <uuid> --as <slug> …` takes the I/O below along:\n",
                devices.len()
            ));
            for a in devices {
                s.push_str(&format!(
                    "  {} {:?}{}\n",
                    a.uuid,
                    a.title.as_deref().unwrap_or(""),
                    a.extern_slug
                        .as_deref()
                        .map(|x| format!(" (extern `{x}`)"))
                        .unwrap_or_default()
                ));
            }
        }
        let reference: Vec<&str> = self
            .pages
            .iter()
            .filter(|r| r.support == Some(Support::Reference))
            .map(|r| r.block_type.as_str())
            .collect();
        if !reference.is_empty() {
            s.push_str(&format!(
                "reference-only types on pages (no verified connector table yet, \
                 docs/connector-db.md): {}\n",
                reference.join(", ")
            ));
        }
        s
    }
}
