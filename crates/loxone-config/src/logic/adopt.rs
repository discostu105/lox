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

use crate::connectors::{
    VERIFIED_BLOCK_VERSIONS, attr_param, builtin, carried_attrs, carried_children, container_type,
};
use crate::doc::{GUI_OWNED_ATTRS, GUI_OWNED_CHILDREN, LoxoneDoc, ObjectSummary, ports};
use crate::error::{Error, Result};
use crate::lock::{Layout, LockedExternal, LockedObject, Lockfile, sha256_hex};
use crate::logic::ast::{
    ArgItem, Binding, BindingKind, Item, MatchSpec, Module, PortRef, RULE_BLOCK_TYPE, WireDecl,
};
use crate::logic::decompile::{DecompileOptions, DecompileScope, Lift, RESERVED, owned_by_block};
use crate::logic::validate::validate_ports;
use crate::periphery::periphery;
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
    // exactly as `loxl compile --module <dir>` will.
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
    /// The block was an extern of the module and is managed now: every
    /// statement on it (its `extern` line, `<-` wires, port assignments,
    /// `.title` claim) moved into the declaration — strip them from the
    /// source with [`Module::strip_extern`]. Its `.room`, `.category` and
    /// `schedule` claims stay: they hold on the managed block (D63).
    pub promoted: Vec<String>,
    /// The block sits on a logic page (`page_title` names it); a virtual
    /// or Modbus device (D64) sits in the periphery tree instead, and
    /// `page_title` names its folder.
    pub on_page: bool,
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
    if o.block_type == RULE_BLOCK_TYPE {
        return adopt_rule(doc, o, &ident, slug, module, lock);
    }
    if owned_by_block(doc, o) {
        return Err(fail(format!(
            "cannot adopt {ident}: it belongs to the block it is nested in (an energy flow \
             monitor's own meter). Adopt that block instead."
        )));
    }
    if builtin(&o.block_type).is_none() {
        return Err(fail(format!(
            "cannot adopt {ident}: `{}` is not a type loxl can manage yet. Keep it as an extern.",
            o.block_type
        )));
    }
    if let Some((s, _)) = lock.objects.iter().find(|(_, lo)| lo.uuid == uuid) {
        return Err(fail(format!(
            "{ident} is already managed as `{s}`. There is nothing to adopt."
        )));
    }
    // Promotion: the object is an extern of the module. It keeps its
    // slug — every reference stays valid — and the module's statements on
    // it become the declaration's arguments.
    let promoted = lock
        .externals
        .iter()
        .find(|(_, le)| le.uuid == uuid)
        .map(|(s, _)| s.clone());
    if let Some(x) = &promoted {
        if x != slug {
            return Err(fail(format!(
                "{ident} is extern `{x}` of this module. Adopt it with `--as {x}`, and rename it afterwards with `loxl rename` if needed."
            )));
        }
        check_promotable(doc, x, uuid, raw, module, lock).map_err(fail)?;
    }
    if let Some(pin) = &lock.target.config_version
        && doc.config_version().as_deref() != Some(pin)
    {
        return Err(fail(format!(
            "Config saved by Loxone Config {}, but the lockfile expects {pin}. Run `loxl compile --accept-version` first.",
            doc.config_version().as_deref().unwrap_or("(none)")
        )));
    }
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
        .chain(lock.rules.keys().cloned())
        .collect();
    if promoted.is_none() && taken.contains(slug) {
        return Err(fail(format!(
            "slug `{slug}` is already taken in this module or lockfile. Pick another."
        )));
    }
    taken.insert(slug.to_string());
    // An object of the object tree (D54, D64): a virtual input, a device
    // with what hangs below it.
    if container_type(&o.block_type).is_some() || periphery(&o.block_type).is_some() {
        return super::adopt_tree::adopt_device(doc, o, slug, raw, module, lock, taken);
    }
    let el = doc.element_at(&o.path).expect("path from objects()");
    verify_rebuildable(el, o).map_err(|why| fail(format!("cannot adopt {ident}: {why}")))?;

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
            })
            // A rule (D60) the source declares already has its slug.
            .or_else(|| {
                lock.rules
                    .iter()
                    .find(|(_, ru)| *ru == u)
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
                     {tk}, which the declaration of `{to_slug}` does not state, so the next \
                     compile would remove it. Add `{tk}: {slug}.{fk},` to the \
                     argument list of `{to_slug}` and run the adoption again.",
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
    if promoted.is_some() {
        // What the config holds on the block is the GUI's part plus the
        // compiler's extern writes. The compiler's part is dropped from the
        // lift and re-stated from the source statements that wrote it —
        // expressions stay expressions, constants stay constants.
        let el_ports = ports(el);
        let key_of = |port_uuid: &str| {
            el_ports
                .iter()
                .find(|p| p.uuid == port_uuid)
                .map(|p| p.key.clone())
        };
        let port_owner: BTreeMap<String, (String, String)> = objects
            .iter()
            .flat_map(|ob| {
                let el = doc.element_at(&ob.path).expect("path from objects()");
                ports(el)
                    .into_iter()
                    .map(move |p| (p.uuid, (ob.uuid.clone(), p.key)))
            })
            .collect();
        let mut owned_wires: BTreeSet<(String, String, String)> = BTreeSet::new();
        for w in &lock.extern_wires {
            if let (Some(to_key), Some((from_obj, from_key))) =
                (key_of(&w.to), port_owner.get(&w.from))
            {
                owned_wires.insert((to_key, from_obj.clone(), from_key.clone()));
            }
        }
        let owned_sets: BTreeSet<String> = lock
            .set_originals
            .keys()
            .filter_map(|u| key_of(u))
            .collect();
        let uuid_of_lift: BTreeMap<&str, &str> = lift
            .slug_of
            .iter()
            .map(|(u, s)| (s.as_str(), u.as_str()))
            .collect();
        block.args.retain(|a| match a {
            ArgItem::Binding(b) => match &b.kind {
                BindingKind::Wire(r) => {
                    let from = uuid_of_lift.get(r.slug.as_str()).copied().unwrap_or("");
                    !owned_wires.contains(&(b.port.clone(), from.to_string(), r.port.clone()))
                }
                BindingKind::Param(_) => !owned_sets.contains(&b.port),
                BindingKind::Expr(_) => true,
            },
            _ => true,
        });
        for item in &raw.items {
            let binding = match item {
                Item::Wire(w) if w.to.slug == slug => Binding {
                    port: w.to.port.clone(),
                    kind: BindingKind::Wire(w.from.clone()),
                    comment: w.comment.clone(),
                },
                Item::ExprWire(w) if w.to.slug == slug => Binding {
                    port: w.to.port.clone(),
                    kind: BindingKind::Expr(w.expr.clone()),
                    comment: w.comment.clone(),
                },
                Item::Set(st) if st.target.slug == slug => Binding {
                    port: st.target.port.clone(),
                    kind: BindingKind::Param(st.value.clone()),
                    comment: st.comment.clone(),
                },
                _ => continue,
            };
            block.args.push(ArgItem::Binding(binding));
        }
    }
    for arg in &mut block.args {
        if let ArgItem::Binding(x) = arg
            && let BindingKind::Wire(r) = &mut x.kind
            && let Some(fin) = rename.get(&r.slug)
        {
            r.slug = fin.clone();
        }
    }
    items.push(Item::Block(block));
    items.extend(wire_items);

    if promoted.is_some() {
        // The extern's records end here: its wires and parameters are the
        // managed block's arguments now, rebuilt from source on every
        // compile, and its title is the block's label.
        let own: BTreeSet<String> = ports(el).into_iter().map(|p| p.uuid).collect();
        lock.externals.remove(slug);
        lock.extern_wires.retain(|w| !own.contains(&w.to));
        lock.set_originals.retain(|u, _| !own.contains(u));
        lock.attr_claims.remove(uuid);
    }
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
            states: Vec::new(),
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
        promoted: promoted.into_iter().collect(),
        on_page: true,
    })
}

/// Why extern `x` (object `uuid`) cannot be promoted to a managed block,
/// if it cannot: a statement the declaration could not hold, or a
/// withdrawal still on its way to the Miniserver.
pub(super) fn check_promotable(
    doc: &LoxoneDoc,
    x: &str,
    uuid: &str,
    raw: &Module,
    expanded: &Module,
    lock: &Lockfile,
) -> std::result::Result<(), String> {
    // Statements a template or an expression writes onto the extern live
    // in the expanded module only — the declaration cannot take them over.
    let count = |m: &Module| {
        m.items
            .iter()
            .filter(|i| matches!(i, Item::Wire(w) if w.to.slug == x))
            .count()
            + m.expr_wires().filter(|w| w.to.slug == x).count()
            + m.sets().filter(|s| s.target.slug == x).count()
    };
    // An expression desugars into one wire onto the sink, so the counts
    // agree unless a template instance adds statements.
    if count(expanded) != count(raw) {
        return Err(format!(
            "a template instance sets `{x}`. Move those statements out of the template, then adopt it."
        ));
    }
    // `.room`/`.category` (D63) and a DayTimer's `schedule` claim hold on
    // a managed block as on an extern, and stay as they are; tiles belong
    // to a Touch Pure Flex, a device — never a page block.
    if raw
        .items
        .iter()
        .any(|i| matches!(i, Item::TilesClaim(c) if c.slug == x))
    {
        return Err(format!(
            "`{x}` carries a tiles claim, which stays on externs"
        ));
    }
    let el = doc
        .objects()
        .into_iter()
        .find(|o| o.uuid == uuid)
        .and_then(|o| doc.element_at(&o.path).cloned())
        .ok_or_else(|| format!("no object with uuid `{uuid}`"))?;
    let own: BTreeSet<String> = ports(&el).into_iter().map(|p| p.uuid).collect();
    if lock.removed_wires.iter().any(|w| own.contains(&w.to))
        || lock.removed_sets.keys().any(|u| own.contains(u))
        || lock.removed_attrs.contains_key(uuid)
    {
        return Err(format!(
            "a removed statement on `{x}` has not reached the Miniserver yet. Push first, then adopt it from a fresh download."
        ));
    }
    Ok(())
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
                states: Vec::new(),
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
    for a in &el.attrs {
        let known = KNOWN_ATTRS.contains(&a.name.as_str())
            || GUI_OWNED_ATTRS.contains(&a.name.as_str())
            || attr_param(&o.block_type, &a.name).is_some()
            || carried_attrs(&o.block_type).contains(&a.name.as_str())
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
            "block version V=\"{}\" is not supported (supported: {}). Keep the block as an extern.",
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
            Node::Element(c)
                if GUI_OWNED_CHILDREN.contains(&c.name.as_str())
                    || carried_children(&o.block_type).contains(&c.name.as_str()) =>
            {
                continue;
            }
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

/// [`adopt_one`] for an Automatic Designer rule (#192): the rule becomes
/// `rule <slug> "Name"` (D60), pinned in the lock's `rules`, its payload
/// lifted as the documented shapes (D57). An extern rule of the module is
/// promoted: its `extern` line and its unnamed `rule` leave the source.
/// The rule's action targets must already be in the lock.
fn adopt_rule(
    doc: &LoxoneDoc,
    o: &ObjectSummary,
    ident: &str,
    slug: &str,
    module: &Module,
    lock: &mut Lockfile,
) -> Result<AdoptedBlock> {
    let fail = |m: String| Error::Compile(m);
    if let Some((s, _)) = lock.rules.iter().find(|(_, u)| **u == o.uuid) {
        return Err(fail(format!(
            "{ident} is already rule `{s}`. There is nothing to adopt."
        )));
    }
    let Some(title) = o.title.clone().filter(|t| !t.is_empty()) else {
        return Err(fail(format!(
            "cannot adopt {ident}: the rule has no name. Name it in the app first."
        )));
    };
    let objects = doc.objects();
    let same_name = objects
        .iter()
        .filter(|r| r.block_type == RULE_BLOCK_TYPE && r.title.as_deref() == Some(&title))
        .count();
    if same_name > 1 {
        return Err(fail(format!(
            "cannot adopt {ident}: the config has {same_name} rules of this name. Rename all but one in the app."
        )));
    }
    let promoted = lock
        .externals
        .iter()
        .find(|(_, le)| le.uuid == o.uuid)
        .map(|(s, _)| s.clone());
    match &promoted {
        Some(x) if x != slug => {
            return Err(fail(format!(
                "{ident} is extern `{x}` of this module. Adopt it with `--as {x}`."
            )));
        }
        Some(_) => {}
        None => {
            let valid = matches!(slug.as_bytes().first(), Some(b) if b.is_ascii_lowercase())
                && slug
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                && !RESERVED.contains(&slug);
            let taken = lock.objects.contains_key(slug)
                || lock.externals.contains_key(slug)
                || lock.rules.contains_key(slug)
                || module.externs().any(|e| e.slug == slug)
                || module.blocks().any(|b| b.slug == slug);
            if !valid || taken {
                return Err(fail(format!(
                    "`{slug}` is taken or not a valid slug ([a-z][a-z0-9_]*). Pick another."
                )));
            }
        }
    }
    let apc = doc
        .element_at(&o.path)
        .and_then(|el| el.attr_decoded("APC"))
        .map(|a| a.into_owned())
        .unwrap_or_default();
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
    let mut decl = super::rule::lift(slug, &apc, doc, &slug_of).map_err(|why| {
        fail(format!(
            "cannot adopt {ident}: {why} (the blocks it names must be managed or extern)"
        ))
    })?;
    decl.title = Some(title);
    let page_title = (1..o.path.len())
        .rev()
        .find_map(|n| {
            let el = doc.element_at(&o.path[..n])?;
            (el.name == "C" && el.attr("Type") == Some("Page"))
                .then(|| el.attr_decoded("Title").map(|t| t.into_owned()))
                .flatten()
        })
        .ok_or_else(|| fail(format!("cannot adopt {ident}: it is not on a logic page")))?;
    if let Some(x) = &promoted {
        lock.externals.remove(x);
    }
    lock.rules.insert(slug.to_string(), o.uuid.clone());
    Ok(AdoptedBlock {
        items: vec![Item::Rule(decl)],
        page_title,
        new_externs: Vec::new(),
        promoted: promoted.into_iter().collect(),
        on_page: true,
    })
}
