//! Tree devices the source creates (`kind:`, `tree:`): each is added to
//! its Tree extension's branch once, unpaired, and the lockfile keeps its
//! UUID. From then on the declaration matches it like any other device.
//! Hardware leaves the config only by `removed <slug>`.

use super::*;
use crate::hardware::is_device;
use crate::lock::Tombstone;

/// The declarations that create a device: slug, kind, tree, branch.
fn creating(module: &Module) -> Vec<(&str, &str, &str, Option<&str>)> {
    module
        .externs()
        .filter_map(|e| match &e.match_spec {
            MatchSpec::Create { kind, tree, branch } => Some((
                e.slug.as_str(),
                kind.as_str(),
                tree.as_str(),
                branch.as_deref(),
            )),
            _ => None,
        })
        .collect()
}

/// Created devices the source no longer creates, before the removals
/// are checked: one taken over by `serial:` or `uuid:` stays as it is,
/// one under `removed <slug>` becomes a tombstone, any other is refused.
pub(super) fn retire_created(base: &LoxoneDoc, module: &Module, lock: &mut Lockfile) -> Result<()> {
    let creating: BTreeSet<&str> = creating(module).into_iter().map(|c| c.0).collect();
    let removed: BTreeSet<&str> = module.removed().map(|r| r.slug.as_str()).collect();
    for (slug, uuid) in lock.hardware.clone() {
        if creating.contains(slug.as_str()) {
            continue;
        }
        if module.externs().any(|e| e.slug == slug) {
            lock.hardware.remove(&slug);
            continue;
        }
        if !removed.contains(slug.as_str()) {
            return Err(Error::Compile(format!(
                "`{slug}` created the Tree device {uuid}, and the source no longer declares it. \
                 To delete the device, add `removed {slug}`; to keep it, declare it by `uuid: \
                 \"{uuid}\"` (or by `serial:` once it is paired)."
            )));
        }
        // Its inputs must not feed a wire Loxone Config drew: the
        // source's wires leave with the declarations.
        if let Some(target) = gui_wired(base, lock, &uuid) {
            return Err(Error::Compile(format!(
                "`removed {slug}`: the Tree device {uuid} drives {target}. Remove that wire \
                 in Loxone Config first."
            )));
        }
        lock.hardware.remove(&slug);
        lock.removed.insert(
            uuid,
            Tombstone {
                slug,
                block_type: "TreeDevice".to_string(),
            },
        );
    }
    Ok(())
}

/// The object a wire Loxone Config drew from the device `uuid` (or an
/// I/O below it) feeds.
fn gui_wired(base: &LoxoneDoc, lock: &Lockfile, uuid: &str) -> Option<String> {
    let mut outputs: BTreeSet<&str> = BTreeSet::new();
    let mut stack = vec![find_c(&base.xml.root, uuid)?];
    while let Some(e) = stack.pop() {
        if e.name == "Co" {
            outputs.extend(e.attr("U"));
        }
        stack.extend(e.child_elements());
    }
    let managed: BTreeSet<&str> = lock.objects.values().map(|o| o.uuid.as_str()).collect();
    base.objects().into_iter().find_map(|o| {
        if managed.contains(o.uuid.as_str()) {
            return None;
        }
        let el = base.element_at(&o.path)?;
        let fed = el.child_elements().filter(|c| c.name == "Co").any(|co| {
            co.child_elements().any(|i| {
                i.name == "In"
                    && i.attr("Input").is_some_and(|from| {
                        outputs.contains(from)
                            && !lock
                                .extern_wires
                                .iter()
                                .any(|w| w.from == from && Some(w.to.as_str()) == co.attr("U"))
                    })
            })
        });
        fed.then(|| o.title.clone().unwrap_or(o.uuid))
    })
}

/// Add each device the source creates and the config lacks to its Tree
/// extension, and record its UUID.
pub(super) fn create_devices(
    doc: &mut LoxoneDoc,
    module: &Module,
    lock: &mut Lockfile,
    opts: &CompileOptions,
) -> Result<()> {
    let fail = |m: String| Err(Error::Compile(m));
    let mut minter: Option<Minter> = None;
    for (slug, kind, tree, branch) in creating(module) {
        let k = crate::tree_kinds::kind(kind).expect("lowering checked the kind");
        let locked = lock.hardware.get(slug).cloned();
        if let Some(el) = locked.as_deref().and_then(|u| find_c(&doc.xml.root, u)) {
            match el.attr("TId") {
                Some(t) if t == k.name => continue,
                t => {
                    return fail(format!(
                        "`{slug}` is a {} in the config, not a {kind}. A device keeps its \
                         kind: add `removed {slug}` and declare the new one under another name.",
                        t.unwrap_or("Tree device")
                    ));
                }
            }
        }
        let Some(ext) = module
            .externs()
            .find(|e| e.slug == tree && e.block_type == "LoxTree")
        else {
            return fail(format!(
                "`{slug}`: `tree: {tree}` names no Tree extension. Declare it, e.g. `{tree} = \
                 LoxTree(\"…\", serial: \"…\")`."
            ));
        };
        let one = Module {
            items: vec![Item::Extern(ext.clone())],
        };
        let tree_uuid = resolve_externs(doc, &one, &mut lock.clone())?.remove(tree);
        let tree_uuid = tree_uuid.expect("resolved");
        let tree_el = find_c_mut(&mut doc.xml.root, &tree_uuid).expect("resolved");
        let version = tree_el.attr("V").unwrap_or_default().to_string();
        // A branch is a `LoxCaption`: `CaptionType="2"` the left one, `3`
        // the right one (or the only one).
        let want = branch.map(|b| if b == "left" { "2" } else { "3" });
        let captions: Vec<usize> = tree_el
            .children
            .iter()
            .enumerate()
            .filter_map(|(i, n)| match n {
                crate::xml::Node::Element(c)
                    if c.attr("Type") == Some("LoxCaption")
                        && want.is_none_or(|w| c.attr("CaptionType") == Some(w)) =>
                {
                    Some(i)
                }
                _ => None,
            })
            .collect();
        let at = match (captions.as_slice(), branch) {
            ([only], _) => *only,
            ([], Some(b)) => return fail(format!("`{slug}`: `{tree}` has no {b} branch.")),
            ([], None) => return fail(format!("`{slug}`: `{tree}` has no branch to add to.")),
            (_, _) => {
                return fail(format!(
                    "`{slug}`: `{tree}` has two branches. Say which with `branch: \"left\"` or \
                     `branch: \"right\"`."
                ));
            }
        };
        let title = module
            .owned()
            .find(|o| o.slug == slug)
            .and_then(|o| o.title.clone())
            .unwrap_or_else(|| k.title.to_string());
        let m = minter.get_or_insert_with(|| seeded_minter(lock, opts));
        let mut el = k.element(&title, &version, m, crate::uuid::entity_for_slug(slug));
        // A base without the device (an older one, or the device deleted
        // in Loxone Config) gets it back under the UUID it had.
        if let Some(u) = &locked {
            el.set_attr("U", u);
        }
        let uuid = el.attr("U").expect("templates carry U").to_string();
        let crate::xml::Node::Element(caption) = &mut tree_el.children[at] else {
            unreachable!()
        };
        caption.push_child(el);
        lock.hardware.insert(slug.to_string(), uuid);
    }
    if let Some(m) = minter {
        lock.counters.next_mint = m.counter();
    }
    Ok(())
}

/// Hardware leaves the config only by `removed`: a device of the base
/// that the output lacks must have a tombstone.
pub(super) fn check_hardware_kept(
    base: &LoxoneDoc,
    doc: &LoxoneDoc,
    lock: &Lockfile,
) -> Result<()> {
    let out: BTreeSet<String> = doc.objects().into_iter().map(|o| o.uuid).collect();
    for o in base.objects() {
        if is_device(&o.block_type) && !out.contains(&o.uuid) && !lock.removed.contains_key(&o.uuid)
        {
            return Err(Error::Compile(format!(
                "the compile would delete the {} \"{}\" ({}). Hardware is deleted only by a \
                 `removed` statement for a device the source created.",
                o.block_type,
                o.title.as_deref().unwrap_or(""),
                o.uuid
            )));
        }
    }
    Ok(())
}

/// The devices the source created that Loxone Config has not paired yet
/// (`Serial` all zeros), by slug.
pub fn unpaired_created(doc: &LoxoneDoc, lock: &Lockfile) -> Vec<String> {
    lock.hardware
        .iter()
        .filter(|(_, u)| {
            find_c(&doc.xml.root, u)
                .and_then(|el| el.attr("Serial"))
                .is_some_and(|s| s.bytes().all(|b| b == b'0'))
        })
        .map(|(s, _)| s.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_leaves_only_by_a_tombstone() {
        let doc = |device: &str| {
            LoxoneDoc::parse(
                format!(
                    "<ControlList Version=\"1\"><C Type=\"Document\" U=\"d\">\
                     <C Type=\"LoxTree\" U=\"t\" Title=\"Tree\">{device}</C></C></ControlList>"
                )
                .as_bytes(),
            )
            .unwrap()
        };
        let base = doc("<C Type=\"TreeDevice\" U=\"x\" Title=\"Touch\"/>");
        let out = doc("");
        let mut lock = Lockfile::default();
        let err = check_hardware_kept(&base, &out, &lock)
            .unwrap_err()
            .to_string();
        assert!(err.contains("TreeDevice \"Touch\" (x)"), "{err}");
        lock.removed.insert(
            "x".to_string(),
            Tombstone {
                slug: "touch".to_string(),
                block_type: "TreeDevice".to_string(),
            },
        );
        check_hardware_kept(&base, &out, &lock).unwrap();
    }
}
