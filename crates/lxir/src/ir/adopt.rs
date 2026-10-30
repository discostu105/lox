//! Adoption: move existing config objects under source control.
//!
//! `adopt` is decompile-with-lock: the same lift that produces the
//! managed-only view also knows every lifted object's UUID, port UUIDs,
//! layout, and page — exactly what a lockfile pins. Compiling the returned
//! module with the returned lock therefore *rebuilds the existing blocks in
//! place* instead of minting duplicates: same object UUIDs, same port UUIDs
//! (so every wire drawn in Loxone Config keeps pointing at them), same
//! position on the same page.
//!
//! Identity comes from the lift, never from matching: adopting by title
//! would be ambiguous in real configs (duplicate titles are common), and
//! there is nothing to match — the object in front of us *is* the identity.
//!
//! The safety property is verification, not translation: the compiler
//! rebuilds managed blocks from scratch, so anything on the original
//! element that the rebuild does not reproduce would be silently lost on
//! the first compile. [`verify_rebuildable`] whitelists exactly what the
//! rebuild emits — which since D19 includes the GUI-owned residue
//! (`Cl`/`LtE`/`WF`, [`crate::doc::GUI_OWNED_ATTRS`], and
//! [`crate::doc::GUI_OWNED_CHILDREN`]) carried forward verbatim from the
//! base on every compile. A block that fails verification is *skipped, not
//! translated*: it stays unmanaged (appearing as a pinned extern where
//! adopted logic wires to it) and the reason lands in
//! [`AdoptReport::refused`] — one bespoke GUI flag never blocks adopting
//! the rest of the house.

use crate::connectors::{VERIFIED_BLOCK_VERSIONS, attr_params, builtin, container_type};
use crate::doc::{GUI_OWNED_ATTRS, GUI_OWNED_CHILDREN, LoxoneDoc, ObjectSummary, ports};
use crate::error::{Error, Result};
use crate::ir::ast::{ArgItem, BindingKind, Item, MatchSpec, Module, PortRef, WireDecl};
use crate::ir::decompile::{DecompileOptions, DecompileScope, Lift, RESERVED};
use crate::ir::validate::validate_ports;
use crate::lock::{Layout, LockedExternal, LockedObject, Lockfile, sha256_hex};
use crate::xml::{Element, Node};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdoptReport {
    /// Existing objects now managed (block declarations + lock entries).
    pub blocks: usize,
    /// Objects wired to them, declared `extern` and pinned in the lock.
    pub externs: usize,
    /// Logic pages the adopted blocks live on.
    pub pages: usize,
    /// Managed-type blocks that stayed unmanaged, with the reason each
    /// one's rebuild would not have been faithful.
    pub refused: Vec<String>,
}

/// Adopt every managed-type object in `doc` whose rebuild is verified
/// faithful: returns the managed-only module, a fresh lockfile pinning each
/// block's existing identity (object UUID, port UUIDs, layout, page) and
/// each extern's resolution, and a report. `compile(doc, module, lock)` is
/// then a semantic no-op.
pub fn adopt(doc: &LoxoneDoc) -> Result<(Module, Lockfile, AdoptReport)> {
    doc.require_unique_uuids("config")?;
    let (lift, refused) = adopt_lift(doc);
    let module = lift.single_module();
    module.validate()?;
    validate_ports(&module)?;
    let (lock, report) = adopt_lock(doc, &lift, refused);
    Ok((module, lock, report))
}

/// The per-page module fragments of an adoption: one `(file stem,
/// fragment)` per page, periphery leading as `_periphery`.
pub type PageFragments = Vec<(String, Module)>;

/// [`adopt`], with the module as directory fragments sharing one
/// namespace — written to a directory they are the `--module <dir>`
/// form of exactly the module `adopt` returns.
pub fn adopt_pages(doc: &LoxoneDoc) -> Result<(PageFragments, Lockfile, AdoptReport)> {
    doc.require_unique_uuids("config")?;
    let (lift, refused) = adopt_lift(doc);
    // Fragments may cross-reference; validation runs on the merged whole,
    // exactly as `lxir compile --module <dir>` will.
    let merged = lift.single_module();
    merged.validate()?;
    validate_ports(&merged)?;
    let fragments = lift.fragment_modules();
    let (lock, report) = adopt_lock(doc, &lift, refused);
    Ok((fragments, lock, report))
}

/// One block adopted into an existing module/lock pair by [`adopt_one`]:
/// the items to append to the module source. The lock has already been
/// extended when this is returned.
#[derive(Debug)]
pub struct AdoptedBlock {
    /// Ready to append, in order: newly pinned externs, the block
    /// declaration, then `<-` wires onto extern ports. Objects the lock
    /// already pins (managed or extern) are referenced by their existing
    /// slugs, never re-declared.
    pub items: Vec<Item>,
    /// Display title of the logic page the block lives on (where a
    /// per-page module directory would file it).
    pub page_title: String,
    /// Slugs of the externs newly pinned in the lock.
    pub new_externs: Vec<String>,
}

/// Adopt the single existing object `uuid` as managed block `slug` into an
/// existing `module`/`lock` pair (the incremental form of [`adopt`]): the
/// lock gains the block's identity (and pins for any newly referenced
/// externs), and the returned items are what the module source needs
/// appended. Wired neighbors already pinned in the lock are referenced by
/// their existing slugs.
///
/// Refused with a hard error — nothing mutated — when the config repeats
/// UUIDs, the rebuild would not be faithful, the identity is already
/// claimed, or the config wires the block into a managed sink that the
/// module source does not declare (the fix is one argument-list line; the
/// error spells it out).
pub fn adopt_one(
    doc: &LoxoneDoc,
    uuid: &str,
    slug: &str,
    module: &Module,
    lock: &mut Lockfile,
) -> Result<AdoptedBlock> {
    doc.require_unique_uuids("config")?;
    let fail = |m: String| Error::Compile(m);
    // Templates (D23) and expressions (D24): the checks below — slug
    // freshness, wires already declared on managed sinks — must see the
    // expanded and desugared module, where an instance's blocks and an
    // expression's synthetic blocks exist under their lock-key names.
    // Expansion of a partial (leniently loaded) module can fail; then the
    // raw view has to do, and the caller's no-op verification stays the
    // backstop.
    let raw = module;
    let expanded = module.expand().and_then(|x| x.desugar().map(|(m, _)| m));
    let module = expanded.as_ref().unwrap_or(module);
    let objects = doc.objects();
    let Some(o) = objects.iter().find(|o| o.uuid == uuid) else {
        return Err(fail(format!("no object with uuid `{uuid}` in the config")));
    };
    let ident = format!(
        "{} \"{}\" (uuid {uuid})",
        o.block_type,
        o.title.as_deref().unwrap_or("")
    );
    if builtin(&o.block_type).is_none() {
        return Err(fail(format!(
            "cannot adopt {ident}: `{}` is not in the verified builtin table — only \
             evidence-verified types can be managed (docs/connector-db.md)",
            o.block_type
        )));
    }
    if let Some(container) = container_type(&o.block_type) {
        return Err(fail(format!(
            "cannot adopt {ident}: an object of the `{container}` folder is minted \
             from source but not adopted (D54) — reference it as an extern"
        )));
    }
    if let Some((s, _)) = lock.objects.iter().find(|(_, lo)| lo.uuid == uuid) {
        return Err(fail(format!(
            "{ident} is already managed as `{s}` — nothing to adopt"
        )));
    }
    if let Some((s, _)) = lock.externals.iter().find(|(_, le)| le.uuid == uuid) {
        return Err(fail(format!(
            "{ident} is already referenced as extern `{s}` — promoting an extern to a \
             managed block is not supported yet; keep it unmanaged or re-adopt the \
             whole config"
        )));
    }
    if let Some(pin) = &lock.target.config_version
        && doc.config_version().as_deref() != Some(pin)
    {
        return Err(fail(format!(
            "the config's ConfigVersion {} differs from the lock's pin {pin} — qualify \
             the release first (docs/design.md D22): one oracle open+save, then \
             `lxir compile --accept-version`",
            doc.config_version().as_deref().unwrap_or("(none)")
        )));
    }
    let el = doc.element_at(&o.path).expect("path from objects()");
    verify_rebuildable(el, o).map_err(|why| fail(format!("cannot adopt {ident}: {why}")))?;

    let valid = matches!(slug.as_bytes().first(), Some(b) if b.is_ascii_lowercase())
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !RESERVED.contains(&slug);
    if !valid {
        return Err(fail(format!(
            "`{slug}` is not a valid slug ([a-z][a-z0-9_]*, no statement keyword)"
        )));
    }
    let mut taken: BTreeSet<String> = module
        .externs()
        .map(|e| e.slug.clone())
        .chain(module.blocks().map(|b| b.slug.clone()))
        .chain(module.lets().map(|l| l.name.clone()))
        .chain(raw.items.iter().filter_map(|i| match i {
            Item::Template(t) => Some(t.name.clone()),
            Item::Instance(b) => Some(b.slug.clone()),
            Item::Moods(m) => Some(m.name.clone()),
            _ => None,
        }))
        .chain(lock.objects.keys().cloned())
        .chain(lock.externals.keys().cloned())
        .collect();
    if taken.contains(slug) {
        return Err(fail(format!(
            "slug `{slug}` is already taken in this module/lock — pick another"
        )));
    }
    taken.insert(slug.to_string());

    // Lift exactly this block: every other managed-type object is excluded,
    // so wired neighbors — managed blocks included — surface as externs.
    let mut opts = DecompileOptions {
        scope: DecompileScope::ManagedOnly,
        ..DecompileOptions::default()
    };
    for other in &objects {
        if other.uuid != uuid && opts.managed_types.contains(&other.block_type) {
            opts.exclude.insert(other.uuid.clone());
        }
    }
    let lift = Lift::build(doc, &opts);
    let [ti] = lift.managed[..] else {
        return Err(fail(format!(
            "cannot adopt {ident}: lift did not isolate it"
        )));
    };
    let Some(page) = lift.page_of(ti) else {
        return Err(fail(format!(
            "cannot adopt {ident}: it is not placed on a logic page"
        )));
    };

    // Final slugs: the block gets the requested one; a lifted extern whose
    // object the lock already pins keeps its existing slug (and is not
    // re-declared); the rest are fresh, deduplicated against everything.
    let lift_slug = |i: usize| lift.slug_of[&lift.objects[i].uuid].clone();
    let mut rename: BTreeMap<String, String> = BTreeMap::new();
    rename.insert(lift_slug(ti), slug.to_string());
    // A controller the lift declared only for its moods is not this
    // block's business: adopting one block appends no mood claims.
    let mut used: BTreeSet<&str> = lift.block_decls[&ti]
        .input_wires()
        .map(|(_, src)| src.slug.as_str())
        .collect();
    used.extend(lift.wires.iter().map(|(w, _, _)| w.to.slug.as_str()));
    let mut new_externs: Vec<(usize, String)> = Vec::new();
    for &i in &lift.externs {
        let u = &lift.objects[i].uuid;
        let ls = lift_slug(i);
        if lift.mood_claims.contains_key(&i) && !used.contains(ls.as_str()) {
            continue;
        }
        let existing = lock
            .objects
            .iter()
            .find(|(_, lo)| &lo.uuid == u)
            .map(|(s, _)| s.clone())
            .or_else(|| {
                lock.externals
                    .iter()
                    .find(|(_, le)| &le.uuid == u)
                    .map(|(s, _)| s.clone())
            });
        let fin = existing.unwrap_or_else(|| {
            let mut candidate = ls.clone();
            let mut n = 1;
            while !taken.insert(candidate.clone()) {
                n += 1;
                candidate = format!("{ls}_{n}");
            }
            new_externs.push((i, candidate.clone()));
            candidate
        });
        rename.insert(ls, fin);
    }

    // Wires out of the block (every lifted wire has it as the source; wires
    // into it became argument bindings). A wire onto a plain extern becomes
    // a `<-` statement; one into a managed sink is only sound if the sink's
    // declaration already states it — the compiler rebuilds managed blocks
    // from source, so an undeclared wire would be torn down on the next
    // compile. Refuse with the exact line to add.
    let mut wire_items = Vec::new();
    for (w, _ti, _fi) in &lift.wires {
        let to_slug = rename[&w.to.slug].clone();
        if lock.objects.contains_key(&to_slug) {
            let declared = module
                .blocks()
                .find(|b| b.slug == to_slug)
                .is_some_and(|b| {
                    b.input_wires().any(|(port, src)| {
                        port == w.to.port && src.slug == slug && src.port == w.from.port
                    })
                });
            if !declared {
                return Err(fail(format!(
                    "the config wires {slug}.{fk} into managed block `{to_slug}` port \
                     {tk}, which `{to_slug}`'s declaration does not state — the next \
                     compile would tear that wire down. Add `{tk}: {slug}.{fk},` to the \
                     argument list of `{to_slug}` and rerun the adoption (the reference \
                     to `{slug}` is fine before it is declared: fragments validate as a \
                     whole after the append)",
                    fk = w.from.port,
                    tk = w.to.port,
                )));
            }
        } else {
            wire_items.push(Item::Wire(WireDecl {
                to: PortRef {
                    slug: to_slug,
                    port: w.to.port.clone(),
                },
                from: PortRef {
                    slug: slug.to_string(),
                    port: w.from.port.clone(),
                },
                comment: None,
            }));
        }
    }

    // All checks passed — build the items and extend the lock.
    let mut items = Vec::new();
    let mut new_slugs = Vec::new();
    for (i, fin) in &new_externs {
        let mut e = lift.extern_decls[i].clone();
        e.slug = fin.clone();
        items.push(Item::Extern(e));
        let obj = &lift.objects[*i];
        lock.externals.insert(
            fin.clone(),
            LockedExternal {
                uuid: obj.uuid.clone(),
                matched_by: match &lift.match_specs[i] {
                    MatchSpec::Uuid(_) => "uuid",
                    MatchSpec::IName(_) => "iname",
                    MatchSpec::Title(_) => "title",
                    MatchSpec::Mirrors(_) => "mirrors",
                }
                .to_string(),
                title_at_match: obj.title.clone(),
                iname_at_match: obj.iname.clone(),
            },
        );
        new_slugs.push(fin.clone());
    }
    let mut block = lift.block_decls[&ti].clone();
    block.slug = slug.to_string();
    block.title = o.title.clone().filter(|t| t != slug);
    for arg in &mut block.args {
        if let ArgItem::Binding(x) = arg
            && let BindingKind::Wire(r) = &mut x.kind
        {
            r.slug = rename[&r.slug].clone();
        }
    }
    items.push(Item::Block(block));
    items.extend(wire_items);

    lock.objects.insert(
        slug.to_string(),
        LockedObject {
            uuid: uuid.to_string(),
            block_type: o.block_type.clone(),
            ports: ports(el).into_iter().map(|p| (p.key, p.uuid)).collect(),
            layout: Some(Layout::of_element(el).expect("verified numeric")),
            superseded_layout: None,
            page_uuid: Some(lift.page_uuids[page].clone()),
            expr_owned: false,
        },
    );
    // The adopted-from config is the new drift baseline: nothing changed
    // semantically, the lock merely claims more of it.
    lock.target.config_version = doc.config_version();
    lock.target.source_config_sha256 = Some(sha256_hex(&doc.to_bytes()));
    lock.target.semantic_fingerprint = Some(crate::diff::semantic_fingerprint(doc));

    Ok(AdoptedBlock {
        items,
        page_title: lift.page_title(page).to_string(),
        new_externs: new_slugs,
    })
}

fn adopt_lift(doc: &LoxoneDoc) -> (Lift, Vec<String>) {
    let mut opts = DecompileOptions {
        scope: DecompileScope::ManagedOnly,
        ..DecompileOptions::default()
    };
    let mut lift = Lift::build(doc, &opts);

    // Verification pass: refused blocks are excluded and the lift rebuilt
    // without them (they may re-enter as externs). One pass suffices —
    // verification is element-local, so exclusions cannot invalidate other
    // blocks.
    let mut refused = Vec::new();
    for &i in &lift.managed {
        let o = &lift.objects[i];
        let el = doc.element_at(&o.path).expect("path from objects()");
        let why = verify_rebuildable(el, o).err().or_else(|| {
            lift.page_of(i)
                .is_none()
                .then(|| "it is not placed on a logic page".to_string())
        });
        if let Some(why) = why {
            refused.push(format!(
                "cannot adopt {} \"{}\" (uuid {}): {why}; the block stays unmanaged",
                o.block_type,
                o.title.as_deref().unwrap_or(""),
                o.uuid
            ));
            opts.exclude.insert(o.uuid.clone());
        }
    }
    if !opts.exclude.is_empty() {
        lift = Lift::build(doc, &opts);
    }
    (lift, refused)
}

fn adopt_lock(doc: &LoxoneDoc, lift: &Lift, refused: Vec<String>) -> (Lockfile, AdoptReport) {
    let mut lock = Lockfile::new();
    for &i in &lift.managed {
        let o = &lift.objects[i];
        let el = doc.element_at(&o.path).expect("path from objects()");
        let page = lift.page_of(i).expect("verified to be on a page");
        lock.objects.insert(
            lift.slug_of[&o.uuid].clone(),
            LockedObject {
                uuid: o.uuid.clone(),
                block_type: o.block_type.clone(),
                ports: ports(el).into_iter().map(|p| (p.key, p.uuid)).collect(),
                layout: Some(Layout::of_element(el).expect("verified numeric")),
                superseded_layout: None,
                page_uuid: Some(lift.page_uuids[page].clone()),
                expr_owned: false,
            },
        );
    }
    for &i in &lift.externs {
        let o = &lift.objects[i];
        lock.externals.insert(
            lift.slug_of[&o.uuid].clone(),
            LockedExternal {
                uuid: o.uuid.clone(),
                matched_by: match &lift.match_specs[&i] {
                    MatchSpec::Uuid(_) => "uuid",
                    MatchSpec::IName(_) => "iname",
                    MatchSpec::Title(_) => "title",
                    MatchSpec::Mirrors(_) => "mirrors",
                }
                .to_string(),
                title_at_match: o.title.clone(),
                iname_at_match: o.iname.clone(),
            },
        );
    }
    // Counters stay 0: the first compile absorbs the document's. The
    // serial stays unset for the same reason (it is a compile option).
    lock.target.config_version = doc.config_version();
    lock.target.source_config_sha256 = Some(sha256_hex(&doc.to_bytes()));
    lock.target.semantic_fingerprint = Some(crate::diff::semantic_fingerprint(doc));

    let d = lift.report();
    (
        lock,
        AdoptReport {
            blocks: d.managed,
            externs: d.externs,
            pages: d.pages,
            refused,
        },
    )
}

/// Element attributes the compiler's rebuild emits. `Cl`/`LtE`/`WF` are
/// carried forward verbatim from the base element (D19), as are the
/// [`GUI_OWNED_ATTRS`] checked alongside this list.
const KNOWN_ATTRS: &[&str] = &[
    "Type", "V", "U", "Title", "Px", "Py", "Px2", "Py2", "Cl", "Nio", "WF", "LtE",
];

/// `Err(reason)` unless rebuilding this block from its lifted declaration
/// reproduces everything on the element (modulo the documented
/// normalizations).
fn verify_rebuildable(el: &Element, o: &ObjectSummary) -> std::result::Result<(), String> {
    let attrs = attr_params(&o.block_type);
    for a in &el.attrs {
        let known = KNOWN_ATTRS.contains(&a.name.as_str())
            || GUI_OWNED_ATTRS.contains(&a.name.as_str())
            || attrs.contains(&a.name.as_str())
            // Validity state travels with the formula (observed:
            // Valid="false" on every Formula=); the rebuild re-emits it.
            || (a.name == "Valid" && o.block_type == "Formula");
        if !known {
            return Err(format!(
                "attribute `{}=\"{}\"` is not understood by the rebuild and would be lost",
                a.name, a.value
            ));
        }
    }
    if !VERIFIED_BLOCK_VERSIONS.contains(&el.attr("V").unwrap_or("")) {
        return Err(format!(
            "block version V=\"{}\" is not one the connector table is verified \
             for ({}) — connector indexes are verified per version, so this \
             needs a save-oracle run for your Loxone Config version first \
             (docs/connector-db.md, \"Admit a type yourself\")",
            el.attr("V").unwrap_or(""),
            VERIFIED_BLOCK_VERSIONS.join(", ")
        ));
    }
    if Layout::of_element(el).is_none() {
        return Err("its position (Px/Py/Px2/Py2) is not numeric".to_string());
    }

    for child in &el.children {
        let co = match child {
            Node::Element(c) if c.name == "Co" => c,
            // GUI-owned subtrees are carried forward wholesale (D19) —
            // their content needs no inspection.
            Node::Element(c) if GUI_OWNED_CHILDREN.contains(&c.name.as_str()) => continue,
            Node::Element(c) => {
                return Err(format!(
                    "child element <{}> is not understood by the rebuild",
                    c.name
                ));
            }
            Node::Text(t) if t.trim().is_empty() => continue,
            Node::Text(_) => return Err("unexpected text content".to_string()),
        };
        let key = co.attr_decoded("K").unwrap_or_default().into_owned();
        // An `Inv=`-carrying connector is GUI-owned (D20): the rebuild
        // re-emits the whole `<Co>` verbatim and the lift keeps its Def
        // and wires out of the source, so anything inside it is faithful
        // by construction.
        for a in &co.attrs {
            if !matches!(a.name.as_str(), "K" | "Nc" | "Def" | "U" | "Inv") {
                return Err(format!(
                    "connector `{key}`: attribute `{}` is not understood",
                    a.name
                ));
            }
        }
        let mut in_count: u64 = 0;
        for child in &co.children {
            match child {
                Node::Element(i) if i.name == "In" => {
                    // `FLG=` is Miniserver/app-created wire metadata; the
                    // oracle probe showed Loxone Config round-trips it
                    // verbatim and accepts its absence, so the rebuild
                    // carries it per (sink, source) pair (D19 residue).
                    if let Some(a) = i
                        .attrs
                        .iter()
                        .find(|a| !matches!(a.name.as_str(), "Input" | "FLG"))
                    {
                        return Err(format!(
                            "connector `{key}`: wire attribute `{}` is not understood",
                            a.name
                        ));
                    }
                    in_count += 1;
                }
                Node::Element(c) => {
                    return Err(format!(
                        "connector `{key}`: child element <{}> is not understood",
                        c.name
                    ));
                }
                Node::Text(t) if t.trim().is_empty() => {}
                Node::Text(_) => {
                    return Err(format!("connector `{key}`: unexpected text content"));
                }
            }
        }
        let nc: u64 = co.attr("Nc").unwrap_or("0").parse().unwrap_or(u64::MAX);
        if nc != in_count {
            return Err(format!(
                "connector `{key}`: Nc=\"{}\" does not match its {in_count} wire(s)",
                co.attr("Nc").unwrap_or("")
            ));
        }
    }

    // The connector SET must equal the spec's (order is cosmetic — the
    // port-UUID index tails prove the spec order is the creation order,
    // Loxone Config just serializes elements in GUI order sometimes).
    let specs = builtin(&o.block_type).expect("lift only manages builtin types");
    let spec_keys: BTreeSet<&str> = specs.iter().map(|s| s.key).collect();
    let real_keys: BTreeSet<String> = ports(el).into_iter().map(|p| p.key).collect();
    let real_keys_ref: BTreeSet<&str> = real_keys.iter().map(String::as_str).collect();
    if real_keys_ref != spec_keys {
        return Err(format!(
            "its connectors [{}] differ from the verified [{}]",
            real_keys_ref.iter().copied().collect::<Vec<_>>().join(", "),
            specs.iter().map(|s| s.key).collect::<Vec<_>>().join(", ")
        ));
    }
    if el.attr("Nio") != Some(specs.len().to_string().as_str()) {
        return Err(format!(
            "Nio=\"{}\" does not match the {} verified connectors",
            el.attr("Nio").unwrap_or(""),
            specs.len()
        ));
    }
    Ok(())
}
