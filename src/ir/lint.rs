//! Advisory lints — findings a compile has no business rejecting but a
//! human wants to see: names that serve nothing, blocks whose outputs
//! feed nothing.
//!
//! Two layers. [`lint_source`] needs only the module: unused externs and
//! constants, uninstantiated templates. [`lint_dead_outputs`] needs the
//! compiled result too, because a managed block's consumers may live
//! outside the source — GUI-drawn wires survive in the compiled config,
//! so only there is "nothing reads this block" a real statement. Both are
//! advisory: a finding can be deliberate (reference externs kept as
//! documentation, an app-visible state block), which is why lint reports
//! and never blocks a compile.

use super::ast::{Item, MatchSpec, Module, TestItem, Value};
use crate::doc::{LoxoneDoc, ports};
use crate::error::Result;
use crate::lock::Lockfile;
use crate::xml::Node;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintKind {
    UnusedExtern,
    UnusedLet,
    UnusedTemplate,
    DeadOutputs,
}

impl LintKind {
    pub fn label(self) -> &'static str {
        match self {
            LintKind::UnusedExtern => "unused-extern",
            LintKind::UnusedLet => "unused-let",
            LintKind::UnusedTemplate => "unused-template",
            LintKind::DeadOutputs => "dead-outputs",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LintFinding {
    pub kind: LintKind,
    pub slug: String,
    pub detail: String,
}

/// Source-only lints: declared names nothing references. Templates are
/// checked before expansion (an uninstantiated template disappears in the
/// expanded view); externs and constants after expansion and desugaring,
/// so template-body captures and expression operands count as uses.
pub fn lint_source(module: &Module) -> Result<Vec<LintFinding>> {
    let mut findings = Vec::new();

    let instantiated: BTreeSet<&str> = module
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Instance(call) => Some(call.block_type.as_str()),
            _ => None,
        })
        .collect();
    for item in &module.items {
        if let Item::Template(t) = item
            && !instantiated.contains(t.name.as_str())
        {
            findings.push(LintFinding {
                kind: LintKind::UnusedTemplate,
                slug: t.name.clone(),
                detail: "declared but never instantiated".into(),
            });
        }
    }

    let (flat, _) = module.expand()?.desugar()?;
    let mut object_refs: BTreeSet<&str> = BTreeSet::new();
    let mut let_refs: BTreeSet<&str> = BTreeSet::new();
    for item in &flat.items {
        match item {
            Item::Wire(w) => {
                object_refs.insert(&w.from.slug);
                object_refs.insert(&w.to.slug);
            }
            Item::Block(b) => {
                for (_, src) in b.input_wires() {
                    object_refs.insert(&src.slug);
                }
                for (k, v) in b.params() {
                    if let Value::Ref(name) = v {
                        // `mirrors:` on a minted ref names an object (D33);
                        // every other `Ref` value is a `let` reference.
                        if k == "mirrors"
                            && matches!(b.block_type.as_str(), "InputRef" | "OutputRef")
                        {
                            object_refs.insert(name);
                        } else {
                            let_refs.insert(name);
                        }
                    }
                }
            }
            Item::Set(s) => {
                object_refs.insert(&s.target.slug);
                if let Value::Ref(name) = &s.value {
                    let_refs.insert(name);
                }
            }
            Item::Attr(a) => {
                object_refs.insert(&a.slug);
                if let Value::Ref(name) = &a.value {
                    let_refs.insert(name);
                }
            }
            Item::Extern(e) => {
                if let MatchSpec::Mirrors(target) = &e.match_spec {
                    object_refs.insert(target);
                }
            }
            // A test driving or asserting a port is a use (D36).
            Item::Test(t) => {
                for stmt in &t.body {
                    let (port, value) = match stmt {
                        TestItem::Inject(s) => (&s.target, &s.value),
                        TestItem::Expect(e) => (&e.port, &e.value),
                        _ => continue,
                    };
                    object_refs.insert(&port.slug);
                    if let Value::Ref(name) = value {
                        let_refs.insert(name);
                    }
                }
            }
            _ => {}
        }
    }

    for item in &flat.items {
        match item {
            Item::Extern(e) if !object_refs.contains(e.slug.as_str()) => {
                findings.push(LintFinding {
                    kind: LintKind::UnusedExtern,
                    slug: e.slug.clone(),
                    detail: format!("extern {} is never referenced", e.block_type),
                });
            }
            Item::Let(l) if !let_refs.contains(l.name.as_str()) => {
                findings.push(LintFinding {
                    kind: LintKind::UnusedLet,
                    slug: l.name.clone(),
                    detail: "constant is never referenced".into(),
                });
            }
            _ => {}
        }
    }
    Ok(findings)
}

/// Blocks whose outputs feed nothing — checked against the *compiled*
/// config, so wires drawn in Loxone Config count as consumers. A block is
/// flagged when nothing draws from any of its ports, or when everything
/// reachable from them is `InputRef`/`OutputRef` plumbing that itself
/// feeds nothing (a mirror of a dead signal is still dead). Expression
/// blocks (D24) are skipped — they feed their sink by construction.
pub fn lint_dead_outputs(
    module: &Module,
    lock: &Lockfile,
    compiled: &LoxoneDoc,
) -> Result<Vec<LintFinding>> {
    let (flat, _) = module.expand()?.desugar()?;

    let objects = compiled.objects();
    let mut port_owner: BTreeMap<String, usize> = BTreeMap::new();
    let mut is_ref = vec![false; objects.len()];
    let mut has_iodata = vec![false; objects.len()];
    for (i, obj) in objects.iter().enumerate() {
        let el = compiled.element_at(&obj.path).expect("path from objects()");
        is_ref[i] = matches!(obj.block_type.as_str(), "InputRef" | "OutputRef");
        has_iodata[i] = el
            .children
            .iter()
            .any(|n| matches!(n, Node::Element(e) if e.name == "IoData"));
        for port in ports(el) {
            port_owner.insert(port.uuid.clone(), i);
        }
    }
    // feeds: source object -> sink objects (through wires).
    let mut feeds: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for wire in compiled.wires() {
        if let (Some(&from), Some(&to)) = (
            port_owner.get(&wire.from_port),
            port_owner.get(&wire.to_port),
        ) {
            feeds.entry(from).or_default().insert(to);
        }
    }
    let uuid_index: BTreeMap<&str, usize> = objects
        .iter()
        .enumerate()
        .map(|(i, o)| (o.uuid.as_str(), i))
        .collect();

    let mut findings = Vec::new();
    for item in &flat.items {
        let Item::Block(b) = item else { continue };
        let Some(entry) = lock.objects.get(&b.slug) else {
            continue; // never compiled — nothing to judge yet
        };
        if entry.expr_owned {
            continue;
        }
        let Some(&start) = uuid_index.get(entry.uuid.as_str()) else {
            continue;
        };
        // Side-channel consumers wires cannot see: central blocks command
        // their targets through the `rec=` uuid list, and a `Code16`
        // program acts through its code (network calls, app interaction)
        // — unwired outputs are normal for both.
        let el = compiled
            .element_at(&objects[start].path)
            .expect("path from objects()");
        if el.attr("rec").is_some_and(|v| !v.is_empty()) || objects[start].block_type == "Code16" {
            continue;
        }
        // Forward reachability; dead when it never leaves ref plumbing.
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut queue: VecDeque<usize> = feeds
            .get(&start)
            .map(|next| next.iter().copied().collect())
            .unwrap_or_default();
        let mut alive = false;
        while let Some(i) = queue.pop_front() {
            if !seen.insert(i) {
                continue;
            }
            if !is_ref[i] {
                alive = true;
                break;
            }
            if let Some(next) = feeds.get(&i) {
                queue.extend(next.iter().copied());
            }
        }
        if alive {
            continue;
        }
        let mut detail = if seen.is_empty() {
            "no wire draws from any of its ports (compiled config, GUI wires included)".to_string()
        } else {
            "feeds only ref plumbing that itself feeds nothing".to_string()
        };
        if has_iodata[start] {
            detail.push_str(" — carries IoData, may be app-visible");
        }
        findings.push(LintFinding {
            kind: LintKind::DeadOutputs,
            slug: b.slug.clone(),
            detail,
        });
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_lints_find_unused_names() {
        let src = "\
extern used = VirtualIn(iname: \"VI1\")
extern lonely = VirtualIn(iname: \"VI2\")
extern sink = VirtualIn(iname: \"VI3\")

let schwelle = 28
let verwaist = 5

template nie_benutzt(x: VirtualIn)
\tg = And(
\t\tI1: x.Q,
\t)
end

gate = GreaterEqual(
\tInput1: used.Q,
\tInput2: schwelle,
)

sink.I <- gate.Q
";
        let m = Module::parse(src).unwrap();
        let findings = lint_source(&m).unwrap();
        let got: Vec<(&str, &str)> = findings
            .iter()
            .map(|f| (f.kind.label(), f.slug.as_str()))
            .collect();
        assert!(got.contains(&("unused-extern", "lonely")), "{got:?}");
        assert!(got.contains(&("unused-let", "verwaist")), "{got:?}");
        assert!(got.contains(&("unused-template", "nie_benutzt")), "{got:?}");
        assert!(!got.iter().any(|(_, s)| *s == "used"), "{got:?}");
        assert!(!got.iter().any(|(_, s)| *s == "schwelle"), "{got:?}");
        assert!(!got.iter().any(|(_, s)| *s == "sink"), "{got:?}");
    }

    #[test]
    fn mirrors_target_counts_as_use() {
        let src = "\
extern status = VirtualIn(iname: \"VI1\")
extern status_ref = InputRef(mirrors: status)
extern sink = VirtualIn(iname: \"VI2\")

status_ref.AI <- status.AQ
sink.I <- status_ref.Q
";
        let m = Module::parse(src).unwrap();
        let findings = lint_source(&m).unwrap();
        assert!(findings.is_empty(), "{findings:?}");
    }
}
