//! Extern edits: port assignments, settings and attribute claims,
//! each with the tombstones of the withdrawn ones.

use super::*;

/// Port assignments (always extern ports — validate rejects managed
/// targets), and settings of physical inputs and outputs.
pub(super) fn apply_sets(doc: &mut LoxoneDoc, lock: &mut Lockfile, cx: &Rebuild) -> Result<()> {
    let Rebuild {
        module,
        managed,
        extern_uuid,
        ..
    } = *cx;
    for s in module.sets() {
        // A setting of the project or the Miniserver: one attribute or
        // two (`Updates`), each with the port assignments' lifecycle.
        if let Some(e) = module.externs().find(|e| e.slug == s.target.slug)
            && let Some(sys) = crate::settings::system_setting(&e.block_type, &s.target.port)
            && let Some(owner) = extern_uuid.get(&s.target.slug)
        {
            let writes = system_writes(module, s, sys)?;
            // Automatic in a 17.2 save's form (`AUE="true"`, no `AUM=`) is
            // automatic already: leave the bytes as they are.
            if sys.kind == crate::settings::SystemKind::Updates
                && writes
                    .iter()
                    .any(|(n, v)| *n == "AUM" && v.as_deref() == Some("1"))
                && read_setting(doc, owner, "AUE").as_deref() == Some("true")
                && read_setting(doc, owner, "AUM").is_none()
            {
                continue;
            }
            for (name, value) in writes {
                let original = read_setting(doc, owner, name);
                if original.is_none() && sys.kind == crate::settings::SystemKind::Text {
                    return Err(Error::Compile(format!(
                        "`{}.{}`: the object ({owner}) has no `{name}=`. Lox Logic changes \
                         this setting but never adds it: save the project once in Loxone \
                         Config 17.",
                        s.target.slug, s.target.port
                    )));
                }
                write_setting(doc, owner, name, value.as_deref());
                lock.set_originals
                    .entry(crate::settings::key(owner, name))
                    .or_insert(original);
            }
            continue;
        }
        // A setting of a physical input or output: an attribute of
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
            let name = s.target.port.as_str();
            let original = read_setting(doc, owner, name);
            // A 0 a save leaves out adds nothing.
            let omitted = crate::connectors::is_zero(&value)
                && crate::connectors::omitted_when_zero(&e.block_type, &s.target.port);
            if original.is_none() && !addable && !omitted {
                return Err(Error::Compile(format!(
                    "`{}.{}`: the object ({owner}) has no `{}` setting. Lox Logic changes \
                     this setting but never adds it.",
                    s.target.slug, s.target.port, s.target.port
                )));
            }
            write_setting(doc, owner, name, Some(&value));
            lock.set_originals
                .entry(crate::settings::key(owner, name))
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
                Some(e)
                    if crate::settings::DEVICE_TYPES
                        .iter()
                        .chain(crate::settings::SYSTEM_TYPES)
                        .any(|t| *t == e.block_type) =>
                {
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
                            .chain(crate::settings::SYSTEM_TYPES)
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

/// The attributes a project or Miniserver setting writes, `None` for one
/// a save leaves out.
fn system_writes(
    module: &Module,
    s: &crate::logic::ast::SetDecl,
    sys: crate::settings::SystemSetting,
) -> Result<Vec<(&'static str, Option<String>)>> {
    use crate::settings::SystemKind;
    let at = || format!("`{}.{}`", s.target.slug, s.target.port);
    Ok(match sys.kind {
        SystemKind::Text | SystemKind::AddableText => {
            vec![(sys.attr, Some(module.resolve_value(&s.value)?.into_owned()))]
        }
        SystemKind::Switch { on, off } => {
            let value = match &s.value {
                Value::Bool(b) => *b,
                _ => {
                    return Err(Error::Compile(format!("{} takes on or off", at())));
                }
            };
            vec![(sys.attr, if value { on } else { off }.map(str::to_string))]
        }
        SystemKind::MinuteOfDay => {
            let minutes = match &s.value {
                Value::Clock(t) => {
                    let mut parts = t.split(':').map(|p| p.parse::<u32>().unwrap_or(0));
                    let (h, m, sec) = (
                        parts.next().unwrap_or(0),
                        parts.next().unwrap_or(0),
                        parts.next().unwrap_or(0),
                    );
                    if sec != 0 {
                        return Err(Error::Compile(format!(
                            "{} takes whole minutes (`{h:02}:{m:02}`)",
                            at()
                        )));
                    }
                    h * 60 + m
                }
                _ => {
                    return Err(Error::Compile(format!(
                        "{} takes a time of day, like `02:00`",
                        at()
                    )));
                }
            };
            vec![(sys.attr, Some(minutes.to_string()))]
        }
        SystemKind::Updates => {
            let word = module.resolve_value(&s.value)?;
            let modes = crate::settings::UPDATE_MODES;
            let Some((_, aue, aum)) = modes.iter().find(|(w, _, _)| *w == word) else {
                let words: Vec<(&str, &str)> = modes.iter().map(|(w, _, _)| (*w, "")).collect();
                return Err(Error::Compile(format!(
                    "{}: `{word}` is not one of {}",
                    at(),
                    crate::connectors::choice_list(&words)
                )));
            };
            vec![
                ("AUE", aue.map(str::to_string)),
                (sys.attr, Some(aum.to_string())),
            ]
        }
    })
}

/// A Def write that vanished from source was withdrawn — tombstone it
/// unless it never changed the port's bytes in the first place.
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

/// Attribute claims on externs, and a managed block's room and
/// category. Planned against the torn-down document before anything
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
    // A managed block's room and category: written onto the rebuilt
    // block on every compile, no lock state — the block is the source's.
    let mut managed_claims: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
    for a in module.attrs().filter(|a| managed.contains_key(&a.slug)) {
        let entry = managed_claims
            .entry(managed[&a.slug].uuid.clone())
            .or_default();
        match a.attr {
            Attr::Room => entry.0 = Some(taxonomy_target(doc, cx, Attr::Room, &a.value)?),
            Attr::Category => {
                entry.1 = Some(taxonomy_target(doc, cx, Attr::Category, &a.value)?);
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
                 object ({uuid}). Keep one assignment.",
                a.slug,
                attr = a.attr
            )));
        }
    }
    let mut planned_claims: Vec<(String, Attr, AttrClaim)> = Vec::new();
    for a in module.attrs().filter(|a| !managed.contains_key(&a.slug)) {
        let uuid = extern_uuid[&a.slug].clone();
        let written = match a.attr {
            Attr::Title => resolve(&a.value)?,
            Attr::Room | Attr::Category => taxonomy_target(doc, cx, a.attr, &a.value)?,
        };
        let original = read_claimed(doc, &uuid, a.attr).ok_or_else(|| {
            Error::Compile(format!(
                "`{}.{}`: the object ({uuid}) has no {}. Set it once in \
                 Loxone Config, since Lox Logic changes it but never adds it.",
                a.slug,
                a.attr,
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

/// A claim that vanished from source was withdrawn — tombstone it
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
