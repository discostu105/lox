//! Emission: the managed `<C>` elements, built and inserted into their
//! pinned page or container where the base had them.

use super::*;
use crate::doc::DocIndex;

/// Builds the managed `<C>` elements and inserts each into its pinned
/// page, where the base had it. The index is taken once after
/// teardown; inserting into a page never shifts the page paths.
pub(super) fn emit_blocks(
    doc: &mut LoxoneDoc,
    lock: &Lockfile,
    cx: &Rebuild,
    params: &Params,
) -> Result<()> {
    let Rebuild {
        module, residue, ..
    } = *cx;
    let page_index = doc.index();
    // Managed blocks carry the document's block version if the builtin
    // table is verified for it, so a GUI save has none to restamp. Otherwise they keep the version the table was first verified
    // for, and Loxone Config migrates them on load.
    let block_version = doc
        .block_version()
        .filter(|v| VERIFIED_BLOCK_VERSIONS.contains(&v.as_str()))
        .unwrap_or_else(|| VERIFIED_BLOCK_VERSIONS[0].to_string());
    // A fresh VirtualIn gets the next free `VI<n>` designation, a
    // fresh virtual or Modbus object the next of its prefix — past
    // every one the torn-down base and the carried residue hold.
    let inames: Vec<String> = doc
        .objects()
        .iter()
        .filter_map(|o| doc.element_at(&o.path)?.attr("IName").map(String::from))
        .chain(residue.values().flat_map(|r| {
            r.attrs
                .iter()
                .filter(|(n, _)| n == "IName")
                .map(|(_, v)| v.clone())
        }))
        .collect();
    let mut next_iname: BTreeMap<&str, u32> = BTreeMap::new();
    for prefix in
        std::iter::once("VI").chain(crate::periphery::PERIPHERY.iter().filter_map(|p| p.iname))
    {
        let n = inames
            .iter()
            .filter_map(|n| n.strip_prefix(prefix)?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        next_iname.insert(prefix, n + 1);
    }
    // A fresh user-defined operating mode gets the next `Mode=`
    // number: one below the lowest the document or the residue holds (the
    // GUI numbers them -1, -2, … — built-in ones are 0 and up).
    let next_mode: i64 = doc
        .objects()
        .iter()
        .filter(|o| o.block_type == "Mode")
        .filter_map(|o| doc.element_at(&o.path)?.attr("Mode")?.parse::<i64>().ok())
        .chain(residue.values().flat_map(|r| {
            r.attrs
                .iter()
                .filter(|(n, _)| n == "Mode")
                .filter_map(|(_, v)| v.parse::<i64>().ok())
        }))
        .min()
        .unwrap_or(0)
        .min(0)
        - 1;
    // Devices before what hangs below them, so a minted I/O finds
    // its minted device.
    let rank = |t: &str| {
        if crate::periphery::device_types(t).is_empty() {
            0
        } else if crate::periphery::PERIPHERY
            .iter()
            .any(|p| crate::periphery::device_types(p.block_type).contains(&t))
        {
            1
        } else {
            2
        }
    };
    let mut build_order: Vec<_> = module.blocks().collect();
    build_order.sort_by_key(|b| rank(&b.block_type));
    let mut fresh = Fresh {
        block_version,
        next_iname,
        next_mode,
    };
    for block in build_order {
        let mut el = element_attrs(cx, lock, params, &mut fresh, block)?;
        push_connectors(cx, params, block, &mut el)?;
        push_children(cx, params, block, &mut el);
        insert_block(doc, &page_index, cx, block, el)?;
    }
    Ok(())
}

/// Document-wide values a built element is stamped with: the block version
/// and the next free designations of fresh objects.
struct Fresh {
    block_version: String,
    /// Next `VI<n>` or periphery designation per prefix.
    next_iname: BTreeMap<&'static str, u32>,
    /// Next user-defined operating mode number.
    next_mode: i64,
}

/// A managed block's element with its attributes: identity, position,
/// display state, ref identity, settings and the carried GUI residue.
fn element_attrs(
    cx: &Rebuild,
    lock: &Lockfile,
    params: &Params,
    fresh: &mut Fresh,
    block: &BlockDecl,
) -> Result<Element> {
    let Rebuild {
        managed,
        residue,
        mirror_of,
        ..
    } = *cx;
    let managed_attrs = &params.managed_attrs;
    let Fresh {
        block_version,
        next_iname,
        next_mode,
    } = fresh;
    let pt = crate::periphery::periphery(&block.block_type);
    let plan = &managed[&block.slug];
    let res = residue.get(&plan.uuid);
    let carried = |name: &str| {
        res.and_then(|r| r.attrs.iter().find(|(n, _)| n == name))
            .map(|(_, v)| v.as_str())
    };
    let claimed = |name: &str| managed_attrs.get(&(block.slug.clone(), name.to_string()));
    let mut el = Element::new("C");
    el.set_attr("Type", &block.block_type);
    // A save writes `Desc=` right after `Type=`.
    if let Some(d) = claimed("Desc") {
        el.set_attr("Desc", d);
    }
    let iname_prefix = if block.block_type == "VirtualIn" {
        Some("VI")
    } else {
        pt.and_then(|p| p.iname)
    };
    if let Some(prefix) = iname_prefix {
        match carried("IName") {
            Some(v) => el.set_attr_raw("IName", v),
            None => {
                let n = next_iname.get_mut(prefix).expect("every prefix counted");
                el.set_attr("IName", &format!("{prefix}{n}"));
                *n += 1;
            }
        }
    }
    if res.is_none()
        && let Some(p) = pt
        && let Some(missing) = p
            .required
            .iter()
            .find(|a| !managed_attrs.contains_key(&(block.slug.clone(), (**a).to_string())))
    {
        return Err(Error::Compile(format!(
            "block `{}`: a new {} needs `{missing}:` ({})",
            block.slug,
            block.block_type,
            p.required.join(", ")
        )));
    }
    el.set_attr("V", block_version);
    el.set_attr("U", &plan.uuid);
    // A ref block's title is GUI-derived from its mirror target
    // (oracle session 11) — emit the derived value so a save is a
    // no-op; validate refuses an explicit label on ref blocks.
    match mirror_of.get(&block.slug) {
        Some(m) => el.set_attr("Title", &m.title),
        None => el.set_attr("Title", block.title.as_deref().unwrap_or(&block.slug)),
    }
    // Blocks without a position get theirs once the wires are drawn;
    // objects of the object tree have none.
    if plan.placed {
        let at = plan.layout.unwrap_or(Layout::at(0, 0, 0, 0));
        el.set_attr("Px", &at.px.to_string());
        el.set_attr("Py", &at.py.to_string());
        el.set_attr("Px2", &at.px2.to_string());
        el.set_attr("Py2", &at.py2.to_string());
    }
    // A block that existed in the base re-emits its GUI-owned display
    // state verbatim, including *absence* (some types lose `WF` on a GUI
    // save). Only a fresh mint writes the defaults. A virtual input gets
    // the folder's grey (seen in real configs) and the same `WF` a save
    // gives a page block (verified with Loxone Config).
    let mint_attr = |name: &str| {
        pt.and_then(|p| p.mint_attrs.iter().find(|(n, _)| *n == name))
            .map(|(_, v)| *v)
    };
    let wf = match mint_attr("WF") {
        Some(v) => v.unwrap_or_default(),
        None if pt.is_some() => "",
        None => "147456",
    };
    let cl = match mint_attr("Cl") {
        Some(v) => v.unwrap_or_default(),
        None if pt.is_some() => "",
        None if plan.placed => "141,255,112",
        None => "238,238,238",
    };
    // A fresh message ref gets what Loxone Config gives one:
    // black, and no `WF=`.
    let message_ref = block.block_type == "OutputRefLM";
    match (res, carried("Cl")) {
        (Some(_), Some(v)) => el.set_attr_raw("Cl", v),
        (Some(_), None) => {}
        (None, _) if message_ref => el.set_attr("Cl", "0,0,0"),
        (None, _) if cl.is_empty() => {}
        (None, _) => el.set_attr("Cl", cl),
    }
    // A device without connectors has no `Nio=`.
    if !plan.keys.is_empty() || pt.is_none() {
        el.set_attr("Nio", &plan.keys.len().to_string());
    }
    // Minted-ref identity, in the GUI's observed attribute
    // order: `Ref=` after `Nio=`; `Analog=` or `Text=`, then
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
        (None, _) if message_ref || wf.is_empty() => {}
        (None, _) => el.set_attr("WF", wf),
    }
    if res.is_none()
        && let Some(o) = lock.objects.get(&block.slug)
        && !o.states.is_empty()
    {
        el.set_attr("SpStates", &o.states.join(","));
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
        virtual_in_attrs(&mut el, &block.slug, managed_attrs, res)?;
    }
    // A fresh virtual or Modbus object: its settings and the
    // defaults a save gives one, in the order the save writes them.
    if res.is_none()
        && let Some(p) = pt
    {
        for (name, default) in p.mint_attrs {
            if matches!(*name, "Cl" | "Nio" | "WF") {
                continue;
            }
            match managed_attrs.get(&(block.slug.clone(), (*name).to_string())) {
                Some(v) => el.set_attr(name, v),
                None => {
                    if let Some(d) = default {
                        el.set_attr(name, d);
                    }
                }
            }
        }
    }
    if block.block_type == "Mode" && res.is_none() {
        el.set_attr("Mode", &next_mode.to_string());
        *next_mode -= 1;
    }
    // `At:` is a fixed time of day, `Typ="0"` (#248). Any other `Typ=`
    // is a time function (sunrise, an hour pulse …), and a fixed time
    // on it would mean something else. `Pulse:` claims `Typ=`.
    if block.block_type == "PulseAt"
        && claimed("Sec").is_some()
        && let Some(typ) = claimed("Typ")
            .map(String::as_str)
            .or_else(|| carried("Typ"))
        && typ != "0"
    {
        return Err(Error::Compile(format!(
            "block `{}`: `At:` sets a fixed time of day, but the block fires on a \
             time function (`Typ=\"{typ}\"`, set in Loxone Config). Remove `At:`, or \
             add `Pulse: \"time\"`.",
            block.slug
        )));
    }
    for param in attr_params(&block.block_type) {
        let name = attr_name(&block.block_type, param);
        // A claimed attribute the base element has is written where the
        // base has it, by the carried pass below.
        if res.is_some_and(|r| r.attrs.iter().any(|(n, _)| n == name)) {
            continue;
        }
        if let Some(v) = claimed(name) {
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
            // attribute the source binds (`Code:`, `At:`) takes
            // the claimed value here, at the base's position; the claim
            // wins over the carried copy.
            let compiler_owned = matches!(n.as_str(), "Cl" | "LtE" | "WF" | "IName")
                || (is_ref_type(&block.block_type)
                    && matches!(n.as_str(), "Ref" | "LinkRefType" | "Analog" | "Text"));
            if let Some(v) = claimed(n) {
                if attr_param(&block.block_type, n).is_some() {
                    el.set_attr(n, v);
                }
            } else if !compiler_owned {
                el.set_attr_raw(n, v);
            }
        }
    }
    // What a save leaves out has no attribute: a bound `off` on an
    // unticked box of a virtual command or output, a bound 0 on a number.
    for param in attr_params(&block.block_type) {
        let name = attr_name(&block.block_type, param);
        if managed_attrs
            .get(&(block.slug.clone(), name.to_string()))
            .is_some_and(|v| omitted_value(&block.block_type, name, v))
        {
            el.remove_attr(name);
        }
    }
    Ok(el)
}

/// The block's connectors in spec order: GUI-owned ones verbatim,
/// the rest with their bound or stale `Def=`.
fn push_connectors(
    cx: &Rebuild,
    params: &Params,
    block: &BlockDecl,
    el: &mut Element,
) -> Result<()> {
    let Rebuild {
        managed, residue, ..
    } = *cx;
    let managed_defs = &params.managed_defs;
    let plan = &managed[&block.slug];
    let res = residue.get(&plan.uuid);
    for key in &plan.keys {
        // A GUI-owned (`Inv=`) connector is re-emitted verbatim at its
        // spec position; a parameter binding on it would be
        // silently overridden, so it is refused instead.
        if let Some(carried) = res.and_then(|r| r.gui_ports.get(key)) {
            if managed_defs.contains_key(&(block.slug.clone(), key.clone())) {
                return Err(Error::Compile(format!(
                    "block `{}`: connector `{key}` is inverted in Loxone Config, \
                         so its value cannot be set from source. Change it in Loxone Config instead.",
                    block.slug
                )));
            }
            el.push_child(carried.clone());
            continue;
        }
        let mut co = Element::new("Co");
        co.set_attr("K", key);
        if let Some(def) = managed_defs.get(&(block.slug.clone(), key.clone())) {
            // A bound 0 is no attribute, as a save writes it.
            if crate::connectors::def_written(def) {
                co.set_attr("Def", def);
            }
        } else if let Some(stale) = res.and_then(|r| r.stale_defs.get(key))
            && block.bindings().any(|b| {
                b.port == *key
                    && matches!(
                        b.kind,
                        crate::logic::ast::BindingKind::Wire(_)
                            | crate::logic::ast::BindingKind::Expr(_)
                    )
            })
        {
            // A stale constant under a wire the source keeps.
            co.set_attr_raw("Def", stale);
        }
        co.set_attr("U", &plan.ports[key]);
        el.push_child(co);
    }
    Ok(())
}

/// The block's child elements after its connectors: a message ref's
/// message, then the carried residue in base order or what a
/// save gives a fresh object.
fn push_children(cx: &Rebuild, params: &Params, block: &BlockDecl, el: &mut Element) {
    let Rebuild {
        managed,
        residue,
        mirror_of,
        ..
    } = *cx;
    let managed_msgs = &params.managed_msgs;
    let pt = crate::periphery::periphery(&block.block_type);
    let plan = &managed[&block.slug];
    let res = residue.get(&plan.uuid);
    let message_ref = block.block_type == "OutputRefLM";
    // The message of a message ref follows its connectors, as
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
        // Attributes and connectors in the order the base wrote them.
        // Loxone Config's order differs from ours (it writes
        // `Desc=` right after `Type=`, 17.2 moves a connector).
        el.attrs = in_base_order(std::mem::take(&mut el.attrs), |a| {
            r.attr_order.iter().position(|n| *n == a.name)
        });
        order_connectors(el, &r.co_order);
    } else if let Some(p) = pt {
        for (name, attrs, open) in p.mint_children {
            let mut c = Element::new(*name);
            for (k, v) in *attrs {
                c.set_attr(k, v);
            }
            c.self_closing = !open;
            el.push_child(c);
        }
    } else if let Some((_, children)) = save_defaults(&block.block_type) {
        for (name, attrs) in children {
            let mut c = Element::new(*name);
            for (k, v) in *attrs {
                c.set_attr(k, v);
            }
            el.push_child(c);
        }
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
}

/// Inserts a built element: an object of the tree into its folder or
/// device, a block into its pinned page.
fn insert_block(
    doc: &mut LoxoneDoc,
    page_index: &DocIndex,
    cx: &Rebuild,
    block: &BlockDecl,
    el: Element,
) -> Result<()> {
    let Rebuild { managed, snap, .. } = *cx;
    let BaseSnapshot {
        base_page_order,
        base_tree_order,
        ..
    } = snap;
    let pt = crate::periphery::periphery(&block.block_type);
    let plan = &managed[&block.slug];
    // The object tree: a device or I/O goes into its folder or
    // device by UUID — inserting a device shifts its siblings' paths.
    if pt.is_some() {
        let order = base_tree_order.get(&plan.page_uuid);
        let container = find_c_mut(&mut doc.xml.root, &plan.page_uuid).ok_or_else(|| {
            Error::Compile(format!(
                "block `{}`: the object `{}` it goes into is not in the config",
                block.slug, plan.page_uuid
            ))
        })?;
        if let Some(f) = container_type(&block.block_type)
            && container.attr("Type") != Some(f)
        {
            return Err(Error::Compile(format!(
                "block `{}`: object `{}` is a `{}`, not a {f}",
                block.slug,
                plan.page_uuid,
                container.attr("Type").unwrap_or("?")
            )));
        }
        insert_in_page(container, el, order.map_or(&[], Vec::as_slice));
        return Ok(());
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
    // A VirtualIn's folder is no page: its order is the tree's.
    let order = base_page_order
        .get(&plan.page_uuid)
        .or_else(|| base_tree_order.get(&plan.page_uuid));
    insert_in_page(page, el, order.map_or(&[], Vec::as_slice));
    Ok(())
}

/// A VirtualIn's input-dialog attributes in the order a Loxone
/// Config save writes them. Each is the source's binding, else the base's
/// value, else (on a fresh mint only) what a save fills in: an analog input gets `EnVal="true"`, a digital one the
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
                "block `{slug}`: `Analog:` is `on` or `off`, not `{v}`"
            )));
        }
        None => carried("Analog").unwrap_or("false"),
    };
    // A digital input's range is fixed at 0 to 1: a save rewrites any
    // other bound range.
    for name in ["MinVal", "MaxVal"] {
        if let Some(v) = bound(name)
            && let Some(e) = crate::connectors::save_rewrite_error(
                slug,
                "VirtualIn",
                name,
                (v, v),
                analog == "false",
            )
        {
            return Err(Error::Compile(e));
        }
    }
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
