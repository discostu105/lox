//! The compiler: base config + Lox Logic module + lockfile → new config.
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
    MESSAGE_REF_TARGET, PortDir, RefValue, VERIFIED_BLOCK_VERSIONS, adopt_only, attr_name,
    attr_param, attr_params, builtin, carried_attrs, carried_children, container_type,
    message_params, object_tree, ref_link_type, save_defaults,
};
use crate::doc::{Counters, GUI_OWNED_ATTRS, GUI_OWNED_CHILDREN, LoxoneDoc, WireView, ports};
use crate::error::{Error, Result};
use crate::layout::{PageGeometry, beside};
use crate::lock::{
    AttrClaim, Layout, LockedExternal, LockedObject, LockedWire, Lockfile, sha256_hex,
};
use crate::logic::ast::{Attr, BlockDecl, Item, MatchSpec, Module, Value};
use crate::logic::validate::{api_mirror_entry, is_ref_type, suggest, validate_ports};
use crate::uuid::{Minter, entity_for_slug};
use crate::xml::Element;
use std::collections::{BTreeMap, BTreeSet};

mod claims;
mod emit;
mod plan;
mod teardown;
mod wires;
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
    /// The source may not wire or set these (see [`add_wire`] /
    /// [`apply_def`] and the param guard in the build loop), so carrying
    /// them can never contradict what the source expresses.
    gui_ports: BTreeMap<String, Element>,
    /// Stale constants (design decision D77): the raw `Def=` of every
    /// wired connector, keyed by `K`. Inert while the wire exists; Loxone
    /// Config keeps it so the value returns when the wire is removed. The
    /// source never states it (a value next to a wire is refused), so the
    /// rebuild carries it onto a port the source still wires.
    stale_defs: BTreeMap<String, String>,
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
    // A virtual or Modbus device and its I/Os (D64) carry everything the
    // rebuild does not write: the source states identity and settings,
    // the rest of the element is the GUI's (D19).
    let periphery = crate::periphery::periphery(el.attr("Type").unwrap_or("")).is_some();
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
                (periphery && !matches!(a.name.as_str(), "Type" | "V" | "U" | "Title" | "Nio"))
                    || matches!(a.name.as_str(), "Cl" | "LtE" | "WF")
                    || GUI_OWNED_ATTRS.contains(&a.name.as_str())
                    || carried_attrs(el.attr("Type").unwrap_or("")).contains(&a.name.as_str())
            })
            .map(|a| (a.name.clone(), a.value.clone()))
            .collect(),
        children: el
            .child_elements()
            .filter(|c| {
                (periphery && c.name != "Co")
                    || GUI_OWNED_CHILDREN.contains(&c.name.as_str())
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
        stale_defs: el
            .child_elements()
            .filter(|c| c.name == "Co" && c.child_elements().any(|i| i.name == "In"))
            .filter_map(|co| {
                Some((
                    co.attr_decoded("K")?.into_owned(),
                    co.attr("Def")?.to_string(),
                ))
            })
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

    check_config_version(base, lock, opts)?;
    // --- Identity: everything below keys on UUIDs, so a base whose UUIDs
    //     repeat would have each lookup silently pick one twin.
    base.require_unique_uuids("base")?;

    apply_moved(module, lock)?;
    let src_slugs = check_removals(base, module, lock, opts)?;

    let mut doc = base.clone();
    lock.absorb_counters(doc.counters());

    // --- Resolve externs (against the untouched base).
    let extern_uuid = resolve_externs(&doc, module, lock)?;
    check_externs_unmanaged(&extern_uuid, module, lock)?;
    let snap = teardown::snapshot_base(&doc, lock);

    // --- Tear down our previous output: managed objects, extern wires,
    //     extern sets. What remains is exactly the Loxone-Config-owned state.
    //     Each removed element hands back its GUI-owned residue (D19) so
    //     the rebuild carries it forward verbatim.
    let (residue, in_torn_down) = teardown::tear_down_managed(&mut doc, lock, &snap.base_page);
    teardown::sweep_tombstones(&mut doc, lock, &src_slugs, &residue, in_torn_down);
    let prior = teardown::tear_down_extern_edits(&mut doc, lock);

    let mut minter = seeded_minter(lock, opts);
    let mint_base = minter.counter();

    // --- Pages the source asks for and the base lacks are created (D56),
    //     before anything is pinned to them.
    create_pages(&mut doc, module, lock, &mut minter)?;

    let mut managed = plan::plan_blocks(
        &doc,
        module,
        lock,
        opts,
        &mut minter,
        &desugar_info,
        &residue,
    )?;
    plan::place_ios_in_devices(module, &mut managed, &extern_uuid, &snap.base_parent)?;

    // Record the mint high-water mark — only when something was minted, so
    // a mint-free compile leaves the lock byte-identical.
    if minter.counter() != mint_base {
        lock.counters.next_mint = minter.counter();
    }

    let params = plan::collect_params(module)?;

    let mirror_of =
        plan::resolve_mirrors(&doc, module, &managed, &extern_uuid, &snap.type_by_uuid)?;
    let cx = Rebuild {
        module,
        managed: &managed,
        extern_uuid: &extern_uuid,
        residue: &residue,
        snap: &snap,
        mirror_of: &mirror_of,
    };

    emit::emit_blocks(&mut doc, lock, &cx, &params)?;

    wires::draw_wires(&mut doc, lock, &cx, &prior)?;

    claims::apply_sets(&mut doc, lock, &cx)?;
    claims::retire_sets(lock, &prior);

    claims::apply_claims(&mut doc, lock, &cx)?;
    claims::retire_claims(lock, &prior);

    apply_features(&mut doc, base, lock, &mut minter, &cx)?;

    place_blocks(&mut doc, lock, &cx);

    finish(&mut doc, base, lock, opts)?;
    Ok(doc)
}

/// What the base carried before teardown, read once from the untouched
/// base: the rebuild puts wires, blocks and I/Os back where they were.
struct BaseSnapshot {
    /// XML type and title per object, for `mirrors:` targets (D33).
    type_by_uuid: BTreeMap<String, (String, Option<String>)>,
    /// Every wire the base carried: the D34 routing pin and the out-routes.
    all_base_wires: Vec<WireView>,
    /// The same wires as (source port, sink port).
    base_wires: BTreeSet<(String, String)>,
    /// Each sink's `<In>` sources in base order (D45).
    base_in_order: BTreeMap<String, Vec<String>>,
    /// Each page's objects in base order (D47).
    base_page_order: BTreeMap<String, Vec<String>>,
    /// The page each managed object is drawn on.
    base_page: BTreeMap<String, String>,
    /// Parent and grandparent of each managed object of the object tree (D64).
    base_parent: BTreeMap<String, (String, Option<String>)>,
    /// Each such parent's objects in base order.
    base_tree_order: BTreeMap<String, Vec<String>>,
}

/// The previous compile's edits on externs, taken back by teardown: what
/// the base still carried of them decides which withdrawn ones leave a
/// tombstone (D31) once the rebuild has redone the rest.
struct PriorEdits {
    /// The lock's extern wires before this compile.
    old_wires: Vec<LockedWire>,
    /// Those of them this base actually carried.
    wires_present: BTreeSet<LockedWire>,
    /// The GUI's `FLG=` on them, keyed by (sink port, source port).
    extern_wire_flags: BTreeMap<(String, String), String>,
    /// The lock's port and setting originals before this compile.
    old_originals: BTreeMap<String, Option<String>>,
    /// What each of those ports carried before the restore.
    defs_written: BTreeMap<String, String>,
    /// The lock's attribute claims (D39) before this compile.
    old_claims: BTreeMap<String, BTreeMap<Attr, AttrClaim>>,
    /// What each claimed object carried before the restore.
    claims_written: BTreeMap<(String, Attr), String>,
}

/// Values of the managed blocks' parameter bindings, keyed by (slug, name).
struct Params {
    /// `Def=` of a connector.
    managed_defs: BTreeMap<(String, String), String>,
    /// An element attribute ([`attr_params`]).
    managed_attrs: BTreeMap<(String, String), String>,
    /// Message parameters of an `OutputRefLM` (D51), written onto its
    /// `<LoggerMailer>` child.
    managed_msgs: BTreeMap<(String, String), String>,
}

/// A minted ref's mirrored object (D33) and the identity attributes its
/// element carries.
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

/// What the rebuild phases (emit, wires, sets, claims, placement) share
/// once the blocks are planned. Read-only; the document and the lock are
/// passed alongside.
#[derive(Clone, Copy)]
struct Rebuild<'a> {
    module: &'a Module,
    managed: &'a BTreeMap<String, PlannedBlock>,
    extern_uuid: &'a BTreeMap<String, String>,
    residue: &'a BTreeMap<String, Residue>,
    snap: &'a BaseSnapshot,
    mirror_of: &'a BTreeMap<String, Mirror>,
}

/// The ConfigVersion pin: a base written by a different Loxone release
/// than the one this project is qualified for is refused until the new
/// release is accepted explicitly.
fn check_config_version(base: &LoxoneDoc, lock: &Lockfile, opts: &CompileOptions) -> Result<()> {
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
            "Base config saved by Loxone Config {}, but the lockfile expects {pinned}. Run `loxl compile --accept-version {}`.",
            base_version.as_deref().unwrap_or("<absent>"),
            base_version.as_deref().unwrap_or("<version>")
        )));
    }
    Ok(())
}

/// Applies `moved` statements to the lock (identity surgery, in source).
/// Idempotent: once the new slug carries the entry, the statement is done.
fn apply_moved(module: &Module, lock: &mut Lockfile) -> Result<()> {
    for mv in module.moved() {
        match (
            lock.objects.contains_key(&mv.from),
            lock.objects.contains_key(&mv.to),
        ) {
            (true, false) => lock.rename_object(&mv.from, &mv.to)?,
            (true, true) => {
                return Err(Error::Compile(format!(
                    "moved `{from} -> {to}`: both slugs are in the lockfile. \
                     Delete the lockfile entry of `{to}` first.",
                    from = mv.from,
                    to = mv.to
                )));
            }
            (false, true) => {} // already applied — the statement is done
            (false, false) => {
                return Err(Error::Compile(format!(
                    "moved `{} -> {}`: neither slug is in the lockfile, so there is nothing to rename",
                    mv.from, mv.to
                )));
            }
        }
    }
    Ok(())
}

/// Sync check: the lock must not know managed blocks the source lost,
/// unless the removal is authorized (per-slug via `removed`, globally via
/// allow_removals, or implicitly for expression-owned blocks — editing the
/// expression IS the removal authorization). Returns the source's slugs.
fn check_removals(
    base: &LoxoneDoc,
    module: &Module,
    lock: &mut Lockfile,
    opts: &CompileOptions,
) -> Result<BTreeSet<String>> {
    let src_slugs: BTreeSet<String> = module.blocks().map(|b| b.slug.clone()).collect();
    let removed_slugs: BTreeSet<&str> = module.removed().map(|r| r.slug.as_str()).collect();
    // D67: `removed <rule>` deletes a named rule the lock pins (D60): its
    // pin becomes a tombstone, which the sweep below deletes from every
    // base that still has the rule, as for a block.
    for r in module.removed() {
        let Some(uuid) = lock.rules.get(&r.slug).cloned() else {
            continue;
        };
        if let Some(target) = wired_from(base, &uuid) {
            return Err(Error::Compile(format!(
                "`removed {}`: the rule {uuid} drives {target}. Remove that wire in \
                 Loxone Config or the source first.",
                r.slug
            )));
        }
        lock.rules.remove(&r.slug);
        lock.removed.insert(
            uuid,
            crate::lock::Tombstone {
                slug: r.slug.clone(),
                block_type: crate::logic::ast::RULE_BLOCK_TYPE.to_string(),
            },
        );
    }
    // A `removed` statement must name a block the lock still knows — as a
    // managed object (the removal happens now) or as a tombstone (D31, the
    // removal is compiled but not yet seen deployed). Once the tombstone
    // clears, a lingering statement is a hard error: the removal is done,
    // delete the line (or it never named anything — a typo).
    for r in module.removed() {
        if !lock.objects.contains_key(&r.slug) && !lock.removed.values().any(|t| t.slug == r.slug) {
            return Err(Error::Compile(format!(
                "`removed {slug}`: no managed block or pending removal with \
                 this slug is in the lockfile. If the removal is done, \
                 delete the `removed` line.",
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
            "managed block `{slug}` is in the lockfile but not in the source. \
             To delete it, add `removed {slug}` (or pass --allow-removals); \
             to rename it, add `moved {slug} -> <new_slug>`."
        )));
    }
    Ok(src_slugs)
}

/// An extern is the GUI's object: one the source manages is rebuilt from
/// its declaration, and a statement on it would be lost (D64 — an adopted
/// device takes its I/O along).
fn check_externs_unmanaged(
    extern_uuid: &BTreeMap<String, String>,
    module: &Module,
    lock: &Lockfile,
) -> Result<()> {
    for (x, u) in extern_uuid {
        if let Some((s, _)) = lock
            .objects
            .iter()
            .find(|(s, o)| o.uuid == *u && module.blocks().any(|b| &b.slug == *s))
        {
            return Err(Error::Compile(format!(
                "extern `{x}` resolves to `{u}`, which the source manages as `{s}`. \
                 Use `{s}` instead."
            )));
        }
    }
    Ok(())
}

/// The compile's UUID minter, seeded past everything a previous compile
/// handed out.
fn seeded_minter(lock: &Lockfile, opts: &CompileOptions) -> Minter {
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
        for s in &o.states {
            minter.avoid(s);
        }
        for p in o.ports.values() {
            minter.avoid(p);
        }
    }
    minter
}

/// The writes kept in their own modules, last of all: Touch Pure Flex
/// tiles (D41), DayTimer schedules (D53), access rights (D49), Automatic
/// Designer rules (D57, D60) and LightController2 moods (D58).
fn apply_features(
    doc: &mut LoxoneDoc,
    base: &LoxoneDoc,
    lock: &mut Lockfile,
    minter: &mut Minter,
    cx: &Rebuild,
) -> Result<()> {
    let Rebuild {
        module,
        managed,
        extern_uuid,
        ..
    } = *cx;
    let mut uuid_of: BTreeMap<String, String> = extern_uuid
        .iter()
        .map(|(s, u)| (s.clone(), u.clone()))
        .chain(managed.iter().map(|(s, b)| (s.clone(), b.uuid.clone())))
        .collect();
    super::tiles::apply(doc, module, &uuid_of)?;
    // DayTimer schedules (D53): the claimed `Modes=`/`N=`/`<Entry>` set,
    //     on extern and managed timers alike; no lock state either.
    super::schedule::apply(doc, module, &uuid_of)?;

    // --- Access rights on NFC Code Touch blocks (D49): users, groups and
    //     schedules are the base's; no lock state, like tiles.
    super::access::apply(doc, module, &uuid_of)?;
    // Automatic Designer rules (D57); a named one (D60) is found or
    //     created first, its UUID pinned in the lock while it is declared.
    //     A rule or mood minted here moves the mint mark.
    let mint_mark = minter.counter();
    lock.rules = super::rule::create(doc, module, &uuid_of, &lock.rules, minter)?;
    uuid_of.extend(lock.rules.iter().map(|(s, u)| (s.clone(), u.clone())));
    super::rule::apply(doc, module, &uuid_of)?;
    super::rule::watch(base, doc);
    // LightController2 moods (D58): the claimed `<LightscenesC>` list; a
    //     new mood mints its UUID (pinned in the lock until the base has
    //     it).
    let minted_moods = std::mem::take(&mut lock.moods);
    lock.moods = super::moods::apply(doc, module, &uuid_of, &minted_moods, minter)?;
    if minter.counter() != mint_mark {
        lock.counters.next_mint = minter.counter();
    }
    Ok(())
}

/// Placement (D42), once every wire is drawn: blocks without a position,
/// and those on pages laid out afresh, go where their wires want them on
/// the page as it now stands.
fn place_blocks(doc: &mut LoxoneDoc, lock: &mut Lockfile, cx: &Rebuild) {
    let Rebuild {
        module,
        managed,
        residue,
        ..
    } = *cx;
    let managed_uuids: BTreeSet<&str> = managed.values().map(|b| b.uuid.as_str()).collect();
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
        let geo = PageGeometry::of(doc, page_uuid).expect("blocks were appended to this page");
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
}

/// Counters and target metadata, then the self-check on the output.
fn finish(
    doc: &mut LoxoneDoc,
    base: &LoxoneDoc,
    lock: &mut Lockfile,
    opts: &CompileOptions,
) -> Result<()> {
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
    lock.target.semantic_fingerprint = Some(crate::diff::semantic_fingerprint(doc));
    // Self-check: the base was unique, so a repeat here was introduced by
    // this compile — a mint that hit a UUID the base already carries (the
    // minter is time + counter + serial: another lock's output, minted in
    // the same second), a lock that maps two identities onto one UUID, or
    // a compiler bug. Never hand such an output to a push.
    let dups = doc.duplicate_uuids();
    if !dups.is_empty() {
        return Err(Error::Compile(format!(
            "Output would contain duplicate UUIDs {}, so nothing was written. Compile again or pass another --time. If that does not help, the lockfile maps two blocks to one UUID.",
            dups.join(", ")
        )));
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
    let page_of_extern = extern_pages(module);
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
    // The declared matcher, evaluated fresh. For `mirrors:` the target's
    // uuid is resolved by the caller (it needs the phase order below).
    let matches_of = |ext: &crate::logic::ast::ExternDecl,
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
                            .is_some_and(|s| crate::doc::same_serial(&s, want))
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
        let mirror_uuid = mirror_target_uuid(ext, lock, &out)?;
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
            if locked.matched_by != matcher_kind(&ext.match_spec)
                || matches!(ext.match_spec, MatchSpec::Mirrors(_))
            {
                let confirms = matches_of(ext, mirror_uuid.as_deref());
                if !confirms.iter().any(|o| o.uuid == locked.uuid) {
                    return Err(Error::Compile(format!(
                        "extern `{}`: matcher changed to {}({}), which does not match \
                         the locked object {}. Fix the matcher, or delete its lockfile \
                         entry to match a new object.",
                        ext.slug,
                        ext.block_type,
                        ext.spec(),
                        locked.uuid
                    )));
                }
                locked.matched_by = matcher_kind(&ext.match_spec).to_string();
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
                        matched_by: matcher_kind(&ext.match_spec).to_string(),
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

/// The `page` statement in force at each extern's declaration site
/// (D28) — `mirrors:` disambiguation is positional page governance (D32).
fn extern_pages(module: &Module) -> BTreeMap<&str, &str> {
    let mut page_of_extern: BTreeMap<&str, &str> = BTreeMap::new();
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
    page_of_extern
}

/// The lockfile's `matched_by` name of a matcher kind.
fn matcher_kind(m: &MatchSpec) -> &'static str {
    match m {
        MatchSpec::Uuid(_) => "uuid",
        MatchSpec::IName(_) => "iname",
        MatchSpec::Title(_) => "title",
        MatchSpec::Mirrors(_) => "mirrors",
    }
}

/// For a `mirrors:` extern, the uuid of the mirrored object: a managed
/// block with a locked identity or an extern `resolved` before it. None
/// for the other matchers.
fn mirror_target_uuid(
    ext: &crate::logic::ast::ExternDecl,
    lock: &Lockfile,
    resolved: &BTreeMap<String, String>,
) -> Result<Option<String>> {
    Ok(match &ext.match_spec {
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
                    .or_else(|| resolved.get(target).cloned())
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
    })
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
            "extern `{slug}` has no port `{port}` in the base config (ports: {}){}. Wire or set the port once in Loxone Config, then compile again.",
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
                "`page {:?}` is created with two different room/category arguments. \
                 Give them once, or the same everywhere.",
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
                    "page {title:?} ({uuid}) is titled {now:?} in the base config. \
                     Write `page {now:?}` in the source, or rename it back in Loxone Config."
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
                "`page {title:?} (room: {room:?})`: Loxone Config needs a real \
                 room for a page. Name a room other than the unassigned one."
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
                        "`page {title:?}`: the base config has no unassigned category. \
                         Name one with `category:`."
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
            "wire sink connector `{}` is inverted in Loxone Config, \
             so a wire into it would be inverted too. Remove the inversion in Loxone Config first.",
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
    if let Some((owner, name)) = crate::settings::split_key(port_uuid) {
        return read_setting(doc, owner, name);
    }
    find_co_mut(&mut doc.xml.root, port_uuid)
        .and_then(|co| co.attr_decoded("Def").map(|v| v.into_owned()))
}

/// Keep `Nc` equal to the number of `<In>` children (absent when zero),
/// in Loxone's canonical position: after `Def`, before `U` (oracle save
/// 2026-08-25 rewrote a compiler-emitted `K,Nc,Def,U` to `K,Def,Nc,U`).
pub(super) fn sync_nc(co: &mut Element) {
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
            "set target connector `{}` is inverted in Loxone Config, \
             so its value cannot be set from source. Change it in Loxone Config instead.",
            co.attr_decoded("K").unwrap_or_default()
        )));
    }
    let original = co.attr_decoded("Def").map(|v| v.into_owned());
    set_attr_ordered(co, "Def", value, &["U"]);
    Ok(original)
}

/// Restore an extern port's `Def=` to its pre-`set` state.
fn restore_def(doc: &mut LoxoneDoc, port_uuid: &str, original: Option<&str>) {
    if let Some((owner, name)) = crate::settings::split_key(port_uuid) {
        write_setting(doc, owner, name, original);
        return;
    }
    if let Some(co) = find_co_mut(&mut doc.xml.root, port_uuid) {
        match original {
            Some(v) => set_attr_ordered(co, "Def", v, &["U"]),
            None => {
                co.remove_attr("Def");
            }
        }
    }
}

/// A setting of a physical input or output (D65): an attribute of the
/// object, or `Unit=` of its `<Display>`.
fn read_setting(doc: &LoxoneDoc, owner: &str, name: &str) -> Option<String> {
    let c = find_c(&doc.xml.root, owner)?;
    let el = if name == "Unit" {
        c.child_elements().find(|e| e.name == "Display")?
    } else {
        c
    };
    el.attr_decoded(name).map(|v| v.into_owned())
}

/// Write (or, with `None`, remove) a setting; an added attribute goes
/// where Loxone Config writes it.
fn write_setting(doc: &mut LoxoneDoc, owner: &str, name: &str, value: Option<&str>) {
    let Some(c) = find_c_mut(&mut doc.xml.root, owner) else {
        return;
    };
    let el = if name == "Unit" {
        match c.child_elements_mut().find(|e| e.name == "Display") {
            Some(d) => d,
            None => return,
        }
    } else {
        c
    };
    match value {
        None => {
            el.remove_attr(name);
        }
        Some(v) if el.attr(name).is_some() => el.set_attr(name, v),
        Some(v) => {
            let names: Vec<&str> = el.attrs.iter().map(|a| a.name.as_str()).collect();
            let pos = crate::settings::insert_position(&names, name);
            el.attrs.insert(
                pos,
                crate::xml::Attr {
                    name: name.to_string(),
                    value: crate::xml::escape(v).into_owned(),
                },
            );
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

/// The first object a wire from one of `uuid`'s connectors feeds: its
/// title (or UUID), `None` when nothing is wired to the object.
fn wired_from(doc: &LoxoneDoc, uuid: &str) -> Option<String> {
    let objects = doc.objects();
    let el = doc.element_at(&objects.iter().find(|o| o.uuid == uuid)?.path)?;
    let outputs: BTreeSet<&str> = el
        .child_elements()
        .filter(|c| c.name == "Co")
        .filter_map(|c| c.attr("U"))
        .collect();
    objects.iter().find_map(|o| {
        let fed = doc
            .element_at(&o.path)?
            .child_elements()
            .filter(|c| c.name == "Co")
            .flat_map(|c| c.child_elements())
            .any(|i| i.name == "In" && i.attr("Input").is_some_and(|u| outputs.contains(u)));
        fed.then(|| o.title.clone().unwrap_or_else(|| o.uuid.clone()))
    })
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
                "no {what} titled {title:?} in the base config{hint}. Create it in \
                 Loxone Config first (known: {}).",
                known.join(", ")
            )))
        }
        _ => Err(Error::Compile(format!(
            "{} {what}s are titled {title:?} ({}). Rename one in Loxone Config.",
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

/// A duration in seconds, as the source resolves it (D27), in the whole
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
