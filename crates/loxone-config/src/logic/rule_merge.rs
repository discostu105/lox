//! Bring rules edited in the app into source (`lox config merge`).
//!
//! The app saves a rule edit into the program file, so a download carries it.
//! A push compiles the module against that download, and a `rule` declaration
//! from before the edit would write the old rule back. [`merge_rules`] lifts
//! the rules the module declares from the live config, rewrites their
//! declarations, and checks that the module then compiles to the live config
//! byte for byte. Nothing else may have changed between base and live: other
//! logic changes are a re-base by hand.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::ast::{Item, Module, RULE_BLOCK_TYPE, RuleDecl, RuleItem};
use super::compile::{CompileOptions, compile};
use crate::diff::DocDiff;
use crate::doc::LoxoneDoc;
use crate::error::{Error, Result};
use crate::lock::Lockfile;

/// What [`merge_rules`] changes, verified, not yet written.
#[derive(Debug)]
pub struct RuleMerge {
    /// The module files whose text changes, with their new text
    pub files: Vec<(PathBuf, String)>,
    /// The lockfile after compiling into the live config
    pub lock: Lockfile,
    /// The compiled output: the live config, byte for byte
    pub out: Vec<u8>,
    /// Declarations rewritten from the live rule: `slug`, then what changed
    pub updated: Vec<(String, Vec<String>)>,
    /// Declarations of rules deleted in the app, removed from source
    pub removed: Vec<String>,
    /// The module changes the live config (source changes not pushed yet)
    pub source_ahead: bool,
    /// Rules of the live config that no declaration owns, by name (new in the
    /// app, say). The push keeps them as they are live.
    pub unmanaged: Vec<String>,
}

impl RuleMerge {
    /// Whether the source changes at all.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// Lift the rules the module declares from `live`. `sources` are the module
/// files with their text, in module order; `base` is the config the lock was
/// compiled against. Fails, with nothing to write, when base and live differ
/// in more than rules, users and access, when a rule cannot be written as
/// source, or when the result does not compile to `live` byte for byte.
pub fn merge_rules(
    sources: &[(PathBuf, String)],
    base: &LoxoneDoc,
    live: &LoxoneDoc,
    lock: &Lockfile,
    opts: &CompileOptions,
) -> Result<RuleMerge> {
    check_only_rules_changed(&crate::diff::diff(base, live))?;

    let mut fragments = Vec::new();
    for (path, src) in sources {
        let m = Module::parse_fragment(src)
            .map_err(|e| Error::Compile(format!("{}: {e}", path.display())))?;
        fragments.push((path.clone(), src.clone(), m));
    }

    let objects = live.objects();
    let rules: BTreeMap<&str, _> = objects
        .iter()
        .filter(|o| o.block_type == RULE_BLOCK_TYPE)
        .map(|o| (o.uuid.as_str(), o))
        .collect();
    let slug_of = |u: &str| -> Option<String> {
        lock.objects
            .iter()
            .find(|(_, lo)| lo.uuid == u)
            .map(|(s, _)| s.clone())
            .or_else(|| {
                lock.externals
                    .iter()
                    .find(|(_, le)| le.uuid == u)
                    .map(|(s, _)| s.clone())
            })
    };
    let uuid_of = |decl: &RuleDecl| -> Option<&String> {
        match decl.title {
            Some(_) => lock.rules.get(&decl.slug),
            None => lock.externals.get(&decl.slug).map(|e| &e.uuid),
        }
    };

    let lift = |doc: &LoxoneDoc, uuid: &str, decl: &RuleDecl| -> Option<Result<RuleDecl>> {
        let o = doc
            .objects()
            .into_iter()
            .find(|o| o.uuid == uuid && o.block_type == RULE_BLOCK_TYPE)?;
        let apc = doc
            .element_at(&o.path)
            .and_then(|el| el.attr_decoded("APC"))
            .map(|a| a.into_owned())
            .unwrap_or_default();
        Some(
            super::rule::lift(&decl.slug, &apc, doc, &slug_of)
                .map(|mut d| {
                    d.title = decl
                        .title
                        .as_ref()
                        .map(|_| o.title.clone().unwrap_or_default());
                    d
                })
                .map_err(|why| {
                    Error::Compile(format!(
                        "`rule {}`: the rule cannot be written as source: {why}",
                        decl.slug
                    ))
                }),
        )
    };
    let conflict = |decl: &RuleDecl| {
        Error::Compile(format!(
            "`rule {}`: changed both in the app and in source since the base. Push or revert \
             the source change first, or edit the rule in the app to match",
            decl.slug
        ))
    };

    let mut new_lock = lock.clone();
    let mut owned = Vec::new();
    let mut taken = Vec::new();
    let mut updated = Vec::new();
    let mut removed = Vec::new();
    for (_, _, module) in &mut fragments {
        let mut kept = Vec::with_capacity(module.items.len());
        for item in std::mem::take(&mut module.items) {
            let Item::Rule(decl) = &item else {
                kept.push(item);
                continue;
            };
            let Some(uuid) = uuid_of(decl).cloned() else {
                // Not compiled yet: the push creates it.
                kept.push(item);
                continue;
            };
            owned.push(uuid.clone());
            // Three ways: what the base had, what the source says, what runs.
            let in_base = lift(base, &uuid, decl).transpose()?;
            let in_live = lift(live, &uuid, decl).transpose()?;
            let same = |a: &RuleDecl, b: &RuleDecl| rule_changes(a, b).is_empty();
            let live_moved = match (&in_base, &in_live) {
                (Some(b), Some(l)) => !same(b, l),
                (None, None) => false,
                _ => true,
            };
            if !live_moved {
                kept.push(item);
                continue;
            }
            let source_moved = in_base.as_ref().is_none_or(|b| !same(b, decl));
            match in_live {
                None => {
                    if source_moved || decl.title.is_none() {
                        return Err(conflict(decl));
                    }
                    new_lock.rules.remove(&decl.slug);
                    removed.push(decl.slug.clone());
                }
                Some(lifted) => {
                    let changes = rule_changes(decl, &lifted);
                    if changes.is_empty() {
                        kept.push(item);
                        continue;
                    }
                    if source_moved {
                        return Err(conflict(decl));
                    }
                    taken.push(uuid.clone());
                    updated.push((decl.slug.clone(), changes));
                    kept.push(Item::Rule(carry_comments(decl, lifted)));
                }
            }
        }
        module.items = kept;
    }

    let unmanaged = rules
        .values()
        .filter(|o| !owned.contains(&o.uuid))
        .map(|o| o.title.clone().unwrap_or_else(|| o.uuid.clone()))
        .collect();

    // Compile the module as `loxl compile` loads it: `file("…")` values read
    // relative to their fragment, instances classified across fragments.
    let load = |frags: Vec<(&PathBuf, &Module)>| -> Result<Module> {
        let mut loaded = Vec::new();
        for (path, m) in frags {
            let mut m = m.clone();
            m.load_files(path.parent().unwrap_or(std::path::Path::new(".")))?;
            loaded.push(m);
        }
        Ok(Module::from_fragments(loaded))
    };
    let original: Vec<Module> = sources
        .iter()
        .map(|(_, src)| Module::parse_fragment(src).expect("parsed above"))
        .collect();
    let old = load(
        sources
            .iter()
            .map(|(p, _)| p)
            .zip(original.iter())
            .collect(),
    )?;
    let source_ahead = compile(base, &old, &mut lock.clone(), opts)?.to_bytes() != base.to_bytes();
    let merged = load(fragments.iter().map(|(p, _, m)| (p, m)).collect())?;
    let doc = compile(live, &merged, &mut new_lock, opts)?;
    let out = doc.to_bytes();
    if !source_ahead && out != live.to_bytes() {
        let d = crate::diff::diff(live, &doc);
        let what = if d.is_empty() {
            "the bytes differ".to_string()
        } else {
            first_lines(&d)
        };
        return Err(Error::Compile(format!(
            "after the merge the module does not compile to the live config ({what}); \
             nothing written"
        )));
    }
    // With source changes not pushed yet, each rule taken over must at least
    // compile to what runs.
    for uuid in &taken {
        let attrs = |d: &LoxoneDoc| {
            d.objects()
                .into_iter()
                .find(|o| &o.uuid == uuid)
                .and_then(|o| {
                    let el = d.element_at(&o.path)?;
                    Some(
                        ["Title", "APC", "STA"]
                            .map(|a| el.attr(a).map(str::to_string))
                            .to_vec(),
                    )
                })
        };
        if attrs(&doc) != attrs(live) {
            return Err(Error::Compile(format!(
                "after the merge the rule {uuid} does not compile to the live rule; nothing \
                 written"
            )));
        }
    }

    let files = fragments
        .iter()
        .filter_map(|(path, src, m)| {
            let text = m.to_text();
            (*src != text).then(|| (path.clone(), text))
        })
        .collect();
    Ok(RuleMerge {
        files,
        lock: new_lock,
        out,
        updated,
        removed,
        source_ahead,
        unmanaged,
    })
}

/// Base and live may differ in rules (their payload, name and the flags the
/// app writes with them), users and access. Anything else is an error.
fn check_only_rules_changed(d: &DocDiff) -> Result<()> {
    let is_rule = |t: &str| t == RULE_BLOCK_TYPE;
    let mut other: Vec<String> = Vec::new();
    for o in d.added.iter().filter(|o| !is_rule(&o.block_type)) {
        other.push(format!(
            "added {} {}",
            o.block_type,
            name(&o.title, &o.uuid)
        ));
    }
    for o in d.removed.iter().filter(|o| !is_rule(&o.block_type)) {
        other.push(format!(
            "removed {} {}",
            o.block_type,
            name(&o.title, &o.uuid)
        ));
    }
    for r in d.renamed.iter().filter(|r| !is_rule(&r.block_type)) {
        other.push(format!(
            "renamed {} {:?} → {:?}",
            r.block_type,
            r.from.as_deref().unwrap_or(""),
            r.to.as_deref().unwrap_or("")
        ));
    }
    for p in d.param_changes.iter().filter(|p| !is_rule(&p.block_type)) {
        other.push(format!(
            "{} {} {}: {:?} → {:?}",
            p.block_type,
            p.object_uuid,
            p.port_key,
            p.from.as_deref().unwrap_or(""),
            p.to.as_deref().unwrap_or("")
        ));
    }
    for p in d.placements.iter().filter(|p| !is_rule(&p.block_type)) {
        other.push(format!(
            "{} {} {}",
            p.block_type,
            name(&p.title, &p.object_uuid),
            p.attr
        ));
    }
    let wires = d.wires_added.len() + d.wires_removed.len();
    if wires > 0 {
        other.push(format!("{wires} wire(s) added or removed"));
    }
    if other.is_empty() {
        return Ok(());
    }
    let n = other.len();
    other.truncate(5);
    Err(Error::Compile(format!(
        "the live config changed in more than rules since the base ({n} change(s), first: {}). \
         Review them with `lox logic diff`, update the module, and set the download as base",
        other.join("; ")
    )))
}

fn name(title: &Option<String>, uuid: &str) -> String {
    match title {
        Some(t) if !t.is_empty() => format!("{t:?}"),
        _ => uuid.to_string(),
    }
}

fn first_lines(d: &DocDiff) -> String {
    let mut parts = Vec::new();
    if !d.added.is_empty() {
        parts.push(format!("{} added", d.added.len()));
    }
    if !d.removed.is_empty() {
        parts.push(format!("{} removed", d.removed.len()));
    }
    if let Some(p) = d.param_changes.first() {
        parts.push(format!(
            "{} param change(s), first {} {}",
            d.param_changes.len(),
            p.block_type,
            p.port_key
        ));
    }
    let wires = d.wires_added.len() + d.wires_removed.len();
    if wires > 0 {
        parts.push(format!("{wires} wire(s)"));
    }
    if parts.is_empty() {
        parts.push("renames, placements or access".into());
    }
    parts.join(", ")
}

/// What differs between a declaration and the one lifted from live, as
/// source lines (`- …`, `+ …`); empty when the two say the same.
fn rule_changes(old: &RuleDecl, new: &RuleDecl) -> Vec<String> {
    let mut out = Vec::new();
    if old.title != new.title {
        out.push(format!(
            "name {:?} → {:?}",
            old.title.as_deref().unwrap_or(""),
            new.title.as_deref().unwrap_or("")
        ));
    }
    let text = |d: &RuleDecl| -> Vec<String> {
        let bare = RuleDecl {
            slug: d.slug.clone(),
            title: None,
            body: d
                .stmts()
                .map(|s| {
                    RuleItem::Stmt(super::ast::RuleStmt {
                        kind: s.kind.clone(),
                        comment: None,
                    })
                })
                .collect(),
            comment: None,
            end_comment: None,
        };
        let m = Module {
            items: vec![Item::Rule(bare)],
        };
        m.to_text()
            .lines()
            .filter(|l| l.starts_with('\t'))
            .map(|l| l.trim().to_string())
            .collect()
    };
    let (a, b) = (text(old), text(new));
    if a != b {
        out.extend(
            a.iter()
                .filter(|l| !b.contains(l))
                .map(|l| format!("- {l}")),
        );
        out.extend(
            b.iter()
                .filter(|l| !a.contains(l))
                .map(|l| format!("+ {l}")),
        );
        if out.is_empty() || out.len() == usize::from(old.title != new.title) {
            out.push("statement order".into());
        }
    }
    out
}

/// The lifted declaration with the old one's comments: the header and `end`
/// comments, whole-line comments before the first statement, and the trailing
/// comment of each statement that stays the same.
fn carry_comments(old: &RuleDecl, mut new: RuleDecl) -> RuleDecl {
    new.comment = old.comment.clone();
    new.end_comment = old.end_comment.clone();
    let leading: Vec<RuleItem> = old
        .body
        .iter()
        .take_while(|i| matches!(i, RuleItem::Comment(_)))
        .cloned()
        .collect();
    for item in &mut new.body {
        if let RuleItem::Stmt(s) = item
            && let Some(o) = old.stmts().find(|o| o.kind == s.kind)
        {
            s.comment = o.comment.clone();
        }
    }
    new.body.splice(0..0, leading);
    new
}
