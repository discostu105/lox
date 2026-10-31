//! Teardown: what the base carried, then the removal of the previous
//! compile's output — managed objects, extern wires, extern sets, attribute
//! claims. What remains is exactly the Loxone-Config-owned state.

use super::*;

/// Reads what the rebuild needs from the base before teardown changes it.
pub(super) fn snapshot_base(doc: &LoxoneDoc, lock: &Lockfile) -> BaseSnapshot {
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
    for w in all_base_wires.clone() {
        base_in_order
            .entry(w.to_port)
            .or_default()
            .push(w.from_port);
    }
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
            .filter_map(|o| Some((o.uuid.clone(), page_of(doc, idx.by_uuid.get(&o.uuid)?)?)))
            .collect()
    };
    // The element each managed object of the object tree sits in (D64): an
    // I/O goes back into the caption of its device it was in.
    // Parent and grandparent, and each such parent's objects in base order.
    let mut base_parent: BTreeMap<String, (String, Option<String>)> = BTreeMap::new();
    let mut base_tree_order: BTreeMap<String, Vec<String>> = BTreeMap::new();
    {
        let idx = doc.index();
        for o in lock.objects.values().filter(|o| object_tree(&o.block_type)) {
            let Some(path) = idx.by_uuid.get(&o.uuid) else {
                continue;
            };
            let Some(parent) = doc.element_at(&path[..path.len() - 1]) else {
                continue;
            };
            let Some(pu) = parent.attr("U") else { continue };
            let grand = doc
                .element_at(&path[..path.len() - 2])
                .and_then(|g| g.attr("U"))
                .map(str::to_string);
            base_parent.insert(o.uuid.clone(), (pu.to_string(), grand));
            base_tree_order.entry(pu.to_string()).or_insert_with(|| {
                parent
                    .child_elements()
                    .filter(|c| c.name == "C")
                    .filter_map(|c| c.attr("U").map(str::to_string))
                    .collect()
            });
        }
    }
    BaseSnapshot {
        type_by_uuid,
        all_base_wires,
        base_wires,
        base_in_order,
        base_page_order,
        base_page,
        base_parent,
        base_tree_order,
    }
}

/// Removes the managed objects, innermost first. Each removed element
/// hands back its GUI-owned residue (D19) so the rebuild carries it
/// forward verbatim. Returns the residue by UUID and the tombstoned objects
/// that went along inside a torn-down device.
pub(super) fn tear_down_managed(
    doc: &mut LoxoneDoc,
    lock: &Lockfile,
    base_page: &BTreeMap<String, String>,
) -> (BTreeMap<String, Residue>, BTreeSet<String>) {
    let mut residue: BTreeMap<String, Residue> = BTreeMap::new();
    // Innermost first: a managed I/O leaves its managed device before the
    // device goes, so neither carries the other.
    let mut teardown: Vec<&LockedObject> = lock.objects.values().collect();
    {
        let idx = doc.index();
        let depth = |o: &LockedObject| idx.by_uuid.get(&o.uuid).map_or(0, Vec::len);
        teardown.sort_by_key(|o| std::cmp::Reverse(depth(o)));
    }
    // Tombstoned objects that went with a torn-down device: still in
    // this base, so their tombstones stay.
    let mut in_torn_down: BTreeSet<String> = BTreeSet::new();
    for obj in teardown {
        if let Some(el) = doc.remove_by_uuid(&obj.uuid) {
            let mut r = harvest_residue(&el);
            // A device carries the I/O the GUI added to it, never one
            // the source withdrew (its tombstone, D31) — also inside the
            // captions a device groups its I/O in.
            fn withdrawn(
                c: &Element,
                removed: &BTreeMap<String, crate::lock::Tombstone>,
                found: &mut BTreeSet<String>,
            ) -> bool {
                let u = c
                    .attr("U")
                    .filter(|u| c.name == "C" && removed.contains_key(*u));
                if let Some(u) = u {
                    found.insert(u.to_string());
                }
                u.is_some()
            }
            fn drop_withdrawn(
                el: &mut Element,
                removed: &BTreeMap<String, crate::lock::Tombstone>,
                found: &mut BTreeSet<String>,
            ) {
                el.children.retain(|n| match n {
                    crate::xml::Node::Element(c) => !withdrawn(c, removed, found),
                    _ => true,
                });
                for n in el.children.iter_mut() {
                    if let crate::xml::Node::Element(c) = n {
                        drop_withdrawn(c, removed, found);
                    }
                }
            }
            r.children
                .retain(|c| !withdrawn(c, &lock.removed, &mut in_torn_down));
            for c in r.children.iter_mut() {
                drop_withdrawn(c, &lock.removed, &mut in_torn_down);
            }
            r.page = base_page.get(&obj.uuid).cloned();
            residue.insert(obj.uuid.clone(), r);
        }
    }
    (residue, in_torn_down)
}

/// The tombstone sweeps (D31) and the retirement of vanished slugs: pending
/// removals of objects, wires and sets the base still carries are deleted
/// again, the others are done.
pub(super) fn sweep_tombstones(
    doc: &mut LoxoneDoc,
    lock: &mut Lockfile,
    src_slugs: &BTreeSet<String>,
    residue: &BTreeMap<String, Residue>,
    in_torn_down: BTreeSet<String>,
) {
    // Tombstone sweep (D31): a pending removal whose object still exists
    // in this base is deleted again (the base predates the deployment of
    // the removal); one the base no longer contains has reached the
    // Miniserver — its tombstone is done.
    // A withdrawn device takes its withdrawn I/O along (D64).
    let pending: Vec<String> = lock.removed.keys().cloned().collect();
    let mut present: BTreeSet<String> = in_torn_down;
    for uuid in &pending {
        if let Some(el) = doc.remove_by_uuid(uuid) {
            present.insert(uuid.clone());
            let mut stack: Vec<&Element> = el.child_elements().collect();
            while let Some(c) = stack.pop() {
                if let Some(u) = c.attr("U").filter(|_| c.name == "C") {
                    present.insert(u.to_string());
                }
                stack.extend(c.child_elements());
            }
        }
    }
    lock.removed.retain(|uuid, _| present.contains(uuid));
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
        .retain(|w| remove_wire(doc, &w.from, &w.to));
    // Withdrawn-set sweep (D31): a port still carrying the value we wrote
    // predates the deployment — restore the original again. A port already
    // showing the original is deployed; any third value means another
    // writer took the port over (drift will surface it). Both retire.
    lock.removed_sets.retain(|port_uuid, rs| {
        let def = current_def(doc, port_uuid);
        if def == rs.original {
            return false;
        }
        if def.as_deref() == Some(rs.written.as_str()) {
            restore_def(doc, port_uuid, rs.original.as_deref());
            return true;
        }
        false
    });
}

/// Takes back the previous compile's extern wires, `Def=` writes and
/// attribute claims, remembering what the base carried of them.
pub(super) fn tear_down_extern_edits(doc: &mut LoxoneDoc, lock: &mut Lockfile) -> PriorEdits {
    let old_wires = std::mem::take(&mut lock.extern_wires);
    // Remember which of our wires this base actually carried — only those
    // can leave a tombstone if they vanish from source (the rebuild below
    // re-records the ones that stay).
    let mut wires_present: BTreeSet<LockedWire> = BTreeSet::new();
    // The GUI's flag on a wire (`FLG=`) is its own; one the base carried
    // on a wire of ours goes back on with the rebuilt wire.
    let mut extern_wire_flags: BTreeMap<(String, String), String> = BTreeMap::new();
    for w in &old_wires {
        if let Some(flg) = wire_flag(doc, &w.from, &w.to) {
            extern_wire_flags.insert((w.to.clone(), w.from.clone()), flg);
        }
        if remove_wire(doc, &w.from, &w.to) {
            wires_present.insert(w.clone());
        }
    }
    let old_originals = std::mem::take(&mut lock.set_originals);
    // Same for Def writes: capture what each port carried before the
    // restore — the "written" marker of a potential tombstone.
    let mut defs_written: BTreeMap<String, String> = BTreeMap::new();
    for (port_uuid, original) in &old_originals {
        if let Some(v) = current_def(doc, port_uuid) {
            defs_written.insert(port_uuid.clone(), v);
        }
        restore_def(doc, port_uuid, original.as_deref());
    }
    // Attribute claims (D39), the same two steps. Withdrawn first: an
    // object still showing the value we wrote predates the deployment —
    // restore again and keep the tombstone; any other value retires it.
    for (uuid, claims) in lock.removed_attrs.iter_mut() {
        claims.retain(|attr, c| {
            if read_claimed(doc, uuid, *attr).as_deref() == Some(c.written.as_str()) {
                restore_claim(doc, uuid, *attr, c);
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
            if let Some(v) = read_claimed(doc, uuid, *attr) {
                claims_written.insert((uuid.clone(), *attr), v);
            }
            restore_claim(doc, uuid, *attr, c);
        }
    }
    PriorEdits {
        old_wires,
        wires_present,
        extern_wire_flags,
        old_originals,
        defs_written,
        old_claims,
        claims_written,
    }
}
