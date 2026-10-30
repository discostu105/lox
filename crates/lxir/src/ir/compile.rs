//! The compiler: base config + IR module + lockfile → new config.
//!
//! The compiler owns exactly four kinds of edits and touches nothing else
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
//! 4. **Attribute claims** — `target.title|room|category = value` on an
//!    extern rewrites its `Title=` or its `<IoData Pr=/Cr=>` binding (D39),
//!    carrying along the descendants that shared the old room/category and
//!    the refs that copy the title (D43); the originals are remembered and
//!    restored the same way.
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

use crate::connectors::{
    MESSAGE_REF_TARGET, PortDir, RefValue, VERIFIED_BLOCK_VERSIONS, adopt_only, attr_params,
    builtin, carried_attrs, carried_children, container_type, message_params, ref_link_type,
};
use crate::doc::{Counters, GUI_OWNED_ATTRS, GUI_OWNED_CHILDREN, LoxoneDoc, ports};
use crate::error::{Error, Result};
use crate::ir::ast::{Attr, Item, MatchSpec, Module, Value};
use crate::ir::validate::{api_mirror_entry, is_ref_type, suggest, validate_ports};
use crate::layout::{PageGeometry, beside};
use crate::lock::{
    AttrClaim, Layout, LockedExternal, LockedObject, LockedWire, Lockfile, sha256_hex,
};
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
    /// Page titles whose managed blocks are placed afresh (D42) — the one
    /// way the compiler moves a block it has placed before. A block wired
    /// to a ref another writer owns stays where it is: the ref was drawn
    /// for that spot, and moving the block alone would tear them apart.
    pub relayout: Vec<String>,
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
    /// Where the block is drawn in the base (D42): a position Loxone Config
    /// moved it to is taken over instead of reverted.
    layout: Option<Layout>,
    /// The page the base draws the block on.
    page: Option<String>,
    /// Every attribute name and every `<Co>` key in base element order:
    /// the rebuild writes its own in the same order (D47).
    attr_order: Vec<String>,
    co_order: Vec<String>,
}

impl Residue {
    /// The base's position of the block, if the base draws it on `page`:
    /// a position on another page says nothing about this one.
    fn layout_on(&self, page: &str) -> Option<Layout> {
        self.layout.filter(|_| self.page.as_deref() == Some(page))
    }
}

/// The `<C Type="Page">` the object at `path` sits under.
fn page_of(doc: &LoxoneDoc, path: &[usize]) -> Option<String> {
    (1..path.len()).rev().find_map(|n| {
        let el = doc.element_at(&path[..n])?;
        if el.name == "C" && el.attr("Type") == Some("Page") {
            el.attr("U").map(str::to_string)
        } else {
            None
        }
    })
}

fn harvest_residue(el: &Element) -> Residue {
    Residue {
        layout: Layout::of_element(el),
        page: None,
        attr_order: el.attrs.iter().map(|a| a.name.clone()).collect(),
        co_order: el
            .child_elements()
            .filter(|c| c.name == "Co")
            .filter_map(|co| Some(co.attr_decoded("K")?.into_owned()))
            .collect(),
        attrs: el
            .attrs
            .iter()
            .filter(|a| {
                matches!(a.name.as_str(), "Cl" | "LtE" | "WF")
                    || GUI_OWNED_ATTRS.contains(&a.name.as_str())
                    || carried_attrs(el.attr("Type").unwrap_or("")).contains(&a.name.as_str())
            })
            .map(|a| (a.name.clone(), a.value.clone()))
            .collect(),
        children: el
            .child_elements()
            .filter(|c| {
                GUI_OWNED_CHILDREN.contains(&c.name.as_str())
                    || carried_children(el.attr("Type").unwrap_or("")).contains(&c.name.as_str())
            })
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
    let all_base_wires = doc.wires();
    let base_wires: BTreeSet<(String, String)> = all_base_wires
        .iter()
        .map(|w| (w.from_port.clone(), w.to_port.clone()))
        .collect();
    // Each sink's `<In>` sources in base order (D45): a wire the rebuild
    // adds back returns to its place among them, so whichever writer last
    // ordered a connector — Loxone Config sorts on save — keeps its order.
    let mut base_in_order: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for w in all_base_wires {
        base_in_order
            .entry(w.to_port)
            .or_default()
            .push(w.from_port);
    }

    // --- Tear down our previous output: managed objects, extern wires,
    //     extern sets. What remains is exactly the Loxone-Config-owned state.
    //     Each removed element hands back its GUI-owned residue (D19) so
    //     the rebuild carries it forward verbatim.
    let mut residue: BTreeMap<String, Residue> = BTreeMap::new();
    // Each page's objects in base order (D47): a rebuilt block goes back
    // to its place among them.
    let base_page_order: BTreeMap<String, Vec<String>> = doc
        .objects()
        .iter()
        .filter(|o| o.block_type == "Page")
        .filter_map(|o| {
            let page = doc.element_at(&o.path)?;
            let objects = page
                .child_elements()
                .filter(|c| c.name == "C")
                .filter_map(|c| c.attr("U").map(str::to_string))
                .collect();
            Some((o.uuid.clone(), objects))
        })
        .collect();
    let base_page: BTreeMap<String, String> = {
        let idx = doc.index();
        lock.objects
            .values()
            .filter_map(|o| Some((o.uuid.clone(), page_of(&doc, idx.by_uuid.get(&o.uuid)?)?)))
            .collect()
    };
    for obj in lock.objects.values() {
        if let Some(el) = doc.remove_by_uuid(&obj.uuid) {
            let mut r = harvest_residue(&el);
            r.page = base_page.get(&obj.uuid).cloned();
            residue.insert(obj.uuid.clone(), r);
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
    // The GUI's flag on a wire (`FLG=`) is its own; one the base carried
    // on a wire of ours goes back on with the rebuilt wire.
    let mut extern_wire_flags: BTreeMap<(String, String), String> = BTreeMap::new();
    for w in &old_wires {
        if let Some(flg) = wire_flag(&mut doc, &w.from, &w.to) {
            extern_wire_flags.insert((w.to.clone(), w.from.clone()), flg);
        }
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
    // Attribute claims (D39), the same two steps. Withdrawn first: an
    // object still showing the value we wrote predates the deployment —
    // restore again and keep the tombstone; any other value retires it.
    for (uuid, claims) in lock.removed_attrs.iter_mut() {
        claims.retain(|attr, c| {
            if read_claimed(&doc, uuid, *attr).as_deref() == Some(c.written.as_str()) {
                restore_claim(&mut doc, uuid, *attr, c);
                return true;
            }
            false
        });
    }
    lock.removed_attrs.retain(|_, claims| !claims.is_empty());
    // Live claims: the object itself is source-owned and always restored;
    // its followers only while they still carry the written value.
    let old_claims = std::mem::take(&mut lock.attr_claims);
    let mut claims_written: BTreeMap<(String, Attr), String> = BTreeMap::new();
    for (uuid, claims) in &old_claims {
        for (attr, c) in claims {
            if let Some(v) = read_claimed(&doc, uuid, *attr) {
                claims_written.insert((uuid.clone(), *attr), v);
            }
            restore_claim(&mut doc, uuid, *attr, c);
        }
    }

    let mut minter = Minter::new(opts.machine, opts.mint_time_unix);
    // Never reuse a (time, sequence) pair a previous compile session
    // handed out: seed past the recorded high-water mark and every locked
    // UUID (older locks have no mark). With a fixed mint time the bare
    // per-run counter would mint a later-added block into a collision.
    minter.seed(lock.counters.next_mint);
    for u in lock
        .pages
        .values()
        .chain(lock.moods.values().flat_map(|m| m.values()))
    {
        minter.avoid(u);
    }
    for o in lock.objects.values() {
        minter.avoid(&o.uuid);
        for p in o.ports.values() {
            minter.avoid(p);
        }
    }
    let mint_base = minter.counter();

    // --- Pages the source asks for and the base lacks are created (D56),
    //     before anything is pinned to them.
    create_pages(&mut doc, module, lock, &mut minter)?;

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
    // Object-tree containers (D54), first of each type in document order.
    let mut containers: BTreeMap<String, String> = BTreeMap::new();
    for o in doc.objects() {
        if o.block_type == "VirtualInCaption" {
            containers
                .entry(o.block_type.clone())
                .or_insert(o.uuid.clone());
        }
        if o.block_type == "Page" {
            pages_by_title
                .entry(o.title.clone().unwrap_or_default())
                .or_default()
                .push(o.uuid);
        }
    }
    let mut managed: BTreeMap<String, PlannedBlock> = BTreeMap::new();
    // Pages to lay out afresh (D42), by UUID.
    let mut relayout_pages: BTreeSet<&str> = BTreeSet::new();
    for title in &opts.relayout {
        let uuids = pages_by_title.get(title.as_str()).ok_or_else(|| {
            Error::Compile(format!(
                "relayout: the base config has no <C Type=\"Page\"> titled `{title}`"
            ))
        })?;
        relayout_pages.extend(uuids.iter().map(String::as_str));
    }
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
                superseded_layout: None,
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
        if entry.uuid.is_empty() && adopt_only(&block.block_type) {
            return Err(Error::Compile(format!(
                "block `{}`: a {} is created by Loxone Config when it pairs the \
                 device — adopt the existing one (`lxir adopt <cfg> --uuid …`) \
                 instead of declaring a new one",
                block.slug, block.block_type
            )));
        }
        if entry.uuid.is_empty() {
            entry.uuid = minter.mint_object().to_string();
            lock.counters.next_obj += 1;
        }
        // D54: an object of the object tree (a VirtualIn) is appended to
        // its container, which the base has exactly once — never pinned,
        // placed, or governed by `page`.
        if let Some(container) = container_type(&block.block_type) {
            let uuid = containers.get(container).cloned().ok_or_else(|| {
                Error::Compile(format!(
                    "block `{}`: the base config has no <C Type=\"{container}\"> to \
                     hold a minted {}",
                    block.slug, block.block_type
                ))
            })?;
            entry.page_uuid = None;
            entry.layout = None;
            entry.superseded_layout = None;
            managed.insert(
                block.slug.clone(),
                PlannedBlock {
                    uuid: entry.uuid.clone(),
                    block_type: block.block_type.clone(),
                    keys,
                    ports: entry.ports.clone(),
                    layout: None,
                    relayout: false,
                    page_uuid: uuid,
                    placed: false,
                },
            );
            continue;
        }
        let pinned_before = entry.page_uuid.clone();
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
                                 base config has no <C Type=\"Page\"> titled `{title}` — \
                                 to create it, name its room: `page \"{title}\" \
                                 (room: \"…\")` (D56)",
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
        // D42: the position is shared with Loxone Config. A block that
        // changed pages is placed afresh. Where the base draws the block on
        // its page is the GUI's: its extents and grid snap always, its
        // position after a drag — unless the base still shows the position
        // a placement replaced (the base predates that placement). New
        // blocks are placed after wiring.
        if pinned_before.is_some_and(|p| p != page_uuid) {
            entry.layout = None;
        }
        let base_layout = residue
            .get(&entry.uuid)
            .and_then(|r| r.layout_on(&page_uuid));
        if let (Some(l), Some(b)) = (entry.layout, base_layout) {
            let stale =
                !l.same_spot(&b) && entry.superseded_layout.is_some_and(|s| s.same_spot(&b));
            if !stale {
                entry.layout = Some(b);
                entry.superseded_layout = None;
            }
        }
        managed.insert(
            block.slug.clone(),
            PlannedBlock {
                uuid: entry.uuid.clone(),
                block_type: block.block_type.clone(),
                keys,
                ports: entry.ports.clone(),
                layout: entry.layout,
                relayout: relayout_pages.contains(page_uuid.as_str()),
                page_uuid,
                placed: true,
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
    // Message parameters of an `OutputRefLM` (D51), written onto its
    // `<LoggerMailer>` child.
    let mut managed_msgs: BTreeMap<(String, String), String> = BTreeMap::new();
    for block in module.blocks() {
        let attrs = attr_params(&block.block_type);
        for (k, v) in block.params() {
            // `mirrors:` is an identity binding, not a value (D33) —
            // resolved separately below.
            if is_ref_type(&block.block_type) && k == "mirrors" {
                continue;
            }
            if message_params(&block.block_type).contains(&k) {
                let value = resolve(v)?;
                let value = if k == "MinimumTime" {
                    seconds_to_ms(&value).ok_or_else(|| {
                        Error::Compile(format!(
                            "block `{}`: `MinimumTime: {v}` is not a duration",
                            block.slug
                        ))
                    })?
                } else {
                    value
                };
                managed_msgs.insert((block.slug.clone(), k.to_string()), value);
                continue;
            }
            let target = if attrs.contains(&k) {
                &mut managed_attrs // element attribute, not a connector Def
            } else {
                &mut managed_defs
            };
            let value = module.resolve_value_for(v, &block.block_type, &block.slug, k)?;
            target.insert((block.slug.clone(), k.to_string()), value.into_owned());
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
        /// `None` on a message ref (D51), which carries no code.
        link_type: Option<u32>,
        value: RefValue,
        /// The GUI derives a ref's `Title=` from its target (oracle
        /// session 11: a save rewrites whatever else stands there) — the
        /// compiler emits the derived title so the file converges.
        title: String,
        /// The mirrored object's XML type.
        target_type: String,
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
            // The title the target ends up with: its own title claim, else
            // the torn-down one — the base may still show a withdrawn claim
            // (D43).
            let title = match module
                .attrs()
                .find(|a| a.slug == target && a.attr == Attr::Title)
            {
                Some(a) => resolve(&a.value)?,
                None => read_claimed(&doc, u, Attr::Title)
                    .or(title)
                    .unwrap_or_else(|| target.to_string()),
            };
            (u.clone(), t, title)
        } else {
            return Err(Error::Compile(format!(
                "block `{}` mirrors `{target}`, but `{target}` is neither a \
                 managed block nor an extern of this module",
                block.slug
            )));
        };
        let (code, value) = if block.block_type == "OutputRefLM" {
            if target_type != MESSAGE_REF_TARGET {
                return Err(Error::Compile(format!(
                    "block `{}` mirrors `{target}` of type `{target_type}` — an \
                     OutputRefLM mirrors a `{MESSAGE_REF_TARGET}`",
                    block.slug
                )));
            }
            (None, RefValue::Digital)
        } else {
            let (code, value) = ref_link_type(&target_type).ok_or_else(|| {
                Error::Compile(format!(
                    "block `{}` mirrors `{target}` of type `{target_type}`, which has \
                     no verified `LinkRefType=` code yet — extend \
                     connectors::ref_link_type from corpus evidence before minting \
                     a ref of it",
                    block.slug
                ))
            })?;
            (Some(code), value)
        };
        mirror_of.insert(
            block.slug.clone(),
            Mirror {
                ref_uuid: uuid,
                link_type: code,
                value,
                title: target_title,
                target_type,
            },
        );
    }

    // --- Build the managed <C> elements and insert each into its pinned
    //     page, where the base had it (D47). The index is taken once after
    //     teardown; inserting into a page never shifts the page paths.
    let page_index = doc.index();
    // D46: managed blocks carry the document's block version, so a GUI
    // save has none to restamp — when the builtin table is verified for
    // it. Otherwise they keep the version the table was first verified
    // for, and Loxone Config migrates them on load.
    let block_version = doc
        .block_version()
        .filter(|v| VERIFIED_BLOCK_VERSIONS.contains(&v.as_str()))
        .unwrap_or_else(|| VERIFIED_BLOCK_VERSIONS[0].to_string());
    // A fresh VirtualIn gets the next free `VI<n>` designation (D54) —
    // past every one the torn-down base and the carried residue hold.
    let mut next_vi = doc
        .objects()
        .iter()
        .filter_map(|o| doc.element_at(&o.path)?.attr("IName").map(String::from))
        .chain(residue.values().flat_map(|r| {
            r.attrs
                .iter()
                .filter(|(n, _)| n == "IName")
                .map(|(_, v)| v.clone())
        }))
        .filter_map(|n| n.strip_prefix("VI")?.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    for block in module.blocks() {
        let plan = &managed[&block.slug];
        let res = residue.get(&plan.uuid);
        let carried = |name: &str| {
            res.and_then(|r| r.attrs.iter().find(|(n, _)| n == name))
                .map(|(_, v)| v.as_str())
        };
        let mut el = Element::new("C");
        el.set_attr("Type", &block.block_type);
        if block.block_type == "VirtualIn" {
            match carried("IName") {
                Some(v) => el.set_attr_raw("IName", v),
                None => {
                    el.set_attr("IName", &format!("VI{next_vi}"));
                    next_vi += 1;
                }
            }
        }
        el.set_attr("V", &block_version);
        el.set_attr("U", &plan.uuid);
        // A ref block's title is GUI-derived from its mirror target
        // (oracle session 11) — emit the derived value so a save is a
        // no-op; validate refuses an explicit label on ref blocks.
        match mirror_of.get(&block.slug) {
            Some(m) => el.set_attr("Title", &m.title),
            None => el.set_attr("Title", block.title.as_deref().unwrap_or(&block.slug)),
        }
        // Blocks without a position get theirs once the wires are drawn;
        // objects of the object tree have none (D54).
        if plan.placed {
            let at = plan.layout.unwrap_or(Layout::at(0, 0, 0, 0));
            el.set_attr("Px", &at.px.to_string());
            el.set_attr("Py", &at.py.to_string());
            el.set_attr("Px2", &at.px2.to_string());
            el.set_attr("Py2", &at.py2.to_string());
        }
        // D19: a block that existed in the base re-emits its GUI-owned
        // display state verbatim — including *absence* (some types lose
        // `WF` on a GUI save). Only a fresh mint writes the defaults; a
        // virtual input has the folder's grey (corpus) and the same `WF`
        // a save gives a page block (oracle session 18).
        let wf = "147456";
        let cl = if plan.placed {
            "141,255,112"
        } else {
            "238,238,238"
        };
        // A fresh message ref (D51) gets what Loxone Config gives one:
        // black, and no `WF=`.
        let message_ref = block.block_type == "OutputRefLM";
        match (res, carried("Cl")) {
            (Some(_), Some(v)) => el.set_attr_raw("Cl", v),
            (Some(_), None) => {}
            (None, _) if message_ref => el.set_attr("Cl", "0,0,0"),
            (None, _) => el.set_attr("Cl", cl),
        }
        el.set_attr("Nio", &plan.keys.len().to_string());
        // Minted-ref identity (D33), in the GUI's observed attribute
        // order: `Ref=` after `Nio=`; `Analog=` or `Text=` (D48), then
        // `LinkRefType=`, after `WF=`.
        if let Some(m) = mirror_of.get(&block.slug) {
            el.set_attr("Ref", &m.ref_uuid);
        }
        if let Some(v) = carried("LtE") {
            el.set_attr_raw("LtE", v);
        }
        match (res, carried("WF")) {
            (Some(_), Some(v)) => el.set_attr_raw("WF", v),
            (Some(_), None) => {}
            (None, _) if message_ref => {}
            (None, _) => el.set_attr("WF", wf),
        }
        if let Some(m) = mirror_of.get(&block.slug) {
            match m.value {
                RefValue::Digital => {}
                RefValue::Analog => el.set_attr("Analog", "true"),
                RefValue::Text => el.set_attr("Text", "true"),
            }
            if let Some(code) = m.link_type {
                el.set_attr("LinkRefType", &code.to_string());
            }
        }
        if block.block_type == "VirtualIn" {
            virtual_in_attrs(&mut el, &block.slug, &managed_attrs, res)?;
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
                // copy verbatim would clobber a retarget. A GUI-owned
                // attribute the source binds (`Code:`, D40) was written
                // above; the claim wins over the carried copy.
                let compiler_owned = matches!(n.as_str(), "Cl" | "LtE" | "WF" | "IName")
                    || (is_ref_type(&block.block_type)
                        && matches!(n.as_str(), "Ref" | "LinkRefType" | "Analog" | "Text"))
                    || managed_attrs.contains_key(&(block.slug.clone(), n.clone()));
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
        // The message of a message ref (D51) follows its connectors, as
        // Loxone Config writes it: the notification, the texts it sends on
        // and off (an empty one is left out), the least time between two
        // messages, and `NL="1"`, which every notification carries.
        if message_ref && let Some(m) = mirror_of.get(&block.slug) {
            let msg = |k: &str| managed_msgs.get(&(block.slug.clone(), k.to_string()));
            let mut lm = Element::new("LoggerMailer");
            lm.set_attr("RefNotification", &m.ref_uuid);
            for k in ["On", "Off"] {
                if let Some(text) = msg(k).filter(|t| !t.is_empty()) {
                    lm.set_attr(k, text);
                }
            }
            lm.set_attr(
                "MinimumTime",
                msg("MinimumTime").map_or(DEFAULT_MINIMUM_TIME_MS, String::as_str),
            );
            lm.set_attr("NL", "1");
            el.push_child(lm);
        }
        if let Some(r) = res {
            for c in &r.children {
                el.push_child(c.clone());
            }
            // D47: attributes and connectors in the order the base wrote
            // them; Loxone Config's order differs from ours (it writes
            // `Desc=` right after `Type=`, 17.2 moves a connector).
            el.attrs = in_base_order(std::mem::take(&mut el.attrs), |a| {
                r.attr_order.iter().position(|n| *n == a.name)
            });
            order_connectors(&mut el, &r.co_order);
        } else if block.block_type == "VirtualIn" {
            // What a save gives a fresh virtual input (oracle session
            // 18): shown in the app, one decimal — the GUI's from then on.
            let mut io = Element::new("IoData");
            io.set_attr("Visu", "true");
            io.set_attr("St", "2");
            el.push_child(io);
            let mut display = Element::new("Display");
            display.set_attr("Type", "1");
            display.set_attr("Unit", "<v.1>");
            display.set_attr("StateOnly", "true");
            el.push_child(display);
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
        let want = container_type(&block.block_type).unwrap_or("Page");
        if page.attr("Type") != Some(want) {
            return Err(Error::Compile(format!(
                "object `{}` recorded as the page for block `{}` is a `{}`, not a {want}",
                plan.page_uuid,
                block.slug,
                page.attr("Type").unwrap_or("?")
            )));
        }
        let order = base_page_order.get(&plan.page_uuid);
        insert_in_page(page, el, order.map_or(&[], Vec::as_slice));
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
            || !matches!(
                o.block_type.as_str(),
                "InputRef" | "OutputRef" | "OutputRefLM"
            )
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
        // The reverse half of an API link through a minted `OutputRef`
        // names the ref's `AI` as its source (`api_mirror_entry`); it
        // lands on the block's API connector, `OutputAPI` — never one the
        // builtin table proves output-only.
        let sink_type = managed
            .get(&to_ref.slug)
            .map(|p| p.block_type.as_str())
            .or_else(|| {
                extern_uuid
                    .get(&to_ref.slug)
                    .and_then(|u| type_by_uuid.get(u))
                    .map(|(t, _)| t.as_str())
            });
        let api_sink = to_ref.port == "OutputAPI"
            && !sink_type
                .and_then(builtin)
                .and_then(|specs| specs.iter().find(|s| s.key == "OutputAPI"))
                .is_some_and(|s| s.dir == PortDir::Output);
        let reverse_api = api_sink
            && managed.get(&from_ref.slug).is_some_and(|p| {
                api_mirror_entry(
                    &p.block_type,
                    &from_ref.port,
                    mirror_of
                        .get(&from_ref.slug)
                        .map(|m| m.target_type.as_str()),
                )
            });
        let mut from = resolve_port(
            &doc,
            &managed,
            &extern_uuid,
            &from_ref.slug,
            &from_ref.port,
            if reverse_api {
                PortDir::Input
            } else {
                PortDir::Output
            },
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
            match managed.get(slug) {
                Some(p) => p.placed.then(|| p.page_uuid.clone()),
                None => path_by_uuid.get(owner_uuid).and_then(|p| page_uuid_of(p)),
            }
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
        let key = (to.port_uuid.clone(), from.port_uuid.clone());
        let flg = residue
            .get(&to.owner_uuid)
            .and_then(|r| r.wire_flags.get(&key))
            .or_else(|| extern_wire_flags.get(&key));
        add_wire(
            &mut doc,
            &to.owner_uuid,
            &to.port_uuid,
            &from.port_uuid,
            flg.map(String::as_str),
            base_in_order.get(&to.port_uuid).map_or(&[], Vec::as_slice),
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
        let extern_type = module
            .externs()
            .find(|e| e.slug == s.target.slug)
            .map_or("", |e| e.block_type.as_str());
        let value = module
            .resolve_value_for(&s.value, extern_type, &s.target.slug, &s.target.port)?
            .into_owned();
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

    // --- Attribute claims on externs (D39). Planned against the torn-down
    //     document before anything is written, so nested claims (a device
    //     and one of its outputs) never record each other's write as their
    //     original, and an explicitly claimed descendant is never a
    //     follower.
    let managed_uuids: BTreeSet<&str> = managed.values().map(|b| b.uuid.as_str()).collect();
    let mut claimed: BTreeMap<(String, Attr), &str> = BTreeMap::new();
    // A managed block's room and category (D63): written onto the rebuilt
    // block on every compile, no lock state — the block is the source's.
    let mut managed_claims: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
    for a in module.attrs().filter(|a| managed.contains_key(&a.slug)) {
        let value = resolve(&a.value)?;
        let entry = managed_claims
            .entry(managed[&a.slug].uuid.clone())
            .or_default();
        match a.attr {
            Attr::Room => entry.0 = Some(taxonomy_uuid(&doc, "Place", "room", &value)?),
            Attr::Category => {
                entry.1 = Some(taxonomy_uuid(&doc, "Category", "category", &value)?);
            }
            Attr::Title => unreachable!("validate refuses a managed block's `.title`"),
        }
    }
    for (uuid, (room, category)) in &managed_claims {
        write_binding(&mut doc, uuid, room.as_deref(), category.as_deref());
    }
    for a in module.attrs().filter(|a| !managed.contains_key(&a.slug)) {
        let uuid = extern_uuid
            .get(&a.slug)
            .expect("validate admitted only extern targets");
        if let Some(other) = claimed.insert((uuid.clone(), a.attr), &a.slug) {
            return Err(Error::Compile(format!(
                "`{}.{attr}` and `{other}.{attr}` claim the {attr} of the same \
                 object ({uuid}) — keep one assignment",
                a.slug,
                attr = a.attr
            )));
        }
    }
    let mut planned_claims: Vec<(String, Attr, AttrClaim)> = Vec::new();
    for a in module.attrs().filter(|a| !managed.contains_key(&a.slug)) {
        let uuid = extern_uuid[&a.slug].clone();
        let value = resolve(&a.value)?;
        let written = match a.attr {
            Attr::Title => value,
            Attr::Room => taxonomy_uuid(&doc, "Place", "room", &value)?,
            Attr::Category => taxonomy_uuid(&doc, "Category", "category", &value)?,
        };
        let original = read_claimed(&doc, &uuid, a.attr).ok_or_else(|| {
            Error::Compile(format!(
                "`{}.{}`: the object ({uuid}) carries no {} — lxir rewrites an \
                 existing {}, it never adds one; set it once in Loxone Config",
                a.slug,
                a.attr,
                claim_slot(a.attr),
                claim_slot(a.attr)
            ))
        })?;
        let stop =
            |u: &str| managed_uuids.contains(u) || claimed.contains_key(&(u.to_string(), a.attr));
        let followers = match a.attr {
            Attr::Title => ref_followers(&doc, &uuid, &stop),
            Attr::Room | Attr::Category => followers_of(&doc, &uuid, a.attr, &original, &stop),
        };
        planned_claims.push((
            uuid,
            a.attr,
            AttrClaim {
                original,
                written,
                followers,
            },
        ));
    }
    for (uuid, attr, c) in planned_claims {
        write_claimed(&mut doc, &uuid, attr, &c.written);
        for f in &c.followers {
            write_claimed(&mut doc, f, attr, &c.written);
        }
        lock.attr_claims.entry(uuid).or_default().insert(attr, c);
    }
    // A claim that vanished from source was withdrawn — tombstone it (D31)
    // unless it never changed the object's bytes in the first place.
    for (uuid, claims) in &old_claims {
        for (attr, c) in claims {
            if lock
                .attr_claims
                .get(uuid)
                .is_some_and(|m| m.contains_key(attr))
            {
                continue; // still claimed from source
            }
            if let Some(written) = claims_written.get(&(uuid.clone(), *attr))
                && *written != c.original
            {
                lock.removed_attrs.entry(uuid.clone()).or_default().insert(
                    *attr,
                    AttrClaim {
                        original: c.original.clone(),
                        written: written.clone(),
                        followers: c.followers.clone(),
                    },
                );
            }
        }
    }

    // --- Touch Pure Flex tiles (D41). Last of the writes: a `to:` target
    //     must be API-linked to the device, and that link may be a wire
    //     this compile just drew. Like `Code:` (D40) the claim keeps no
    //     lock state — a withdrawn claim leaves the pushed programs.
    let mut uuid_of: BTreeMap<String, String> = extern_uuid
        .iter()
        .map(|(s, u)| (s.clone(), u.clone()))
        .chain(managed.iter().map(|(s, b)| (s.clone(), b.uuid.clone())))
        .collect();
    super::tiles::apply(&mut doc, module, &uuid_of)?;
    // DayTimer schedules (D53): the claimed `Modes=`/`N=`/`<Entry>` set,
    //     on extern and managed timers alike; no lock state either.
    super::schedule::apply(&mut doc, module, &uuid_of)?;

    // --- Access rights on NFC Code Touch blocks (D49): users, groups and
    //     schedules are the base's; no lock state, like tiles.
    super::access::apply(&mut doc, module, &uuid_of)?;
    // Automatic Designer rules (D57); a named one (D60) is found or
    //     created first, its UUID pinned in the lock while it is declared.
    //     A rule or mood minted here moves the mint mark.
    let mint_mark = minter.counter();
    lock.rules = super::rule::create(&mut doc, module, &uuid_of, &lock.rules, &mut minter)?;
    uuid_of.extend(lock.rules.iter().map(|(s, u)| (s.clone(), u.clone())));
    super::rule::apply(&mut doc, module, &uuid_of)?;
    // LightController2 moods (D58): the claimed `<LightscenesC>` list; a
    //     new mood mints its UUID (pinned in the lock until the base has
    //     it).
    let minted_moods = std::mem::take(&mut lock.moods);
    lock.moods = super::moods::apply(&mut doc, module, &uuid_of, &minted_moods, &mut minter)?;
    if minter.counter() != mint_mark {
        lock.counters.next_mint = minter.counter();
    }

    // --- Placement (D42), once every wire is drawn: blocks without a
    //     position, and those on pages laid out afresh, go where their
    //     wires want them on the page as it now stands.
    let mut to_place: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for block in module.blocks() {
        let plan = &managed[&block.slug];
        if plan.placed && (plan.layout.is_none() || plan.relayout) {
            to_place
                .entry(plan.page_uuid.as_str())
                .or_default()
                .push(block.slug.as_str());
        }
    }
    for (page_uuid, slugs) in &to_place {
        let geo = PageGeometry::of(&doc, page_uuid).expect("blocks were appended to this page");
        let foreign_ref = |j: usize| {
            is_ref_type(&geo.objects[j].block_type)
                && !managed_uuids.contains(geo.objects[j].uuid.as_str())
        };
        let mut free = Vec::new();
        let mut prev = BTreeMap::new();
        let mut slug_of = BTreeMap::new();
        for slug in slugs {
            let plan = &managed[*slug];
            let i = geo
                .index_of(&plan.uuid)
                .expect("the block was appended to its page");
            if let Some(l) = plan.layout {
                // Wired to a ref the compile does not own and drawn beside
                // it: arranged with that ref in the GUI, so it stays. A
                // wire routed through a ref elsewhere (D34) pins nothing.
                let pinned = geo.wires.iter().any(|w| {
                    let other = match (w.from.0 == i, w.to.0 == i) {
                        (true, _) => w.to.0,
                        (_, true) => w.from.0,
                        _ => return false,
                    };
                    foreign_ref(other) && beside(&geo.objects[i].rect, &geo.objects[other].rect)
                });
                if pinned {
                    continue;
                }
                prev.insert(i, l);
            }
            free.push(i);
            slug_of.insert(i, *slug);
        }
        let placed = crate::layout::place(&geo, &free, &prev);
        let index = doc.index();
        for (i, at) in placed {
            let slug = slug_of[&i];
            let uuid = &managed[slug].uuid;
            // Placed where the base already draws it: the GUI's rectangle.
            let base = residue.get(uuid).and_then(|r| r.layout_on(page_uuid));
            let at = match base {
                Some(b) if b.same_spot(&at) => b,
                _ => at,
            };
            let el = doc
                .element_at_mut(&index.by_uuid[uuid])
                .expect("indexed path");
            el.set_attr("Px", &at.px.to_string());
            el.set_attr("Py", &at.py.to_string());
            el.set_attr("Px2", &at.px2.to_string());
            el.set_attr("Py2", &at.py2.to_string());
            let entry = lock.objects.get_mut(slug).expect("planned from the lock");
            entry.layout = Some(at);
            entry.superseded_layout = base.filter(|b| !b.same_spot(&at));
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

/// A VirtualIn's input-dialog attributes (D54) in the order a Loxone
/// Config save writes them. Each is the source's binding, else the base's
/// value, else — on a fresh mint only — what a save fills in (oracle
/// session 18): an analog input gets `EnVal="true"`, a digital one the
/// 0/1 range and its change filter.
fn virtual_in_attrs(
    el: &mut Element,
    slug: &str,
    bound: &BTreeMap<(String, String), String>,
    res: Option<&Residue>,
) -> Result<()> {
    let bound = |n: &str| bound.get(&(slug.to_string(), n.to_string()));
    let carried = |n: &str| {
        res.and_then(|r| r.attrs.iter().find(|(a, _)| a == n))
            .map(|(_, v)| v.as_str())
    };
    let analog = match bound("Analog") {
        Some(v) if v == "true" || v == "false" => v.as_str(),
        Some(v) => {
            return Err(Error::Compile(format!(
                "block `{slug}`: `Analog:` is \"true\" or \"false\", not `{v}`"
            )));
        }
        None => carried("Analog").unwrap_or("false"),
    };
    let fresh: &[(&str, &str)] = if analog == "true" {
        &[("Analog", "true"), ("EnVal", "true")]
    } else {
        &[
            ("Analog", "false"),
            ("MaxVal", "1"),
            ("MinChange", "0.5"),
            ("MinTime", "10"),
        ]
    };
    for name in [
        "Analog",
        "EnVal",
        "DefVal",
        "MinVal",
        "MaxVal",
        "MinChange",
        "MinTime",
    ] {
        if let Some(v) = bound(name) {
            el.set_attr(name, v);
        } else if let Some(v) = carried(name) {
            el.set_attr_raw(name, v);
        } else if res.is_none()
            && let Some((_, v)) = fresh.iter().find(|(n, _)| *n == name)
        {
            el.set_attr(name, v);
        }
    }
    Ok(())
}

struct PlannedBlock {
    uuid: String,
    block_type: String,
    /// All port keys in connector-index order.
    keys: Vec<String>,
    ports: BTreeMap<String, String>,
    /// `None` until placement (D42) gives the block a position.
    layout: Option<Layout>,
    /// The block's page is laid out afresh this compile.
    relayout: bool,
    /// UUID of the page the block is (re)built on — or, when not
    /// `placed`, of its object-tree container (D54).
    page_uuid: String,
    /// Drawn on a page (laid out, `Px=`/`Py=`); false for objects of the
    /// object tree.
    placed: bool,
}

struct ResolvedPort {
    owner_uuid: String,
    port_uuid: String,
    is_extern: bool,
}

/// The object each extern of `module` resolves to in `base` (slug →
/// UUID), the way [`compile`] pins it; `lock` stays as it is. The
/// language server shows this identity on hover.
pub fn resolve_extern_uuids(
    base: &LoxoneDoc,
    module: &Module,
    lock: &Lockfile,
) -> Result<BTreeMap<String, String>> {
    resolve_externs(base, &module.expand()?, &mut lock.clone())
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
                    && ext.device.as_ref().is_none_or(|want| {
                        doc.device_serial(&o.path)
                            .is_some_and(|s| s.eq_ignore_ascii_case(want))
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
                if !matches!(
                    ext.block_type.as_str(),
                    "InputRef" | "OutputRef" | "OutputRefLM"
                ) {
                    return Err(Error::Compile(format!(
                        "extern `{}`: `mirrors:` applies to InputRef/OutputRef/OutputRefLM only, \
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

/// `items` with the ones `rank` places in rank order, the base's (D47).
/// An item the base did not have stays right behind the item it follows
/// in `items`, so a value new to the block keeps its emitted neighbour.
fn in_base_order<T>(items: Vec<T>, rank: impl Fn(&T) -> Option<usize>) -> Vec<T> {
    let mut prev = None;
    let mut keyed: Vec<_> = items
        .into_iter()
        .enumerate()
        .map(|(i, item)| {
            let key = match rank(&item) {
                Some(r) => {
                    prev = Some(r);
                    (Some(r), false, i)
                }
                None => (prev, true, i),
            };
            (key, item)
        })
        .collect();
    keyed.sort_by_key(|(key, _)| *key);
    keyed.into_iter().map(|(_, item)| item).collect()
}

/// Reorder the block's `<Co>` children into `base` key order, leaving
/// every other child where it is.
fn order_connectors(el: &mut Element, base: &[String]) {
    use crate::xml::Node;
    let slots: Vec<usize> = (0..el.children.len())
        .filter(|&i| matches!(&el.children[i], Node::Element(e) if e.name == "Co"))
        .collect();
    let cos: Vec<Node> = slots
        .iter()
        .map(|&i| std::mem::replace(&mut el.children[i], Node::Text(String::new())))
        .collect();
    let rank = |n: &Node| match n {
        Node::Element(e) => e
            .attr_decoded("K")
            .and_then(|k| base.iter().position(|b| *b == k)),
        Node::Text(_) => None,
    };
    for (i, co) in slots.into_iter().zip(in_base_order(cos, rank)) {
        el.children[i] = co;
    }
}

/// Create the pages the source names with `room:` that the base has no
/// page titled for (D56), as Loxone Config's "Neue Seite" does: a
/// `<C Type="Page">` in the program, sorted in by title, assigned to the
/// room and category through its `<IoData>`. The lock remembers each
/// created page's UUID — a recompile against a base from before the
/// deployment re-creates the same page, and a base where Loxone Config
/// renamed it is an error instead of a second page. Once the base carries
/// the page, Loxone Config owns it like any other; a vanished statement
/// only releases the lock entry, it never deletes a page.
fn create_pages(
    doc: &mut LoxoneDoc,
    module: &Module,
    lock: &mut Lockfile,
    minter: &mut Minter,
) -> Result<()> {
    // Title → (room, category); the statements for one title must agree.
    let mut wanted: BTreeMap<&str, (&str, Option<&str>)> = BTreeMap::new();
    for item in &module.items {
        let Item::Page(p) = item else { continue };
        let Some(room) = p.room.as_deref() else {
            continue;
        };
        let spec = (room, p.category.as_deref());
        if let Some(prev) = wanted.insert(p.title.as_str(), spec)
            && prev != spec
        {
            return Err(Error::Compile(format!(
                "`page {:?}` is created with two different room/category arguments — \
                 give them once, or the same everywhere",
                p.title
            )));
        }
    }
    let old = std::mem::take(&mut lock.pages);
    if wanted.is_empty() {
        return Ok(());
    }
    let objects = doc.objects();
    let block_version = doc
        .block_version()
        .unwrap_or_else(|| VERIFIED_BLOCK_VERSIONS[VERIFIED_BLOCK_VERSIONS.len() - 1].into());
    for (title, (room, category)) in wanted {
        let page_titled = |want: &str| {
            objects
                .iter()
                .find(|o| o.block_type == "Page" && o.title.as_deref() == Some(want))
        };
        if let Some(uuid) = old.get(title)
            && let Some(o) = objects.iter().find(|o| &o.uuid == uuid)
        {
            let now = o.title.as_deref().unwrap_or_default();
            if now != title {
                return Err(Error::Compile(format!(
                    "page {title:?} was created by the compiler as {uuid}; the base \
                     titles it {now:?} — renamed in Loxone Config? Write \
                     `page {now:?}` in the source (the page then belongs to Loxone \
                     Config), or rename it back"
                )));
            }
            lock.pages.insert(title.to_string(), uuid.clone());
            continue;
        }
        if page_titled(title).is_some() {
            continue; // Loxone Config's page: nothing to create.
        }
        let place = taxonomy_uuid(doc, "Place", "room", room)?;
        let first = |uuid: &str| {
            objects.iter().any(|o| {
                o.uuid == uuid
                    && doc
                        .element_at(&o.path)
                        .is_some_and(|el| el.attr("First") == Some("true"))
            })
        };
        if first(&place) {
            return Err(Error::Compile(format!(
                "`page {title:?} (room: {room:?})`: a page needs a real room — Loxone \
                 Config does not accept the unassigned one"
            )));
        }
        let cat = match category {
            Some(c) => taxonomy_uuid(doc, "Category", "category", c)?,
            None => objects
                .iter()
                .find(|o| o.block_type == "Category" && first(&o.uuid))
                .map(|o| o.uuid.clone())
                .ok_or_else(|| {
                    Error::Compile(format!(
                        "`page {title:?}`: the base has no unassigned category \
                         (<C Type=\"Category\" First=\"true\">) — name one with `category:`"
                    ))
                })?,
        };
        let uuid = match old.get(title) {
            Some(u) => u.clone(),
            None => {
                lock.counters.next_obj += 1;
                minter.mint_object().to_string()
            }
        };
        let program = objects
            .iter()
            .find(|o| o.block_type == "Program")
            .map(|o| o.path.clone())
            .ok_or_else(|| {
                Error::Compile(format!(
                    "`page {title:?}`: the base has no <C Type=\"Program\"> to hold a page"
                ))
            })?;
        let mut el = Element::new("C");
        el.set_attr("Type", "Page");
        el.set_attr_raw("V", &block_version);
        el.set_attr("U", &uuid);
        el.set_attr("Title", title);
        el.set_attr("WF", "16384");
        let mut io = Element::new("IoData");
        io.set_attr("Cr", &cat);
        io.set_attr("Pr", &place);
        el.push_child(io);
        let program = doc
            .element_at_mut(&program)
            .expect("the object's path is in the document");
        insert_page(program, el, title);
        lock.pages.insert(title.to_string(), uuid);
    }
    Ok(())
}

/// Where Loxone Config files a new page: among the program's pages in
/// case-insensitive title order. The GUI does not re-sort on load, so a
/// base's list need not be sorted: the page goes behind the last page
/// whose title sorts at or before its own, else behind the special pages
/// (`SpNo`) leading the list, else first.
fn insert_page(program: &mut Element, el: Element, title: &str) {
    use crate::xml::Node;
    let key = title.to_lowercase();
    fn page(n: &Node) -> Option<&Element> {
        match n {
            Node::Element(e) if e.name == "C" && e.attr("Type") == Some("Page") => Some(e),
            _ => None,
        }
    }
    let after = |n: &Node| {
        page(n).is_some_and(|e| {
            e.attr("SpNo").is_some()
                || e.attr_decoded("Title")
                    .is_some_and(|t| t.to_lowercase() <= key)
        })
    };
    let at = match program.children.iter().rposition(after) {
        Some(i) => i + 1,
        None => program
            .children
            .iter()
            .position(|n| page(n).is_some())
            .unwrap_or(program.children.len()),
    };
    program.children.insert(at, Node::Element(el));
    program.self_closing = false;
}

/// Insert a rebuilt block into its page where the base had it (D47):
/// before the first object the base listed after it, or that the base did
/// not have; else behind the page's last object. A block new to the page
/// is appended, as before.
fn insert_in_page(page: &mut Element, el: Element, base: &[String]) {
    use crate::xml::Node;
    let rank = |u: Option<&str>| u.and_then(|u| base.iter().position(|b| b == u));
    let Some(mine) = rank(el.attr("U")) else {
        page.push_child(el);
        return;
    };
    let object = |n: &Node| matches!(n, Node::Element(e) if e.name == "C");
    let at = page
        .children
        .iter()
        .position(|n| {
            matches!(n, Node::Element(e)
                if e.name == "C" && rank(e.attr("U")).is_none_or(|r| r > mine))
        })
        .or_else(|| page.children.iter().rposition(object).map(|i| i + 1));
    match at {
        Some(i) => page.children.insert(i, Node::Element(el)),
        None => page.push_child(el),
    }
}

/// Put `<In Input=from/>` under the sink `<Co>` and maintain `Nc`.
/// `flg` re-emits a harvested `FLG=` wire flag verbatim (D19 residue);
/// `base_order` is the sink's `<In>` sources as the base listed them.
fn add_wire(
    doc: &mut LoxoneDoc,
    owner_uuid: &str,
    to_port: &str,
    from_port: &str,
    flg: Option<&str>,
    base_order: &[String],
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
    // A declared wire keeps the place the base gave it (D45). `<In>` order
    // is semantically unordered, but Loxone Config sorts it on save; a
    // wire the teardown removed goes back before the first `<In>` the base
    // listed after it, so compiling a GUI-saved base reproduces it byte
    // for byte. An already-present duplicate (first compile after adopt —
    // the wire exists in the base) stays where it is, keeping its
    // attributes, which is where every later compile puts it back. Only a
    // wire the base did not carry is appended.
    let exists = co.children.iter().any(|n| {
        matches!(n, crate::xml::Node::Element(e)
            if e.name == "In" && e.attr("Input") == Some(from_port))
    });
    if !exists {
        let mut input = Element::new("In");
        input.set_attr("Input", from_port);
        if let Some(v) = flg {
            input.set_attr_raw("FLG", v);
        }
        let rank = |src: &str| base_order.iter().position(|s| s == src);
        let before = rank(from_port).and_then(|mine| {
            co.children.iter().position(|n| {
                matches!(n, crate::xml::Node::Element(e)
                    if e.name == "In"
                        && e.attr("Input").and_then(rank).is_none_or(|r| r > mine))
            })
        });
        match before {
            Some(i) => co.children.insert(i, crate::xml::Node::Element(input)),
            None => co.push_child(input),
        }
    }
    sync_nc(co);
    Ok(())
}

/// The raw `FLG=` of the `<In>` wiring `from_port` into `to_port`.
fn wire_flag(doc: &mut LoxoneDoc, from_port: &str, to_port: &str) -> Option<String> {
    find_co_mut(&mut doc.xml.root, to_port)?
        .child_elements()
        .find(|e| e.name == "In" && e.attr("Input") == Some(from_port))?
        .attr("FLG")
        .map(str::to_string)
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

/// Where a claimed attribute lives, for messages.
fn claim_slot(attr: Attr) -> &'static str {
    match attr {
        Attr::Title => "title (`Title=`)",
        Attr::Room => "room binding (`<IoData Pr=…>`)",
        Attr::Category => "category binding (`<IoData Cr=…>`)",
    }
}

fn find_c<'a>(el: &'a Element, uuid: &str) -> Option<&'a Element> {
    if el.name == "C" && el.attr("U") == Some(uuid) {
        return Some(el);
    }
    el.child_elements().find_map(|c| find_c(c, uuid))
}

/// A claimed attribute on one `<C>`: the element's own `Title=`, or `Pr=`
/// / `Cr=` on its `<IoData>` child.
fn claimed_value(c: &Element, attr: Attr) -> Option<String> {
    let v = match attr {
        Attr::Title => c.attr_decoded("Title"),
        Attr::Room | Attr::Category => c
            .child_elements()
            .find(|e| e.name == "IoData")?
            .attr_decoded(if attr == Attr::Room { "Pr" } else { "Cr" }),
    };
    v.map(|v| v.into_owned())
}

fn read_claimed(doc: &LoxoneDoc, uuid: &str, attr: Attr) -> Option<String> {
    claimed_value(find_c(&doc.xml.root, uuid)?, attr)
}

/// Rewrite a claimed attribute in place. Only ever called where the
/// attribute exists (claims refuse objects without it), so position and
/// form of the element are untouched.
fn write_claimed(doc: &mut LoxoneDoc, uuid: &str, attr: Attr, value: &str) {
    let Some(c) = find_c_mut(&mut doc.xml.root, uuid) else {
        return;
    };
    let (el, name) = match attr {
        Attr::Title => (Some(c), "Title"),
        Attr::Room => (c.child_elements_mut().find(|e| e.name == "IoData"), "Pr"),
        Attr::Category => (c.child_elements_mut().find(|e| e.name == "IoData"), "Cr"),
    };
    if let Some(el) = el
        && el.attr(name).is_some()
    {
        el.set_attr(name, value);
    }
}

/// Bind a managed block to a room and/or category: `Pr=`/`Cr=` on its
/// `<IoData>`. A block without one — minted, never saved by Loxone
/// Config — gets the element Loxone Config gives a new block: after its
/// connectors, shown in the user interface (`Visu="true"`), so the
/// assignment puts it in the app.
fn write_binding(doc: &mut LoxoneDoc, uuid: &str, room: Option<&str>, category: Option<&str>) {
    let Some(c) = find_c_mut(&mut doc.xml.root, uuid) else {
        return;
    };
    if !c.child_elements().any(|e| e.name == "IoData") {
        let mut io = Element::new("IoData");
        io.set_attr("Visu", "true");
        let at = c
            .children
            .iter()
            .rposition(|n| matches!(n, crate::xml::Node::Element(e) if e.name == "Co"))
            .map_or(c.children.len(), |i| i + 1);
        c.self_closing = false;
        c.children.insert(at, crate::xml::Node::Element(io));
    }
    let io = c
        .child_elements_mut()
        .find(|e| e.name == "IoData")
        .expect("inserted above");
    // Loxone Config's attribute order: `Cr` before `Pr`.
    if let Some(v) = category {
        io.set_attr("Cr", v);
    }
    if let Some(v) = room {
        io.set_attr("Pr", v);
    }
}

/// Undo one claim: the object back to its original, and each follower that
/// still shows the written value.
fn restore_claim(doc: &mut LoxoneDoc, uuid: &str, attr: Attr, c: &AttrClaim) {
    write_claimed(doc, uuid, attr, &c.original);
    for f in &c.followers {
        if read_claimed(doc, f, attr).as_deref() == Some(c.written.as_str()) {
            write_claimed(doc, f, attr, &c.original);
        }
    }
}

/// The descendants of `uuid` whose room/category equals the object's
/// `original` — those follow a claim on it (D39). `stop` excludes an
/// object and its subtree: managed blocks (their binding is D19 residue)
/// and descendants claimed explicitly for the same attribute.
fn followers_of(
    doc: &LoxoneDoc,
    uuid: &str,
    attr: Attr,
    original: &str,
    stop: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    fn walk(
        el: &Element,
        attr: Attr,
        original: &str,
        stop: &dyn Fn(&str) -> bool,
        out: &mut Vec<String>,
    ) {
        for c in el.child_elements().filter(|c| c.name == "C") {
            let Some(u) = c.attr("U") else { continue };
            if stop(u) {
                continue;
            }
            if claimed_value(c, attr).as_deref() == Some(original) {
                out.push(u.to_string());
            }
            walk(c, attr, original, stop, out);
        }
    }
    let mut out = Vec::new();
    if let Some(root) = find_c(&doc.xml.root, uuid) {
        walk(root, attr, original, stop, &mut out);
    }
    out
}

/// The refs that copy `uuid`'s title — those follow a title claim on it
/// (D43): every `InputRef`/`OutputRef` with `Ref=` the object, since a
/// save re-derives their `Title=` from the target whatever stood there.
/// Not the inputs an alarm block collected itself (`WF` bit 0x1000 on an
/// `InputRef`): the GUI keeps their titles. `stop` excludes managed refs
/// (emitted with the derived title already) and explicitly claimed ones.
fn ref_followers(doc: &LoxoneDoc, uuid: &str, stop: &dyn Fn(&str) -> bool) -> Vec<String> {
    const COLLECTED: u32 = 0x1000;
    doc.objects()
        .into_iter()
        .filter(|o| is_ref_type(&o.block_type) && o.title.is_some() && !stop(&o.uuid))
        .filter(|o| {
            let el = doc.element_at(&o.path).expect("path from objects()");
            let wf = el
                .attr("WF")
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0);
            el.attr("Ref") == Some(uuid) && !(o.block_type == "InputRef" && wf & COLLECTED != 0)
        })
        .map(|o| o.uuid)
        .collect()
}

/// The UUID of the one `Place`/`Category` titled `title` — zero or several
/// are refused (a room is named by its title; there is nothing else to
/// pick by).
fn taxonomy_uuid(doc: &LoxoneDoc, ty: &str, what: &str, title: &str) -> Result<String> {
    let all: Vec<_> = doc
        .objects()
        .into_iter()
        .filter(|o| o.block_type == ty)
        .collect();
    let hits: Vec<_> = all
        .iter()
        .filter(|o| o.title.as_deref() == Some(title))
        .collect();
    match hits.as_slice() {
        [one] => Ok(one.uuid.clone()),
        [] => {
            let mut known: Vec<&str> = all.iter().filter_map(|o| o.title.as_deref()).collect();
            known.sort_unstable();
            let hint = suggest(title, known.iter().copied());
            Err(Error::Compile(format!(
                "no {what} titled {title:?} in the base{hint} — {what}s are created in \
                 Loxone Config; known: {}",
                known.join(", ")
            )))
        }
        _ => Err(Error::Compile(format!(
            "{} {what}s are titled {title:?} ({}) — rename one in Loxone Config",
            hits.len(),
            hits.iter()
                .map(|o| o.uuid.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// `MinimumTime=` of a message ref without a binding: Loxone Config's
/// default, 5 s (the GUI writes it for a new block; 53 corpus
/// `<LoggerMailer>` elements carry it).
const DEFAULT_MINIMUM_TIME_MS: &str = "5000";

/// A duration in seconds, as the IR resolves it (D27), in the whole
/// milliseconds `MinimumTime=` stores.
fn seconds_to_ms(seconds: &str) -> Option<String> {
    let s: f64 = seconds.parse().ok()?;
    (s.is_finite() && s >= 0.0).then(|| format!("{}", (s * 1000.0).round() as u64))
}

#[cfg(test)]
mod tests {
    use super::in_base_order;

    #[test]
    fn base_order_wins_and_new_items_keep_their_neighbour() {
        let base = ["Type", "Desc", "V", "U", "Title"];
        let rank = |a: &&str| base.iter().position(|b| b == a);
        // Emitted: `Desc=` late, `Nio=` new to the block, `New=` first.
        let emitted = vec!["New", "Type", "V", "U", "Title", "Nio", "Desc"];
        assert_eq!(
            in_base_order(emitted, rank),
            ["New", "Type", "Desc", "V", "U", "Title", "Nio"]
        );
    }
}
