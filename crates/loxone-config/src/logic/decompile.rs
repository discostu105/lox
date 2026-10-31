//! The decompiler: existing config → Lox Logic view.
//!
//! `decompile` answers "what is in this config?" in the language's own
//! terms. Objects whose type is in the managed set become `slug = Type(…)`
//! declarations — their `Def=` values and incoming wires as argument
//! bindings. In the default [`DecompileScope::Full`] view, every other
//! page object with connectors becomes an `extern` declaration, and every
//! wire between lifted objects is shown (wires into managed blocks in the
//! argument list, wires onto extern ports as `target.Port <- source.Port`,
//! sorted by sink) — except `InputRef`/`OutputRef` plumbing, which folds
//! behind a `# mirrors …` note on the ref's extern declaration (D29).
//! Output is grouped by logic page: `page "Title"` statements head the
//! sections of the single-module view (D28), one module per page from
//! [`decompile_pages`]; the periphery keeps a `# periphery` comment.
//!
//! The full view is for reading, not compiling: compiling it against the
//! same base would mint duplicates of the managed blocks and claim
//! ownership of every shown wire. [`DecompileScope::ManagedOnly`] restricts
//! the view to managed-type objects and what is wired to them — the
//! starting point for adopting existing logic.
//!
//! Honest limits of the view: parameters (`Def=`) of extern objects are
//! never lifted (a `target.Port = value` statement would claim ownership
//! of the value); wires between two unlifted periphery objects stay raw
//! (the view covers page logic, not the device tree); objects whose type
//! or port keys cannot be written as language identifiers stay raw and
//! their wires are not shown. The report counts what was not lifted.
//!
//! Access rights on NFC Code Touch blocks lift into `access` declarations
//! (D49), in both scopes, with an extern for every Code Touch that has
//! any: adopting a config takes its access over unchanged.
//!
//! LightController2 moods lift into `moods` sets with a claim per
//! controller (D58), in both scopes, again with an extern for every
//! controller that has moods of its own: controllers with the same moods
//! share one set. A controller with only the stock moods is left alone.
//!
//! Match specs for externs prefer stability over readability:
//! `iname` when it is unique for the type (INames are locale-stable),
//! else `title` when unique, else the exact `uuid`.

use crate::connectors::{BUILTIN_TYPES, attr_params, container_type};
use crate::doc::{GUI_OWNED_ATTRS, LoxoneDoc, ObjectSummary, ports};
use crate::error::Result;
use crate::logic::ast::{
    ACCESS_BLOCK_TYPE, AccessDecl, AccessGrant, AccessItem, ArgItem, Binding, BindingKind,
    BlockDecl, ExternDecl, Item, MatchSpec, Module, MoodLine, MoodsClaim, MoodsDecl, MoodsItem,
    PageDecl, PortRef, RULE_BLOCK_TYPE, RightKind, RuleDecl, Value, WireDecl,
};
use std::collections::{BTreeMap, BTreeSet};

/// How much of the config the view lifts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecompileScope {
    /// Every page object with connectors: managed types as block
    /// declarations, everything else as `extern`s, plus every wire between
    /// lifted objects. The orientation view — and the default.
    Full,
    /// Only managed-type objects and the objects wired to them: the
    /// faithful subset of what the compiler could own, used as the
    /// starting point for adoption.
    ManagedOnly,
}

#[derive(Debug, Clone)]
pub struct DecompileOptions {
    /// Block types to lift into managed block declarations. Defaults to the
    /// verified builtin table, so a lifted block declaration is always one
    /// the compiler could rebuild.
    pub managed_types: BTreeSet<String>,
    pub scope: DecompileScope,
    /// Object UUIDs to treat as unmanaged even though their type is in
    /// `managed_types` (they can still appear as externs). `adopt` uses
    /// this for individual blocks whose rebuild would not be faithful.
    pub exclude: BTreeSet<String>,
    /// Show parameters whose value equals the corpus-observed GUI default
    /// (D30) in the `Full` view instead of eliding them (`--all-params`).
    /// The `ManagedOnly` view always keeps every parameter — rebuilding
    /// must write the exact `Def=` back — so this only affects `Full`.
    pub all_params: bool,
}

impl Default for DecompileOptions {
    fn default() -> Self {
        DecompileOptions {
            // Objects of the object tree (a VirtualIn, D54) can be minted
            // but are not adopted — existing ones stay externs.
            managed_types: BUILTIN_TYPES
                .iter()
                .filter(|t| container_type(t).is_none())
                .map(|t| String::from(*t))
                .collect(),
            scope: DecompileScope::Full,
            exclude: BTreeSet::new(),
            all_params: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecompileReport {
    /// Objects lifted into managed block declarations.
    pub managed: usize,
    /// Objects lifted into `extern`s.
    pub externs: usize,
    /// Logic pages with lifted content: sections in the single-module
    /// view, files from [`decompile_pages`].
    pub pages: usize,
    /// Objects left as raw XML (not counting Document/Page containers) —
    /// the honest measure of what the view does not cover.
    pub raw_objects: usize,
    /// Plumbing wires between an `InputRef`/`OutputRef` and the object
    /// its `Ref=` attribute names, folded out of the `Full` view (D29) —
    /// the ref's extern declaration carries a `# mirrors …` note instead.
    /// Always 0 in the `ManagedOnly` view.
    pub ref_wires_folded: usize,
    /// Parameters elided from the `Full` view because their value equals
    /// the corpus-observed GUI default (D30, [`crate::observed_defaults`]);
    /// `all_params` shows them. Always 0 in the `ManagedOnly` view.
    pub default_params_elided: usize,
}

/// One logic page's slice of the view, from [`decompile_pages`].
#[derive(Debug, Clone)]
pub struct PageModule {
    /// The page's display title as in the config.
    pub title: String,
    /// Slugified file-name stem, unique across the document's pages
    /// (`eg_buero` for "EG Büro").
    pub slug: String,
    /// A self-contained module opening with its `page` statement (D28);
    /// objects the page references but does not contain are declared as
    /// externs with a `# page: …` / `# periphery` origin comment.
    pub module: Module,
}

/// The whole config as one module, grouped into sections headed by
/// `page "Title"` statements (objects not on any logic page lead in a
/// `# periphery` section).
pub fn decompile(doc: &LoxoneDoc, opts: &DecompileOptions) -> Result<(Module, DecompileReport)> {
    // Repeated UUIDs would surface as a confusing duplicate-slug error
    // on the lifted module; name the cause instead.
    doc.require_unique_uuids("config")?;
    let lift = Lift::build(doc, opts);
    let module = lift.single_module();
    module.validate()?;
    Ok((module, lift.report()))
}

/// The config as one module per logic page (pages without lifted content
/// are skipped). Objects outside any page appear only as foreign externs
/// in the modules that reference them.
pub fn decompile_pages(
    doc: &LoxoneDoc,
    opts: &DecompileOptions,
) -> Result<(Vec<PageModule>, DecompileReport)> {
    doc.require_unique_uuids("config")?;
    let lift = Lift::build(doc, opts);
    let pages = lift.page_modules()?;
    Ok((pages, lift.report()))
}

/// Everything both output shapes need, computed once: which objects are
/// lifted (and as what), their slugs, their page, and the lifted wires.
/// `pub(super)` because `adopt` builds on the same lift.
pub(super) struct Lift {
    pub(super) objects: Vec<ObjectSummary>,
    /// Page titles, document order — all pages, with or without content.
    page_titles: Vec<String>,
    /// Page UUIDs, parallel to `page_titles`.
    pub(super) page_uuids: Vec<String>,
    /// Object UUID → index into `page_titles` (absent: not on any page).
    page_of: BTreeMap<String, usize>,
    /// Object UUID → slug, one namespace across the whole document, and
    /// the reverse direction (slug → object index).
    pub(super) slug_of: BTreeMap<String, String>,
    obj_of_slug: BTreeMap<String, usize>,
    /// Object indexes lifted as managed blocks / externs, emission order.
    pub(super) managed: Vec<usize>,
    pub(super) externs: Vec<usize>,
    /// Prebuilt declarations, keyed by object index. `match_specs` also
    /// covers managed objects, for foreign-extern declarations in the
    /// per-page view.
    pub(super) extern_decls: BTreeMap<usize, ExternDecl>,
    pub(super) block_decls: BTreeMap<usize, BlockDecl>,
    pub(super) match_specs: BTreeMap<usize, MatchSpec>,
    /// `device:` constraints (D55) for the title matches that need one.
    devices: BTreeMap<usize, String>,
    /// `<-` statements with the sink and source object indexes,
    /// deduplicated — document order in the adoptable view, sorted by
    /// (sink, source) in the full view (D29).
    pub(super) wires: Vec<(WireDecl, usize, usize)>,
    /// Ref plumbing wires folded out of the full view (D29).
    ref_wires_folded: usize,
    /// Observed-default parameters elided from the full view (D30).
    default_params_elided: usize,
    /// Automatic Designer rule summaries (full view only): extern object
    /// index → `#` comment lines rendered from the rule's `APC=` payload.
    rule_notes: BTreeMap<usize, Vec<String>>,
    /// The `access` declaration of each NFC Code Touch extern with access
    /// rights (D49), or why its rights cannot be written as one.
    access_decls: BTreeMap<usize, std::result::Result<AccessDecl, String>>,
    /// The `rule` declaration of each Automatic Designer rule whose
    /// payload reads as the documented shapes (D57).
    rule_decls: BTreeMap<usize, RuleDecl>,
    /// Mood sets lifted from LightController2s (D58), each with the
    /// bucket it is declared in: the earliest of its controllers'.
    mood_sets: Vec<(MoodsDecl, Option<usize>)>,
    /// Each controller with moods of its own: the index of the set it
    /// claims, or why its moods cannot be written as one.
    pub(super) mood_claims: BTreeMap<usize, std::result::Result<usize, String>>,
}

/// One page's (or the periphery's) share of the lifted items, as indexes
/// into the [`Lift`] tables.
#[derive(Default)]
struct Bucket {
    externs: Vec<usize>,
    blocks: Vec<usize>,
    wires: Vec<usize>,
}

impl Lift {
    pub(super) fn build(doc: &LoxoneDoc, opts: &DecompileOptions) -> Lift {
        let objects = doc.objects();
        let idx = doc.index();
        let obj_index: BTreeMap<&str, usize> = objects
            .iter()
            .enumerate()
            .map(|(i, o)| (o.uuid.as_str(), i))
            .collect();

        // Pages in document order; an object belongs to its deepest
        // ancestor page (by element-path prefix).
        let mut page_paths: Vec<&[usize]> = Vec::new();
        let mut page_titles: Vec<String> = Vec::new();
        let mut page_uuids: Vec<String> = Vec::new();
        for o in &objects {
            if o.block_type == "Page" {
                page_paths.push(&o.path);
                page_titles.push(o.title.clone().unwrap_or_else(|| "Page".into()));
                page_uuids.push(o.uuid.clone());
            }
        }
        let mut page_of: BTreeMap<String, usize> = BTreeMap::new();
        for o in &objects {
            if matches!(o.block_type.as_str(), "Document" | "Page") {
                continue;
            }
            let mut best: Option<usize> = None;
            for (p, pp) in page_paths.iter().enumerate() {
                if pp.len() < o.path.len()
                    && o.path[..pp.len()] == **pp
                    && best.is_none_or(|b| page_paths[b].len() < pp.len())
                {
                    best = Some(p);
                }
            }
            if let Some(p) = best {
                page_of.insert(o.uuid.clone(), p);
            }
        }

        // Ref plumbing (D29, full view only): an `InputRef`/`OutputRef`
        // carries a `Ref=` attribute naming the object it mirrors, and the
        // wires between the two are GUI routing, not logic. They are
        // folded out of the view — the ref's extern declaration carries a
        // `# mirrors …` note instead — and a periphery object whose only
        // connection was such plumbing is never pulled in as an extern.
        let mirror_target: BTreeMap<usize, &str> = objects
            .iter()
            .enumerate()
            .filter(|(_, o)| matches!(o.block_type.as_str(), "InputRef" | "OutputRef"))
            .filter_map(|(i, o)| {
                doc.element_at(&o.path)
                    .and_then(|el| el.attr("Ref"))
                    .map(|r| (i, r))
            })
            .collect();
        // D34 inversion (both scopes): a consumer wire on a ref is emitted
        // against the mirrored object itself — compile routes it back
        // through the very same ref (same-page reuse, pinned by the base
        // wire). Maps a ref's consumer-facing port uuid to the target
        // (uuid, key) it stands for, plus the ref uuids carrying that
        // port (one in a sound config; several only where a push tool
        // gave mirrors shared ports, D38); a wire folds only when its
        // consumer shares a carrying ref's page.
        type Fold = (String, String, Vec<String>);
        let mut ref_fold: BTreeMap<String, Fold> = BTreeMap::new();
        for (&ri, &tgt) in &mirror_target {
            let Some(&mi) = obj_index.get(tgt) else {
                continue;
            };
            // A mirror of a mirror is not resolvable, and only a nameable
            // target type can appear in source.
            if mirror_target.contains_key(&mi) || !is_ident(&objects[mi].block_type) {
                continue;
            }
            let tel = doc
                .element_at(&objects[mi].path)
                .expect("path from objects()");
            let tps = ports(tel);
            let rel = doc
                .element_at(&objects[ri].path)
                .expect("path from objects()");
            let rps = ports(rel);
            let rp = |k: &str| rps.iter().find(|p| p.key == k);
            let mut fold = |port_uuid: &str, tkey: &str| {
                let e = ref_fold
                    .entry(port_uuid.to_string())
                    .or_insert_with(|| (tgt.to_string(), tkey.to_string(), Vec::new()));
                if e.0 == tgt && e.1 == tkey {
                    e.2.push(objects[ri].uuid.clone());
                }
            };
            if objects[ri].block_type == "InputRef" {
                // A fed input serves its output: target idx0 → AI serves
                // AQ, idx1 → I serves Q (the GUI's own feed convention —
                // anything else stays an explicit ref).
                for (fed, serves, tidx) in [("AI", "AQ", 0usize), ("I", "Q", 1)] {
                    if let (Some(f), Some(s), Some(tp)) = (rp(fed), rp(serves), tps.get(tidx))
                        && f.inputs.as_slice() == std::slice::from_ref(&tp.uuid)
                        && is_ident(&tp.key)
                    {
                        fold(&s.uuid, &tp.key);
                    }
                }
            } else if let (Some(ai), Some(aq)) = (rp("AI"), rp("AQ")) {
                // OutputRef: its AQ must feed exactly one port of the
                // target — a write into the ref's AI stands for a write
                // into that port.
                let sinks: BTreeSet<String> = doc
                    .wires()
                    .iter()
                    .filter(|w| w.from_port == aq.uuid)
                    .map(|w| w.to_port.clone())
                    .collect();
                if sinks.len() == 1
                    && let Some(one) = sinks.iter().next()
                    && let Some((so, sk)) = idx.port_owner.get(one)
                    && so == tgt
                    && is_ident(sk)
                {
                    fold(&ai.uuid, sk);
                }
            }
        }
        // A fold applies when the consumer's page carries one of the refs.
        let page_serves = |f: &Fold, consumer_uuid: &str| -> bool {
            let cp = page_of.get(consumer_uuid);
            cp.is_some() && f.2.iter().any(|r| page_of.get(r.as_str()) == cp)
        };

        // Seed: managed-type objects; in the full view also every page
        // object with connectors and a language-writable type.
        let mut lifted = vec![false; objects.len()];
        let mut managed = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            if opts.managed_types.contains(&o.block_type) && !opts.exclude.contains(&o.uuid) {
                lifted[i] = true;
                managed.push(i);
            }
        }
        let mut externs = Vec::new();
        if opts.scope == DecompileScope::Full {
            for (i, o) in objects.iter().enumerate() {
                if !lifted[i]
                    && !matches!(o.block_type.as_str(), "Document" | "Page")
                    // Refs are page visuals, not logic (D34): one appears
                    // in the view only when an unfoldable wire names it.
                    && !mirror_target.contains_key(&i)
                    && page_of.contains_key(&o.uuid)
                    && is_ident(&o.block_type)
                    && !ports(doc.element_at(&o.path).expect("path from objects()")).is_empty()
                {
                    lifted[i] = true;
                    externs.push(i);
                }
            }
        }

        let mut ref_wires_folded = 0usize;

        // Wires touching the seed pull their other endpoint in as an
        // extern. Depth 1 on purpose: an extern lifted here does not pull
        // in *its* other wires, so periphery-to-periphery wiring stays out.
        let seed = lifted.clone();
        let mut seen = BTreeSet::new();
        let mut touched: Vec<(usize, String, usize, String)> = Vec::new();
        for w in doc.wires() {
            let (Some((fo, fk)), Some((to, tk))) = (
                idx.port_owner.get(&w.from_port),
                idx.port_owner.get(&w.to_port),
            ) else {
                continue; // dangling <In> — not representable, stays raw
            };
            let (mut fi, mut ti) = (obj_index[fo.as_str()], obj_index[to.as_str()]);
            let (mut fk, mut tk) = (fk.clone(), tk.clone());
            // D34 inversion: a ref endpoint is rewritten to its target
            // when the consumer sits on the ref's page — compile routes
            // the wire back through that very ref.
            if let Some(f) = ref_fold.get(&w.from_port)
                && page_serves(f, &objects[ti].uuid)
            {
                fi = obj_index[f.0.as_str()];
                fk = f.1.clone();
            } else if let Some(f) = ref_fold.get(&w.to_port)
                && page_serves(f, &objects[fi].uuid)
            {
                ti = obj_index[f.0.as_str()];
                tk = f.1.clone();
            }
            // A page-placed ref an unfoldable wire still names re-enters
            // the view here (it is no longer a seed by itself): plumbing
            // below still folds its feeds, but a real wire lifts it.
            let page_ref = |i: usize| {
                opts.scope == DecompileScope::Full
                    && mirror_target.contains_key(&i)
                    && page_of.contains_key(&objects[i].uuid)
            };
            if !(seed[fi] || seed[ti] || page_ref(fi) || page_ref(ti)) {
                continue;
            }
            if !is_ident(&fk)
                || !is_ident(&tk)
                || !is_ident(&objects[fi].block_type)
                || !is_ident(&objects[ti].block_type)
            {
                continue; // not representable, stays raw
            }
            if idx.inv_ports.contains(&w.to_port) {
                // Wire into a GUI-owned (`Inv=`) connector: the inversion
                // would silently negate a lifted statement, so the wire is
                // GUI content — carried verbatim by the rebuild (managed
                // sink) or left untouched in the base (extern sink), and
                // its source is not pulled in as an extern.
                continue;
            }
            let plumbing = |ri: usize, oi: usize| {
                mirror_target
                    .get(&ri)
                    .is_some_and(|t| *t == objects[oi].uuid)
            };
            if opts.scope == DecompileScope::Full && (plumbing(ti, fi) || plumbing(fi, ti)) {
                ref_wires_folded += 1;
                continue;
            }
            if !seen.insert((w.from_port.clone(), w.to_port.clone())) {
                continue;
            }
            for i in [fi, ti] {
                if !lifted[i] {
                    lifted[i] = true;
                    externs.push(i);
                }
            }
            touched.push((fi, fk, ti, tk));
        }

        // NFC Code Touch blocks with access rights (D49), in both scopes:
        // the `access` declaration needs the block lifted — as an extern,
        // unless it is lifted as a managed block already.
        let access = crate::access::AccessModel::read(doc);
        let rights = access.access_rights();
        let mut with_rights: Vec<usize> = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            if o.block_type == ACCESS_BLOCK_TYPE && rights.iter().any(|r| r.target == o.uuid) {
                with_rights.push(i);
                if !lifted[i] {
                    lifted[i] = true;
                    externs.push(i);
                }
            }
        }

        // Automatic Designer rules whose payload reads as the documented
        // shapes (D57), in both scopes: the `rule` declaration needs the
        // rule and its action targets as externs.
        let uuid_index: BTreeMap<&str, usize> = objects
            .iter()
            .enumerate()
            .map(|(i, o)| (o.uuid.as_str(), i))
            .collect();
        let mut liftable_rules: Vec<(usize, String)> = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            if o.block_type != RULE_BLOCK_TYPE || opts.exclude.contains(&o.uuid) {
                continue;
            }
            let Some(apc) = doc
                .element_at(&o.path)
                .and_then(|el| el.attr_decoded("APC"))
                .map(|a| a.into_owned())
            else {
                continue;
            };
            let Some(rule) =
                crate::autopilot::parse(&apc).and_then(|v| crate::autopilot::interpret(&v).ok())
            else {
                continue;
            };
            let targets: Option<Vec<usize>> = rule
                .blocks()
                .into_iter()
                .map(|u| uuid_index.get(u).copied())
                .collect();
            let Some(targets) = targets else { continue };
            if rule.groups.iter().any(Vec::is_empty) || rule.actions.is_empty() {
                continue;
            }
            for j in std::iter::once(i).chain(targets) {
                if !lifted[j] {
                    lifted[j] = true;
                    externs.push(j);
                }
            }
            liftable_rules.push((i, apc));
        }

        // LightController2s with moods of their own (D58), in both scopes:
        // the claim needs the controller as an extern — also one excluded
        // from management, whose moods a claim owns all the same.
        type Lines = std::result::Result<Vec<MoodLine>, String>;
        let mut mood_lines: Vec<(usize, Lines)> = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            if o.block_type != "LightController2" {
                continue;
            }
            let Some(lines) = doc.element_at(&o.path).and_then(super::moods::lift) else {
                continue;
            };
            if !lifted[i] {
                lifted[i] = true;
                externs.push(i);
            }
            mood_lines.push((i, lines));
        }

        // Slug assignment: managed first (document order), then externs
        // (seed order, then order of first wire reference), all in one
        // namespace.
        let mut slugs = SlugTable::default();
        let mut slug_of: BTreeMap<String, String> = BTreeMap::new();
        let mut obj_of_slug: BTreeMap<String, usize> = BTreeMap::new();
        for &i in managed.iter().chain(&externs) {
            let o = &objects[i];
            let base = o.title.as_deref().or(o.iname.as_deref());
            let slug = slugs.assign(base.unwrap_or(&o.block_type));
            obj_of_slug.insert(slug.clone(), i);
            slug_of.insert(o.uuid.clone(), slug);
        }
        // One set per distinct list of moods, named after the first
        // controller that has it.
        let mut mood_sets: Vec<(MoodsDecl, Option<usize>)> = Vec::new();
        let mut mood_claims: BTreeMap<usize, std::result::Result<usize, String>> = BTreeMap::new();
        for (i, lines) in mood_lines {
            let page = page_of.get(&objects[i].uuid).copied();
            let claim = lines.map(|lines| {
                if let Some(k) = mood_sets
                    .iter()
                    .position(|(set, _)| set.moods().eq(lines.iter()))
                {
                    mood_sets[k].1 = mood_sets[k].1.min(page);
                    return k;
                }
                let name = slugs.assign(&format!("{}_moods", slug_of[&objects[i].uuid]));
                let body = lines.into_iter().map(MoodsItem::Mood).collect();
                let set = MoodsDecl {
                    name,
                    body,
                    comment: None,
                    end_comment: None,
                };
                mood_sets.push((set, page));
                mood_sets.len() - 1
            });
            mood_claims.insert(i, claim);
        }

        // Match specs for every lifted object (managed ones may be needed
        // as foreign externs in the per-page view). A ref whose `Ref=`
        // target has a slug in this module says so semantically (D32,
        // `mirrors:`) — but only when it is the sole ref of its type on
        // that target, so resolution cannot need more than the slug.
        // A title that repeats across identical devices, but not within
        // its own: `title:` narrowed by the device's serial (D55).
        let device_unique = |i: usize| -> Option<std::borrow::Cow<'_, str>> {
            let obj = &objects[i];
            let title = obj.title.as_deref()?;
            let serial = doc.device_serial(&obj.path)?;
            (objects
                .iter()
                .filter(|o| {
                    o.block_type == obj.block_type
                        && o.title.as_deref() == Some(title)
                        && doc
                            .device_serial(&o.path)
                            .is_some_and(|s| crate::doc::same_serial(&s, &serial))
                })
                .count()
                == 1)
                .then_some(serial)
        };
        let match_spec = |i: usize| -> (MatchSpec, Option<String>) {
            let obj = &objects[i];
            if let Some(&target) = mirror_target.get(&i)
                && let Some(slug) = slug_of.get(target)
                // …and the target is not itself a ref: a mirror of a
                // mirror is not resolvable (compile refuses it).
                && obj_index
                    .get(target)
                    .is_some_and(|mi| !mirror_target.contains_key(mi))
                && mirror_target
                    .iter()
                    .filter(|(j, t)| **t == target && objects[**j].block_type == obj.block_type)
                    .count()
                    == 1
            {
                return (MatchSpec::Mirrors(slug.clone()), None);
            }
            let unique = |get: fn(&ObjectSummary) -> Option<&str>| {
                get(obj).filter(|v| {
                    objects
                        .iter()
                        .filter(|o| o.block_type == obj.block_type && get(o) == Some(v))
                        .count()
                        == 1
                })
            };
            if let Some(iname) = unique(|o| o.iname.as_deref()) {
                (MatchSpec::IName(iname.to_string()), None)
            } else if let Some(title) = unique(|o| o.title.as_deref()) {
                (MatchSpec::Title(title.to_string()), None)
            } else if let Some(title) = obj.title.as_deref()
                && let Some(serial) = device_unique(i)
            {
                let device = Some(serial.into_owned());
                (MatchSpec::Title(title.to_string()), device)
            } else {
                (MatchSpec::Uuid(obj.uuid.clone()), None)
            }
        };
        let mut devices: BTreeMap<usize, String> = BTreeMap::new();
        let match_specs: BTreeMap<usize, MatchSpec> = managed
            .iter()
            .chain(&externs)
            .map(|&i| {
                let (spec, device) = match_spec(i);
                devices.extend(device.map(|d| (i, d)));
                (i, spec)
            })
            .collect();
        // A ref extern says what it mirrors (D29) — the plumbing wires
        // that carried that fact were folded above.
        let mirror_note = |i: usize| -> Option<String> {
            let &mi = obj_index.get(*mirror_target.get(&i)?)?;
            let m = &objects[mi];
            let name = m.title.as_deref().or(m.iname.as_deref()).unwrap_or(&m.uuid);
            Some(format!(" mirrors {} \"{name}\"", m.block_type))
        };
        let extern_decls: BTreeMap<usize, ExternDecl> = externs
            .iter()
            .map(|&i| {
                (
                    i,
                    ExternDecl {
                        room: None,
                        category: None,
                        device: devices.get(&i).cloned(),
                        slug: slug_of[&objects[i].uuid].clone(),
                        block_type: objects[i].block_type.clone(),
                        match_spec: match_specs[&i].clone(),
                        // A `mirrors:` matcher already says what the ref
                        // mirrors — the note is for the other match kinds.
                        comment: match &match_specs[&i] {
                            MatchSpec::Mirrors(_) => None,
                            _ => mirror_note(i),
                        },
                    },
                )
            })
            .collect();

        let access_decls: BTreeMap<usize, std::result::Result<AccessDecl, String>> = with_rights
            .iter()
            .map(|&i| {
                let slug = &slug_of[&objects[i].uuid];
                (i, access_decl(&access, &rights, &objects[i].uuid, slug))
            })
            .collect();

        let mut rule_names: BTreeMap<&str, usize> = BTreeMap::new();
        for o in objects.iter().filter(|o| o.block_type == RULE_BLOCK_TYPE) {
            if let Some(t) = &o.title {
                *rule_names.entry(t.as_str()).or_default() += 1;
            }
        }
        let rule_decls: BTreeMap<usize, RuleDecl> = liftable_rules
            .iter()
            .filter_map(|(i, apc)| {
                let slug = &slug_of[&objects[*i].uuid];
                let mut decl =
                    super::rule::lift(slug, apc, doc, &|u| slug_of.get(u).cloned()).ok()?;
                // A rule with a name no other rule has is declared by it
                // (D60, #192): the source owns it whole, no extern.
                decl.title = objects[*i]
                    .title
                    .clone()
                    .filter(|t| !t.is_empty() && rule_names.get(t.as_str()) == Some(&1));
                Some((*i, decl))
            })
            .collect();

        // Blocks: per port in connector order, the `Def=` value becomes a
        // parameter binding, then each incoming wire a wire binding.
        let mut block_decls: BTreeMap<usize, BlockDecl> = BTreeMap::new();
        let mut default_params_elided = 0usize;
        for &i in &managed {
            let o = &objects[i];
            let el = doc.element_at(&o.path).expect("path from objects()");
            let mut args = Vec::new();
            // Attribute parameters (block logic stored as an element
            // attribute, e.g. `Formula=`) lead the argument list. A
            // GUI-owned one (`Code=`, D40) stays unclaimed: the compile
            // carries it, and claiming it is a source decision.
            for name in attr_params(&o.block_type) {
                if GUI_OWNED_ATTRS.contains(name) {
                    continue;
                }
                if let Some(v) = el.attr_decoded(name) {
                    args.push(ArgItem::Binding(Binding {
                        port: (*name).to_string(),
                        kind: BindingKind::Param(Value::from_literal(&v)),
                        comment: None,
                    }));
                }
            }
            for p in ports(el) {
                if !is_ident(&p.key) {
                    continue; // not representable, stays raw
                }
                if p.inv {
                    // GUI-owned connector (D20): its Def and wires are
                    // carried verbatim by the rebuild, never restated in
                    // source — the Inv flag would silently invert them.
                    continue;
                }
                if let Some(def) = &p.def {
                    // A value at the corpus-observed GUI default is noise
                    // to a reader — elided from the full view (D30,
                    // `--all-params` shows it). The adoptable view keeps
                    // every parameter: rebuilding writes `Def=` back.
                    if opts.scope == DecompileScope::Full
                        && !opts.all_params
                        && crate::observed_defaults::observed_default(&o.block_type, &p.key)
                            == Some(def.as_str())
                    {
                        default_params_elided += 1;
                    } else {
                        args.push(ArgItem::Binding(Binding {
                            port: p.key.clone(),
                            kind: BindingKind::Param(Value::from_literal(def)),
                            comment: None,
                        }));
                    }
                }
                for input in &p.inputs {
                    let Some((src_obj, src_key)) = idx.port_owner.get(input) else {
                        continue; // dangling <In> — not representable, stays raw
                    };
                    // D34 inversion, argument-list side: this block is the
                    // consumer — a ref source on its page names the target.
                    let (src_obj, src_key) = match ref_fold.get(input) {
                        Some(f) if page_serves(f, &o.uuid) => (&f.0, &f.1),
                        _ => (src_obj, src_key),
                    };
                    let Some(src_slug) = slug_of.get(src_obj) else {
                        continue;
                    };
                    if !is_ident(src_key) {
                        continue;
                    }
                    args.push(ArgItem::Binding(Binding {
                        port: p.key.clone(),
                        kind: BindingKind::Wire(PortRef {
                            slug: src_slug.clone(),
                            port: src_key.clone(),
                        }),
                        comment: None,
                    }));
                }
            }
            let slug = slug_of[&o.uuid].clone();
            block_decls.insert(
                i,
                BlockDecl {
                    block_type: o.block_type.clone(),
                    // A title identical to the slug is what `compile`
                    // defaults to — dropping it keeps the view minimal.
                    // The read-only full view also drops a title the slug
                    // already encodes (`slugify(title) == slug`, D29); the
                    // adoptable view keeps it, because rebuilding must
                    // write the exact `Title=` back.
                    title: o.title.clone().filter(|t| {
                        t != &slug && (opts.scope != DecompileScope::Full || slugify(t) != slug)
                    }),
                    slug,
                    args,
                    comment: None,
                    close_comment: None,
                },
            );
        }

        // `<-` statements: wires with an extern sink. Wires into managed
        // blocks were lifted into the blocks' argument lists above. The
        // check is per *instance*, not per type: an excluded managed-type
        // object (adopt refusal) is an extern here, and its incoming wires
        // must appear as `<-` statements, not vanish.
        let is_managed: BTreeSet<usize> = managed.iter().copied().collect();
        let mut wires = Vec::new();
        for (fi, fk, ti, tk) in touched {
            if is_managed.contains(&ti) {
                continue;
            }
            wires.push((
                WireDecl {
                    to: PortRef {
                        slug: slug_of[&objects[ti].uuid].clone(),
                        port: tk,
                    },
                    from: PortRef {
                        slug: slug_of[&objects[fi].uuid].clone(),
                        port: fk,
                    },
                    comment: None,
                },
                ti,
                fi,
            ));
        }
        if opts.scope == DecompileScope::Full {
            // Big fan-ins pile up dozens of `<-` lines; sorted by sink,
            // then source, they read as a table (D29). View-only: the
            // adoptable view keeps document order, because the relative
            // `<In>` order within a sink is part of the compiled bytes.
            wires.sort_by(|a, b| {
                (&a.0.to.slug, &a.0.to.port, &a.0.from.slug, &a.0.from.port).cmp(&(
                    &b.0.to.slug,
                    &b.0.to.port,
                    &b.0.from.slug,
                    &b.0.from.port,
                ))
            });
        }

        // An `AutopilotRule` extern's whole rule lives in its `APC=`
        // JSON — invisible as a bare declaration. The full view renders
        // it as comment lines above the extern (view-only liberty, D29;
        // the payload stays app-owned and is never language surface).
        let mut rule_notes: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        if opts.scope == DecompileScope::Full {
            let resolve = |uuid: &str| -> Option<String> {
                let &i = obj_index.get(uuid)?;
                let o = &objects[i];
                o.title.clone().or_else(|| o.iname.clone())
            };
            for &i in &externs {
                let o = &objects[i];
                if o.block_type != "AutopilotRule" {
                    continue;
                }
                let Some(rule) = doc
                    .element_at(&o.path)
                    .and_then(|el| el.attr_decoded("APC"))
                    .and_then(|apc| crate::autopilot::parse(&apc))
                else {
                    continue; // no or unreadable payload — bare extern
                };
                let lines = crate::autopilot::summary(&rule, &resolve);
                if !lines.is_empty() {
                    rule_notes.insert(i, lines);
                }
            }
        }

        Lift {
            objects,
            page_titles,
            page_uuids,
            page_of,
            slug_of,
            obj_of_slug,
            managed,
            externs,
            extern_decls,
            block_decls,
            match_specs,
            devices,
            wires,
            ref_wires_folded,
            default_params_elided,
            rule_notes,
            access_decls,
            rule_decls,
            mood_sets,
            mood_claims,
        }
    }

    pub(super) fn page_of(&self, obj: usize) -> Option<usize> {
        self.page_of.get(&self.objects[obj].uuid).copied()
    }

    /// The Automatic Designer rule summary, as comment items above the
    /// rule's extern declaration (full view only — `rule_notes` is empty
    /// otherwise).
    fn push_rule_notes(&self, obj: usize, items: &mut Vec<Item>) {
        if let Some(lines) = self.rule_notes.get(&obj) {
            items.extend(lines.iter().map(|l| Item::Comment(format!(" {l}"))));
        }
    }

    /// The `access` declarations of the given lifted objects (D49); a
    /// block whose rights cannot be written as one gets a comment saying
    /// why.
    fn push_access<'a>(&self, objs: impl IntoIterator<Item = &'a usize>, items: &mut Vec<Item>) {
        for i in objs {
            match self.access_decls.get(i) {
                Some(Ok(decl)) => items.push(Item::Access(decl.clone())),
                Some(Err(why)) => items.push(Item::Comment(format!(
                    " access {}: not lifted — {why}",
                    self.slug_of[&self.objects[*i].uuid]
                ))),
                None => {}
            }
        }
    }

    /// The mood sets and claims of a bucket's controllers (D58). A
    /// `self_contained` module declares every set its claims name; in
    /// one shared namespace each set is declared once, in its home
    /// bucket.
    fn push_moods(
        &self,
        page: Option<usize>,
        b: &Bucket,
        self_contained: bool,
        items: &mut Vec<Item>,
    ) {
        let claims: Vec<(usize, &std::result::Result<usize, String>)> = b
            .externs
            .iter()
            .chain(&b.blocks)
            .filter_map(|i| Some((*i, self.mood_claims.get(i)?)))
            .collect();
        for (k, (set, home)) in self.mood_sets.iter().enumerate() {
            let wanted = if self_contained {
                claims
                    .iter()
                    .any(|(_, c)| c.as_ref().is_ok_and(|ck| *ck == k))
            } else {
                *home == page
            };
            if wanted {
                items.push(Item::Moods(set.clone()));
            }
        }
        for (i, claim) in claims {
            let slug = self.slug_of[&self.objects[i].uuid].clone();
            items.push(match claim {
                Ok(k) => Item::MoodsClaim(MoodsClaim {
                    slug,
                    moods: self.mood_sets[*k].0.name.clone(),
                    comment: None,
                }),
                Err(why) => Item::Comment(format!(" moods {slug}: not lifted — {why}")),
            });
        }
    }

    /// A rule declared by its name: no `extern` line for it.
    fn named_rule(&self, i: usize) -> bool {
        self.rule_decls.get(&i).is_some_and(|d| d.title.is_some())
    }

    /// The `rule` declarations of the given externs (D57).
    fn push_rules(&self, externs: &[usize], items: &mut Vec<Item>) {
        for i in externs {
            if let Some(decl) = self.rule_decls.get(i) {
                items.push(Item::Rule(decl.clone()));
            }
        }
    }

    pub(super) fn page_title(&self, page: usize) -> &str {
        &self.page_titles[page]
    }

    /// Lifted items grouped by page, in document order; `None` (objects on
    /// no page — the periphery) sorts first. Only non-empty groups. A wire
    /// belongs to its sink's page, falling back to its source's.
    fn buckets(&self) -> Vec<(Option<usize>, Bucket)> {
        let mut map: BTreeMap<Option<usize>, Bucket> = BTreeMap::new();
        for &i in &self.externs {
            map.entry(self.page_of(i)).or_default().externs.push(i);
        }
        for &i in &self.managed {
            map.entry(self.page_of(i)).or_default().blocks.push(i);
        }
        for (wi, &(_, ti, fi)) in self.wires.iter().enumerate() {
            let page = self.page_of(ti).or_else(|| self.page_of(fi));
            map.entry(page).or_default().wires.push(wi);
        }
        map.into_iter().collect()
    }

    pub(super) fn single_module(&self) -> Module {
        let mut items = Vec::new();
        for (page, b) in self.buckets() {
            items.extend(self.bucket_items(page, &b));
        }
        Module { items }
    }

    fn bucket_items(&self, page: Option<usize>, b: &Bucket) -> Vec<Item> {
        // A real `page` statement (D28) — placement is part of the source.
        // The periphery is not a page, so its section stays a comment.
        let mut items = vec![match page {
            Some(p) => Item::Page(PageDecl {
                title: self.page_titles[p].clone(),
                room: None,
                category: None,
                comment: None,
            }),
            None => Item::Comment(" periphery (not placed on a page)".to_string()),
        }];
        for &i in &b.externs {
            if self.named_rule(i) {
                continue;
            }
            self.push_rule_notes(i, &mut items);
            items.push(Item::Extern(self.extern_decls[&i].clone()));
        }
        for &i in &b.blocks {
            items.push(Item::Block(self.block_decls[&i].clone()));
        }
        for &wi in &b.wires {
            items.push(Item::Wire(self.wires[wi].0.clone()));
        }
        self.push_access(b.externs.iter().chain(&b.blocks), &mut items);
        self.push_rules(&b.externs, &mut items);
        self.push_moods(page, b, false, &mut items);
        items
    }

    /// The lifted view as module-directory fragments: one `(file stem,
    /// fragment)` per non-empty page bucket, the periphery leading as
    /// `_periphery` (its underscore also sorts it first in the merge
    /// order). Unlike [`Lift::page_modules`]'s self-contained modules,
    /// the fragments share one namespace — concatenated in this order
    /// they are exactly [`Lift::single_module`], so declarations appear
    /// once and cross-file references resolve on the merged whole
    /// (`Module::parse_fragment` semantics).
    pub(super) fn fragment_modules(&self) -> Vec<(String, Module)> {
        // File stems are assigned over ALL pages in document order, so a
        // page gaining content never renames another page's file.
        let mut names = SlugTable::default();
        let file_slugs: Vec<String> = self.page_titles.iter().map(|t| names.assign(t)).collect();
        self.buckets()
            .into_iter()
            .map(|(page, b)| {
                let stem = match page {
                    Some(p) => file_slugs[p].clone(),
                    None => "_periphery".to_string(),
                };
                (
                    stem,
                    Module {
                        items: self.bucket_items(page, &b),
                    },
                )
            })
            .collect()
    }

    fn page_modules(&self) -> Result<Vec<PageModule>> {
        // File slugs are assigned over ALL pages in document order, so a
        // page gaining content never renames another page's file.
        let mut names = SlugTable::default();
        let file_slugs: Vec<String> = self.page_titles.iter().map(|t| names.assign(t)).collect();

        let mut out = Vec::new();
        for (page, b) in self.buckets() {
            let Some(p) = page else {
                // Periphery objects appear only as foreign externs in the
                // modules that reference them.
                continue;
            };
            let mut items: Vec<Item> = vec![Item::Page(PageDecl {
                title: self.page_titles[p].clone(),
                room: None,
                category: None,
                comment: None,
            })];
            for &i in &b.externs {
                if self.named_rule(i) {
                    continue;
                }
                self.push_rule_notes(i, &mut items);
                items.push(Item::Extern(self.extern_decls[&i].clone()));
            }

            // Referenced objects the page does not contain become foreign
            // externs, annotated with where they live.
            let mut declared: BTreeSet<&str> = b
                .externs
                .iter()
                .chain(&b.blocks)
                .map(|&i| self.slug_of[&self.objects[i].uuid].as_str())
                .collect();
            let mut foreign: Vec<usize> = Vec::new();
            {
                let mut reference = |slug: &str| {
                    if !declared.contains(slug) {
                        foreign.push(self.obj_of_slug[slug]);
                    }
                };
                for &i in &b.blocks {
                    for (_, src) in self.block_decls[&i].input_wires() {
                        reference(&src.slug);
                    }
                }
                for &wi in &b.wires {
                    let w = &self.wires[wi].0;
                    reference(&w.from.slug);
                    reference(&w.to.slug);
                }
                // A rule's targets may live on another page.
                for i in &b.externs {
                    if let Some(decl) = self.rule_decls.get(i) {
                        decl.slugs().for_each(&mut reference);
                    }
                }
            }
            let mut seen: BTreeSet<usize> = BTreeSet::new();
            for i in foreign {
                if !seen.insert(i) {
                    continue;
                }
                let origin = match self.page_of(i) {
                    Some(op) => format!(" page: {}", self.page_titles[op]),
                    None => " periphery".to_string(),
                };
                let mut decl = self.extern_decls.get(&i).cloned().unwrap_or_else(|| {
                    // A managed block on another page: referenced here as
                    // an extern.
                    ExternDecl {
                        room: None,
                        category: None,
                        device: self.devices.get(&i).cloned(),
                        slug: self.slug_of[&self.objects[i].uuid].clone(),
                        block_type: self.objects[i].block_type.clone(),
                        match_spec: self.match_specs[&i].clone(),
                        comment: None,
                    }
                });
                decl.comment = Some(origin);
                items.push(Item::Extern(decl));
                declared.insert(self.slug_of[&self.objects[i].uuid].as_str());
            }

            for &i in &b.blocks {
                items.push(Item::Block(self.block_decls[&i].clone()));
            }
            for &wi in &b.wires {
                items.push(Item::Wire(self.wires[wi].0.clone()));
            }
            self.push_access(b.externs.iter().chain(&b.blocks), &mut items);
            self.push_rules(&b.externs, &mut items);
            self.push_moods(page, &b, true, &mut items);

            let module = Module { items };
            module.validate()?;
            out.push(PageModule {
                title: self.page_titles[p].clone(),
                slug: file_slugs[p].clone(),
                module,
            });
        }
        Ok(out)
    }

    pub(super) fn report(&self) -> DecompileReport {
        DecompileReport {
            managed: self.managed.len(),
            externs: self.externs.len(),
            pages: self.buckets().iter().filter(|(p, _)| p.is_some()).count(),
            raw_objects: self
                .objects
                .iter()
                .filter(|o| !matches!(o.block_type.as_str(), "Document" | "Page"))
                .filter(|o| !self.slug_of.contains_key(&o.uuid))
                .count(),
            ref_wires_folded: self.ref_wires_folded,
            default_params_elided: self.default_params_elided,
        }
    }
}

/// The access rights on `block` as an `access` declaration (D49): users,
/// then groups, each in document order. Titles are the references, so a
/// principal or schedule without a unique title cannot be written down.
fn access_decl(
    model: &crate::access::AccessModel,
    rights: &[crate::access::AccessRight<'_>],
    block: &str,
    slug: &str,
) -> std::result::Result<AccessDecl, String> {
    use crate::access::PrincipalKind;
    let unique = |what: &str, uuid: &str, titles: Vec<(&str, Option<&str>)>| {
        let title = titles
            .iter()
            .find(|(u, _)| *u == uuid)
            .and_then(|(_, t)| *t)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| format!("{what} {uuid} has no title"))?;
        let n = titles.iter().filter(|(_, t)| *t == Some(title)).count();
        if n > 1 {
            return Err(format!("{n} {what}s are titled \"{title}\""));
        }
        Ok(title.to_string())
    };
    let users = || {
        model
            .users
            .iter()
            .map(|u| (u.uuid.as_str(), u.title.as_deref()))
            .collect()
    };
    let groups = || {
        model
            .groups
            .iter()
            .map(|g| (g.uuid.as_str(), g.title.as_deref()))
            .collect()
    };
    let schedules = || {
        model
            .schedules
            .iter()
            .map(|s| (s.uuid.as_str(), s.title.as_deref()))
            .collect()
    };
    let mut body = Vec::new();
    for r in rights.iter().filter(|r| r.target == block) {
        let principal = match r.kind {
            PrincipalKind::User => unique("user", r.principal, users())?,
            PrincipalKind::Group => unique("group", r.principal, groups())?,
        };
        body.push(AccessItem::Grant(AccessGrant {
            kind: r.kind,
            principal,
            schedule: Some(unique("access schedule", r.schedule, schedules())?),
            comment: None,
        }));
    }
    Ok(AccessDecl {
        right: RightKind::Access,
        slug: slug.to_string(),
        body,
        comment: None,
        end_comment: None,
    })
}

/// Whether `s` can be written as a language identifier
/// (`[A-Za-z][A-Za-z0-9_]*`) — required for lifted types and port keys.
fn is_ident(s: &str) -> bool {
    match s.as_bytes().first() {
        Some(b) if b.is_ascii_alphabetic() => {
            s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        }
        _ => false,
    }
}

/// Statement-initial keywords (v1, plus reserved v0 words) and the
/// expression operators (D24). A slug colliding with one would change the
/// meaning of the line or expression it appears in, so [`SlugTable`] never
/// hands them out and the parser refuses them as names.
pub(super) const RESERVED: &[&str] = &[
    "let", "extern", "removed", "moved", "template", "end", "use", "page", "block", "wire", "set",
    "and", "or", "not", "test", "tick", "expect", "clock", "tiles", "tile", "schedule", "moods",
];

/// Slug generation with umlaut transliteration and `_2`-style
/// deduplication; statement keywords are pre-claimed.
struct SlugTable {
    used: BTreeSet<String>,
}

impl Default for SlugTable {
    fn default() -> Self {
        SlugTable {
            used: RESERVED.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl SlugTable {
    fn assign(&mut self, name: &str) -> String {
        let base = slugify(name);
        let mut candidate = base.clone();
        let mut n = 1;
        while !self.used.insert(candidate.clone()) {
            n += 1;
            candidate = format!("{base}_{n}");
        }
        candidate
    }
}

/// Turn a display title into a valid Lox Logic slug (`[a-z][a-z0-9_]*`).
/// German umlauts transliterate (`ü`→`ue`); everything else non-alphanumeric
/// collapses to `_`.
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            'ä' | 'Ä' => out.push_str("ae"),
            'ö' | 'Ö' => out.push_str("oe"),
            'ü' | 'Ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            c if c.is_ascii_alphanumeric() => out.push(c.to_ascii_lowercase()),
            _ => {
                if !out.ends_with('_') && !out.is_empty() {
                    out.push('_');
                }
            }
        }
    }
    let out = out.trim_matches('_').to_string();
    match out.chars().next() {
        Some(c) if c.is_ascii_lowercase() => out,
        Some(_) => format!("x{out}"),
        None => "x".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_rules() {
        assert_eq!(slugify("Beschattung Süd"), "beschattung_sued");
        assert_eq!(slugify("Temp über 28"), "temp_ueber_28");
        assert_eq!(slugify("3. OG Licht"), "x3_og_licht");
        assert_eq!(slugify("Größe"), "groesse");
        assert_eq!(slugify("---"), "x");
    }

    #[test]
    fn slug_table_dedupes() {
        let mut t = SlugTable::default();
        // `or` itself is reserved (expression operator, D24), so the
        // first Or block already dedupes.
        assert_eq!(t.assign("Or"), "or_2");
        assert_eq!(t.assign("Or"), "or_3");
        assert_eq!(t.assign("Memory"), "memory");
        assert_eq!(t.assign("Memory"), "memory_2");
    }

    #[test]
    fn statement_keywords_never_become_slugs() {
        // A block titled "Set" must not produce `set = …` — that line
        // would parse as a (reserved) keyword statement.
        let mut t = SlugTable::default();
        assert_eq!(t.assign("Set"), "set_2");
        assert_eq!(t.assign("Wire"), "wire_2");
        assert_eq!(t.assign("Let"), "let_2");
    }

    #[test]
    fn ident_rule() {
        assert!(is_ident("AutoShade"));
        assert!(is_ident("Q"));
        assert!(is_ident("I1"));
        assert!(!is_ident("2"), "numeric type ids are not writable");
        assert!(!is_ident("174"));
        assert!(!is_ident(""));
        assert!(!is_ident("Türkontakt"), "non-ASCII stays raw");
    }
}
