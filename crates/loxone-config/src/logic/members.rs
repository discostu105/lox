//! Central block members from source.
//!
//! A `<central>.members = (a, b, …)` claim makes a CentralLight's or a
//! CentralShade's `rec=` exactly those blocks' UUIDs, in the order
//! written. A save keeps the list as written (oracle session 47): its
//! order, and a member of another type or one listed twice too, so the
//! compile checks each member's type itself. An empty list leaves the
//! attribute out, as a save does; a new one goes right before
//! `SpStates=`, where a save puts it. No lock state: withdrawing the
//! claim leaves the last written list in place.

use super::ast::Module;
use crate::connectors::central_member_type;
use crate::doc::LoxoneDoc;
use crate::error::{Error, Result};
use std::collections::BTreeMap;

/// Write every claimed member list onto its central block in the compiled
/// doc. `uuid_of` maps extern and managed slugs to their object UUIDs.
pub(crate) fn apply(
    doc: &mut LoxoneDoc,
    module: &Module,
    uuid_of: &BTreeMap<String, String>,
) -> Result<()> {
    if module.members_claims().next().is_none() {
        return Ok(());
    }
    let objects = doc.objects();
    let path_of: BTreeMap<&str, &Vec<usize>> =
        objects.iter().map(|o| (o.uuid.as_str(), &o.path)).collect();
    let type_of: BTreeMap<&str, &str> = objects
        .iter()
        .map(|o| (o.uuid.as_str(), o.block_type.as_str()))
        .collect();
    let mut claimed: BTreeMap<&str, &str> = BTreeMap::new();
    let mut writes: Vec<(&Vec<usize>, String)> = Vec::new();
    for c in module.members_claims() {
        let fail = |m: String| Error::Compile(format!("`{c}`: {m}"));
        let uuid = uuid_of[&c.slug].as_str();
        if let Some(other) = claimed.insert(uuid, &c.slug) {
            return Err(Error::Compile(format!(
                "`{c}` and `{other}.members = …` claim the members of the same block \
                 ({uuid}). Keep one."
            )));
        }
        let ty = type_of.get(uuid).copied().unwrap_or_default();
        let Some(want) = central_member_type(ty) else {
            return Err(fail(format!(
                "{uuid} is a {ty}, not a CentralLight or a CentralShade"
            )));
        };
        let mut list: Vec<&str> = Vec::with_capacity(c.members.len());
        for m in &c.members {
            let mu = uuid_of[m].as_str();
            match type_of.get(mu) {
                Some(t) if *t == want => {}
                Some(t) => {
                    return Err(fail(format!(
                        "`{m}` ({mu}) is a {t}; the members of a {ty} are {want} blocks"
                    )));
                }
                None => return Err(fail(format!("`{m}` ({mu}) is not in the config"))),
            }
            if list.contains(&mu) {
                return Err(fail(format!(
                    "`{m}` names {mu}, which the list already holds"
                )));
            }
            list.push(mu);
        }
        writes.push((path_of[uuid], list.join(",")));
    }
    for (path, rec) in writes {
        let el = doc
            .element_at_mut(path)
            .expect("object path from objects()");
        write(el, &rec);
    }
    Ok(())
}

/// Set `rec=` to `rec`: in place, else right before `SpStates=`; an empty
/// list removes it.
fn write(el: &mut crate::xml::Element, rec: &str) {
    if rec.is_empty() {
        el.remove_attr("rec");
        return;
    }
    if el.attr("rec").is_some() {
        el.set_attr("rec", rec);
        return;
    }
    let pos = el
        .attrs
        .iter()
        .position(|a| a.name == "SpStates")
        .unwrap_or(el.attrs.len());
    el.attrs.insert(
        pos,
        crate::xml::Attr {
            name: "rec".to_string(),
            value: crate::xml::escape(rec).into_owned(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::Element;

    fn el(attrs: &[(&str, &str)]) -> Element {
        let mut e = Element::new("C");
        for (n, v) in attrs {
            e.set_attr(n, v);
        }
        e
    }

    fn names(e: &Element) -> Vec<&str> {
        e.attrs.iter().map(|a| a.name.as_str()).collect()
    }

    #[test]
    fn a_new_list_goes_before_the_special_states() {
        let mut e = el(&[("Type", "CentralLight"), ("Nio", "24"), ("SpStates", "x")]);
        write(&mut e, "a,b");
        assert_eq!(names(&e), ["Type", "Nio", "rec", "SpStates"]);
        assert_eq!(e.attr("rec"), Some("a,b"));
    }

    #[test]
    fn a_list_keeps_its_place_and_an_empty_one_goes() {
        let mut e = el(&[("Type", "CentralShade"), ("rec", "a"), ("SpStates", "x")]);
        write(&mut e, "b,a");
        assert_eq!(names(&e), ["Type", "rec", "SpStates"]);
        assert_eq!(e.attr("rec"), Some("b,a"));
        write(&mut e, "");
        assert_eq!(names(&e), ["Type", "SpStates"]);
    }
}
