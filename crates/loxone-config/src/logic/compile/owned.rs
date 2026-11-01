//! What hardware declarations own: title, room, category and settings of
//! an existing device or I/O, written on every compile. Unlike a claim, no
//! original is kept: a declaration that leaves the source leaves the
//! values as they are, and the object is Loxone Config's again.

use super::*;
use crate::hardware::{STATISTICS, stats_value};

pub(super) fn apply_owned(doc: &mut LoxoneDoc, lock: &mut Lockfile, cx: &Rebuild) -> Result<()> {
    let Rebuild {
        module,
        extern_uuid,
        ..
    } = *cx;
    let paths: BTreeMap<String, Vec<usize>> = doc
        .objects()
        .into_iter()
        .map(|o| (o.uuid, o.path))
        .collect();
    let mut by_uuid: BTreeMap<&str, &str> = BTreeMap::new();
    for o in module.owned() {
        let uuid = extern_uuid[&o.slug].as_str();
        if let Some(other) = by_uuid.insert(uuid, &o.slug) {
            return Err(Error::Compile(format!(
                "`{other}` and `{}` declare the same {} ({uuid}). Keep one.",
                o.slug, o.block_type
            )));
        }
        if let Some(dev) = &o.device {
            let dev_uuid = &extern_uuid[dev];
            if !paths[uuid].starts_with(&paths[dev_uuid]) {
                return Err(Error::Compile(format!(
                    "`{}` ({uuid}) is not below its device `{dev}` ({dev_uuid}).",
                    o.slug
                )));
            }
        }
        // Owned is not claimed: a claim on the same object would restore
        // what the declaration writes.
        let claimed = lock.attr_claims.get(uuid).is_some_and(|m| {
            m.keys().any(|a| match a {
                Attr::Title => o.title.is_some(),
                Attr::Room => o.room.is_some(),
                Attr::Category => o.category.is_some(),
            })
        }) || o
            .settings
            .iter()
            .any(|(k, _)| lock.set_originals.contains_key(&crate::settings::key(uuid, k)));
        if claimed {
            return Err(Error::Compile(format!(
                "`{}` declares the {} ({uuid}) another statement claims an attribute of. \
                 Keep the declaration and delete the claim.",
                o.slug, o.block_type
            )));
        }
        write_owned(doc, module, uuid, o)?;
        // A claim withdrawn in favor of the declaration leaves no
        // tombstone: the declaration's value stands.
        if let Some(m) = lock.removed_attrs.get_mut(uuid) {
            m.retain(|a, _| match a {
                Attr::Title => o.title.is_none(),
                Attr::Room => o.room.is_none(),
                Attr::Category => o.category.is_none(),
            });
            if m.is_empty() {
                lock.removed_attrs.remove(uuid);
            }
        }
        for (k, _) in &o.settings {
            lock.removed_sets.remove(&crate::settings::key(uuid, k));
        }
    }
    Ok(())
}

fn write_owned(
    doc: &mut LoxoneDoc,
    module: &Module,
    uuid: &str,
    o: &crate::logic::ast::OwnedDecl,
) -> Result<()> {
    let missing = |what: &str| {
        Error::Compile(format!(
            "`{}`: the {} ({uuid}) has no {what}. Set it once in Loxone Config, since Lox \
             Logic changes it but never adds it.",
            o.slug, o.block_type
        ))
    };
    if let Some(t) = &o.title {
        write_claimed(doc, uuid, Attr::Title, t);
        // Loxone Config gives a ref its target's title on every save.
        for r in ref_followers(doc, uuid, &|_| false) {
            write_claimed(doc, &r, Attr::Title, t);
        }
    }
    for (attr, value, ty) in [
        (Attr::Room, &o.room, "Place"),
        (Attr::Category, &o.category, "Category"),
    ] {
        let Some(v) = value else { continue };
        let title = module.resolve_value(v)?.to_string();
        let target = taxonomy_uuid(doc, ty, attr.name(), &title)?;
        if read_claimed(doc, uuid, attr).is_none() {
            return Err(missing(claim_slot(attr)));
        }
        write_claimed(doc, uuid, attr, &target);
    }
    for (name, v) in &o.settings {
        if name == STATISTICS {
            let Some(c) = find_c_mut(&mut doc.xml.root, uuid) else {
                continue;
            };
            if *v == Value::Bool(true) {
                c.set_attr("StatsType", stats_value(&o.block_type).expect("lowering checked"));
            } else {
                c.remove_attr("StatsType");
                c.remove_attr("StatsAutoDel");
            }
            continue;
        }
        let addable = crate::settings::setting(&o.block_type, name).expect("lowering checked");
        let value = module
            .resolve_value_for(v, &o.block_type, &o.slug, name)?
            .into_owned();
        if read_setting(doc, uuid, name).is_none() && !addable {
            return Err(missing(&format!("`{name}` setting")));
        }
        write_setting(doc, uuid, name, Some(&value));
    }
    Ok(())
}
