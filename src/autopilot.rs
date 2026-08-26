//! Reading the Automatic Designer rule payload.
//!
//! An `AutopilotRule` block carries its entire rule — conditions
//! ("events"), actions, activation flags — as JSON in the element's
//! `APC=` attribute ([loxone-format.md](../docs/loxone-format.md)). The
//! app and the Miniserver own these blocks; lxir never writes them. This
//! module gives the read side eyes: a human summary for the decompile
//! full view and a field-path diff for `lxir diff`.
//!
//! The payload is handled as untyped JSON deliberately: a typed struct
//! would silently drop fields a newer app version adds, and the diff
//! must see exactly what changed, known schema or not. Only the handful
//! of fields the summary renders are interpreted, and only as far as
//! corpus evidence supports (a `controlUUID`/action `id` is the UUID of
//! a control in the same document; `command` is verbatim app text).
//! Everything else is shown raw — refuse, never guess.

use serde_json::Value;

/// Parse a decoded `APC=` attribute value. `None` if it is not JSON —
/// callers fall back to showing the raw text.
pub fn parse(apc: &str) -> Option<Value> {
    serde_json::from_str(apc).ok()
}

/// The payload in canonical text form (compact, keys sorted — serde's
/// default map is ordered), for semantic comparison and fingerprinting.
/// The raw attribute text is not stable enough to compare: two of the
/// house config's rules carry a raw CRLF *inside* the JSON (the same
/// not-actually-XML deviation as `Code=`), so a writer reflowing the
/// payload would read as drift on every rule. `None` if not JSON —
/// comparisons fall back to the raw text.
pub fn canonical(apc: &str) -> Option<String> {
    parse(apc).map(|v| v.to_string())
}

/// One line per fact, for `#`-comment rendering in the decompile full
/// view. `resolve` maps an object UUID to a display name (the caller
/// knows the document); an unresolved UUID stays raw.
pub fn summary(rule: &Value, resolve: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut flags = Vec::new();
    if rule.get("active").and_then(Value::as_bool) == Some(false) {
        flags.push("inactive");
    }
    if rule.get("invalid").and_then(Value::as_bool) == Some(true) {
        flags.push("invalid");
    }
    if !flags.is_empty() {
        out.push(format!("rule is {}", flags.join(", ")));
    }
    let groups = rule
        .get("eventGroups")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for (gi, group) in groups.iter().enumerate() {
        let events = group
            .get("events")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for event in events {
            let group_tag = if groups.len() > 1 {
                format!(" (group {})", gi + 1)
            } else {
                String::new()
            };
            out.push(format!("when{group_tag} {}", event_line(event, resolve)));
        }
    }
    let actions = rule
        .get("actions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for action in actions {
        out.push(format!("then {}", action_line(action, resolve)));
    }
    out
}

/// A condition: a control event names its control (resolvable UUID);
/// anything else is shown by its raw type/descID/id — their semantics
/// are not publicly specified, and the view does not guess.
fn event_line(event: &Value, resolve: &dyn Fn(&str) -> Option<String>) -> String {
    let mut line = match event.get("controlUUID").and_then(Value::as_str) {
        Some(uuid) => format!(
            "{} ({})",
            display_name(uuid, resolve),
            brief(event, &["descID"])
        ),
        None => brief(event, &["type", "descID", "id"]),
    };
    if let Some(v) = event.get("value") {
        line.push_str(&format!(": value {v}"));
    }
    // The modal operator (1) is elided like a GUI-default parameter
    // (D30); anything else is a fact worth showing.
    if let Some(op) = event.get("operator").and_then(Value::as_i64)
        && op != 1
    {
        line.push_str(&format!(", op {op}"));
    }
    if event.get("forceEdge").and_then(Value::as_bool) == Some(true) {
        line.push_str(", forceEdge");
    }
    line
}

/// An action: the target control (its `id` is an object UUID) plus the
/// verbatim command text.
fn action_line(action: &Value, resolve: &dyn Fn(&str) -> Option<String>) -> String {
    let target = match action.get("id").and_then(Value::as_str) {
        Some(uuid) => display_name(uuid, resolve),
        None => brief(action, &["type", "descID", "id"]),
    };
    match action.get("command").and_then(Value::as_str) {
        Some(cmd) => format!("{target}: {cmd}"),
        None => format!("{target}: {}", brief(action, &["descID"])),
    }
}

fn display_name(uuid: &str, resolve: &dyn Fn(&str) -> Option<String>) -> String {
    match resolve(uuid) {
        Some(name) => format!("\"{name}\""),
        None => uuid.to_string(),
    }
}

/// `key1 v1, key2 v2` over the listed keys that are present.
fn brief(obj: &Value, keys: &[&str]) -> String {
    keys.iter()
        .filter_map(|k| obj.get(k).map(|v| format!("{k} {v}")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Field-path changes between two rule payloads, one line each
/// (`eventGroups[0].events[1].value: 540 -> 600`). Structural and
/// schema-agnostic: fields this crate does not know still diff.
pub fn changes(old: &Value, new: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk("", old, new, &mut out);
    out
}

fn walk(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    if a == b {
        return;
    }
    match (a, b) {
        (Value::Object(ma), Value::Object(mb)) => {
            let keys: std::collections::BTreeSet<&String> = ma.keys().chain(mb.keys()).collect();
            for key in keys {
                let sub = join(path, key);
                match (ma.get(key.as_str()), mb.get(key.as_str())) {
                    (Some(va), Some(vb)) => walk(&sub, va, vb, out),
                    (Some(va), None) => out.push(format!("{sub}: {va} -> absent")),
                    (None, Some(vb)) => out.push(format!("{sub}: absent -> {vb}")),
                    (None, None) => unreachable!(),
                }
            }
        }
        (Value::Array(va), Value::Array(vb)) => {
            for (i, (ea, eb)) in va.iter().zip(vb).enumerate() {
                walk(&format!("{path}[{i}]"), ea, eb, out);
            }
            for (i, e) in va.iter().enumerate().skip(vb.len()) {
                out.push(format!("{path}[{i}]: {e} -> absent"));
            }
            for (i, e) in vb.iter().enumerate().skip(va.len()) {
                out.push(format!("{path}[{i}]: absent -> {e}"));
            }
        }
        _ => out.push(format!("{path}: {a} -> {b}")),
    }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Corpus-shaped rule (synthetic UUIDs): one time event, one control
    /// event, one action.
    fn rule() -> Value {
        serde_json::from_str(
            r#"{
              "type": 1, "id": "30000009-0000-0090-ffff504f94112233",
              "active": true, "activeIfValid": false,
              "name": "Skimmer Ein", "description": "",
              "operator": 3, "invalid": false,
              "eventGroups": [{"operator": 1, "events": [
                {"type": 5, "descID": 272001, "id": 272, "value": 540,
                 "operator": 1, "forceEdge": false},
                {"type": 0, "descID": 42001,
                 "id": "30000001-0000-0010-ffff504f94112233",
                 "controlUUID": "30000001-0000-0010-ffff504f94112233",
                 "value": 65, "operator": 5, "forceEdge": false}
              ]}],
              "actions": [{"type": 0, "descID": 342101,
                "id": "30000005-0000-0050-ffff504f94112233",
                "command": "on"}]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn summary_resolves_controls_and_keeps_the_rest_raw() {
        let resolve = |uuid: &str| match uuid {
            "30000001-0000-0010-ffff504f94112233" => Some("Wassertemperatur".to_string()),
            "30000005-0000-0050-ffff504f94112233" => Some("Freigabe".to_string()),
            _ => None,
        };
        let lines = summary(&rule(), &resolve);
        assert_eq!(
            lines,
            vec![
                "when type 5, descID 272001, id 272: value 540",
                "when \"Wassertemperatur\" (descID 42001): value 65, op 5",
                "then \"Freigabe\": on",
            ]
        );
        // Flags only when notable.
        let mut inactive = rule();
        inactive["active"] = Value::Bool(false);
        assert_eq!(summary(&inactive, &resolve)[0], "rule is inactive");
    }

    #[test]
    fn changes_report_field_paths() {
        let old = rule();
        let mut new = rule();
        new["eventGroups"][0]["events"][0]["value"] = 600.into();
        new["active"] = Value::Bool(false);
        new["actions"][0]["command"] = "off".into();
        assert_eq!(
            changes(&old, &new),
            vec![
                "actions[0].command: \"on\" -> \"off\"",
                "active: true -> false",
                "eventGroups[0].events[0].value: 540 -> 600",
            ]
        );
        // Array growth is reported whole, element by element.
        let mut extra = rule();
        let arr = extra["actions"].as_array_mut().unwrap();
        arr.push(serde_json::json!({"command": "off"}));
        let c = changes(&old, &extra);
        assert_eq!(c, vec!["actions[1]: absent -> {\"command\":\"off\"}"]);
        assert!(changes(&old, &old).is_empty());
    }
}
