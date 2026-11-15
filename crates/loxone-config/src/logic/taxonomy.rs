//! Rooms and categories from source (#324).
//!
//! A room is a `Place` in the "Räume" folder, a category a `Category` in
//! "Kategorien"; objects name them with `<IoData Pr=… Cr=…>`. Declaring one
//! (`garage = Place("Garage")`) mints it like any object of the object
//! tree (see `periphery`). What a fresh one needs beyond its own element
//! is here: Loxone Config gives every room and category a permission
//! group (`RightGroup`, named by `RGR=`) and every room a node in the
//! folder's sort tree (`<nd>`), and keeps the group's title equal to the
//! object's (oracle session 95).

use crate::doc::LoxoneDoc;
use crate::error::{Error, Result};
use crate::lock::Lockfile;
use crate::xml::{Element, Node};
use std::collections::BTreeSet;

/// The block types of rooms and categories.
pub const TYPES: [&str; 2] = ["Place", "Category"];

/// The icon of a room "Create room" makes: the location pin.
pub const ROOM_ICON: &str = "0000015c-00ff-0000-0000000000000000";

/// The icon of a category "Create category" makes: the binders.
pub const CATEGORY_ICON: &str = "00000038-00ff-0000-0000000000000000";

/// The room types of `PType=`, as the room dialog lists them.
pub const ROOM_TYPES: [(u8, &str); 6] = [
    (1, "Bedroom"),
    (2, "Common Room"),
    (3, "Thoroughfare"),
    (4, "Central"),
    (5, "Other"),
    (6, "Outdoor area"),
];

/// How many companion objects a fresh object of this type gets: a room
/// its permission group and its sort node, a category its group.
pub fn companion_count(block_type: &str) -> usize {
    match block_type {
        "Place" => 2,
        "Category" => 1,
        _ => 0,
    }
}

/// `GT=` of a permission group: 1 a room's, 2 a category's.
fn group_type(block_type: &str) -> &'static str {
    if block_type == "Place" { "1" } else { "2" }
}

/// After the managed objects are rebuilt: give a room or category the
/// compiler created its permission group and sort node, keep a managed
/// one's group titled like it, and take the group and node of a removed
/// one with it. A removed room or category that objects are still
/// assigned to is an error.
pub(crate) fn apply(doc: &mut LoxoneDoc, base: &LoxoneDoc, lock: &Lockfile) -> Result<()> {
    let managed: std::collections::BTreeMap<&str, (&String, &crate::lock::LockedObject)> = lock
        .objects
        .iter()
        .filter(|(_, o)| TYPES.contains(&o.block_type.as_str()))
        .map(|(slug, o)| (o.uuid.as_str(), (slug, o)))
        .collect();
    let in_base: BTreeSet<String> = base.objects().into_iter().map(|o| o.uuid).collect();
    // In document order: groups and nodes are appended as Loxone Config
    // appends them, in the order the objects were made.
    let taxonomy: Vec<_> = doc
        .objects()
        .into_iter()
        .filter(|o| TYPES.contains(&o.block_type.as_str()))
        .collect();
    for obj in &taxonomy {
        let Some(&(slug, o)) = managed.get(obj.uuid.as_str()) else {
            continue;
        };
        // By UUID: a node inserted into the folder shifts the paths.
        let el = find(&doc.xml.root, &|e| is_c(e, &obj.uuid)).expect("listed object");
        // A new one must not repeat a title: rooms and categories are
        // named by title everywhere else.
        if !in_base.contains(&o.uuid)
            && let Some(twin) = taxonomy.iter().find(|t| {
                t.uuid != obj.uuid && t.block_type == obj.block_type && t.title == obj.title
            })
        {
            return Err(Error::Compile(format!(
                "`{slug}`: a {} titled {:?} already exists ({}). Name it with \
                 `extern {slug} = {}(uuid: \"{}\")`, or adopt it.",
                obj.block_type,
                obj.title.as_deref().unwrap_or_default(),
                twin.uuid,
                obj.block_type,
                twin.uuid
            )));
        }
        if o.block_type == "Place"
            && let Some(t) = el.attr("PType")
            && !ROOM_TYPES.iter().any(|(n, _)| n.to_string() == t)
        {
            return Err(Error::Compile(format!(
                "room `{slug}`: `PType: {t}` is no room type. Use {}.",
                ROOM_TYPES
                    .iter()
                    .map(|(n, name)| format!("{n} ({name})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        let title = el.attr("Title").unwrap_or_default().to_string();
        let group = el.attr("RGR").map(str::to_string);
        if let Some(el) = find_mut(&mut doc.xml.root, &|e| is_c(e, &o.uuid)) {
            order_settings(el);
        }
        // The version it was saved at, a fresh one its folder's: Loxone
        // Config writes the folders' objects at its own version (17.3
        // saves them at 179 while old blocks stay at 175), and a save
        // keeps an existing object's.
        let version = match find(&base.xml.root, &|e| is_c(e, &o.uuid)) {
            Some(b) => b.attr("V").map(str::to_string),
            None => {
                let folder = crate::periphery::folder(&o.block_type).expect("a folder type");
                find(&doc.xml.root, &|e| {
                    e.name == "C" && e.attr("Type") == Some(folder)
                })
                .and_then(|f| f.attr("V"))
                .map(str::to_string)
            }
        };
        if let Some(v) = version
            && let Some(el) = find_mut(&mut doc.xml.root, &|e| is_c(e, &o.uuid))
        {
            el.set_attr_raw("V", &v);
        }
        let Some(group) = group else { continue };
        if o.companions.first() == Some(&group)
            && find(&doc.xml.root, &|e| is_c(e, &group)).is_none()
        {
            let version = find(&doc.xml.root, &groups_folder)
                .and_then(|f| f.attr("V"))
                .unwrap_or_default()
                .to_string();
            let mut g = Element::new("C");
            for (k, v) in [
                ("Type", "RightGroup"),
                ("V", version.as_str()),
                ("U", group.as_str()),
            ] {
                g.set_attr(k, v);
            }
            g.set_attr_raw("Title", &title);
            for (k, v) in [
                ("Cl", "0,0,0"),
                ("WF", "16384"),
                ("GT", group_type(&o.block_type)),
                ("MG", ""),
            ] {
                g.set_attr(k, v);
            }
            let groups = find_mut(&mut doc.xml.root, &groups_folder).ok_or_else(|| {
                Error::Compile(format!(
                    "`{slug}`: the base config has no permission groups folder \
                     (\"Berechtigungsgruppen\") for its group"
                ))
            })?;
            groups.push_child(g);
        }
        if let Some(node) = o.companions.get(1) {
            let folder = find_mut(&mut doc.xml.root, &|e| {
                e.name == "C" && e.attr("Type") == Some("PlaceCaption")
            })
            .expect("the room was put into this folder");
            let has_node = folder
                .child_elements()
                .any(|c| c.name == "nd" && c.attr("ctrlUuid") == Some(o.uuid.as_str()));
            if !has_node {
                let mut nd = Element::new("nd");
                for (k, v) in [
                    ("u", node.as_str()),
                    ("t", "1"),
                    ("ico", ROOM_ICON),
                    ("ctrlUuid", o.uuid.as_str()),
                ] {
                    nd.set_attr(k, v);
                }
                // After the folder's other nodes, before its rooms.
                let at = folder
                    .children
                    .iter()
                    .rposition(|n| matches!(n, Node::Element(e) if e.name == "nd"))
                    .map_or(0, |i| i + 1);
                folder.self_closing = false;
                folder.children.insert(at, Node::Element(nd));
            }
        }
    }
    rename_groups(doc, base);
    remove_gone(doc, base)
}

/// Loxone Config renames a permission group with its room or category:
/// so does a compile that renamed one (a label, a `.title`).
fn rename_groups(doc: &mut LoxoneDoc, base: &LoxoneDoc) {
    let was: std::collections::BTreeMap<String, Option<String>> = base
        .objects()
        .into_iter()
        .filter(|o| TYPES.contains(&o.block_type.as_str()))
        .map(|o| (o.uuid, o.title))
        .collect();
    let renamed: Vec<(String, String)> = doc
        .objects()
        .into_iter()
        .filter(|o| TYPES.contains(&o.block_type.as_str()))
        .filter(|o| was.get(&o.uuid).is_some_and(|t| *t != o.title))
        .filter_map(|o| {
            let el = doc.element_at(&o.path)?;
            Some((el.attr("RGR")?.to_string(), el.attr("Title")?.to_string()))
        })
        .collect();
    for (group, title) in renamed {
        if let Some(g) = find_mut(&mut doc.xml.root, &|e| is_c(e, &group)) {
            g.set_attr_raw("Title", &title);
        }
    }
}

/// A room or category the base has and the compiled document has not was
/// removed from source: its group and node go with it.
fn remove_gone(doc: &mut LoxoneDoc, base: &LoxoneDoc) -> Result<()> {
    let now: BTreeSet<String> = doc
        .objects()
        .into_iter()
        .filter(|o| TYPES.contains(&o.block_type.as_str()))
        .map(|o| o.uuid)
        .collect();
    for o in base.objects() {
        if !TYPES.contains(&o.block_type.as_str()) || now.contains(&o.uuid) {
            continue;
        }
        let el = base.element_at(&o.path).expect("path from objects()");
        if let Some(group) = el.attr("RGR") {
            remove_where(&mut doc.xml.root, &|e| is_c(e, group));
        }
        remove_where(&mut doc.xml.root, &|e| {
            e.name == "nd" && e.attr("ctrlUuid") == Some(o.uuid.as_str())
        });
        let users = count_refs(&doc.xml.root, &o.uuid);
        if users > 0 {
            let what = if o.block_type == "Place" {
                "room"
            } else {
                "category"
            };
            return Err(Error::Compile(format!(
                "the {what} {:?} ({}) is removed, but {users} object(s) still name it. \
                 Assign them another {what} first.",
                o.title.as_deref().unwrap_or_default(),
                o.uuid
            )));
        }
    }
    Ok(())
}

/// A setting the source added to a room or category goes where Loxone
/// Config writes it: `Rating=` and `UseFav=` after the icon, `PType=`
/// before the colour, the area and the group (oracle session 95).
fn order_settings(el: &mut Element) {
    let mut place = |name: &str, after: &[&str], before: &[&str]| {
        let Some(at) = el.attrs.iter().position(|a| a.name == name) else {
            return;
        };
        let attr = el.attrs.remove(at);
        let pos = el
            .attrs
            .iter()
            .rposition(|a| after.contains(&a.name.as_str()))
            .map(|i| i + 1)
            .or_else(|| {
                el.attrs
                    .iter()
                    .position(|a| before.contains(&a.name.as_str()))
            })
            .unwrap_or(el.attrs.len());
        el.attrs.insert(pos, attr);
    };
    place(
        "Rating",
        &["Icon", "First"],
        &["UseFav", "CatGroup", "PGroup", "PType"],
    );
    place(
        "UseFav",
        &["Icon", "First", "Rating"],
        &["CatGroup", "PGroup", "PType"],
    );
    place(
        "PType",
        &["Icon", "First", "Rating", "UseFav", "CatGroup", "PGroup"],
        &["BkColor", "Sqm", "RGR"],
    );
}

/// The "Berechtigungsgruppen" folder.
fn groups_folder(e: &Element) -> bool {
    e.name == "C" && e.attr("Type") == Some("LoxCaption") && e.attr("CaptionType") == Some("13")
}

fn is_c(e: &Element, uuid: &str) -> bool {
    e.name == "C" && e.attr("U") == Some(uuid)
}

fn find<'a>(el: &'a Element, pred: &dyn Fn(&Element) -> bool) -> Option<&'a Element> {
    if pred(el) {
        return Some(el);
    }
    el.child_elements().find_map(|c| find(c, pred))
}

fn find_mut<'a>(el: &'a mut Element, pred: &dyn Fn(&Element) -> bool) -> Option<&'a mut Element> {
    if pred(el) {
        return Some(el);
    }
    el.child_elements_mut().find_map(|c| find_mut(c, pred))
}

fn remove_where(el: &mut Element, pred: &dyn Fn(&Element) -> bool) {
    el.children
        .retain(|n| !matches!(n, Node::Element(e) if pred(e)));
    for c in el.child_elements_mut() {
        remove_where(c, pred);
    }
}

/// How many elements name `uuid` in an attribute (`Pr=`, `Cr=`, …).
fn count_refs(el: &Element, uuid: &str) -> usize {
    let own = usize::from(el.attrs.iter().any(|a| a.value == uuid));
    own + el
        .child_elements()
        .map(|c| count_refs(c, uuid))
        .sum::<usize>()
}
