//! Planning: the managed blocks' identity, page and container, their
//! parameter values and the objects their refs mirror.

use super::*;
use crate::logic::DesugarInfo;

/// Plans the managed blocks: the builtin port list is the full port list
/// and pinned identity for every one of them. Each block is pinned to a
/// page: adopted blocks to the page they were drawn on, new blocks to the
/// options' page (resolved lazily so a block-free module compiles against a
/// page-less config).
pub(super) fn plan_blocks(
    doc: &LoxoneDoc,
    module: &Module,
    lock: &mut Lockfile,
    opts: &CompileOptions,
    minter: &mut Minter,
    desugar_info: &DesugarInfo,
    residue: &BTreeMap<String, Residue>,
) -> Result<BTreeMap<String, PlannedBlock>> {
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
        if o.block_type == "VirtualInCaption"
            || crate::periphery::PERIPHERY
                .iter()
                .any(|p| matches!(p.home, crate::periphery::Home::Folder(f) if f == o.block_type))
        {
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
    let pins = Pins {
        default_page_uuid,
        declared_page: &declared_page,
        pages_by_title: &pages_by_title,
        containers: &containers,
        relayout_pages: &relayout_pages,
        residue,
        opts,
    };
    for block in module.blocks() {
        let plan = plan_block(&pins, lock, minter, desugar_info, block)?;
        managed.insert(block.slug.clone(), plan);
    }
    Ok(managed)
}

/// Where a block may go: the page and container lookups of [`plan_block`].
struct Pins<'a> {
    default_page_uuid: Option<String>,
    declared_page: &'a BTreeMap<&'a str, &'a str>,
    pages_by_title: &'a BTreeMap<String, Vec<String>>,
    containers: &'a BTreeMap<String, String>,
    relayout_pages: &'a BTreeSet<&'a str>,
    residue: &'a BTreeMap<String, Residue>,
    opts: &'a CompileOptions,
}

/// One block's plan: its pinned identity, then its container (object tree,
/// D54) or its page (D28) and position (D42).
fn plan_block(
    pins: &Pins,
    lock: &mut Lockfile,
    minter: &mut Minter,
    desugar_info: &DesugarInfo,
    block: &BlockDecl,
) -> Result<PlannedBlock> {
    let Pins {
        default_page_uuid,
        declared_page,
        pages_by_title,
        containers,
        relayout_pages,
        residue,
        opts,
    } = pins;
    // Type, port names, and directions were checked by validate_ports.
    let specs = builtin(&block.block_type).expect("validate_ports admitted the type");
    let keys: Vec<String> = specs.iter().map(|s| s.key.to_string()).collect();

    let entry = pin_identity(lock, minter, desugar_info, block, &keys)?;
    // D54: an object of the object tree (a VirtualIn) is appended to
    // its container, which the base has exactly once — never pinned,
    // placed, or governed by `page`. An I/O (D64) goes into its device,
    // resolved once every block has its UUID.
    if object_tree(&block.block_type) {
        let uuid = match container_type(&block.block_type) {
            Some(container) => containers.get(container).cloned().ok_or_else(|| {
                Error::Compile(format!(
                    "block `{}`: the base config has no <C Type=\"{container}\"> to \
                         hold a minted {}",
                    block.slug, block.block_type
                ))
            })?,
            None => String::new(),
        };
        entry.page_uuid = None;
        entry.layout = None;
        entry.superseded_layout = None;
        return Ok(PlannedBlock {
            uuid: entry.uuid.clone(),
            block_type: block.block_type.clone(),
            keys,
            ports: entry.ports.clone(),
            layout: None,
            relayout: false,
            page_uuid: uuid,
            placed: false,
        });
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
        let stale = !l.same_spot(&b) && entry.superseded_layout.is_some_and(|s| s.same_spot(&b));
        if !stale {
            entry.layout = Some(b);
            entry.superseded_layout = None;
        }
    }
    Ok(PlannedBlock {
        uuid: entry.uuid.clone(),
        block_type: block.block_type.clone(),
        keys,
        ports: entry.ports.clone(),
        layout: entry.layout,
        relayout: relayout_pages.contains(page_uuid.as_str()),
        page_uuid,
        placed: true,
    })
}

/// Pins a block's identity: reuses the lock entry, minting only what is
/// missing. For a brand-new block, ports are minted before the object —
/// matching the counter order observed in Loxone Config output.
fn pin_identity<'a>(
    lock: &'a mut Lockfile,
    minter: &mut Minter,
    desugar_info: &DesugarInfo,
    block: &BlockDecl,
    keys: &[String],
) -> Result<&'a mut LockedObject> {
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
            states: Vec::new(),
        });
    // Ownership follows the current source: a slug the desugarer
    // produced is expression-owned; one it no longer produces (but a
    // hand now declares) is hand-owned again.
    entry.expr_owned = desugar_info.synthetic.contains(&block.slug);
    if entry.uuid.is_empty() {
        entry.block_type = block.block_type.clone();
    } else if entry.block_type != block.block_type {
        if desugar_info.signals.contains(&block.slug) {
            return Err(Error::Compile(format!(
                "Named signal `{slug}` changed its top operator, so its block type \
                 changed from {} to {}. Give the signal a new name and add `removed {slug}`.",
                entry.block_type,
                block.block_type,
                slug = block.slug,
            )));
        }
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
                 device — adopt the existing one (`loxl adopt <cfg> --uuid …`) \
                 instead of declaring a new one",
            block.slug, block.block_type
        )));
    }
    if entry.uuid.is_empty() {
        entry.uuid = minter.mint_object().to_string();
        lock.counters.next_obj += 1;
        // D66: the special states a save would give it, for its entry
        // in the app.
        if let Some((n, _)) = save_defaults(&block.block_type) {
            entry.states = (0..n).map(|_| minter.mint_object().to_string()).collect();
        }
    }
    Ok(entry)
}

/// D64: an I/O goes into the device its `device:` names — into the caption
/// of that device it sat in, if the base had it in one.
pub(super) fn place_ios_in_devices(
    module: &Module,
    managed: &mut BTreeMap<String, PlannedBlock>,
    extern_uuid: &BTreeMap<String, String>,
    base_parent: &BTreeMap<String, (String, Option<String>)>,
) -> Result<()> {
    for block in module.blocks() {
        if crate::periphery::device_types(&block.block_type).is_empty() {
            continue;
        }
        let target = block
            .params()
            .find_map(|(k, v)| match (k, v) {
                ("device", Value::Ref(t)) => Some(t.as_str()),
                _ => None,
            })
            .expect("validate_ports requires device: on an I/O");
        let device = match managed.get(target) {
            Some(p) => p.uuid.clone(),
            None => extern_uuid.get(target).cloned().ok_or_else(|| {
                Error::Compile(format!(
                    "block `{}`: its device `{target}` did not resolve",
                    block.slug
                ))
            })?,
        };
        let plan = managed.get_mut(&block.slug).expect("planned above");
        plan.page_uuid = match base_parent.get(&plan.uuid) {
            Some((parent, Some(grand))) if *grand == device => parent.clone(),
            _ => device,
        };
    }
    Ok(())
}

/// Def values on managed ports come from the blocks' parameter bindings
/// (port assignment statements are for extern ports only — enforced by
/// validate). `let` references resolve to their literal here.
pub(super) fn collect_params(module: &Module) -> Result<Params> {
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
            if (is_ref_type(&block.block_type) && k == "mirrors")
                || crate::periphery::is_device_binding(&block.block_type, k)
            {
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
            // An attribute parameter is keyed by the attribute it writes
            // (`At:` → `Sec=`).
            let name = attr_name(&block.block_type, k);
            target.insert((block.slug.clone(), name.to_string()), value.into_owned());
        }
    }
    Ok(Params {
        managed_defs,
        managed_attrs,
        managed_msgs,
    })
}

/// `mirrors:` on minted refs (D33): resolves the mirrored object and
/// precomputes the identity attributes its element carries (`Ref=`,
/// `LinkRefType=`, `Analog=`). The target is any managed block of this
/// module (a block minted this very compile included — the ref is ours to
/// create) or any resolved extern; the target's XML type picks the GUI's
/// type-registry code.
pub(super) fn resolve_mirrors(
    doc: &LoxoneDoc,
    module: &Module,
    managed: &BTreeMap<String, PlannedBlock>,
    extern_uuid: &BTreeMap<String, String>,
    type_by_uuid: &BTreeMap<String, (String, Option<String>)>,
) -> Result<BTreeMap<String, Mirror>> {
    let resolve = |v: &Value| -> Result<String> { Ok(module.resolve_value(v)?.to_string()) };
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
                None => read_claimed(doc, u, Attr::Title)
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
    Ok(mirror_of)
}
