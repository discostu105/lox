//! Automatic Designer rules from source (D57).
//!
//! An `AutopilotRule` block keeps its whole rule as JSON in `APC=`
//! ([`crate::autopilot`]). A `rule <slug>` declaration owns what the rule
//! does: whether it is active, its conditions and its actions. Compiling
//! compares that with the payload *as read* ([`crate::autopilot::Rule`])
//! and writes only when the two differ — so a declaration lifted from a
//! config reproduces it byte for byte, raw spelling included.
//!
//! A write is the edit the app makes: an unchanged event or action keeps
//! its JSON object, a changed one keeps every field but the ones the
//! source states, and only a new one is built. `STA="1"` follows `active`
//! (set right after `APC=`, removed when inactive — the app's own toggle,
//! docs/loxone-format.md). A new action needs the action's `descID`, a
//! code per target type and command that ships only inside the app: it
//! is copied from an action with the same target type and command — in
//! this document, else in the corpus table [`OBSERVED_ACTIONS`] — and
//! refused when there is none. Rules with shapes outside the documented
//! ones are not written at all.
//!
//! A named rule (`rule <slug> "Name"`, D60) is the source's own: [`create`]
//! finds it in the base (by its lock UUID, else by name) or builds it next
//! to the rules the document already has, and from there it is edited
//! like any other.

use super::ast::{Module, RULE_BLOCK_TYPE, RuleDecl, RuleItem, RuleStmt, RuleStmtKind};
use crate::autopilot::{self, Action, Event, Rule};
use crate::doc::LoxoneDoc;
use crate::error::{Error, Result};
use crate::uuid::{LoxUuid, Minter};
use crate::xml::{Attr, Element, Node};
use serde_json::Value;
use std::collections::BTreeMap;

/// Action `descID`s observed for a target block type and command, with no
/// action of that kind in the document to copy from (corpus: the house
/// config's rules and the use-case configs, docs/loxone-format.md).
const OBSERVED_ACTIONS: &[(&str, &str, i64)] = &[
    ("PushButton", "on", 342_101),
    ("PushButton", "off", 342_102),
    ("PButtonT", "pulse", 493_103),
];

const DAYS: &[(&str, &[u8])] = &[
    ("mon", &[0]),
    ("tue", &[1]),
    ("wed", &[2]),
    ("thu", &[3]),
    ("fri", &[4]),
    ("sat", &[5]),
    ("sun", &[6]),
    ("weekdays", &[0, 1, 2, 3, 4]),
    ("weekend", &[5, 6]),
    ("daily", &[0, 1, 2, 3, 4, 5, 6]),
];

/// The weekday indices (`0` = Monday) a day keyword stands for.
pub(crate) fn weekdays(word: &str) -> Option<&'static [u8]> {
    DAYS.iter().find(|(k, _)| *k == word).map(|(_, d)| *d)
}

/// The shortest keywords for a set of weekdays: a whole group where the
/// set is one, single days otherwise.
fn day_words(days: &[u8]) -> Vec<String> {
    for (k, d) in &DAYS[7..] {
        if *d == days {
            return vec![(*k).to_string()];
        }
    }
    days.iter()
        .map(|&d| DAYS[d as usize].0.to_string())
        .collect()
}

/// The connectors of an `AutopilotRule`, in index order (10/10 corpus
/// rules).
const RULE_PORTS: [&str; 7] = ["InputTrigger", "Reset", "On", "run", "Q", "A", "OutputAPI"];

/// The vertical gap between two rules stacked on their page (house
/// config: four of five gaps).
const RULE_GAP: i64 = 168;

/// The rule UUIDs of the named rules (D60), slug → UUID, creating the ones
/// the base lacks. `pinned` is the lock's map (slug → UUID); the result is
/// its new content. A pinned UUID the base carries is the rule — the
/// source owns its name, so a differing `Title=` is rewritten. Otherwise
/// the base's one rule of that name is adopted, and with none the rule is
/// created: under the pinned UUID when there is one (a base from before
/// the deployment), else a minted one.
pub(crate) fn create(
    doc: &mut LoxoneDoc,
    module: &Module,
    uuid_of: &BTreeMap<String, String>,
    pinned: &BTreeMap<String, String>,
    minter: &mut Minter,
) -> Result<BTreeMap<String, String>> {
    // The extern rules: a named rule must not be one of them.
    let mut out: BTreeMap<String, String> = module
        .rule_decls()
        .filter(|d| d.title.is_none())
        .filter_map(|d| Some((d.slug.clone(), uuid_of.get(&d.slug)?.clone())))
        .collect();
    let externs: Vec<String> = out.keys().cloned().collect();
    for decl in module.rule_decls() {
        let Some(title) = &decl.title else { continue };
        let ctx = format!("`rule {} {title:?}`", decl.slug);
        let objects = doc.objects();
        let rules: Vec<_> = objects
            .iter()
            .filter(|o| o.block_type == RULE_BLOCK_TYPE)
            .collect();
        let pin = pinned.get(&decl.slug);
        if let Some(o) = pin.and_then(|u| objects.iter().find(|o| &o.uuid == u))
            && o.block_type != RULE_BLOCK_TYPE
        {
            return Err(Error::Compile(format!(
                "{ctx}: the lock pins the rule to {}, which this config has as a `{}` — drop \
                 the lock entry `rules.{}` to create the rule afresh",
                o.uuid, o.block_type, decl.slug
            )));
        }
        let uuid = if let Some(o) = pin.and_then(|u| rules.iter().find(|o| &o.uuid == u)) {
            let el = doc.element_at_mut(&o.path).expect("path from objects()");
            if el.attr_decoded("Title").as_deref() != Some(title.as_str()) {
                rename(&ctx, el, title)?;
            }
            o.uuid.clone()
        } else {
            let named: Vec<_> = rules
                .iter()
                .filter(|o| o.title.as_deref() == Some(title.as_str()))
                .collect();
            match named.as_slice() {
                [o] => o.uuid.clone(),
                [] => build(&ctx, doc, title, pin, minter)?,
                _ => {
                    return Err(Error::Compile(format!(
                        "{ctx}: the config has {} rules of this name — rename all but one \
                         in the app, or address one by its extern",
                        named.len()
                    )));
                }
            }
        };
        if let Some((other, _)) = out.iter().find(|(_, u)| **u == uuid) {
            return Err(Error::Compile(format!(
                "{ctx}: `rule {other}` already declares this rule"
            )));
        }
        out.insert(decl.slug.clone(), uuid);
    }
    for slug in externs {
        out.remove(&slug);
    }
    Ok(out)
}

/// Write `title` as the rule's name: `Title=` and the payload's `name`.
fn rename(ctx: &str, el: &mut Element, title: &str) -> Result<()> {
    let raw = el.attr_decoded("APC").unwrap_or_default().into_owned();
    let mut payload = autopilot::parse(&raw).ok_or_else(|| {
        Error::Compile(format!(
            "{ctx}: the rule has no readable payload (`APC=`) to rename"
        ))
    })?;
    payload["name"] = Value::String(title.to_string());
    el.set_attr("Title", title);
    el.set_attr("APC", &autopilot::to_text(&payload));
    Ok(())
}

/// A new, inactive rule without conditions or actions (the edit that
/// follows writes them), on the page of the document's rules, below the
/// last one. Everything not stated comes from that rule: the block
/// version, size, colour and flags, the `ref=` to the `AutoPilot`
/// container. A document without a rule is refused — where its rules live
/// is the app's choice (a special page in the house config, logic pages in
/// the use-case configs).
fn build(
    ctx: &str,
    doc: &mut LoxoneDoc,
    title: &str,
    pin: Option<&String>,
    minter: &mut Minter,
) -> Result<String> {
    let objects = doc.objects();
    let Some(last) = objects
        .iter()
        .filter(|o| o.block_type == RULE_BLOCK_TYPE)
        .max_by_key(|o| o.path.clone())
    else {
        return Err(Error::Compile(format!(
            "{ctx}: the config has no Automatic Designer rule yet — create one in the app \
             once, then the compile adds new ones next to it"
        )));
    };
    let template = doc
        .element_at(&last.path)
        .expect("path from objects()")
        .clone();
    let page_path = &last.path[..last.path.len() - 1];
    // The lowest free spot in the column of the page's rules.
    let page = doc.element_at(page_path).expect("parent of an object");
    let int = |e: &Element, k: &str| e.attr(k).and_then(|v| v.parse::<i64>().ok());
    let column: Vec<&Element> = page
        .children
        .iter()
        .filter_map(|n| match n {
            Node::Element(e) if e.attr("Type") == Some(RULE_BLOCK_TYPE) => Some(e),
            _ => None,
        })
        .collect();
    let (Some(px), Some(py), Some(px2), Some(py2)) = (
        int(&template, "Px"),
        int(&template, "Py"),
        int(&template, "Px2"),
        int(&template, "Py2"),
    ) else {
        return Err(Error::Compile(format!(
            "{ctx}: the config's rule {} has no position (`Px`/`Py`/`Px2`/`Py2`) to place \
             a new one by",
            last.uuid
        )));
    };
    let top = column
        .iter()
        .filter_map(|e| int(e, "Py2"))
        .max()
        .unwrap_or(py2)
        + RULE_GAP;
    let (Some(container), Some(version)) = (template.attr("ref"), template.attr("V")) else {
        return Err(Error::Compile(format!(
            "{ctx}: the config's rule {} has no `ref=`/`V=` to copy",
            last.uuid
        )));
    };

    let uuid = match pin {
        Some(u) => u.clone(),
        None => {
            // The ports first, the rule last, as Loxone mints them.
            minter.seed(minter.counter() + RULE_PORTS.len() as u32);
            minter.mint_object().to_string()
        }
    };
    let ports = rule_ports(&uuid).ok_or_else(|| {
        Error::Compile(format!(
            "{ctx}: the lock pins the rule to a bad UUID {uuid}"
        ))
    })?;

    let payload = serde_json::json!({
        "type": 1, "id": uuid, "active": false, "activeIfValid": false,
        "name": title, "description": "", "operator": 3, "invalid": false,
        "eventGroups": [{"operator": 1, "events": []}], "actions": [],
    });
    let mut el = Element::new("C");
    el.set_attr("Type", RULE_BLOCK_TYPE);
    el.set_attr_raw("V", version);
    el.set_attr("U", &uuid);
    el.set_attr("Title", title);
    el.set_attr("Px", &px.to_string());
    el.set_attr("Py", &top.to_string());
    el.set_attr("Px2", &px2.to_string());
    el.set_attr("Py2", &(top + py2 - py).to_string());
    for k in ["Cl", "Nio", "WF"] {
        if let Some(v) = template.attr(k) {
            el.set_attr_raw(k, v);
        }
    }
    el.set_attr("APC", &autopilot::to_text(&payload));
    el.set_attr_raw("ref", container);
    for (k, u) in RULE_PORTS.iter().zip(&ports) {
        let mut co = Element::new("Co");
        co.set_attr("K", k);
        co.set_attr("U", u);
        el.push_child(co);
    }
    let mut io = Element::new("IoData");
    io.self_closing = false;
    el.push_child(io);

    let at = *last.path.last().expect("an object has a parent") + 1;
    let page = doc.element_at_mut(page_path).expect("parent of an object");
    page.children.insert(at, Node::Element(el));
    Ok(uuid)
}

/// A rule's connector UUIDs, derived from its own: Loxone mints the seven
/// ports right before the rule (house config, 5/5), each with its index
/// and the rule's machine tail.
fn rule_ports(rule: &str) -> Option<Vec<String>> {
    let u = LoxUuid::parse(rule).ok()?;
    let counter = ((u.mid as u32) << 16 | u.seq as u32).checked_sub(RULE_PORTS.len() as u32)?;
    Some(
        (0..RULE_PORTS.len() as u32)
            .map(|i| {
                let c = counter + i;
                let mut tail = [0u8; 8];
                tail[0] = i as u8;
                tail[1] = 0xff;
                tail[2..].copy_from_slice(&u.tail[2..]);
                LoxUuid {
                    time: u.time,
                    mid: (c >> 16) as u16,
                    seq: c as u16,
                    tail,
                }
                .to_string()
            })
            .collect(),
    )
}

pub(crate) fn apply(
    doc: &mut LoxoneDoc,
    module: &Module,
    uuid_of: &BTreeMap<String, String>,
) -> Result<()> {
    if module.rule_decls().next().is_none() {
        return Ok(());
    }
    let objects = doc.objects();
    let types: BTreeMap<&str, &str> = objects
        .iter()
        .map(|o| (o.uuid.as_str(), o.block_type.as_str()))
        .collect();
    let path_of: BTreeMap<&str, &[usize]> = objects
        .iter()
        .map(|o| (o.uuid.as_str(), o.path.as_slice()))
        .collect();

    // Every documented action in the document, by target type and command:
    // the templates for new actions.
    let mut templates: BTreeMap<(String, String), Value> = BTreeMap::new();
    for o in objects.iter().filter(|o| o.block_type == RULE_BLOCK_TYPE) {
        let Some(rule) = doc
            .element_at(&o.path)
            .and_then(|el| el.attr_decoded("APC"))
            .and_then(|apc| autopilot::parse(&apc))
        else {
            continue;
        };
        for a in rule["actions"].as_array().into_iter().flatten() {
            if let Ok(act) = autopilot::interpret_action(a)
                && let Some(ty) = types.get(act.target.as_str())
            {
                templates
                    .entry((ty.to_string(), act.command))
                    .or_insert_with(|| a.clone());
            }
        }
    }

    let mut writes = Vec::new();
    for decl in module.rule_decls() {
        let ctx = format!("`rule {}`", decl.slug);
        let uuid = uuid_of[&decl.slug].as_str();
        let ty = types.get(uuid).copied().unwrap_or("");
        if ty != RULE_BLOCK_TYPE {
            return Err(Error::Compile(format!(
                "{ctx}: the block ({uuid}) is a `{ty}`, not an Automatic Designer rule \
                 (`{RULE_BLOCK_TYPE}`)"
            )));
        }
        let el = doc.element_at(path_of[uuid]).expect("path from objects()");
        let raw = el.attr_decoded("APC").unwrap_or_default().into_owned();
        let old = autopilot::parse(&raw).ok_or_else(|| {
            Error::Compile(format!(
                "{ctx}: the rule has no readable payload (`APC=`) — create it in the app \
                 or Loxone Config first"
            ))
        })?;
        let have = autopilot::interpret(&old).map_err(|why| {
            Error::Compile(format!(
                "{ctx}: the rule has {why} — Lox Logic writes only the documented shapes; keep \
                 this rule in the app and drop the declaration"
            ))
        })?;
        let want = wanted(decl, uuid_of)?;
        let targets: Vec<&str> = want
            .actions
            .iter()
            .map(|a| types.get(a.target.as_str()).copied().unwrap_or(""))
            .collect();
        if have == want {
            continue;
        }
        let new = rewrite(&ctx, &old, &want, &targets, &templates)?;
        writes.push((uuid.to_string(), autopilot::to_text(&new), want.active));
    }

    for (uuid, text, active) in writes {
        let el = doc
            .element_at_mut(path_of[uuid.as_str()])
            .expect("path from objects()");
        el.set_attr("APC", &text);
        match (active, el.attr("STA").is_some()) {
            (true, false) => {
                let at = el
                    .attrs
                    .iter()
                    .position(|a| a.name == "APC")
                    .map_or(el.attrs.len(), |i| i + 1);
                el.attrs.insert(
                    at,
                    Attr {
                        name: "STA".into(),
                        value: "1".into(),
                    },
                );
            }
            (false, true) => {
                el.remove_attr("STA");
            }
            _ => {}
        }
    }
    Ok(())
}

/// The rule a declaration states, its action targets as UUIDs.
fn wanted(decl: &RuleDecl, uuid_of: &BTreeMap<String, String>) -> Result<Rule> {
    let mut rule = Rule {
        active: false,
        events: Vec::new(),
        actions: Vec::new(),
    };
    for st in decl.stmts() {
        match &st.kind {
            RuleStmtKind::Active(a) => rule.active = *a,
            RuleStmtKind::Time(m) => rule.events.push(Event::Time(*m)),
            RuleStmtKind::Weekday(words) => {
                let mut days: Vec<u8> = words
                    .iter()
                    .flat_map(|w| weekdays(w).expect("checked by validate").iter().copied())
                    .collect();
                days.sort_unstable();
                rule.events.push(Event::Weekdays(days));
            }
            RuleStmtKind::Then { target, command } => {
                let uuid = uuid_of.get(target).ok_or_else(|| {
                    Error::Compile(format!(
                        "`rule {}`: `{}` — `{target}` resolves to no block",
                        decl.slug, st.kind
                    ))
                })?;
                rule.actions.push(Action {
                    target: uuid.clone(),
                    command: command.clone(),
                });
            }
        }
    }
    Ok(rule)
}

/// `old` edited into `want`: unchanged parts keep their JSON, changed
/// ones keep every field the source does not state.
fn rewrite(
    ctx: &str,
    old: &Value,
    want: &Rule,
    targets: &[&str],
    templates: &BTreeMap<(String, String), Value>,
) -> Result<Value> {
    let mut new = old.clone();
    // Both flags, as the app's activation writes them.
    new["active"] = Value::Bool(want.active);
    new["activeIfValid"] = Value::Bool(want.active);

    let old_events = old["eventGroups"][0]["events"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let events: Vec<Value> = want
        .events
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let same = |o: &&Value| autopilot::interpret_event(o).as_ref() == Ok(e);
            if let Some(o) = old_events.get(i).filter(same) {
                return o.clone();
            }
            match (e, old_events.get(i).map(autopilot::interpret_event)) {
                (Event::Time(m), Some(Ok(Event::Time(_)))) => {
                    let mut o = old_events[i].clone();
                    o["value"] = (*m).into();
                    o
                }
                (Event::Weekdays(d), Some(Ok(Event::Weekdays(_)))) => {
                    let mut o = old_events[i].clone();
                    match (d.as_slice(), o.get("arrValues").is_some()) {
                        // The one-day spelling stays one-day.
                        ([one], false) => o["value"] = (*one).into(),
                        _ => {
                            o["value"] = autopilot::WEEKDAY_SENTINEL.into();
                            o["arrValues"] = d.clone().into();
                        }
                    }
                    o
                }
                (Event::Time(m), _) => autopilot::time_event(*m),
                (Event::Weekdays(d), _) => autopilot::weekday_event(d),
            }
        })
        .collect();
    new["eventGroups"][0]["events"] = Value::Array(events);

    let old_actions = old["actions"].as_array().cloned().unwrap_or_default();
    let mut actions = Vec::new();
    for (i, (a, ty)) in want.actions.iter().zip(targets).enumerate() {
        let read = old_actions.get(i).map(autopilot::interpret_action);
        if let Some(Ok(o)) = &read
            && o == a
        {
            actions.push(old_actions[i].clone());
            continue;
        }
        let mut action = match templates.get(&(ty.to_string(), a.command.clone())) {
            Some(t) => t.clone(),
            None => match OBSERVED_ACTIONS
                .iter()
                .find(|(t, c, _)| t == ty && *c == a.command)
            {
                Some((_, _, desc)) => serde_json::json!({
                    "type": 0, "descID": desc, "command": a.command,
                }),
                None => {
                    return Err(Error::Compile(format!(
                        "{ctx}: no rule in this config sends `{}` to a `{ty}`, and Lox Logic \
                         knows no action code (`descID`) for it — create one such action \
                         in the app once, then the compile can copy it",
                        a.command
                    )));
                }
            },
        };
        action["id"] = Value::String(a.target.clone());
        actions.push(action);
    }
    new["actions"] = Value::Array(actions);
    Ok(new)
}

/// A rule's payload as a `rule` declaration, with `slug_of` naming the
/// action targets. `Err` says why it cannot be one.
pub(crate) fn lift(
    rule_slug: &str,
    apc: &str,
    slug_of: &dyn Fn(&str) -> Option<String>,
) -> std::result::Result<RuleDecl, String> {
    let payload = autopilot::parse(apc).ok_or("the payload is not JSON")?;
    let rule = autopilot::interpret(&payload)?;
    if rule.events.is_empty() || rule.actions.is_empty() {
        return Err("the rule has no condition or no action".into());
    }
    let stmt = |kind| {
        RuleItem::Stmt(RuleStmt {
            kind,
            comment: None,
        })
    };
    let mut body = vec![stmt(RuleStmtKind::Active(rule.active))];
    for e in &rule.events {
        body.push(stmt(match e {
            Event::Time(m) => RuleStmtKind::Time(*m),
            Event::Weekdays(d) => RuleStmtKind::Weekday(day_words(d)),
        }));
    }
    for a in &rule.actions {
        let target = slug_of(&a.target)
            .ok_or_else(|| format!("its action target {} is not in the config", a.target))?;
        body.push(stmt(RuleStmtKind::Then {
            target,
            command: a.command.clone(),
        }));
    }
    Ok(RuleDecl {
        slug: rule_slug.to_string(),
        title: None,
        body,
        comment: None,
        end_comment: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_words_prefer_groups() {
        assert_eq!(day_words(&[1]), ["tue"]);
        assert_eq!(day_words(&[0, 1, 2, 3, 4]), ["weekdays"]);
        assert_eq!(day_words(&[0, 6]), ["mon", "sun"]);
        assert_eq!(weekdays("weekend"), Some(&[5u8, 6][..]));
        assert_eq!(weekdays("monday"), None);
    }
}
