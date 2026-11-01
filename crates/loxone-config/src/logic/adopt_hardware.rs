//! Adopting hardware: a Tree or Air device becomes a declaration, and so
//! does every input and output below it. An extension (`LoxTree`,
//! `LoxAIR`) is adopted alone. The declarations own what the config holds
//! now, so the next compile changes nothing. Wires the GUI drew stay with
//! the GUI; a promoted extern's statements move into its declaration.

use std::collections::BTreeSet;

use crate::doc::{LoxoneDoc, ObjectSummary, element_serial, ports, same_serial};
use crate::error::{Error, Result};
use crate::hardware::{STATISTICS, ios, is_device, parents, stats_value};
use crate::lock::{LockedExternal, Lockfile, sha256_hex};
use crate::logic::adopt::{AdoptedBlock, check_promotable};
use crate::logic::adopt_tree::value_of;
use crate::logic::ast::{
    ArgItem, Attr, AttrDecl, Binding, BindingKind, BlockDecl, Item, Module, Value,
};
use crate::logic::decompile::slugify;
use crate::xml::Element;

/// A serial Loxone Config writes for a device it has not paired yet.
fn unpaired(serial: &str) -> bool {
    serial.bytes().all(|b| b == b'0' || b == b':')
}

/// A setting as the element holds it: an attribute, or `Unit=` of its
/// `<Display>`.
fn read_setting(el: &Element, name: &str) -> Option<String> {
    let el = if name == "Unit" {
        el.child_elements().find(|e| e.name == "Display")?
    } else {
        el
    };
    el.attr_decoded(name).map(|v| v.into_owned())
}

/// The title of the `Place`/`Category` an I/O's `<IoData>` names, when
/// exactly one carries it (the source names a room by its title).
fn taxonomy_title(doc: &LoxoneDoc, el: &Element, attr: &str, ty: &str) -> Option<String> {
    let uuid = el
        .child_elements()
        .find(|e| e.name == "IoData")?
        .attr(attr)?;
    let all = doc.objects();
    let title = all
        .iter()
        .find(|o| o.uuid == uuid && o.block_type == ty)?
        .title
        .clone()?;
    let same = all
        .iter()
        .filter(|o| o.block_type == ty && o.title.as_deref() == Some(title.as_str()))
        .count();
    (same == 1).then_some(title)
}

pub(super) fn adopt(
    doc: &LoxoneDoc,
    root: &ObjectSummary,
    slug: &str,
    raw: &Module,
    expanded: &Module,
    lock: &mut Lockfile,
    mut taken: BTreeSet<String>,
) -> Result<AdoptedBlock> {
    let fail = |m: String| Error::Compile(m);
    let ident = |o: &ObjectSummary| {
        format!(
            "{} \"{}\" (uuid {})",
            o.block_type,
            o.title.as_deref().unwrap_or(""),
            o.uuid
        )
    };
    if !is_device(&root.block_type) {
        return Err(fail(format!(
            "cannot adopt {}: an input or output comes with its device. Adopt the {} it sits in.",
            ident(root),
            parents(&root.block_type).join(" or ")
        )));
    }
    let objects = doc.objects();
    // The I/Os below the device: those of its types whose nearest device
    // is this one.
    let nearest_device = |o: &ObjectSummary| {
        objects
            .iter()
            .filter(|d| {
                is_device(&d.block_type)
                    && d.path.len() < o.path.len()
                    && o.path.starts_with(&d.path)
            })
            .max_by_key(|d| d.path.len())
            .map(|d| d.uuid.clone())
    };
    let members: Vec<&ObjectSummary> = std::iter::once(root)
        .chain(objects.iter().filter(|o| {
            ios(&root.block_type).contains(&o.block_type.as_str())
                && nearest_device(o).as_deref() == Some(root.uuid.as_str())
        }))
        .collect();

    // The device: by serial when it has a paired one no other device of
    // its type shares.
    let root_el = doc.element_at(&root.path).expect("path from objects()");
    let serial = element_serial(root_el)
        .map(|s| s.into_owned())
        .filter(|s| !s.is_empty() && !unpaired(s))
        .filter(|s| {
            objects
                .iter()
                .filter(|o| o.block_type == root.block_type)
                .filter_map(|o| element_serial(doc.element_at(&o.path)?).map(|x| x.into_owned()))
                .filter(|x| same_serial(x, s))
                .count()
                == 1
        });

    let mut items: Vec<Item> = Vec::new();
    let mut promoted: Vec<String> = Vec::new();
    let mut device_slug = String::new();
    for o in &members {
        let el = doc.element_at(&o.path).expect("path from objects()");
        if lock.objects.values().any(|lo| lo.uuid == o.uuid) {
            return Err(fail(format!("{} is managed already", ident(o))));
        }
        let ext = lock
            .externals
            .iter()
            .find(|(_, e)| e.uuid == o.uuid)
            .map(|(s, e)| (s.clone(), e.owned));
        let s = match ext {
            Some((x, true)) => {
                return Err(fail(format!("{} is declared as `{x}` already", ident(o))));
            }
            Some((x, false)) => {
                if o.uuid == root.uuid && x != slug {
                    return Err(fail(format!(
                        "{} is extern `{x}` of this module. Adopt it `--as {x}` to keep the \
                         slug the statements already use.",
                        ident(o)
                    )));
                }
                check_promotable(doc, &x, &o.uuid, raw, expanded, lock)
                    .map_err(|why| fail(format!("cannot promote `{x}`: {why}")))?;
                promoted.push(x.clone());
                x
            }
            None if o.uuid == root.uuid => slug.to_string(),
            None => {
                let base = format!("{slug}_{}", slugify(o.title.as_deref().unwrap_or("io")));
                let mut candidate = base.clone();
                let mut n = 1;
                while !taken.insert(candidate.clone()) {
                    n += 1;
                    candidate = format!("{base}_{n}");
                }
                candidate
            }
        };

        let param = |port: &str, v: Value| {
            ArgItem::Binding(Binding {
                port: port.to_string(),
                kind: BindingKind::Param(v),
                comment: None,
            })
        };
        let mut args: Vec<ArgItem> = Vec::new();
        let matched_by;
        if o.uuid == root.uuid {
            device_slug = s.clone();
            matched_by = match &serial {
                Some(x) => {
                    args.push(param("serial", Value::Str(x.clone())));
                    "serial"
                }
                None => {
                    args.push(param("uuid", Value::Str(o.uuid.clone())));
                    "uuid"
                }
            };
        } else {
            args.push(param("device", Value::Ref(device_slug.clone())));
            // By designation when the device goes by serial and no
            // sibling of its type shares it.
            let iname = o.iname.clone().filter(|i| {
                serial.is_some()
                    && members
                        .iter()
                        .filter(|m| {
                            m.block_type == o.block_type && m.iname.as_deref() == Some(i.as_str())
                        })
                        .count()
                        == 1
            });
            matched_by = match iname {
                Some(i) => {
                    args.push(param("iname", Value::Str(i)));
                    "iname"
                }
                None => {
                    args.push(param("uuid", Value::Str(o.uuid.clone())));
                    "uuid"
                }
            };
        }
        for name in crate::settings::names(&o.block_type) {
            if let Some(v) = read_setting(el, name) {
                args.push(param(name, value_of(&v)));
            }
        }
        // Statistics the type records with; any other kind stays with
        // Loxone Config.
        if let Some(want) = stats_value(&o.block_type)
            && el.attr("StatsType") == Some(want)
        {
            args.push(param(STATISTICS, Value::Bool(true)));
        }
        if promoted.contains(&s) {
            for item in &raw.items {
                let (port, kind) = match item {
                    Item::Wire(w) if w.to.slug == s => {
                        (&w.to.port, BindingKind::Wire(w.from.clone()))
                    }
                    Item::ExprWire(w) if w.to.slug == s => {
                        (&w.to.port, BindingKind::Expr(w.expr.clone()))
                    }
                    Item::Set(st) if st.target.slug == s => {
                        (&st.target.port, BindingKind::Param(st.value.clone()))
                    }
                    _ => continue,
                };
                // A claimed setting holds the config's value now.
                args.retain(|a| !matches!(a, ArgItem::Binding(b) if b.port == *port));
                args.push(ArgItem::Binding(Binding {
                    port: port.clone(),
                    kind,
                    comment: None,
                }));
            }
        }
        items.push(Item::Block(BlockDecl {
            slug: s.clone(),
            block_type: o.block_type.clone(),
            title: o.title.clone(),
            args,
            comment: None,
            close_comment: None,
        }));
        // A promoted extern's `.room`/`.category` stay in the source.
        let claimed = |attr: Attr| {
            raw.items
                .iter()
                .any(|i| matches!(i, Item::Attr(a) if a.slug == s && a.attr == attr))
        };
        for (attr, xml, ty) in [
            (Attr::Room, "Pr", "Place"),
            (Attr::Category, "Cr", "Category"),
        ] {
            if claimed(attr) {
                continue;
            }
            if let Some(t) = taxonomy_title(doc, el, xml, ty) {
                items.push(Item::Attr(AttrDecl {
                    slug: s.clone(),
                    attr,
                    value: Value::Str(t),
                    comment: None,
                    bulk: None,
                }));
            }
        }

        // The lock: the declaration replaces the extern's records.
        let own: BTreeSet<String> = ports(el).into_iter().map(|p| p.uuid).collect();
        lock.extern_wires.retain(|w| !own.contains(&w.to));
        let prefix = format!("{}#", o.uuid);
        lock.set_originals.retain(|k, _| !k.starts_with(&prefix));
        lock.attr_claims.remove(&o.uuid);
        lock.externals.insert(
            s,
            LockedExternal {
                uuid: o.uuid.clone(),
                matched_by: matched_by.to_string(),
                title_at_match: o.title.clone(),
                iname_at_match: o.iname.clone(),
                owned: true,
            },
        );
    }
    lock.target.config_version = doc.config_version();
    lock.target.source_config_sha256 = Some(sha256_hex(&doc.to_bytes()));
    lock.target.semantic_fingerprint = Some(crate::diff::semantic_fingerprint(doc));

    Ok(AdoptedBlock {
        items,
        // Hardware goes into one file.
        page_title: "Hardware".to_string(),
        new_externs: Vec::new(),
        promoted,
        on_page: false,
    })
}
