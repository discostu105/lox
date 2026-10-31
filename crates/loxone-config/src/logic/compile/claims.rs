//! Extern edits: port assignments, settings (D65) and attribute claims
//! (D39), each with the tombstones of the withdrawn ones (D31).

use super::*;

/// Port assignments (always extern ports — validate rejects managed
/// targets), and settings of physical inputs and outputs (D65).
pub(super) fn apply_sets(doc: &mut LoxoneDoc, lock: &mut Lockfile, cx: &Rebuild) -> Result<()> {
    let Rebuild {
        module,
        managed,
        extern_uuid,
        ..
    } = *cx;
    for s in module.sets() {
        // A setting of a physical input or output (D65): an attribute of
        // the extern object, with the port assignments' lifecycle.
        if let Some(e) = module.externs().find(|e| e.slug == s.target.slug)
            && let Some(addable) = crate::settings::setting(&e.block_type, &s.target.port)
            && let Some(owner) = extern_uuid.get(&s.target.slug)
            && find_c(&doc.xml.root, owner).is_some_and(|c| {
                !c.child_elements()
                    .any(|co| co.name == "Co" && co.attr("K") == Some(s.target.port.as_str()))
            })
        {
            let value = module
                .resolve_value_for(&s.value, &e.block_type, &s.target.slug, &s.target.port)?
                .into_owned();
            let original = read_setting(doc, owner, &s.target.port);
            if original.is_none() && !addable {
                return Err(Error::Compile(format!(
                    "`{}.{}`: the object ({owner}) carries no `{}` — Lox Logic rewrites \
                     this setting, it never adds one (it belongs to another kind \
                     of input or output)",
                    s.target.slug, s.target.port, s.target.port
                )));
            }
            write_setting(doc, owner, &s.target.port, Some(&value));
            lock.set_originals
                .entry(crate::settings::key(owner, &s.target.port))
                .or_insert(original);
            continue;
        }
        let target = resolve_port(
            doc,
            managed,
            extern_uuid,
            &s.target.slug,
            &s.target.port,
            PortDir::Param,
        )
        .map_err(
            |err| match module.externs().find(|e| e.slug == s.target.slug) {
                // A device's port that is none: name its settings.
                Some(e) if crate::settings::DEVICE_TYPES.contains(&e.block_type.as_str()) => {
                    Error::Compile(format!(
                        "`{}.{}`: neither a port nor a setting of a {} the source can \
                         own ({})",
                        s.target.slug,
                        s.target.port,
                        e.block_type,
                        crate::settings::names(&e.block_type).join(", ")
                    ))
                }
                Some(e)
                    if crate::settings::is_setting(&s.target.port)
                        && crate::settings::setting(&e.block_type, &s.target.port).is_none() =>
                {
                    Error::Compile(format!(
                        "`{}.{}`: {} is not a physical input or output whose settings \
                     the source can own ({})",
                        s.target.slug,
                        s.target.port,
                        e.block_type,
                        crate::settings::SETTING_TYPES
                            .iter()
                            .chain(crate::settings::DEVICE_TYPES)
                            .copied()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                }
                _ => err,
            },
        )?;
        let extern_type = module
            .externs()
            .find(|e| e.slug == s.target.slug)
            .map_or("", |e| e.block_type.as_str());
        let value = module
            .resolve_value_for(&s.value, extern_type, &s.target.slug, &s.target.port)?
            .into_owned();
        let original = apply_def(doc, &target.owner_uuid, &target.port_uuid, &value)?;
        lock.set_originals
            .entry(target.port_uuid)
            .or_insert(original);
    }
    Ok(())
}

/// A Def write that vanished from source was withdrawn — tombstone it
/// (D31) unless it never changed the port's bytes in the first place.
pub(super) fn retire_sets(lock: &mut Lockfile, prior: &PriorEdits) {
    let PriorEdits {
        old_originals,
        defs_written,
        ..
    } = prior;
    for (port_uuid, original) in old_originals {
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
}

/// Attribute claims on externs (D39), and a managed block's room and
/// category (D63). Planned against the torn-down document before anything
/// is written, so nested claims (a device and one of its outputs) never
/// record each other's write as their original, and an explicitly claimed
/// descendant is never a follower.
pub(super) fn apply_claims(doc: &mut LoxoneDoc, lock: &mut Lockfile, cx: &Rebuild) -> Result<()> {
    let Rebuild {
        module,
        managed,
        extern_uuid,
        ..
    } = *cx;
    let resolve = |v: &Value| -> Result<String> { Ok(module.resolve_value(v)?.to_string()) };
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
            Attr::Room => entry.0 = Some(taxonomy_uuid(doc, "Place", "room", &value)?),
            Attr::Category => {
                entry.1 = Some(taxonomy_uuid(doc, "Category", "category", &value)?);
            }
            Attr::Title => unreachable!("validate refuses a managed block's `.title`"),
        }
    }
    for (uuid, (room, category)) in &managed_claims {
        write_binding(doc, uuid, room.as_deref(), category.as_deref());
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
            Attr::Room => taxonomy_uuid(doc, "Place", "room", &value)?,
            Attr::Category => taxonomy_uuid(doc, "Category", "category", &value)?,
        };
        let original = read_claimed(doc, &uuid, a.attr).ok_or_else(|| {
            Error::Compile(format!(
                "`{}.{}`: the object ({uuid}) carries no {} — Lox Logic rewrites an \
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
            Attr::Title => ref_followers(doc, &uuid, &stop),
            Attr::Room | Attr::Category => followers_of(doc, &uuid, a.attr, &original, &stop),
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
        write_claimed(doc, &uuid, attr, &c.written);
        for f in &c.followers {
            write_claimed(doc, f, attr, &c.written);
        }
        lock.attr_claims.entry(uuid).or_default().insert(attr, c);
    }
    Ok(())
}

/// A claim that vanished from source was withdrawn — tombstone it (D31)
/// unless it never changed the object's bytes in the first place.
pub(super) fn retire_claims(lock: &mut Lockfile, prior: &PriorEdits) {
    let PriorEdits {
        old_claims,
        claims_written,
        ..
    } = prior;
    for (uuid, claims) in old_claims {
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
}
