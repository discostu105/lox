//! Adopting a virtual or Modbus device: the device and every I/O
//! below it become managed blocks in one go — an I/O hangs below its
//! device, so a managed device carries none it does not manage. Adopting
//! one I/O alone leaves its device with the GUI (an extern its `device:`
//! names). A virtual input (#198) is adopted the same way: alone, its
//! input-dialog settings left to the GUI as for a minted one.

use std::collections::{BTreeMap, BTreeSet};

use crate::doc::{LoxoneDoc, ObjectSummary, ports};
use crate::error::{Error, Result};
use crate::lock::{LockedExternal, LockedObject, Lockfile, sha256_hex};
use crate::logic::adopt::{AdoptedBlock, check_promotable};
use crate::logic::ast::{
    ArgItem, Binding, BindingKind, BlockDecl, ExternDecl, Item, MatchSpec, Module, PortRef, Value,
};
use crate::logic::decompile::slugify;
use crate::periphery::{device_types, periphery};
use crate::xml::{Element, Node};

/// A value as source text writes it: a number where the attribute holds
/// one (the parser canonicalizes the same way), a string otherwise.
pub(super) fn value_of(raw: &str) -> Value {
    let numeric = {
        let digits = raw.strip_prefix('-').unwrap_or(raw);
        let mut parts = digits.splitn(2, '.');
        let int = parts.next().unwrap_or("");
        let frac = parts.next();
        !int.is_empty()
            && int.bytes().all(|b| b.is_ascii_digit())
            && (int == "0" || !int.starts_with('0'))
            && frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
    };
    if numeric {
        Value::Number(raw.to_string())
    } else {
        Value::Str(raw.to_string())
    }
}

/// `Err(reason)` unless the rebuild reproduces the element: its
/// connectors are the verified ones and carry nothing but wires.
fn verify(el: &Element, block_type: &str) -> std::result::Result<(), String> {
    let spec = periphery(block_type)
        .map(|p| p.ports)
        .or_else(|| crate::connectors::builtin(block_type))
        .expect("a periphery or builtin type");
    let keys: BTreeSet<String> = ports(el).into_iter().map(|p| p.key).collect();
    let want: BTreeSet<String> = spec.iter().map(|p| p.key.to_string()).collect();
    if keys != want {
        return Err(format!(
            "its connectors [{}] differ from the verified [{}]",
            keys.into_iter().collect::<Vec<_>>().join(", "),
            want.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    if !spec.is_empty() && el.attr("Nio") != Some(spec.len().to_string().as_str()) {
        return Err(format!(
            "Nio=\"{}\" does not match its {} connectors",
            el.attr("Nio").unwrap_or(""),
            spec.len()
        ));
    }
    if !crate::connectors::VERIFIED_BLOCK_VERSIONS.contains(&el.attr("V").unwrap_or("")) {
        return Err(format!(
            "block version V=\"{}\" is not one this version of Lox Logic is verified for",
            el.attr("V").unwrap_or("")
        ));
    }
    for co in el.child_elements().filter(|c| c.name == "Co") {
        if let Some(a) = co
            .attrs
            .iter()
            .find(|a| !matches!(a.name.as_str(), "K" | "U" | "Nc"))
        {
            return Err(format!(
                "connector `{}`: attribute `{}` is not understood",
                co.attr("K").unwrap_or(""),
                a.name
            ));
        }
        for n in &co.children {
            match n {
                Node::Element(i)
                    if i.name == "In"
                        && i.attrs
                            .iter()
                            .all(|a| matches!(a.name.as_str(), "Input" | "FLG")) => {}
                Node::Text(t) if t.trim().is_empty() => {}
                _ => {
                    return Err(format!(
                        "connector `{}`: content not understood",
                        co.attr("K").unwrap_or("")
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn adopt_device(
    doc: &LoxoneDoc,
    root: &ObjectSummary,
    slug: &str,
    raw: &Module,
    expanded: &Module,
    lock: &mut Lockfile,
    mut taken: BTreeSet<String>,
) -> Result<AdoptedBlock> {
    let fail = |m: String| Error::Compile(m);
    let objects = doc.objects();
    let by_uuid: BTreeMap<&str, &ObjectSummary> =
        objects.iter().map(|o| (o.uuid.as_str(), o)).collect();
    let parent_of = |o: &ObjectSummary| -> Option<&ObjectSummary> {
        (1..o.path.len()).rev().find_map(|n| {
            let el = doc.element_at(&o.path[..n])?;
            if el.name != "C" {
                return None;
            }
            by_uuid.get(el.attr("U")?).copied()
        })
    };
    // The device and what hangs below it, in document order: every
    // object of a periphery type inside the root.
    let members: Vec<&ObjectSummary> = objects
        .iter()
        .filter(|o| {
            o.uuid == root.uuid
                || (o.path.starts_with(&root.path) && periphery(&o.block_type).is_some())
        })
        .collect();
    let managed_uuid: BTreeMap<&str, &str> = lock
        .objects
        .iter()
        .map(|(s, o)| (o.uuid.as_str(), s.as_str()))
        .collect();
    let extern_slug: BTreeMap<&str, &str> = lock
        .externals
        .iter()
        .map(|(s, e)| (e.uuid.as_str(), s.as_str()))
        .collect();

    // Slugs: an extern keeps its own (promotion), the root takes `slug`,
    // the rest are named after the root and their title.
    let mut slug_of: BTreeMap<String, String> = BTreeMap::new();
    let mut promoted: Vec<String> = Vec::new();
    for o in &members {
        let ident = format!(
            "{} \"{}\" (uuid {})",
            o.block_type,
            o.title.as_deref().unwrap_or(""),
            o.uuid
        );
        if let Some(s) = managed_uuid.get(o.uuid.as_str()) {
            return Err(fail(format!("{ident} is already managed as `{s}`")));
        }
        let el = doc.element_at(&o.path).expect("path from objects()");
        verify(el, &o.block_type).map_err(|why| fail(format!("cannot adopt {ident}: {why}")))?;
        if let Some(x) = extern_slug.get(o.uuid.as_str()) {
            if o.uuid == root.uuid && *x != slug {
                return Err(fail(format!(
                    "{ident} is extern `{x}` of this module. Adopt it `--as {x}` to keep the slug the statements already use."
                )));
            }
            check_promotable(doc, x, &o.uuid, raw, expanded, lock)
                .map_err(|why| fail(format!("cannot promote `{x}`: {why}")))?;
            slug_of.insert(o.uuid.clone(), x.to_string());
            promoted.push(x.to_string());
            continue;
        }
        let s = if o.uuid == root.uuid {
            // The caller checked and claimed it.
            slug.to_string()
        } else {
            let base = format!("{slug}_{}", slugify(o.title.as_deref().unwrap_or("io")));
            let mut candidate = base.clone();
            let mut n = 1;
            while !taken.insert(candidate.clone()) {
                n += 1;
                candidate = format!("{base}_{n}");
            }
            candidate
        };
        slug_of.insert(o.uuid.clone(), s);
    }

    // What the source already writes onto a promoted sink moves into its
    // declaration; the config's copy of those wires is not lifted again.
    let mut source_wires: BTreeSet<(String, String)> = lock
        .extern_wires
        .iter()
        .map(|w| (w.to.clone(), w.from.clone()))
        .collect();
    // A source wire the compile drew through an OutputRef lands on
    // the ref's `AI`; the ref's `AQ` into the sink is the same wire.
    let source_to: BTreeSet<&str> = lock.extern_wires.iter().map(|w| w.to.as_str()).collect();
    for o in objects.iter().filter(|o| o.block_type == "OutputRef") {
        let el = doc.element_at(&o.path).expect("path from objects()");
        let ps = ports(el);
        let (Some(ai), Some(aq)) = (
            ps.iter().find(|p| p.key == "AI"),
            ps.iter().find(|p| p.key == "AQ"),
        ) else {
            continue;
        };
        if !source_to.contains(ai.uuid.as_str()) {
            continue;
        }
        for w in doc.wires().iter().filter(|w| w.from_port == aq.uuid) {
            source_wires.insert((w.to_port.clone(), aq.uuid.clone()));
        }
    }
    let port_owner: BTreeMap<String, (&ObjectSummary, String)> = objects
        .iter()
        .flat_map(|ob| {
            let el = doc.element_at(&ob.path).expect("path from objects()");
            ports(el).into_iter().map(move |p| (p.uuid, (ob, p.key)))
        })
        .collect();

    let mut items: Vec<Item> = Vec::new();
    let mut new_externs: Vec<String> = Vec::new();
    // A neighbour the source does not name yet becomes an extern pinned
    // by UUID.
    let mut pin = |o: &ObjectSummary,
                   items: &mut Vec<Item>,
                   lock: &mut Lockfile,
                   taken: &mut BTreeSet<String>|
     -> String {
        if let Some(s) = slug_of.get(&o.uuid) {
            return s.clone();
        }
        if let Some((s, _)) = lock.objects.iter().find(|(_, lo)| lo.uuid == o.uuid) {
            return s.clone();
        }
        if let Some((s, _)) = lock.externals.iter().find(|(_, le)| le.uuid == o.uuid) {
            return s.clone();
        }
        let base = slugify(o.title.as_deref().unwrap_or(&o.block_type));
        let mut candidate = base.clone();
        let mut n = 1;
        while !taken.insert(candidate.clone()) {
            n += 1;
            candidate = format!("{base}_{n}");
        }
        items.push(Item::Extern(ExternDecl {
            slug: candidate.clone(),
            block_type: o.block_type.clone(),
            match_spec: MatchSpec::Uuid(o.uuid.clone()),
            room: None,
            category: None,
            device: None,
            comment: o.title.as_ref().map(|t| format!(" \"{t}\"")),
        }));
        lock.externals.insert(
            candidate.clone(),
            LockedExternal {
                uuid: o.uuid.clone(),
                matched_by: "uuid".to_string(),
                title_at_match: o.title.clone(),
                iname_at_match: o.iname.clone(),
                owned: false,
            },
        );
        new_externs.push(candidate.clone());
        candidate
    };

    let mut blocks: Vec<BlockDecl> = Vec::new();
    for o in &members {
        let el = doc.element_at(&o.path).expect("path from objects()");
        let settings = periphery(&o.block_type).map_or(&[][..], |p| p.settings);
        let s = slug_of[&o.uuid].clone();
        let mut args: Vec<ArgItem> = Vec::new();
        if !device_types(&o.block_type).is_empty() {
            let device = parent_of(o)
                .and_then(|p| {
                    // A caption (`SensorCaption`) is not the device.
                    if device_types(&o.block_type).contains(&p.block_type.as_str()) {
                        Some(p)
                    } else {
                        parent_of(p)
                    }
                })
                .filter(|p| device_types(&o.block_type).contains(&p.block_type.as_str()))
                .ok_or_else(|| {
                    fail(format!(
                        "{} \"{}\" does not sit in a {}",
                        o.block_type,
                        o.title.as_deref().unwrap_or(""),
                        device_types(&o.block_type).join(" or ")
                    ))
                })?;
            let d = pin(device, &mut items, lock, &mut taken);
            args.push(ArgItem::Binding(Binding {
                port: "device".to_string(),
                kind: BindingKind::Param(Value::Ref(d)),
                comment: None,
            }));
        }
        // A calendar entry's mode, by its one spelling: a day word, the
        // module's slug for it, or its title.
        if o.block_type == "CalendarEntry" {
            let number = el.attr("CalType").map_or(Ok(0), str::parse).map_err(|_| {
                fail(format!(
                    "CalendarEntry \"{}\": `CalType=\"{}\"` is no number",
                    o.title.as_deref().unwrap_or(""),
                    el.attr("CalType").unwrap_or("")
                ))
            })?;
            let known: BTreeMap<String, String> = lock
                .objects
                .iter()
                .map(|(s, lo)| (lo.uuid.clone(), s.clone()))
                .chain(
                    lock.externals
                        .iter()
                        .map(|(s, le)| (le.uuid.clone(), s.clone())),
                )
                .chain(slug_of.iter().map(|(u, s)| (u.clone(), s.clone())))
                .collect();
            let mode = crate::logic::calendar::lift(doc, number, &known).map_err(|why| {
                fail(format!(
                    "cannot adopt CalendarEntry \"{}\": {why}",
                    o.title.as_deref().unwrap_or("")
                ))
            })?;
            args.push(ArgItem::Binding(Binding {
                port: crate::logic::calendar::SWITCHES.to_string(),
                kind: BindingKind::Param(mode),
                comment: None,
            }));
            // Its date, as a date (`from: 10-01, to: 04-30`).
            let date =
                crate::logic::calendar::lift_date(|n| el.attr_decoded(n).map(|v| v.into_owned()))
                    .map_err(|why| {
                    fail(format!(
                        "cannot adopt CalendarEntry \"{}\": {why}",
                        o.title.as_deref().unwrap_or("")
                    ))
                })?;
            for (port, v) in date {
                args.push(ArgItem::Binding(Binding {
                    port: port.to_string(),
                    kind: BindingKind::Param(v),
                    comment: None,
                }));
            }
        }
        for name in settings {
            if crate::logic::calendar::is_raw_date(&o.block_type, name) {
                continue;
            }
            // A setting a save leaves out at 0 or unticked reads as such
            // where a fresh mint would fill in something else.
            let v = el
                .attr_decoded(name)
                .or_else(|| crate::periphery::missing_value(&o.block_type, name).map(Into::into));
            if let Some(v) = v {
                args.push(ArgItem::Binding(Binding {
                    port: (*name).to_string(),
                    kind: BindingKind::Param(
                        if crate::connectors::bool_params(&o.block_type).contains(name)
                            && (v == "true" || v == "false")
                        {
                            Value::Bool(v == "true") // `on`/`off`
                        } else {
                            // A duration reads in its unit (`4s`, not `4000`).
                            crate::logic::ast::duration_value(&o.block_type, name, &v)
                                .unwrap_or_else(|| value_of(&v))
                        },
                    ),
                    comment: None,
                }));
            }
        }
        // Wires the GUI drew into a sink: bindings from their source.
        for co in el.child_elements().filter(|c| c.name == "Co") {
            let key = co.attr_decoded("K").unwrap_or_default().into_owned();
            let to = co.attr("U").unwrap_or("").to_string();
            for i in co.child_elements().filter(|i| i.name == "In") {
                let from = i.attr("Input").unwrap_or("").to_string();
                if source_wires.contains(&(to.clone(), from.clone())) {
                    continue;
                }
                let (owner, from_key) = port_owner.get(&from).cloned().ok_or_else(|| {
                    fail(format!(
                        "{s}.{key}: the wire from `{from}` names no connector in the config"
                    ))
                })?;
                let src = pin(owner, &mut items, lock, &mut taken);
                args.push(ArgItem::Binding(Binding {
                    port: key.clone(),
                    kind: BindingKind::Wire(PortRef {
                        slug: src,
                        port: from_key,
                    }),
                    comment: None,
                }));
            }
        }
        if promoted.contains(&s) {
            for item in &raw.items {
                let binding = match item {
                    Item::Wire(w) if w.to.slug == s => Binding {
                        port: w.to.port.clone(),
                        kind: BindingKind::Wire(w.from.clone()),
                        comment: w.comment.clone(),
                    },
                    Item::ExprWire(w) if w.to.slug == s => Binding {
                        port: w.to.port.clone(),
                        kind: BindingKind::Expr(w.expr.clone()),
                        comment: w.comment.clone(),
                    },
                    Item::Set(st) if st.target.slug == s => {
                        return Err(fail(format!(
                            "cannot promote `{s}`: `{s}.{} = …` sets a connector value, which a {} does not take. Remove it first.",
                            st.target.port, o.block_type
                        )));
                    }
                    _ => continue,
                };
                args.push(ArgItem::Binding(binding));
            }
        }
        blocks.push(BlockDecl {
            slug: s.clone(),
            block_type: o.block_type.clone(),
            title: o.title.clone().filter(|t| *t != s),
            args,
            comment: None,
            close_comment: None,
        });
    }

    // The lock: identities of every member; a promoted extern's records
    // end here (its wires are the block's arguments now).
    for o in &members {
        let el = doc.element_at(&o.path).expect("path from objects()");
        let s = slug_of[&o.uuid].clone();
        if promoted.contains(&s) {
            let own: BTreeSet<String> = ports(el).into_iter().map(|p| p.uuid).collect();
            lock.externals.remove(&s);
            lock.extern_wires.retain(|w| !own.contains(&w.to));
            lock.set_originals.retain(|u, _| !own.contains(u));
            lock.attr_claims.remove(&o.uuid);
        }
        lock.objects.insert(
            s,
            LockedObject {
                uuid: o.uuid.clone(),
                block_type: o.block_type.clone(),
                ports: ports(el).into_iter().map(|p| (p.key, p.uuid)).collect(),
                layout: None,
                superseded_layout: None,
                page_uuid: None,
                expr_owned: false,
                states: Vec::new(),
            },
        );
    }
    lock.target.config_version = doc.config_version();
    lock.target.source_config_sha256 = Some(sha256_hex(&doc.to_bytes()));
    lock.target.semantic_fingerprint = Some(crate::diff::semantic_fingerprint(doc));

    items.extend(blocks.into_iter().map(Item::Block));
    // The folder the device sits in names the file it goes into.
    let folder = (1..root.path.len())
        .rev()
        .filter_map(|n| doc.element_at(&root.path[..n]))
        .find(|el| {
            el.name == "C"
                && el
                    .attr("Type")
                    .is_some_and(|t| t.ends_with("Caption") && periphery(t).is_none())
        })
        .and_then(|el| el.attr_decoded("Title").map(|t| t.into_owned()))
        .unwrap_or_else(|| "Peripherie".to_string());
    Ok(AdoptedBlock {
        items,
        page_title: folder,
        new_externs,
        promoted,
        on_page: false,
    })
}
