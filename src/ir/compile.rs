//! The compiler: base config + IR module + lockfile → new config.
//!
//! The compiler owns exactly three kinds of edits and touches nothing else
//! in the (losslessly parsed) base document:
//!
//! 1. **Managed blocks** — `slug = Type(…)` declarations are (re)built from
//!    scratch on every compile, with identity pinned by the lockfile.
//! 2. **Extern wires** — wires whose sink is an extern port
//!    (`target.Port <- source.Port`) are added to the extern's `<Co>`; the
//!    lockfile records them so removing the statement removes the `<In>`
//!    again without disturbing wires drawn in Loxone Config.
//! 3. **Extern sets** — `target.Port = value` on an extern port rewrites its
//!    `Def=`; the pre-set value is remembered and restored when the
//!    statement disappears.
//!
//! Everything is deterministic: same base + module + lock + options → the
//! same output bytes. New UUIDs come from [`crate::uuid::Minter`] (no clock,
//! no RNG) and are recorded in the lock immediately, so later compiles reuse
//! them.
//!
//! v0 limitations (documented, checked, erroring — never silent):
//! - Only block types in [`crate::connectors::builtin`] can be created.
//! - Wiring or assigning an extern port requires that port's `<Co>` to exist
//!   in the base config (Loxone omits nothing we have observed, but an
//!   unverified type could; we refuse to invent port UUIDs for them).
//! - `And`/`Or` are fixed two-input blocks: wiring `I3`+ is an error. The
//!   Wine oracle showed Loxone Config 17 silently deletes off-descriptor
//!   connectors (and their wires) on save, so minting them would lose logic.

use crate::connectors::{PortDir, attr_params, builtin, ref_link_type};
use crate::doc::{Counters, GUI_OWNED_ATTRS, GUI_OWNED_CHILDREN, LoxoneDoc, ports};
use crate::error::{Error, Result};
use crate::ir::ast::{Item, MatchSpec, Module, Value};
use crate::ir::validate::{is_ref_type, suggest, validate_ports};
use crate::lock::{Layout, LockedExternal, LockedObject, LockedWire, Lockfile, sha256_hex};
use crate::uuid::{Minter, entity_for_slug};
use crate::xml::Element;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct CompileOptions {
    /// 6-byte machine id stamped into minted object UUIDs — conventionally
    /// the Miniserver serial ([`crate::uuid::parse_serial`]).
    pub machine: [u8; 6],
    /// Creation time recorded in minted UUIDs (segment 1). Caller-provided
    /// so compiles are reproducible.
    pub mint_time_unix: i64,
    /// Title of the `<C Type="Page">` to place managed blocks on;
    /// `None` = the document's first page.
    pub page_title: Option<String>,
    /// A managed block present in the lock but missing from source is a
    /// hard error by default (protection against accidental deletion).
    /// The preferred authorization is a `removed <slug>` statement in the
    /// module — scoped and reviewable. `allow_removals` is the blunt
    /// alternative: *every* vanished block is deleted from the config.
    /// Either way the block leaves a lockfile tombstone (D31) that keeps
    /// deleting it from bases predating the deployment. The third option,
    /// [`Lockfile::remove_object`], *forgets* the block without a
    /// tombstone, leaving its XML in the config as an unmanaged orphan.
    pub allow_removals: bool,
    /// Explicit acceptance of a base whose `ConfigVersion` differs from
    /// the lock's pin (i.e. a new Loxone Config release). Must equal the
    /// base's version exactly; `None` leaves the mismatch a hard error.
    /// Qualify the release first — one oracle open+save run
    /// (docs/oracle-wine.md) — then accept.
    pub accept_version: Option<String>,
}

/// GUI-owned residue of an existing managed block (D19), harvested at
/// teardown and re-emitted verbatim by the rebuild: the GUI owns this
/// content, so it is read from the base on every compile — never stored —
/// and later GUI edits to it are carried, not reverted.
struct Residue {
    /// `Cl`/`LtE`/`WF` and [`GUI_OWNED_ATTRS`], raw values in element order.
    attrs: Vec<(String, String)>,
    /// [`GUI_OWNED_CHILDREN`] subtrees in element order.
    children: Vec<Element>,
    /// GUI-owned connectors (design decision D20): the whole `<Co>` of
    /// every `Inv=`-carrying connector, keyed by `K`, re-emitted verbatim
    /// at its spec position — inversion flag, `Def=`, and wires included.
    /// The IR is refused from wiring or setting these (see [`add_wire`] /
    /// [`apply_def`] and the param guard in the build loop), so carrying
    /// them can never contradict what the source expresses.
    gui_ports: BTreeMap<String, Element>,
    /// `FLG=` wire flags, keyed by (sink port UUID, source port UUID).
    /// Miniserver/app-created wire metadata: the oracle probe (2026-08-25)
    /// showed Loxone Config round-trips the flag verbatim, never
    /// regenerates it, and accepts its absence — so it is carried exactly
    /// like the element residue. A wire whose source changed no longer
    /// matches its key and is emitted plain, which the GUI accepts.
    wire_flags: BTreeMap<(String, String), String>,
}

fn harvest_residue(el: &Element) -> Residue {
    Residue {
        attrs: el
            .attrs
            .iter()
            .filter(|a| {
                matches!(a.name.as_str(), "Cl" | "LtE" | "WF")
                    || GUI_OWNED_ATTRS.contains(&a.name.as_str())
            })
            .map(|a| (a.name.clone(), a.value.clone()))
            .collect(),
        children: el
            .child_elements()
            .filter(|c| GUI_OWNED_CHILDREN.contains(&c.name.as_str()))
            .cloned()
            .collect(),
        wire_flags: el
            .child_elements()
            .filter(|c| c.name == "Co")
            .filter_map(|co| Some((co.attr("U")?.to_string(), co)))
            .flat_map(|(port, co)| {
                co.child_elements()
                    .filter(|i| i.name == "In")
                    .filter_map(move |i| {
                        Some((
                            (port.clone(), i.attr("Input")?.to_string()),
                            i.attr("FLG")?.to_string(),
                        ))
                    })
            })
            .collect(),
        gui_ports: el
            .child_elements()
            .filter(|c| c.name == "Co" && c.attr("Inv").is_some())
            .filter_map(|co| Some((co.attr_decoded("K")?.into_owned(), co.clone())))
            .collect(),
    }
}

/// Managed-block geometry, matching what Loxone Config draws:
/// blocks are 1344 wide, 504 tall plus 192 per port beyond two,
/// stacked with a 192 gap on a fixed column.
const BLOCK_X: i64 = 7392;
const BLOCK_W: i64 = 1344;
const BLOCK_H_BASE: i64 = 504;
const PORT_H: i64 = 192;
const GAP: i64 = 192;
/// `InputRef`/`OutputRef` tag width — the GUI redraws refs 2112 wide and
/// one port-row tall (oracle session 11).
const REF_W: i64 = 2112;

pub fn compile(
    base: &LoxoneDoc,
    module: &Module,
    lock: &mut Lockfile,
    opts: &CompileOptions,
) -> Result<LoxoneDoc> {
    // Templates expand first (D23), then expressions desugar (D24):
    // everything below — the lockfile, minting — sees only plain
    // statements. Validation runs before desugaring so reference errors
    // name what the user wrote, and again after as a self-check on the
    // generated blocks.
    let module = &module.expand()?;
    module.validate()?;
    let (module, desugar_info) = module.desugar()?;
    let module = &module;
    module.validate()?;
    validate_ports(module)?;

    // --- ConfigVersion pin: a base written by a different Loxone release
    //     than the one this project is qualified for is refused until the
    //     new release is accepted explicitly.
    let base_version = base.config_version();
    if let Some(accept) = &opts.accept_version {
        if base_version.as_ref() != Some(accept) {
            return Err(Error::Compile(format!(
                "--accept-version {accept} does not match the base's ConfigVersion {}",
                base_version.as_deref().unwrap_or("<absent>")
            )));
        }
    } else if let Some(pinned) = &lock.target.config_version
        && base_version.as_ref() != Some(pinned)
    {
        return Err(Error::Compile(format!(
            "base ConfigVersion {} differs from the project pin {pinned} — a new \
             Loxone release must be qualified first (one oracle open+save run, \
             docs/oracle-wine.md), then accepted with --accept-version {}",
            base_version.as_deref().unwrap_or("<absent>"),
            base_version.as_deref().unwrap_or("<version>")
        )));
    }
    // --- Identity: everything below keys on UUIDs, so a base whose UUIDs
    //     repeat would have each lookup silently pick one twin.
    base.require_unique_uuids("base")?;

    // --- Apply `moved` statements to the lock (identity surgery, in
    //     source). Idempotent: once the new slug carries the entry, the
    //     statement is done.
    for mv in module.moved() {
        match (
            lock.objects.contains_key(&mv.from),
            lock.objects.contains_key(&mv.to),
        ) {
            (true, false) => lock.rename_object(&mv.from, &mv.to)?,
            (true, true) => {
                return Err(Error::Compile(format!(
                    "moved `{from} -> {to}`: both slugs are in the lockfile — \
                     `{to}` already has its own identity; remove one entry first \
                     (Lockfile::remove_object)",
                    from = mv.from,
                    to = mv.to
                )));
            }
            (false, true) => {} // already applied — the statement is done
            (false, false) => {
                return Err(Error::Compile(format!(
                    "moved `{} -> {}`: neither slug is in the lockfile — nothing to rename",
                    mv.from, mv.to
                )));
            }
        }
    }

    // --- Sync check: the lock must not know managed blocks the source
    //     lost, unless the removal is authorized (per-slug via `removed`,
    //     globally via allow_removals, or implicitly for expression-owned
    //     blocks — editing the expression IS the removal authorization).
    let src_slugs: BTreeSet<String> = module.blocks().map(|b| b.slug.clone()).collect();
    let removed_slugs: BTreeSet<&str> = module.removed().map(|r| r.slug.as_str()).collect();
    // A `removed` statement must name a block the lock still knows — as a
    // managed object (the removal happens now) or as a tombstone (D31, the
    // removal is compiled but not yet seen deployed). Once the tombstone
    // clears, a lingering statement is a hard error: the removal is done,
    // delete the line (or it never named anything — a typo).
    for r in module.removed() {
        if !lock.objects.contains_key(&r.slug) && !lock.removed.values().any(|t| t.slug == r.slug) {
            return Err(Error::Compile(format!(
                "`removed {slug}`: no managed block or pending removal with \
                 this slug in the lockfile — the removal is complete (delete \
                 the `removed` line) or the slug never existed",
                slug = r.slug
            )));
        }
    }
    if !opts.allow_removals
        && let Some(slug) = lock.objects.iter().find_map(|(s, o)| {
            (!src_slugs.contains(s) && !removed_slugs.contains(s.as_str()) && !o.expr_owned)
                .then_some(s)
        })
    {
        return Err(Error::Compile(format!(
            "managed block `{slug}` is in the lockfile but not in the source; \
             if the removal is intended, add `removed {slug}` to the module \
             (or compile with allow_removals); use `moved {slug} -> <new_slug>` \
             for a rename, or Lockfile::remove_object(\"{slug}\") to orphan it"
        )));
    }

    let mut doc = base.clone();
    lock.absorb_counters(doc.counters());

    // --- Resolve externs (against the untouched base).
    let extern_uuid = resolve_externs(&doc, module, lock)?;
    // XML type and title per object, for `mirrors:` targets (D33) — taken
    // before teardown so it covers everything the base knows.
    let type_by_uuid: BTreeMap<String, (String, Option<String>)> = doc
        .objects()
        .iter()
        .map(|o| (o.uuid.clone(), (o.block_type.clone(), o.title.clone())))
        .collect();

    // Every wire the base carried, pre-teardown: the D34 routing pin.
    // It remembers which mirror a consumer wire went through — the lock's
    // extern_wires can't, since wires into managed sinks vanish with their
    // block and are never recorded there.
    let base_wires: BTreeSet<(String, String)> = doc
        .wires()
        .iter()
        .map(|w| (w.from_port.clone(), w.to_port.clone()))
        .collect();

    // --- Tear down our previous output: managed objects, extern wires,
    //     extern sets. What remains is exactly the Loxone-Config-owned state.
    //     Each removed element hands back its GUI-owned residue (D19) so
    //     the rebuild carries it forward verbatim.
    let mut residue: BTreeMap<String, Residue> = BTreeMap::new();
    for obj in lock.objects.values() {
        if let Some(el) = doc.remove_by_uuid(&obj.uuid) {
            residue.insert(obj.uuid.clone(), harvest_residue(&el));
        }
    }
    // Tombstone sweep (D31): a pending removal whose object still exists
    // in this base is deleted again (the base predates the deployment of
    // the removal); one the base no longer contains has reached the
    // Miniserver — its tombstone is done.
    lock.removed
        .retain(|uuid, _| doc.remove_by_uuid(uuid).is_some());
    // Vanished slugs (authorized via `removed`, allow_removals, or
    // expression ownership) are deleted from this output and leave a
    // tombstone so every later compile against an older base deletes them
    // too instead of passing them through unmanaged. Only blocks this base
    // actually contained (their teardown harvested residue) are
    // tombstoned — one minted and dropped between deployments was never
    // in any config a tombstone could still meet.
    for (slug, o) in &lock.objects {
        if !src_slugs.contains(slug) && residue.contains_key(&o.uuid) {
            lock.removed.insert(
                o.uuid.clone(),
                crate::lock::Tombstone {
                    slug: slug.clone(),
                    block_type: o.block_type.clone(),
                },
            );
        }
    }
    lock.objects.retain(|slug, _| src_slugs.contains(slug));
    // Withdrawn-wire sweep (D31): same lifecycle as object tombstones —
    // still in this base → delete again and keep; gone → retired.
    lock.removed_wires
        .retain(|w| remove_wire(&mut doc, &w.from, &w.to));
    // Withdrawn-set sweep (D31): a port still carrying the value we wrote
    // predates the deployment — restore the original again. A port already
    // showing the original is deployed; any third value means another
    // writer took the port over (drift will surface it). Both retire.
    lock.removed_sets.retain(|port_uuid, rs| {
        let def = current_def(&mut doc, port_uuid);
        if def == rs.original {
            return false;
        }
        if def.as_deref() == Some(rs.written.as_str()) {
            restore_def(&mut doc, port_uuid, rs.original.as_deref());
            return true;
        }
        false
    });
    let old_wires = std::mem::take(&mut lock.extern_wires);
    // Remember which of our wires this base actually carried — only those
    // can leave a tombstone if they vanish from source (the rebuild below
    // re-records the ones that stay).
    let mut wires_present: BTreeSet<LockedWire> = BTreeSet::new();
    for w in &old_wires {
        if remove_wire(&mut doc, &w.from, &w.to) {
            wires_present.insert(w.clone());
        }
    }
    let old_originals = std::mem::take(&mut lock.set_originals);
    // Same for Def writes: capture what each port carried before the
    // restore — the "written" marker of a potential tombstone.
    let mut defs_written: BTreeMap<String, String> = BTreeMap::new();
    for (port_uuid, original) in &old_originals {
        if let Some(v) = current_def(&mut doc, port_uuid) {
            defs_written.insert(port_uuid.clone(), v);
        }
        restore_def(&mut doc, port_uuid, original.as_deref());
    }

    // --- Plan managed blocks: the builtin port list is the full port list
    //     and pinned identity for every one of them. Each block is pinned
    //     to a page: adopted blocks to the page they were drawn on, new
    //     blocks to the options' page (resolved lazily so a block-free
    //     module compiles against a page-less config).
    let default_page_uuid: Option<String> =
        doc.page_path(opts.page_title.as_deref()).and_then(|p| {
            doc.element_at(&p)
                .and_then(|el| el.attr("U"))
                .map(String::from)
        });
    // `page "Title"` statements govern the blocks that follow them (D28).
    // The statement is authoritative: the named page must exist in the
    // base, and a governed block whose pin no longer matches a page with
    // that title moves. Placement is positional, so synthetic expression
    // blocks land on the page their expression is written under.
    let mut declared_page: BTreeMap<&str, &str> = BTreeMap::new();
    {
        let mut current: Option<&str> = None;
        for item in &module.items {
            match item {
                Item::Page(p) => current = Some(p.title.as_str()),
                Item::Block(b) => {
                    if let Some(t) = current {
                        declared_page.insert(b.slug.as_str(), t);
                    }
                }
                _ => {}
            }
        }
    }
    // Title → page UUIDs in document order. Titles need not be unique; a
    // kept pin only has to match one of them.
    let mut pages_by_title: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for o in doc.objects() {
        if o.block_type == "Page" {
            pages_by_title
                .entry(o.title.clone().unwrap_or_default())
                .or_default()
                .push(o.uuid);
        }
    }
    let mut minter = Minter::new(opts.machine, opts.mint_time_unix);
    // Never reuse a (time, sequence) pair a previous compile session
    // handed out: seed past the recorded high-water mark and every locked
    // UUID (older locks have no mark). With a fixed mint time the bare
    // per-run counter would mint a later-added block into a collision.
    minter.seed(lock.counters.next_mint);
    for o in lock.objects.values() {
        minter.avoid(&o.uuid);
        for p in o.ports.values() {
            minter.avoid(p);
        }
    }
    let mint_base = minter.counter();
    let mut managed: BTreeMap<String, PlannedBlock> = BTreeMap::new();
    let mut py_cursor = next_free_py(lock);
    for block in module.blocks() {
        // Type, port names, and directions were checked by validate_ports.
        let specs = builtin(&block.block_type).expect("validate_ports admitted the type");
        let keys: Vec<String> = specs.iter().map(|s| s.key.to_string()).collect();

        // Pin identity: reuse the lock entry, minting only what is missing.
        // For a brand-new block, ports are minted before the object —
        // matching the counter order observed in Loxone Config output.
        let entity = entity_for_slug(&block.slug);
        let entry = lock
            .objects
            .entry(block.slug.clone())
            .or_insert_with(|| LockedObject {
                uuid: String::new(),
                block_type: block.block_type.clone(),
                ports: BTreeMap::new(),
                layout: None,
                page_uuid: None,
                expr_owned: false,
            });
        // Ownership follows the current source: a slug the desugarer
        // produced is expression-owned; one it no longer produces (but a
        // hand now declares) is hand-owned again.
        entry.expr_owned = desugar_info.synthetic.contains(&block.slug);
        if entry.uuid.is_empty() {
            entry.block_type = block.block_type.clone();
        } else if entry.block_type != block.block_type {
            return Err(Error::Compile(format!(
                "block `{}` changed type from `{}` to `{}`; remove it from the lock first \
                 (Lockfile::remove_object) to accept new identity",
                block.slug, entry.block_type, block.block_type
            )));
        }
        for (index, key) in keys.iter().enumerate() {
            entry
                .ports
                .entry(key.clone())
                .or_insert_with(|| minter.mint_port(index as u8, entity).to_string());
        }
        if entry.uuid.is_empty() {
            entry.uuid = minter.mint_object().to_string();
            lock.counters.next_obj += 1;
        }
        let page_uuid = match declared_page.get(block.slug.as_str()) {
            // Governed by a `page` statement (D28): keep a pin that still
            // matches a page with the declared title, otherwise (re)pin to
            // the first matching page in document order.
            Some(&title) => {
                let candidates = pages_by_title.get(title).map_or(&[][..], Vec::as_slice);
                match &entry.page_uuid {
                    Some(u) if candidates.contains(u) => u.clone(),
                    _ => {
                        let u = candidates.first().cloned().ok_or_else(|| {
                            Error::Compile(format!(
                                "block `{}` is placed with `page \"{title}\"`, but the \
                                 base config has no <C Type=\"Page\"> titled `{title}`",
                                block.slug
                            ))
                        })?;
                        entry.page_uuid = Some(u.clone());
                        u
                    }
                }
            }
            None => match &entry.page_uuid {
                Some(u) => u.clone(),
                None => {
                    let u = default_page_uuid.clone().ok_or_else(|| {
                        Error::Compile(match &opts.page_title {
                            Some(t) => {
                                format!("no <C Type=\"Page\"> titled `{t}` in the base config")
                            }
                            None => "the base config has no <C Type=\"Page\"> to place blocks on"
                                .to_string(),
                        })
                    })?;
                    entry.page_uuid = Some(u.clone());
                    u
                }
            },
        };
        // Refs draw as a flat tag, not a block — the GUI's own footprint
        // (oracle session 11: a save corrects anything else to this).
        let (width, height) = if is_ref_type(&block.block_type) {
            (REF_W, PORT_H)
        } else {
            (
                BLOCK_W,
                BLOCK_H_BASE + PORT_H * (keys.len() as i64 - 2).max(0),
            )
        };
        let layout = *entry.layout.get_or_insert_with(|| {
            let l = Layout {
                px: BLOCK_X,
                py: py_cursor,
                px2: BLOCK_X + width,
                py2: py_cursor + height,
            };
            py_cursor = l.py2 + GAP;
            l
        });
        managed.insert(
            block.slug.clone(),
            PlannedBlock {
                uuid: entry.uuid.clone(),
                block_type: block.block_type.clone(),
                keys,
                ports: entry.ports.clone(),
                layout,
                page_uuid,
            },
        );
    }

    // Record the mint high-water mark — only when something was minted, so
    // a mint-free compile leaves the lock byte-identical.
    if minter.counter() != mint_base {
        lock.counters.next_mint = minter.counter();
    }

    // --- Def values on managed ports come from the blocks' parameter
    //     bindings (port assignment statements are for extern ports only —
    //     enforced by validate). `let` references resolve to their literal
    //     here.
    let resolve = |v: &Value| -> Result<String> { Ok(module.resolve_value(v)?.to_string()) };
    let mut managed_defs: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut managed_attrs: BTreeMap<(String, String), String> = BTreeMap::new();
    for block in module.blocks() {
        let attrs = attr_params(&block.block_type);
        for (k, v) in block.params() {
            // `mirrors:` is an identity binding, not a value (D33) —
            // resolved separately below.
            if is_ref_type(&block.block_type) && k == "mirrors" {
                continue;
            }
            let target = if attrs.contains(&k) {
                &mut managed_attrs // element attribute, not a connector Def
            } else {
                &mut managed_defs
            };
            target.insert((block.slug.clone(), k.to_string()), resolve(v)?);
        }
    }

    // --- `mirrors:` on minted refs (D33): resolve the mirrored object and
    //     precompute the identity attributes its element carries (`Ref=`,
    //     `LinkRefType=`, `Analog=`). The target is any managed block of
    //     this module (a block minted this very compile included — the ref
    //     is ours to create) or any resolved extern; the target's XML type
    //     picks the GUI's type-registry code.
    struct Mirror {
        ref_uuid: String,
        link_type: u32,
        analog: bool,
        /// The GUI derives a ref's `Title=` from its target (oracle
        /// session 11: a save rewrites whatever else stands there) — the
        /// compiler emits the derived title so the file converges.
        title: String,
    }
    let mut mirror_of: BTreeMap<String, Mirror> = BTreeMap::new();
    for block in module.blocks() {
        if !is_ref_type(&block.block_type) {
            continue;
        }
        let target = block
            .params()
            .find_map(|(k, v)| match (k, v) {
                ("mirrors", Value::Ref(t)) => Some(t.as_str()),
                _ => None,
            })
            .expect("validate_ports requires mirrors: on ref blocks");
        let (uuid, target_type, target_title) = if let Some(plan) = managed.get(target) {
            let title = module
                .blocks()
                .find(|b| b.slug == target)
                .and_then(|b| b.title.clone())
                .unwrap_or_else(|| target.to_string());
            (plan.uuid.clone(), plan.block_type.clone(), title)
        } else if let Some(u) = extern_uuid.get(target) {
            let (t, title) = type_by_uuid.get(u).cloned().ok_or_else(|| {
                Error::Compile(format!(
                    "block `{}` mirrors extern `{target}`, whose resolved object \
                     {u} is not in the base config",
                    block.slug
                ))
            })?;
            (u.clone(), t, title.unwrap_or_else(|| target.to_string()))
        } else {
            return Err(Error::Compile(format!(
                "block `{}` mirrors `{target}`, but `{target}` is neither a \
                 managed block nor an extern of this module",
                block.slug
            )));
        };
        let (code, analog) = ref_link_type(&target_type).ok_or_else(|| {
            Error::Compile(format!(
                "block `{}` mirrors `{target}` of type `{target_type}`, which has \
                 no verified `LinkRefType=` code yet — extend \
                 connectors::ref_link_type from corpus evidence before minting \
                 a ref of it",
                block.slug
            ))
        })?;
        mirror_of.insert(
            block.slug.clone(),
            Mirror {
                ref_uuid: uuid,
                link_type: code,
                analog,
                title: target_title,
            },
        );
    }

    // --- Build the managed <C> elements and append each to its pinned
    //     page. The index is taken once after teardown; appending children
    //     never shifts the recorded page paths.
    let page_index = doc.index();
    for block in module.blocks() {
        let plan = &managed[&block.slug];
        let mut el = Element::new("C");
        el.set_attr("Type", &block.block_type);
        el.set_attr("V", "175");
        el.set_attr("U", &plan.uuid);
        // A ref block's title is GUI-derived from its mirror target
        // (oracle session 11) — emit the derived value so a save is a
        // no-op; validate refuses an explicit label on ref blocks.
        match mirror_of.get(&block.slug) {
            Some(m) => el.set_attr("Title", &m.title),
            None => el.set_attr("Title", block.title.as_deref().unwrap_or(&block.slug)),
        }
        el.set_attr("Px", &plan.layout.px.to_string());
        el.set_attr("Py", &plan.layout.py.to_string());
        el.set_attr("Px2", &plan.layout.px2.to_string());
        el.set_attr("Py2", &plan.layout.py2.to_string());
        // D19: a block that existed in the base re-emits its GUI-owned
        // display state verbatim — including *absence* (some types lose
        // `WF` on a GUI save). Only a fresh mint writes the defaults.
        let res = residue.get(&plan.uuid);
        let carried = |name: &str| {
            res.and_then(|r| r.attrs.iter().find(|(n, _)| n == name))
                .map(|(_, v)| v.as_str())
        };
        match (res, carried("Cl")) {
            (Some(_), Some(v)) => el.set_attr_raw("Cl", v),
            (Some(_), None) => {}
            (None, _) => el.set_attr("Cl", "141,255,112"),
        }
        el.set_attr("Nio", &plan.keys.len().to_string());
        // Minted-ref identity (D33), in the GUI's observed attribute
        // order: `Ref=` after `Nio=`, `Analog=`/`LinkRefType=` after `WF=`.
        if let Some(m) = mirror_of.get(&block.slug) {
            el.set_attr("Ref", &m.ref_uuid);
        }
        if let Some(v) = carried("LtE") {
            el.set_attr_raw("LtE", v);
        }
        match (res, carried("WF")) {
            (Some(_), Some(v)) => el.set_attr_raw("WF", v),
            (Some(_), None) => {}
            (None, _) => el.set_attr("WF", "147456"),
        }
        if let Some(m) = mirror_of.get(&block.slug) {
            if m.analog {
                el.set_attr("Analog", "true");
            }
            el.set_attr("LinkRefType", &m.link_type.to_string());
        }
        for name in attr_params(&block.block_type) {
            if let Some(v) = managed_attrs.get(&(block.slug.clone(), (*name).to_string())) {
                el.set_attr(name, v);
            }
        }
        // Every observed Formula= attribute travels with Valid="false"
        // (the GUI revalidates the expression on load).
        if block.block_type == "Formula"
            && managed_attrs.contains_key(&(block.slug.clone(), "Formula".to_string()))
        {
            el.set_attr("Valid", "false");
        }
        if let Some(r) = res {
            for (n, v) in &r.attrs {
                // On a ref block the mirror identity is compiler-owned
                // (recomputed from `mirrors:` above) — carrying the base's
                // copy verbatim would clobber a retarget.
                let compiler_owned = matches!(n.as_str(), "Cl" | "LtE" | "WF")
                    || (is_ref_type(&block.block_type)
                        && matches!(n.as_str(), "Ref" | "LinkRefType" | "Analog"));
                if !compiler_owned {
                    el.set_attr_raw(n, v);
                }
            }
        }
        for key in &plan.keys {
            // A GUI-owned (`Inv=`) connector is re-emitted verbatim at its
            // spec position (D20); a parameter binding on it would be
            // silently overridden, so it is refused instead.
            if let Some(carried) = res.and_then(|r| r.gui_ports.get(key)) {
                if managed_defs.contains_key(&(block.slug.clone(), key.clone())) {
                    return Err(Error::Compile(format!(
                        "block `{}`: connector `{key}` carries the GUI's input \
                         inversion (`Inv=`) and is GUI-owned — its value cannot \
                         be set from source; change it in Loxone Config instead",
                        block.slug
                    )));
                }
                el.push_child(carried.clone());
                continue;
            }
            let mut co = Element::new("Co");
            co.set_attr("K", key);
            if let Some(def) = managed_defs.get(&(block.slug.clone(), key.clone())) {
                co.set_attr("Def", def);
            }
            co.set_attr("U", &plan.ports[key]);
            el.push_child(co);
        }
        if let Some(r) = res {
            for c in &r.children {
                el.push_child(c.clone());
            }
        }
        let page_path = page_index.by_uuid.get(&plan.page_uuid).ok_or_else(|| {
            Error::Compile(format!(
                "the page `{}` recorded for block `{}` no longer exists in the \
                 base config; re-pin it by editing the lockfile entry's page_uuid \
                 (or remove the block from the lock to place it afresh)",
                plan.page_uuid, block.slug
            ))
        })?;
        let page = doc.element_at_mut(page_path).expect("indexed path");
        if page.attr("Type") != Some("Page") {
            return Err(Error::Compile(format!(
                "object `{}` recorded as the page for block `{}` is a `{}`, not a Page",
                plan.page_uuid,
                block.slug,
                page.attr("Type").unwrap_or("?")
            )));
        }
        page.push_child(el);
    }

    // --- Mirror routing (D34): a wire may name the mirrored object
    //     directly; when the consumer's *own page* carries a ref of it fed
    //     from exactly that port, the wire is drawn through the ref — an
    //     `AI`-fed mirror serves `AQ`, an `I`-fed mirror serves `Q`.
    //     Reuse-only and page-local: no same-page ref, no rerouting (a
    //     direct wire is legal, oracle session 6); a ref on another page
    //     is someone else's visual. Symmetric for actors: a wire into an
    //     extern port that the base feeds from an OutputRef's `AQ` lands
    //     on that ref's `AI` instead — the ref on the *writer's* page.
    #[derive(Clone)]
    struct MirrorRoute {
        /// Port the rerouted wire uses instead of the named one.
        port: String,
        /// The ref object itself (owner of `port`).
        ref_uuid: String,
        page: Option<String>,
    }
    let routed_objects = doc.objects();
    let managed_uuids: BTreeSet<&str> = managed.values().map(|p| p.uuid.as_str()).collect();
    let pages_idx: Vec<(&[usize], &str)> = routed_objects
        .iter()
        .filter(|o| o.block_type == "Page")
        .map(|o| (o.path.as_slice(), o.uuid.as_str()))
        .collect();
    let page_uuid_of = |path: &[usize]| -> Option<String> {
        pages_idx
            .iter()
            .filter(|(p, _)| path.len() > p.len() && path.starts_with(p))
            .max_by_key(|(p, _)| p.len())
            .map(|(_, u)| (*u).to_string())
    };
    let path_by_uuid: BTreeMap<&str, &[usize]> = routed_objects
        .iter()
        .map(|o| (o.uuid.as_str(), o.path.as_slice()))
        .collect();
    let mut in_routes: BTreeMap<String, Vec<MirrorRoute>> = BTreeMap::new();
    let mut aq_of_outputref: BTreeMap<String, MirrorRoute> = BTreeMap::new();
    for o in &routed_objects {
        if managed_uuids.contains(o.uuid.as_str())
            || !matches!(o.block_type.as_str(), "InputRef" | "OutputRef")
        {
            continue;
        }
        let el = doc.element_at(&o.path).expect("path from objects()");
        let ps = ports(el);
        let port = |k: &str| ps.iter().find(|p| p.key == k);
        let page = page_uuid_of(&o.path);
        if o.block_type == "InputRef" {
            for (fed, serves) in [("AI", "AQ"), ("I", "Q")] {
                if let (Some(f), Some(s)) = (port(fed), port(serves)) {
                    for src in &f.inputs {
                        in_routes.entry(src.clone()).or_default().push(MirrorRoute {
                            port: s.uuid.clone(),
                            ref_uuid: o.uuid.clone(),
                            page: page.clone(),
                        });
                    }
                }
            }
        } else if let (Some(aq), Some(ai)) = (port("AQ"), port("AI")) {
            aq_of_outputref.insert(
                aq.uuid.clone(),
                MirrorRoute {
                    port: ai.uuid.clone(),
                    ref_uuid: o.uuid.clone(),
                    page,
                },
            );
        }
    }
    let mut out_routes: BTreeMap<String, Vec<MirrorRoute>> = BTreeMap::new();
    for w in doc.wires() {
        if let Some(r) = aq_of_outputref.get(&w.from_port) {
            out_routes
                .entry(w.to_port.clone())
                .or_default()
                .push(r.clone());
        }
    }
    // Same-page candidates only; several on one page fall back to the
    // previous compile's recorded wire, then refuse.
    let pick_route = |cands: &[MirrorRoute],
                      ctx_page: Option<&String>,
                      pinned: &dyn Fn(&MirrorRoute) -> bool|
     -> std::result::Result<Option<MirrorRoute>, Vec<String>> {
        let local: Vec<&MirrorRoute> = cands
            .iter()
            .filter(|c| c.page.as_ref() == ctx_page && ctx_page.is_some())
            .collect();
        match local.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some((*one).clone())),
            many => {
                // Each mirror carries its own output ports (a base where
                // twins share them is refused up front), so several
                // candidates are a real choice.
                let hits: Vec<&&MirrorRoute> = many.iter().filter(|c| pinned(c)).collect();
                match hits.as_slice() {
                    [one] => Ok(Some((**one).clone())),
                    _ => Err(many.iter().map(|c| c.ref_uuid.clone()).collect()),
                }
            }
        }
    };

    // --- Wires: block argument bindings (sink = the declaring block) and
    //     `<-` statements (sink = an extern port), in source order.
    for (from_ref, to_ref) in module.wire_pairs() {
        let mut from = resolve_port(
            &doc,
            &managed,
            &extern_uuid,
            &from_ref.slug,
            &from_ref.port,
            PortDir::Output,
        )?;
        let mut to = resolve_port(
            &doc,
            &managed,
            &extern_uuid,
            &to_ref.slug,
            &to_ref.port,
            PortDir::Input,
        )?;
        // Ref-typed endpoints wire literally (feed wires, explicit ref
        // externs); routing is for wires naming the mirrored object.
        let endpoint_type = |slug: &str, owner_uuid: &str| -> Option<&str> {
            managed
                .get(slug)
                .map(|p| p.block_type.as_str())
                .or_else(|| type_by_uuid.get(owner_uuid).map(|(t, _)| t.as_str()))
        };
        let page_of_endpoint = |slug: &str, owner_uuid: &str| -> Option<String> {
            managed
                .get(slug)
                .map(|p| p.page_uuid.clone())
                .or_else(|| path_by_uuid.get(owner_uuid).and_then(|p| page_uuid_of(p)))
        };
        let from_is_ref = endpoint_type(&from_ref.slug, &from.owner_uuid).is_some_and(is_ref_type);
        let to_is_ref = endpoint_type(&to_ref.slug, &to.owner_uuid).is_some_and(is_ref_type);
        if !from_is_ref && !to_is_ref {
            let route_err = |which: &str, endpoint: String, refs: Vec<String>| {
                Error::Compile(format!(
                    "wire `{to_ref} <- {from_ref}`: several mirrors of {which} \
                     `{endpoint}` sit on the consumer's page ({}) — declare the \
                     intended one explicitly (`extern … = \
                     InputRef(mirrors: …)` or a `uuid:` pin) and wire through it",
                    refs.join(", ")
                ))
            };
            if let Some(cands) = in_routes.get(&from.port_uuid) {
                let sink_page = page_of_endpoint(&to_ref.slug, &to.owner_uuid);
                let pin =
                    |c: &MirrorRoute| base_wires.contains(&(c.port.clone(), to.port_uuid.clone()));
                match pick_route(cands, sink_page.as_ref(), &pin) {
                    Ok(Some(r)) => {
                        from.owner_uuid = r.ref_uuid;
                        from.port_uuid = r.port;
                    }
                    Ok(None) => {}
                    Err(refs) => {
                        return Err(route_err("source", from_ref.to_string(), refs));
                    }
                }
            }
            if let Some(cands) = out_routes.get(&to.port_uuid) {
                let from_page = page_of_endpoint(&from_ref.slug, &from.owner_uuid);
                let pin = |c: &MirrorRoute| {
                    base_wires.contains(&(from.port_uuid.clone(), c.port.clone()))
                };
                match pick_route(cands, from_page.as_ref(), &pin) {
                    Ok(Some(r)) => {
                        to.owner_uuid = r.ref_uuid;
                        to.port_uuid = r.port;
                        to.is_extern = true;
                    }
                    Ok(None) => {}
                    Err(refs) => {
                        return Err(route_err("sink", to_ref.to_string(), refs));
                    }
                }
            }
            // Reverse-recorded distributions (API connectors): there the
            // extern input is the wire's *from* and the managed output its
            // *to* — the OutputRef redirect applies to the source side,
            // still on the writer's page.
            if let Some(cands) = out_routes.get(&from.port_uuid) {
                let writer_page = page_of_endpoint(&to_ref.slug, &to.owner_uuid);
                let pin =
                    |c: &MirrorRoute| base_wires.contains(&(c.port.clone(), to.port_uuid.clone()));
                match pick_route(cands, writer_page.as_ref(), &pin) {
                    Ok(Some(r)) => {
                        from.owner_uuid = r.ref_uuid;
                        from.port_uuid = r.port;
                    }
                    Ok(None) => {}
                    Err(refs) => {
                        return Err(route_err("source", from_ref.to_string(), refs));
                    }
                }
            }
        }
        let flg = residue.get(&to.owner_uuid).and_then(|r| {
            r.wire_flags
                .get(&(to.port_uuid.clone(), from.port_uuid.clone()))
        });
        add_wire(
            &mut doc,
            &to.owner_uuid,
            &to.port_uuid,
            &from.port_uuid,
            flg.map(String::as_str),
        )?;
        if to.is_extern {
            lock.extern_wires.push(LockedWire {
                from: from.port_uuid,
                to: to.port_uuid,
            });
        }
    }
    lock.extern_wires.sort();
    lock.extern_wires.dedup();
    // A wire this base carried that the source no longer draws was
    // withdrawn — tombstone it (D31) so pre-deployment bases keep losing
    // it instead of passing it through.
    for w in &old_wires {
        if wires_present.contains(w) && !lock.extern_wires.contains(w) {
            lock.removed_wires.insert(w.clone());
        }
    }

    // --- Port assignments (always extern ports — validate rejects managed
    //     targets).
    for s in module.sets() {
        let target = resolve_port(
            &doc,
            &managed,
            &extern_uuid,
            &s.target.slug,
            &s.target.port,
            PortDir::Param,
        )?;
        let value = resolve(&s.value)?;
        let original = apply_def(&mut doc, &target.owner_uuid, &target.port_uuid, &value)?;
        lock.set_originals
            .entry(target.port_uuid)
            .or_insert(original);
    }
    // A Def write that vanished from source was withdrawn — tombstone it
    // (D31) unless it never changed the port's bytes in the first place.
    for (port_uuid, original) in &old_originals {
        if lock.set_originals.contains_key(port_uuid) {
            continue; // still set from source
        }
        if let Some(written) = defs_written.get(port_uuid)
            && original.as_deref() != Some(written.as_str())
        {
            lock.removed_sets.insert(
                port_uuid.clone(),
                crate::lock::RetiredSet {
                    original: original.clone(),
                    written: written.clone(),
                },
            );
        }
    }

    // --- Counters and target metadata.
    doc.set_counters(Counters {
        next_obj: lock.counters.next_obj,
        next_const: lock.counters.next_const,
        next_note: lock.counters.next_note,
        next_mem: lock.counters.next_mem,
    });
    lock.target.config_version = doc.config_version();
    lock.target.miniserver_serial = Some(
        opts.machine
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<String>(),
    );
    lock.target.source_config_sha256 = Some(sha256_hex(&base.to_bytes()));
    lock.target.semantic_fingerprint = Some(crate::diff::semantic_fingerprint(&doc));
    // Self-check: the base was unique, so a repeat here was introduced by
    // this compile — a mint that hit a UUID the base already carries (the
    // minter is time + counter + serial: another lock's output, minted in
    // the same second), a lock that maps two identities onto one UUID, or
    // a compiler bug. Never hand such an output to a push.
    let dups = doc.duplicate_uuids();
    if !dups.is_empty() {
        return Err(Error::Compile(format!(
            "output would carry duplicate UUID(s) {} — either a freshly minted \
             UUID collides with one the base already carries (same serial and \
             mint second; compile again, or pass another --time), or the \
             lockfile maps two identities onto one UUID; nothing was written",
            dups.join(", ")
        )));
    }
    Ok(doc)
}

struct PlannedBlock {
    uuid: String,
    block_type: String,
    /// All port keys in connector-index order.
    keys: Vec<String>,
    ports: BTreeMap<String, String>,
    layout: Layout,
    /// UUID of the page the block is (re)built on.
    page_uuid: String,
}

struct ResolvedPort {
    owner_uuid: String,
    port_uuid: String,
    is_extern: bool,
}

fn resolve_externs(
    doc: &LoxoneDoc,
    module: &Module,
    lock: &mut Lockfile,
) -> Result<BTreeMap<String, String>> {
    let objects = doc.objects();
    // Room/category lookup for composite matching: `<IoData Pr=…/Cr=…>`
    // on an object points at a `Place`/`Category` element (validated
    // corpus-wide, docs/loxone-format.md); constraints compare titles.
    let titles_of = |ty: &str| -> BTreeMap<String, String> {
        objects
            .iter()
            .filter(|o| o.block_type == ty)
            .filter_map(|o| Some((o.uuid.clone(), o.title.clone()?)))
            .collect()
    };
    let place_titles = titles_of("Place");
    let cat_titles = titles_of("Category");
    let iodata_ref = |o: &crate::doc::ObjectSummary, attr: &str| -> Option<String> {
        doc.element_at(&o.path)?
            .child_elements()
            .find(|c| c.name == "IoData")?
            .attr(attr)
            .map(str::to_string)
    };
    // `mirrors:` disambiguation is positional page governance (D32): the
    // `page` statement in force at the extern's declaration site (D28).
    let mut page_of_extern: BTreeMap<&str, &str> = BTreeMap::new();
    {
        let mut current: Option<&str> = None;
        for item in &module.items {
            match item {
                Item::Page(p) => current = Some(p.title.as_str()),
                Item::Extern(e) => {
                    if let Some(t) = current {
                        page_of_extern.insert(e.slug.as_str(), t);
                    }
                }
                _ => {}
            }
        }
    }
    // Pages with their tree paths: a candidate's page is the longest
    // `Page` path that prefixes its own.
    let pages: Vec<(&[usize], Option<&str>)> = objects
        .iter()
        .filter(|o| o.block_type == "Page")
        .map(|o| (o.path.as_slice(), o.title.as_deref()))
        .collect();
    let page_title_of = |path: &[usize]| -> Option<&str> {
        pages
            .iter()
            .filter(|(p, _)| path.len() > p.len() && path.starts_with(p))
            .max_by_key(|(p, _)| p.len())
            .and_then(|(_, t)| *t)
    };
    let ref_attr =
        |o: &crate::doc::ObjectSummary| -> Option<&str> { doc.element_at(&o.path)?.attr("Ref") };
    let kind = |m: &MatchSpec| match m {
        MatchSpec::Uuid(_) => "uuid",
        MatchSpec::IName(_) => "iname",
        MatchSpec::Title(_) => "title",
        MatchSpec::Mirrors(_) => "mirrors",
    };
    // The declared matcher, evaluated fresh. For `mirrors:` the target's
    // uuid is resolved by the caller (it needs the phase order below).
    let matches_of = |ext: &crate::ir::ast::ExternDecl,
                      mirror_uuid: Option<&str>|
     -> Vec<&crate::doc::ObjectSummary> {
        let mut v: Vec<_> = objects
            .iter()
            .filter(|o| {
                o.block_type == ext.block_type
                    && match &ext.match_spec {
                        MatchSpec::Uuid(u) => &o.uuid == u,
                        MatchSpec::IName(v) => o.iname.as_deref() == Some(v),
                        MatchSpec::Title(v) => o.title.as_deref() == Some(v),
                        MatchSpec::Mirrors(_) => {
                            mirror_uuid.is_some() && ref_attr(o) == mirror_uuid
                        }
                    }
                    && ext.room.as_ref().is_none_or(|want| {
                        iodata_ref(o, "Pr")
                            .and_then(|u| place_titles.get(&u))
                            .is_some_and(|t| t == want)
                    })
                    && ext.category.as_ref().is_none_or(|want| {
                        iodata_ref(o, "Cr")
                            .and_then(|u| cat_titles.get(&u))
                            .is_some_and(|t| t == want)
                    })
            })
            .collect();
        // Page narrowing, `mirrors:` only (D32): several refs of the same
        // target usually sit on different pages — the declaring file's
        // page picks its own. Narrowing never widens: if no candidate is
        // on the declared page, the ambiguity stands and is reported.
        if v.len() > 1
            && matches!(ext.match_spec, MatchSpec::Mirrors(_))
            && let Some(want) = page_of_extern.get(ext.slug.as_str())
        {
            let narrowed: Vec<_> = v
                .iter()
                .copied()
                .filter(|o| page_title_of(&o.path) == Some(want))
                .collect();
            if !narrowed.is_empty() {
                v = narrowed;
            }
        }
        v
    };
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    // Two phases: plain matchers first, then `mirrors:` — whose target may
    // be any managed block with a locked identity or any plain-matched
    // extern, regardless of declaration order. (A mirror of a mirror is
    // refused; so is a mirror of a block minted this very compile — such a
    // block cannot have a ref in the base yet.)
    let (plain, mirrored): (Vec<_>, Vec<_>) = module
        .externs()
        .partition(|e| !matches!(e.match_spec, MatchSpec::Mirrors(_)));
    for ext in plain.into_iter().chain(mirrored) {
        let mirror_uuid: Option<String> = match &ext.match_spec {
            MatchSpec::Mirrors(target) => {
                if !matches!(ext.block_type.as_str(), "InputRef" | "OutputRef") {
                    return Err(Error::Compile(format!(
                        "extern `{}`: `mirrors:` applies to InputRef/OutputRef only, \
                         not {}",
                        ext.slug, ext.block_type
                    )));
                }
                Some(
                    lock.objects
                        .get(target)
                        .map(|o| o.uuid.clone())
                        .or_else(|| out.get(target).cloned())
                        .ok_or_else(|| {
                            Error::Compile(format!(
                                "extern `{}` mirrors `{target}`, but `{target}` is neither \
                                 a managed block with a locked identity nor a plain-matched \
                                 extern of this module",
                                ext.slug
                            ))
                        })?,
                )
            }
            _ => None,
        };
        // Lock pin wins as long as it still resolves to an object of the
        // declared type; otherwise fall back to fresh resolution by spec.
        // If the matcher *kind* changed in source, the new matcher must
        // confirm the pinned object before the pin is kept — a matcher
        // that contradicts its pin is a source bug, not drift to tolerate.
        // A `mirrors:` matcher re-confirms on every compile: unlike a
        // title (whose drift the pin deliberately tolerates), the claim
        // "this ref mirrors X" must stay true, or the source lies — and a
        // ref the GUI re-pointed elsewhere is drift worth stopping on.
        // A `uuid:` matcher *is* the identity — no drift for a pin to
        // tolerate — so an edited value means the source re-points: it
        // resolves afresh (a typo is a NoMatch, not a silent no-op).
        if let Some(locked) = lock.externals.get_mut(&ext.slug)
            && objects
                .iter()
                .any(|o| o.uuid == locked.uuid && o.block_type == ext.block_type)
            && !(locked.matched_by == "uuid"
                && matches!(&ext.match_spec, MatchSpec::Uuid(u) if *u != locked.uuid))
        {
            if locked.matched_by != kind(&ext.match_spec)
                || matches!(ext.match_spec, MatchSpec::Mirrors(_))
            {
                let confirms = matches_of(ext, mirror_uuid.as_deref());
                if !confirms.iter().any(|o| o.uuid == locked.uuid) {
                    return Err(Error::Compile(format!(
                        "extern `{}`: matcher changed to {}({}), which does not match \
                         the locked object {} — fix the matcher, or drop the lock \
                         entry to accept a new identity",
                        ext.slug,
                        ext.block_type,
                        ext.spec(),
                        locked.uuid
                    )));
                }
                locked.matched_by = kind(&ext.match_spec).to_string();
            }
            out.insert(ext.slug.clone(), locked.uuid.clone());
            continue;
        }
        let matches = matches_of(ext, mirror_uuid.as_deref());
        match matches.as_slice() {
            [] => {
                return Err(Error::NoMatch {
                    slug: ext.slug.clone(),
                    spec: format!("{}({})", ext.block_type, ext.spec()),
                });
            }
            [only] => {
                lock.externals.insert(
                    ext.slug.clone(),
                    LockedExternal {
                        uuid: only.uuid.clone(),
                        matched_by: kind(&ext.match_spec).to_string(),
                        title_at_match: only.title.clone(),
                        iname_at_match: only.iname.clone(),
                    },
                );
                out.insert(ext.slug.clone(), only.uuid.clone());
            }
            many => {
                return Err(Error::AmbiguousMatch {
                    slug: ext.slug.clone(),
                    spec: format!("{}({})", ext.block_type, ext.spec()),
                    count: many.len(),
                    candidates: many.iter().map(|o| o.uuid.clone()).collect(),
                });
            }
        }
    }
    // Drop resolutions for externs no longer declared.
    let declared: BTreeSet<&str> = module.externs().map(|e| e.slug.as_str()).collect();
    lock.externals
        .retain(|slug, _| declared.contains(slug.as_str()));
    Ok(out)
}

fn resolve_port(
    doc: &LoxoneDoc,
    managed: &BTreeMap<String, PlannedBlock>,
    extern_uuid: &BTreeMap<String, String>,
    slug: &str,
    port: &str,
    want: PortDir,
) -> Result<ResolvedPort> {
    if let Some(plan) = managed.get(slug) {
        let port_uuid =
            plan.ports.get(port).cloned().ok_or_else(|| {
                Error::Compile(format!("no port `{port}` on managed block `{slug}`"))
            })?;
        // Direction check against the builtin table (extras are inputs).
        let index = plan.keys.iter().position(|k| k == port).expect("planned");
        let dir = builtin(&plan.block_type)
            .and_then(|specs| specs.get(index).map(|s| s.dir))
            .unwrap_or(PortDir::Input);
        let ok = match want {
            // Api connectors are wire-bidirectional (see PortDir::Api).
            PortDir::Output => matches!(dir, PortDir::Output | PortDir::Api),
            // Wire sinks accept inputs, params, and Api connectors; a Def
            // assignment does not (Api ports never carry Def= — evidence
            // on PortDir::Api).
            PortDir::Input => dir != PortDir::Output,
            PortDir::Param => !matches!(dir, PortDir::Output | PortDir::Api),
            PortDir::Api => unreachable!("no caller wants Api"),
        };
        if !ok {
            return Err(Error::Compile(format!(
                "`{slug}.{port}` is an {} port and cannot be used as {}",
                if dir == PortDir::Output {
                    "output"
                } else {
                    "input"
                },
                if want == PortDir::Output {
                    "a wire source"
                } else {
                    "a wire sink / set target"
                },
            )));
        }
        return Ok(ResolvedPort {
            owner_uuid: plan.uuid.clone(),
            port_uuid,
            is_extern: false,
        });
    }
    let owner_uuid = extern_uuid
        .get(slug)
        .ok_or_else(|| Error::Compile(format!("`{slug}` is not a declared block or extern")))?;
    // Externs are open-world: the port must merely exist in the base config.
    let idx = doc.index();
    let path = idx
        .by_uuid
        .get(owner_uuid)
        .ok_or_else(|| Error::Compile(format!("extern `{slug}` vanished from the document")))?;
    let el = doc.element_at(path).expect("indexed path");
    let known = ports(el);
    match known.iter().find(|p| p.key == port) {
        Some(p) => Ok(ResolvedPort {
            owner_uuid: owner_uuid.clone(),
            port_uuid: p.uuid.clone(),
            is_extern: true,
        }),
        None => Err(Error::Compile(format!(
            "extern `{slug}` has no port `{port}` in the base config; present ports: {}{}. \
             (v0 cannot mint ports for unverified types — wire or set it once in Loxone \
             Config so the connector exists.)",
            known
                .iter()
                .map(|p| p.key.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            suggest(port, known.iter().map(|p| p.key.as_str()))
        ))),
    }
}

/// Find the `<C>` element with this object UUID, mutably.
fn find_c_mut<'a>(el: &'a mut Element, uuid: &str) -> Option<&'a mut Element> {
    if el.name == "C" && el.attrs.iter().any(|a| a.name == "U" && a.value == uuid) {
        return Some(el);
    }
    el.child_elements_mut().find_map(|c| find_c_mut(c, uuid))
}

/// Find the `<C>` owning a port and the `<Co>` itself, mutably.
fn find_co_mut<'a>(el: &'a mut Element, port_uuid: &str) -> Option<&'a mut Element> {
    let owns = el.name == "C"
        && el
            .child_elements()
            .any(|c| c.name == "Co" && c.attr("U") == Some(port_uuid));
    if owns {
        return el
            .child_elements_mut()
            .find(|c| c.name == "Co" && c.attr("U") == Some(port_uuid));
    }
    el.child_elements_mut()
        .find_map(|c| find_co_mut(c, port_uuid))
}

/// Append `<In Input=from/>` under the sink `<Co>` and maintain `Nc`.
/// `flg` re-emits a harvested `FLG=` wire flag verbatim (D19 residue).
fn add_wire(
    doc: &mut LoxoneDoc,
    owner_uuid: &str,
    to_port: &str,
    from_port: &str,
    flg: Option<&str>,
) -> Result<()> {
    let owner = find_c_mut(&mut doc.xml.root, owner_uuid)
        .ok_or_else(|| Error::Compile(format!("wire sink object `{owner_uuid}` not found")))?;
    let co = owner
        .child_elements_mut()
        .find(|c| c.name == "Co" && c.attr("U") == Some(to_port))
        .ok_or_else(|| Error::Compile(format!("wire sink port `{to_port}` not found")))?;
    if co.attr("Inv").is_some() {
        return Err(Error::Compile(format!(
            "wire sink connector `{}` carries the GUI's input inversion \
             (`Inv=`) and is GUI-owned — a declared wire into it would be \
             silently inverted; remove the inversion in Loxone Config first",
            co.attr_decoded("K").unwrap_or_default()
        )));
    }
    // A declared wire always ends up as the LAST <In> of its sink: an
    // already-present duplicate (first compile after adopt — the wire
    // exists in the base) is moved there instead of re-created, keeping
    // its attributes and making the first compile byte-identical to every
    // later one (which tears the wire down and re-appends it). `<In>`
    // order is semantically unordered — the GUI rewrites it on save.
    let existing = co.children.iter().position(|n| {
        matches!(n, crate::xml::Node::Element(e)
            if e.name == "In" && e.attr("Input") == Some(from_port))
    });
    if let Some(pos) = existing {
        let node = co.children.remove(pos);
        co.children.push(node);
    } else {
        let mut input = Element::new("In");
        input.set_attr("Input", from_port);
        if let Some(v) = flg {
            input.set_attr_raw("FLG", v);
        }
        co.push_child(input);
    }
    sync_nc(co);
    Ok(())
}

/// Remove `<In Input=from/>` from the sink `<Co>` (used to tear down wires
/// the previous compile owned).
/// Returns whether a wire was actually present (and thus removed) — the
/// tombstone lifecycle (D31) keys on this.
fn remove_wire(doc: &mut LoxoneDoc, from_port: &str, to_port: &str) -> bool {
    if let Some(co) = find_co_mut(&mut doc.xml.root, to_port) {
        let before = co.children.len();
        co.children.retain(|n| {
            !matches!(n, crate::xml::Node::Element(e)
                if e.name == "In" && e.attr("Input") == Some(from_port))
        });
        let removed = co.children.len() != before;
        if removed {
            sync_nc(co);
        }
        removed
    } else {
        false
    }
}

/// The current (decoded) `Def=` of an extern port, if any.
fn current_def(doc: &mut LoxoneDoc, port_uuid: &str) -> Option<String> {
    find_co_mut(&mut doc.xml.root, port_uuid)
        .and_then(|co| co.attr_decoded("Def").map(|v| v.into_owned()))
}

/// Keep `Nc` equal to the number of `<In>` children (absent when zero),
/// in Loxone's canonical position: after `Def`, before `U` (oracle save
/// 2026-08-25 rewrote a compiler-emitted `K,Nc,Def,U` to `K,Def,Nc,U`).
fn sync_nc(co: &mut Element) {
    let n = co.child_elements().filter(|c| c.name == "In").count();
    if n == 0 {
        co.remove_attr("Nc");
        if co.children.is_empty() {
            co.self_closing = true;
        }
    } else {
        set_attr_ordered(co, "Nc", &n.to_string(), &["U"]);
    }
}

/// Rewrite an extern port's `Def=`, returning the previous (decoded) value.
fn apply_def(
    doc: &mut LoxoneDoc,
    owner_uuid: &str,
    port_uuid: &str,
    value: &str,
) -> Result<Option<String>> {
    let owner = find_c_mut(&mut doc.xml.root, owner_uuid)
        .ok_or_else(|| Error::Compile(format!("set target object `{owner_uuid}` not found")))?;
    let co = owner
        .child_elements_mut()
        .find(|c| c.name == "Co" && c.attr("U") == Some(port_uuid))
        .ok_or_else(|| Error::Compile(format!("set target port `{port_uuid}` not found")))?;
    if co.attr("Inv").is_some() {
        return Err(Error::Compile(format!(
            "set target connector `{}` carries the GUI's input inversion \
             (`Inv=`) and is GUI-owned — its value cannot be set from \
             source; change it in Loxone Config instead",
            co.attr_decoded("K").unwrap_or_default()
        )));
    }
    let original = co.attr_decoded("Def").map(|v| v.into_owned());
    set_attr_ordered(co, "Def", value, &["U"]);
    Ok(original)
}

/// Restore an extern port's `Def=` to its pre-`set` state.
fn restore_def(doc: &mut LoxoneDoc, port_uuid: &str, original: Option<&str>) {
    if let Some(co) = find_co_mut(&mut doc.xml.root, port_uuid) {
        match original {
            Some(v) => set_attr_ordered(co, "Def", v, &["U"]),
            None => {
                co.remove_attr("Def");
            }
        }
    }
}

/// Set an attribute, inserting it before the first of `before` when absent
/// (Loxone keeps `<Co>` attributes in the order `K, Nc, Def, U`).
fn set_attr_ordered(el: &mut Element, name: &str, value: &str, before: &[&str]) {
    if el.attr(name).is_some() {
        el.set_attr(name, value);
        return;
    }
    let pos = el
        .attrs
        .iter()
        .position(|a| before.contains(&a.name.as_str()))
        .unwrap_or(el.attrs.len());
    el.attrs.insert(
        pos,
        crate::xml::Attr {
            name: name.to_string(),
            value: crate::xml::escape(value).into_owned(),
        },
    );
}

/// First stacking position below every layout already in the lock, so new
/// blocks never land on top of existing ones.
fn next_free_py(lock: &Lockfile) -> i64 {
    lock.objects
        .values()
        .filter_map(|o| o.layout.map(|l| l.py2 + GAP))
        .max()
        .unwrap_or(GAP)
}
