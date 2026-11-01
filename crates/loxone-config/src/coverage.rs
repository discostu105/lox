//! How much of a config the source governs: every object of the config,
//! by type, against the lockfile — managed, referenced as an extern, a
//! rule, or untouched — and what Lox Logic can do with its type,
//! and per managed type the element attributes the compile carries from
//! the base instead of writing them from source (#248). `loxl coverage`
//! prints it; the goal it measures is a config whose logic is all source.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::connectors::{
    adopt_only, attr_name, attr_param, attr_params, attr_save_default, builtin, carried_attrs,
    container_type,
};
use crate::doc::{GUI_OWNED_ATTRS, LoxoneDoc};
use crate::lock::Lockfile;
use crate::logic::Module;

/// What Lox Logic can do with objects of a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Support {
    /// Verified builtin: declared, minted and rebuilt from source.
    Managed,
    /// Verified builtin that Loxone Config creates when it pairs a device
    /// (Code Touch, Intercom, Touch Pure Flex): adopted, never minted.
    AdoptOnly,
    /// Hardware Loxone Config paired: a declaration takes it over with
    /// its inputs and outputs, never creates it.
    Hardware,
    /// An Autopilot rule: one the source names is its own, one made
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
        } else if crate::hardware::is_hardware(block_type) {
            Support::Hardware
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
            Support::Hardware => "hardware",
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
    /// Attributes of managed blocks the compile carries verbatim from the
    /// base: behaviour that lives in Loxone Config, not in the source.
    pub carried: Vec<CarriedAttr>,
}

/// One element attribute of one managed type that the compile carries
/// from the base rather than writing it from source.
#[derive(Debug, Clone, Serialize)]
pub struct CarriedAttr {
    pub block_type: String,
    pub attr: String,
    /// Managed blocks of the type whose element has the attribute.
    pub blocks: usize,
    /// The source parameter that claims it (`At:` for `Sec=` on PulseAt,
    /// `Code:` for `Code=` on Code16), when the language has one.
    pub param: Option<&'static str>,
    /// Of `blocks`, those whose declaration binds `param` — written from
    /// source, not carried. Counted only when the module is known.
    pub claimed: usize,
}

/// Carried attributes that are identity, not behaviour: the UUIDs of a
/// block's special states, minted once and pinned, and a virtual
/// input's `VI<n>` designation.
const IDENTITY_ATTRS: &[&str] = &["SpStates", "IName"];

/// A program block's `Task="1"`: every save writes it, and Loxone
/// Config has no field for it, so there is nothing to claim.
fn is_save_constant(block_type: &str, attr: &str) -> bool {
    matches!(block_type, "Code16" | "Code8") && attr == "Task"
}

/// The attributes a `<timer>.schedule = <set>` claim writes.
const SCHEDULE_ATTRS: &[&str] = &["Modes", "N", "UserModes"];

/// Page furniture that is not logic of its own: the page, the mirrors
/// (refs) the GUI draws for wires across pages, and comment boxes.
fn is_plumbing(block_type: &str) -> bool {
    matches!(
        block_type,
        "Page" | "InputRef" | "OutputRef" | "OutputRefLM" | "Text"
    )
}

/// `module` is the one the lock was compiled from, when known: an
/// attribute the source claims counts as written from source.
pub fn coverage(doc: &LoxoneDoc, lock: &Lockfile, module: Option<&Module>) -> Coverage {
    // What a hardware declaration owns counts as managed.
    let managed: BTreeSet<&str> = lock
        .objects
        .values()
        .map(|o| o.uuid.as_str())
        .chain(
            lock.externals
                .values()
                .filter(|e| e.owned)
                .map(|e| e.uuid.as_str()),
        )
        .collect();
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
        let device = crate::periphery::periphery(&o.block_type).is_some()
            || crate::hardware::is_hardware(&o.block_type);
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
            // A virtual or Modbus device is adopted with its I/O: the
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
            // A hardware device is taken over with its inputs and outputs.
            let hardware = crate::hardware::is_device(&o.block_type);
            if (on_page || topmost || hardware)
                && matches!(
                    t.support,
                    Some(Support::Managed | Support::AdoptOnly | Support::Hardware)
                )
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
        carried: carried(doc, lock, module),
    }
}

/// The `Circuit<n>:` parameter that claims a light controller's
/// `NameAI<n-1>=`/`PresM<n-1>=`.
fn circuit_of(block_type: &str, attr: &str) -> Option<&'static str> {
    let i: usize = ["NameAI", "PresM"]
        .iter()
        .find_map(|p| attr.strip_prefix(p))?
        .parse()
        .ok()?;
    crate::connectors::CIRCUIT_PARAMS
        .get(i)
        .copied()
        .filter(|k| crate::connectors::circuit_param(block_type, k).is_some())
}

/// Per managed type, the attributes its elements carry from the base:
/// the GUI-owned ones and the type's own carried set.
fn carried(doc: &LoxoneDoc, lock: &Lockfile, module: Option<&Module>) -> Vec<CarriedAttr> {
    // (object UUID, attribute) the source writes: a bound attribute
    // parameter of a managed block.
    let mut claims: BTreeSet<(String, String)> = BTreeSet::new();
    if let Some(m) = module.and_then(|m| m.expand().ok()) {
        for b in m.blocks() {
            let Some(o) = lock.objects.get(&b.slug) else {
                continue;
            };
            for (k, _) in b.params() {
                if attr_params(&b.block_type).contains(&k) {
                    claims.insert((o.uuid.clone(), attr_name(&b.block_type, k).to_string()));
                }
                // `Circuit<n>:` writes the circuit's name and presence flag.
                if let Some(n) = crate::connectors::circuit_param(&b.block_type, k) {
                    for a in ["NameAI", "PresM"] {
                        claims.insert((o.uuid.clone(), format!("{a}{}", n - 1)));
                    }
                }
            }
        }
        for c in m.schedule_claims() {
            if let Some(o) = lock.objects.get(&c.slug) {
                for a in SCHEDULE_ATTRS {
                    claims.insert((o.uuid.clone(), (*a).to_string()));
                }
            }
        }
    }
    let index = doc.index();
    let mut out: BTreeMap<(String, String), CarriedAttr> = BTreeMap::new();
    for o in lock.objects.values() {
        let Some(el) = index
            .by_uuid
            .get(&o.uuid)
            .and_then(|path| doc.element_at(path))
        else {
            continue;
        };
        for a in &el.attrs {
            let name = a.name.as_str();
            let gui =
                GUI_OWNED_ATTRS.contains(&name) || carried_attrs(&o.block_type).contains(&name);
            // A ref's mirror identity is the compiler's, from `mirrors:`.
            let mirror = matches!(
                o.block_type.as_str(),
                "InputRef" | "OutputRef" | "OutputRefLM"
            ) && matches!(name, "Ref" | "LinkRefType" | "Analog" | "Text");
            if !gui
                || mirror
                || IDENTITY_ATTRS.contains(&name)
                || is_save_constant(&o.block_type, name)
                // What a save writes when nothing is set is no setting.
                || attr_save_default(&o.block_type, name) == Some(a.value.as_str())
            {
                continue;
            }
            let param = attr_param(&o.block_type, name).or_else(|| circuit_of(&o.block_type, name));
            let c = out
                .entry((o.block_type.clone(), name.to_string()))
                .or_insert_with(|| CarriedAttr {
                    block_type: o.block_type.clone(),
                    attr: name.to_string(),
                    blocks: 0,
                    param,
                    claimed: 0,
                });
            c.blocks += 1;
            if claims.contains(&(o.uuid.clone(), name.to_string())) {
                c.claimed += 1;
            }
        }
    }
    let mut v: Vec<CarriedAttr> = out.into_values().filter(|c| c.claimed < c.blocks).collect();
    v.sort_by(|a, b| {
        a.block_type
            .cmp(&b.block_type)
            .then(b.blocks.cmp(&a.blocks))
            .then(a.attr.cmp(&b.attr))
    });
    v
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
            "\npage logic: {managed} of {total} blocks managed, {manageable} of {total} of a type \
             Lox Logic can manage\n"
        ));
        let todo: Vec<&Adoptable> = self.pages.iter().flat_map(|r| &r.adoptable).collect();
        if !todo.is_empty() {
            s.push_str(&format!(
                "not managed yet ({}), adopt with `loxl adopt <cfg> --uuid <uuid> --as <slug> …` \
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
                "virtual and Modbus devices not managed yet ({}). `loxl adopt <cfg> \
                 --uuid <uuid> --as <slug> …` adopts a device with its I/O:\n",
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
        if !self.carried.is_empty() {
            s.push_str(
                "attributes of managed blocks set in Loxone Config, not in the source \
                 (type, attribute, blocks):\n",
            );
            for c in &self.carried {
                let claim = match c.param {
                    Some(p) => format!("  claim with `{p}:`"),
                    None if c.block_type == "DayTimer"
                        && SCHEDULE_ATTRS.contains(&c.attr.as_str()) =>
                    {
                        "  claim with `<timer>.schedule = <set>`".to_string()
                    }
                    None => String::new(),
                };
                s.push_str(&format!(
                    "  {:<28} {:<16} {:>5}{claim}\n",
                    c.block_type,
                    c.attr,
                    c.blocks - c.claimed
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
                "reference-only types on pages (Lox Logic does not know their connectors yet): {}\n",
                reference.join(", ")
            ));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::{CompileOptions, compile};

    #[test]
    fn carried_attributes_are_listed_until_the_source_claims_them() {
        let base =
            LoxoneDoc::parse(&std::fs::read("examples/configs/pool.Loxone").unwrap()).unwrap();
        let opts = CompileOptions {
            machine: crate::uuid::parse_serial("504F94112233").unwrap(),
            mint_time_unix: 1_767_225_600,
            page_title: Some("Pool".into()),
            allow_removals: false,
            accept_version: None,
            relayout: Vec::new(),
        };
        let unclaimed = Module::parse("p = PulseAt(\"P\")\n").unwrap();
        let mut lock = Lockfile::new();
        let mut doc = compile(&base, &unclaimed, &mut lock, &opts).unwrap();
        // The GUI sets a time and a daily pulse.
        let o = doc
            .objects()
            .into_iter()
            .find(|o| o.title.as_deref() == Some("P"))
            .unwrap();
        let el = doc.element_at_mut(&o.path).unwrap();
        el.set_attr("Sec", "68400");
        el.set_attr("Typ", "3");

        let rows = |m: &Module| -> Vec<(String, Option<&'static str>, usize)> {
            coverage(&doc, &lock, Some(m))
                .carried
                .into_iter()
                .map(|c| (c.attr, c.param, c.blocks - c.claimed))
                .collect()
        };
        assert_eq!(
            rows(&unclaimed),
            vec![
                ("Sec".to_string(), Some("At"), 1),
                ("Typ".to_string(), Some("Pulse"), 1)
            ]
        );
        let at = Module::parse("p = PulseAt(\"P\", At: 19:00)\n").unwrap();
        assert_eq!(rows(&at), vec![("Typ".to_string(), Some("Pulse"), 1)]);
        let both = Module::parse("p = PulseAt(\"P\", At: 19:00, Pulse: \"time\")\n").unwrap();
        assert!(rows(&both).is_empty());
        let text = coverage(&doc, &lock, None).to_text();
        assert!(text.contains("claim with `At:`"), "{text}");
    }

    #[test]
    fn a_declared_circuit_is_owned() {
        let base =
            LoxoneDoc::parse(&std::fs::read("examples/library/lighting/lighting.Loxone").unwrap())
                .unwrap();
        let opts = CompileOptions {
            machine: crate::uuid::parse_serial("504F94112233").unwrap(),
            mint_time_unix: 1_767_225_600,
            page_title: None,
            allow_removals: false,
            accept_version: None,
            relayout: Vec::new(),
        };
        let src = "page \"Probe\" (room: \"Wohnzimmer\")\n\
                   extern relais2 = Actor(iname: \"Q2\")\n\
                   l1 = LightController2(\"L1\", Circuit1: \"dimmer\")\n\
                   relais2.I <- l1.AQ1\n";
        let module = Module::parse(src).unwrap();
        let mut lock = Lockfile::new();
        let mut doc = compile(&base, &module, &mut lock, &opts).unwrap();
        // A second circuit named in Loxone Config, unclaimed.
        let o = doc
            .objects()
            .into_iter()
            .find(|o| o.title.as_deref() == Some("L1"))
            .unwrap();
        let el = doc.element_at_mut(&o.path).unwrap();
        el.set_attr("NameAI1", "Flur");
        el.set_attr("PresM1", "65535");
        let rows: Vec<(String, Option<&'static str>)> = coverage(&doc, &lock, Some(&module))
            .carried
            .into_iter()
            .filter(|c| c.block_type == "LightController2")
            .map(|c| (c.attr, c.param))
            .collect();
        assert!(
            !rows.iter().any(|(a, _)| a == "NameAI0" || a == "PresM0"),
            "{rows:?}"
        );
        assert!(
            rows.contains(&("NameAI1".to_string(), Some("Circuit2"))),
            "{rows:?}"
        );
    }
}
