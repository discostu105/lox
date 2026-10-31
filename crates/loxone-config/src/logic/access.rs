//! Access rights on NFC Code Touch blocks (D49) and the recipients of push
//! notifications (D51).
//!
//! A user or group may open a Code Touch when its `<PH>` list has a
//! `<PRM REF="<block>" TIM="<schedule>"/>`: the right is on the block,
//! `TIM=` names the access schedule (a `SwitchingTimer`) that says when.
//! The same `<PRM>` may carry `ACT=`, the right to operate the block in the
//! app; that one is not an access right and stays as it is.
//!
//! An `access <slug>` declaration owns every `TIM=` on its block, across
//! all users and groups: compiling it grants what it lists, changes
//! schedules, and revokes what it does not list. Blocks without a
//! declaration are not touched. Users, groups and schedules stay with
//! Loxone Config and the app; the declaration names them by title, and a
//! title that is missing or not unique is an error. Like tiles (D41) the
//! declaration keeps no lock state: withdrawing it leaves the rights as
//! they are.
//!
//! A `recipients <slug>` declaration does the same for `ACT=` on a push
//! notification (`Notification`): a user or group with `ACT=` on it gets
//! the message. A new recipient gets `ACT="3"`, the value Loxone Config 17.2
//! writes (oracle session 15); an `ACT=` already there counts whatever its
//! value (older configs have `ACT="1"`) and stays.
//!
//! The edit is the one Loxone Config makes (docs/loxone-format.md, *Users,
//! groups and access rights*): a new right is a `<PRM REF TIM/>` appended
//! to the list, `NUM=` counts the list, and nothing else of the user or
//! group changes — not even `LastEdit=`. A declaration that matches the
//! config writes nothing, so compiling it reproduces the bytes.

use super::ast::{ACCESS_BLOCK_TYPE, AccessDecl, Module, PrincipalKind, RightKind};
use super::lint::{LintFinding, LintKind};
use crate::access::AccessModel;
use crate::doc::LoxoneDoc;
use crate::error::{Error, Result};
use crate::xml::{Attr, Element, Node};
use std::collections::{BTreeMap, BTreeSet};

/// The rights one declaration wants on its block: principal UUID → the
/// value of `attr` (the schedule UUID for `TIM=`, `3` for `ACT=`).
struct Plan {
    block: String,
    attr: &'static str,
    want: BTreeMap<String, String>,
}

/// The `ACT=` Loxone Config 17.2 writes for a new recipient.
const RECIPIENT_ACT: &str = "3";

pub(crate) fn apply(
    doc: &mut LoxoneDoc,
    module: &Module,
    uuid_of: &BTreeMap<String, String>,
) -> Result<()> {
    if module.access_decls().next().is_none() {
        return Ok(());
    }
    let model = AccessModel::read(doc);
    let types: BTreeMap<String, String> = doc
        .objects()
        .into_iter()
        .map(|o| (o.uuid, o.block_type))
        .collect();

    // Resolve everything first: a title that does not resolve writes nothing.
    let mut plans = Vec::new();
    let mut claimed: BTreeMap<&str, &str> = BTreeMap::new();
    for a in module.access_decls() {
        let kw = a.right.keyword();
        let block = uuid_of[&a.slug].as_str();
        if let Some(other) = claimed.insert(block, &a.slug) {
            return Err(Error::Compile(format!(
                "`{kw} {}` and `{kw} {other}` are the same block ({block}) — keep one",
                a.slug
            )));
        }
        let ty = types.get(block).map_or("", String::as_str);
        if ty != a.right.block_type() {
            return Err(Error::Compile(format!(
                "`{kw} {}`: the block ({block}) is a `{ty}`, not {} (`{}`)",
                a.slug,
                a.right.block_noun(),
                a.right.block_type()
            )));
        }
        plans.push(resolve(a, block, &model)?);
    }

    let principals: Vec<(String, Vec<usize>)> = model
        .users
        .iter()
        .map(|u| (u.uuid.clone(), u.path.clone()))
        .chain(
            model
                .groups
                .iter()
                .map(|g| (g.uuid.clone(), g.path.clone())),
        )
        .collect();
    for (uuid, path) in principals {
        let el = doc
            .element_at_mut(&path)
            .expect("path from AccessModel::read");
        rewrite(el, &uuid, &plans)?;
    }
    Ok(())
}

fn resolve(a: &AccessDecl, block: &str, model: &AccessModel) -> Result<Plan> {
    let ctx = format!("`{} {}`", a.right.keyword(), a.slug);
    let mut want = BTreeMap::new();
    for g in a.grants() {
        let (what, candidates): (&str, Vec<(&str, Option<&str>)>) = match g.kind {
            PrincipalKind::User => (
                "user",
                model
                    .users
                    .iter()
                    .map(|u| (u.uuid.as_str(), u.title.as_deref()))
                    .collect(),
            ),
            PrincipalKind::Group => (
                "group",
                model
                    .groups
                    .iter()
                    .map(|g| (g.uuid.as_str(), g.title.as_deref()))
                    .collect(),
            ),
        };
        let principal = by_title(&ctx, what, &g.principal, &candidates)?;
        let value = match (&a.right, &g.schedule) {
            (RightKind::Access, Some(schedule)) => {
                let schedules: Vec<(&str, Option<&str>)> = model
                    .schedules
                    .iter()
                    .map(|s| (s.uuid.as_str(), s.title.as_deref()))
                    .collect();
                by_title(&ctx, "access schedule", schedule, &schedules)?
            }
            (RightKind::Recipients, None) => RECIPIENT_ACT,
            _ => unreachable!("Module::validate checks schedules against the right"),
        };
        want.insert(principal.to_string(), value.to_string());
    }
    Ok(Plan {
        block: block.to_string(),
        attr: match a.right {
            RightKind::Access => "TIM",
            RightKind::Recipients => "ACT",
        },
        want,
    })
}

/// The one object titled `title` among `candidates` (UUID, title).
fn by_title<'a>(
    ctx: &str,
    what: &str,
    title: &str,
    candidates: &[(&'a str, Option<&str>)],
) -> Result<&'a str> {
    let hits: Vec<&str> = candidates
        .iter()
        .filter(|(_, t)| *t == Some(title))
        .map(|(u, _)| *u)
        .collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => {
            let hint = super::validate::suggest(title, candidates.iter().filter_map(|(_, t)| *t));
            Err(Error::Compile(format!(
                "{ctx}: no {what} titled \"{title}\" in the config{hint} — users, groups \
                 and access schedules are created in Loxone Config or the app"
            )))
        }
        many => Err(Error::Compile(format!(
            "{ctx}: {} {what}s are titled \"{title}\" ({}) — give them distinct titles \
             in Loxone Config or the app",
            many.len(),
            many.join(", ")
        ))),
    }
}

/// Bring one user's or group's `<PH>` list in line with the plans.
fn rewrite(el: &mut Element, principal: &str, plans: &[Plan]) -> Result<()> {
    let is_ph = |n: &Node| matches!(n, Node::Element(e) if e.name == "PH");
    let mut changed = false;
    for plan in plans {
        let want = plan.want.get(principal);
        let ph = el.children.iter().position(is_ph);
        let prm = match ph.map(|i| &el.children[i]) {
            Some(Node::Element(ph)) => {
                let hits: Vec<usize> = ph
                    .children
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| {
                        matches!(n, Node::Element(p)
                            if p.name == "PRM" && p.attr("REF") == Some(plan.block.as_str()))
                    })
                    .map(|(i, _)| i)
                    .collect();
                if hits.len() > 1 {
                    return Err(Error::Compile(format!(
                        "{} \"{}\" ({principal}) lists {} rights on the block {} — Loxone \
                         Config writes one; fix it there before declaring its rights",
                        el.attr("Type").unwrap_or("object"),
                        el.attr_decoded("Title").unwrap_or_default(),
                        hits.len(),
                        plan.block
                    )));
                }
                hits.first().copied()
            }
            _ => None,
        };
        match (prm, want) {
            (Some(i), want) => {
                let ph = element(&mut el.children[ph.unwrap()]);
                let p = element(&mut ph.children[i]);
                match (p.attr(plan.attr), want) {
                    (Some(have), Some(want)) if have == want => continue,
                    // A recipient is a recipient, whatever its `ACT=` says.
                    (Some(_), Some(_)) if plan.attr == "ACT" => continue,
                    (_, Some(want)) => set_right(p, plan.attr, want),
                    (Some(_), None) => {
                        p.remove_attr(plan.attr);
                        // Nothing but the reference left: the right is gone.
                        if p.attrs.iter().all(|a| a.name == "REF") {
                            ph.children.remove(i);
                        }
                    }
                    (None, None) => continue,
                }
                changed = true;
            }
            (None, Some(want)) => {
                let ph = match ph {
                    Some(i) => i,
                    None => {
                        el.push_child(Element::new("PH"));
                        el.children.len() - 1
                    }
                };
                let ph = element(&mut el.children[ph]);
                let mut p = Element::new("PRM");
                p.set_attr("REF", &plan.block);
                p.set_attr(plan.attr, want);
                ph.push_child(p);
                changed = true;
            }
            (None, None) => {}
        }
    }
    if !changed {
        return Ok(());
    }
    let ph = el
        .children
        .iter()
        .position(is_ph)
        .expect("a change has a list");
    let count = element(&mut el.children[ph])
        .child_elements()
        .filter(|e| e.name == "PRM")
        .count();
    if count == 0 {
        // Config writes no empty list (docs/loxone-format.md).
        el.children.remove(ph);
        if el.child_elements().next().is_none() {
            el.children.clear();
            el.self_closing = true;
        }
    } else {
        element(&mut el.children[ph]).set_attr("NUM", &count.to_string());
    }
    Ok(())
}

fn element(n: &mut Node) -> &mut Element {
    match n {
        Node::Element(e) => e,
        Node::Text(_) => unreachable!("positions come from element matches"),
    }
}

/// `attr` in the order Loxone Config writes (`REF TIM ACT`): `TIM=`
/// right after `REF=`, `ACT=` last.
fn set_right(p: &mut Element, attr: &str, value: &str) {
    if p.attr(attr).is_some() || attr == "ACT" {
        p.set_attr(attr, value);
        return;
    }
    let at = p
        .attrs
        .iter()
        .position(|a| a.name == "REF")
        .map_or(0, |i| i + 1);
    p.attrs.insert(
        at,
        Attr {
            name: attr.into(),
            value: crate::xml::escape(value).into_owned(),
        },
    );
}

/// Users with an NFC tag or an access code but no access right on any NFC
/// Code Touch — not their own, not through a group: the tag opens nothing.
/// Full access (`UgType=4`) does not count: it gives every right except
/// access (live-verified, docs/loxone-format.md).
pub fn lint_access(doc: &LoxoneDoc) -> Vec<LintFinding> {
    let model = AccessModel::read(doc);
    let code_touches: BTreeSet<String> = doc
        .objects()
        .into_iter()
        .filter(|o| o.block_type == ACCESS_BLOCK_TYPE)
        .map(|o| o.uuid)
        .collect();
    if code_touches.is_empty() {
        return Vec::new();
    }
    let with_access: BTreeSet<&str> = model
        .access_rights()
        .iter()
        .filter(|r| code_touches.contains(r.target))
        .map(|r| r.principal)
        .collect();
    model
        .users
        .iter()
        .filter(|u| u.has_tags_or_codes())
        .filter(|u| {
            !with_access.contains(u.uuid.as_str())
                && !model
                    .groups_of_transitive(&u.uuid)
                    .iter()
                    .any(|g| with_access.contains(g.uuid.as_str()))
        })
        .map(|u| {
            let mut has = Vec::new();
            if !u.nfc_tags.is_empty() {
                has.push("an NFC tag");
            }
            if !u.codes.is_empty() {
                has.push("an access code");
            }
            LintFinding {
                kind: LintKind::NoAccessRight,
                slug: u.title.clone().unwrap_or_else(|| u.uuid.clone()),
                detail: format!(
                    "user with {} but no access right on any NFC Code Touch, neither \
                     directly nor through a group — the tag or code opens nothing; grant \
                     one in an `access <code_touch>` declaration or in the app",
                    has.join(" and ")
                ),
            }
        })
        .collect()
}
