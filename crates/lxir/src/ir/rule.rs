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

use super::ast::{Module, RULE_BLOCK_TYPE, RuleDecl, RuleItem, RuleStmt, RuleStmtKind};
use crate::autopilot::{self, Action, Event, Rule};
use crate::doc::LoxoneDoc;
use crate::error::{Error, Result};
use crate::xml::Attr;
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
                "{ctx}: the rule has {why} — lxir writes only the documented shapes; keep \
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
    new["active"] = Value::Bool(want.active);

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
                        "{ctx}: no rule in this config sends `{}` to a `{ty}`, and lxir \
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
