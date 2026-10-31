//! Semantic diff between two `.Loxone` documents, keyed by UUID.
//!
//! Its main job is telling *real* edits apart from noise, in particular the
//! locale renames: saving a config in a differently-localized Loxone Config
//! renames every built-in object (Modes, weather fields, caption folders …)
//! into the UI language — 111 renames in one observed save with zero
//! semantic change. [`Rename::locale_suspect`] flags those.

use crate::access::{AccessChange, AccessModel};
use crate::connectors::{attr_name, attr_param, attr_params, clock_params, file_params};
use crate::doc::{LoxoneDoc, ObjectSummary, WireView, ports};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Element attributes the diff projection compares for a block type: the
/// language-surface attribute parameters (`Formula=`, Code16's program
/// text `Code=`), plus `APC=` on
/// `AutopilotRule` — the app-owned Automatic Designer rule payload.
/// `APC` is *not* an `attr_params` entry because it is not language
/// surface (the app and the Miniserver own those blocks; Lox Logic never
/// writes them), but it is block logic stored in an attribute all the
/// same, and a diff blind to it would call an edited rule "semantically
/// empty". The same holds for `Formula=`.
fn diff_attrs(block_type: &str) -> impl Iterator<Item = &'static str> {
    diff_attrs_v(block_type, FINGERPRINT_VERSION)
}

/// The attribute set of fingerprint `version`: from v8 on, the settings
/// of virtual and Modbus devices and of physical inputs and
/// outputs (`Unit` is `<Display Unit=…>`, see [`setting_value`]).
fn diff_attrs_v(block_type: &str, version: u8) -> impl Iterator<Item = &'static str> {
    let params: &'static [&'static str] = if (version < 8
        && crate::periphery::periphery(block_type).is_some())
        // A PulseAt's fire time (`At:` → `Sec=`, #248) joined in v10.
        || (version < 10 && !clock_params(block_type).is_empty())
    {
        &[]
    } else {
        attr_params(block_type)
    };
    // Device settings (#197) joined in v9.
    let settings =
        if version >= 9 || (version == 8 && !crate::settings::DEVICE_TYPES.contains(&block_type)) {
            crate::settings::names(block_type)
        } else {
            Vec::new()
        };
    params
        .iter()
        .map(move |p| attr_name(block_type, p))
        .chain(settings)
        .chain((block_type == "AutopilotRule").then_some("APC"))
}

const FINGERPRINT_VERSION: u8 = 10;

/// One compared attribute of an object: its own, or `Unit` on its
/// `<Display>` for a physical input or output.
fn setting_value(doc: &LoxoneDoc, o: &ObjectSummary, name: &str) -> Option<String> {
    let el = doc.element_at(&o.path)?;
    let el = if name == "Unit" && crate::settings::setting(&o.block_type, name).is_some() {
        el.child_elements().find(|e| e.name == "Display")?
    } else {
        el
    };
    el.attr_decoded(name).map(|v| v.into_owned())
}

/// The comparison/fingerprint form of one such attribute value. `APC=`
/// is JSON and its raw text is not writer-stable (raw newlines *inside*
/// the payload exist in the wild), so it compares canonicalized — a
/// reformatted rule is not drift, an edited one is. Unparseable text,
/// and every other attribute, compares verbatim.
fn canonical_attr(name: &str, value: &Option<String>) -> Option<String> {
    match value {
        Some(v) if name == "APC" => {
            Some(crate::autopilot::canonical(v).unwrap_or_else(|| v.clone()))
        }
        _ => value.clone(),
    }
}

/// How a changed attribute reads in the diff: by its parameter name,
/// and a fire time as a time of day (`At` `18:58` → `06:58`, #248).
fn shown_attr(block_type: &str, attr: &str, value: Option<String>) -> (String, Option<String>) {
    match attr_param(block_type, attr) {
        Some(p) if clock_params(block_type).contains(&p) => {
            let clock = value
                .as_deref()
                .and_then(|v| v.parse().ok().and_then(crate::logic::clock_text));
            (p.to_string(), clock.or(value))
        }
        Some(p) => (p.to_string(), value),
        None => (attr.to_string(), value),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DocDiff {
    pub added: Vec<ObjectSummary>,
    pub removed: Vec<ObjectSummary>,
    pub renamed: Vec<Rename>,
    pub param_changes: Vec<ParamChange>,
    /// Room/category binding changes.
    pub placements: Vec<PlacementChange>,
    pub wires_added: Vec<WireView>,
    pub wires_removed: Vec<WireView>,
    /// The users/access section ([`crate::access::changes`]): users and
    /// groups (which the object lists above leave out), membership,
    /// credentials, rights. Secrets show as fingerprints only.
    pub access: Vec<AccessChange>,
}

impl DocDiff {
    pub fn is_empty(&self) -> bool {
        self.logic_is_empty() && self.access.is_empty()
    }

    /// Empty apart from the users/access section: what the Miniserver
    /// runs as logic is the same. `lox config push` keeps users and access
    /// rights as they are live, so its drift check asks this.
    pub fn logic_is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.renamed.is_empty()
            && self.param_changes.is_empty()
            && self.placements.is_empty()
            && self.wires_added.is_empty()
            && self.wires_removed.is_empty()
    }
}

/// Object types the users/access section reports instead of the object
/// lists.
fn in_access_section(block_type: &str) -> bool {
    matches!(block_type, "User" | "UserGroup")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rename {
    pub uuid: String,
    pub block_type: String,
    pub from: Option<String>,
    pub to: Option<String>,
    /// `true` when this looks like a locale-volatile built-in title rather
    /// than a deliberate user edit.
    pub locale_suspect: bool,
}

/// A `Def=` change on one port of an object present in both documents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParamChange {
    pub object_uuid: String,
    pub block_type: String,
    pub port_key: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// A room or category binding change (`<IoData Pr=…>` / `Cr=…`) on an
/// object present in both documents. The values are `Place`/`Category`
/// UUIDs; the titles are resolved in each value's own document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlacementChange {
    pub object_uuid: String,
    pub block_type: String,
    /// The object's title in the new document.
    pub title: Option<String>,
    /// `"room"` or `"category"`.
    pub attr: &'static str,
    pub from: Option<String>,
    pub to: Option<String>,
    pub from_title: Option<String>,
    pub to_title: Option<String>,
}

/// The room/category bindings of one object, from its `<IoData>` child —
/// where every binding lives (docs/loxone-format.md). Since Lox Logic can
/// rewrite them, they are part of the projection: a diff blind to
/// them would call a moved device semantically empty.
fn placement(doc: &LoxoneDoc, o: &ObjectSummary) -> [(&'static str, Option<String>); 2] {
    let io = doc
        .element_at(&o.path)
        .and_then(|el| el.child_elements().find(|c| c.name == "IoData"));
    let get = |name: &str| io.and_then(|io| io.attr_decoded(name).map(|v| v.into_owned()));
    [("room", get("Pr")), ("category", get("Cr"))]
}

/// The programmed functions of a Touch Pure Flex's tiles, keyed
/// `tile <i> click|up|down`: every attribute of the function element
/// except the fixed `bT=`, decoded, `api`/`cT`/`ctrls` first. A function
/// without one is left out — like a port without `Def=`, it reads the
/// same as an absent one. Tile layout (`x`/`y`/`w`/`h`, icons) is not part
/// of the projection, as block positions are not.
fn tile_functions(doc: &LoxoneDoc, o: &ObjectSummary) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(btns) = doc
        .element_at(&o.path)
        .and_then(|el| el.child_elements().find(|c| c.name == "btns"))
    else {
        return out;
    };
    const SLOTS: [(&str, &str); 3] = [("f1", "click"), ("f2", "up"), ("f3", "down")];
    const ORDER: [&str; 3] = ["api", "cT", "ctrls"];
    for btn in btns.child_elements().filter(|e| e.name == "btn") {
        let i = btn.attr_decoded("i").unwrap_or_default();
        for (element, key) in SLOTS {
            let Some(f) = btn.child_elements().find(|e| e.name == element) else {
                continue;
            };
            let mut attrs: Vec<_> = f.attrs.iter().filter(|a| a.name != "bT").collect();
            attrs.sort_by_key(|a| {
                (
                    ORDER
                        .iter()
                        .position(|n| *n == a.name)
                        .unwrap_or(ORDER.len()),
                    a.name.clone(),
                )
            });
            if attrs.is_empty() {
                continue;
            }
            let projection = attrs
                .iter()
                .map(|a| format!("{}={:?}", a.name, crate::xml::unescape(&a.value)))
                .collect::<Vec<_>>()
                .join(" ");
            out.insert(format!("tile {i} {key}"), projection);
        }
    }
    out
}

/// What an object's `<LoggerMailer>` does: the notification,
/// logger, mailer, caller or tracker it reports to, the texts it sends on
/// and off, and the least time between two messages — one entry per
/// attribute, keyed `message <attr>`. A message ref (`OutputRefLM`) is one
/// of these; Loxone Config puts the same child on inputs and outputs that
/// log or notify. An object without one projects nothing.
fn message_fields(doc: &LoxoneDoc, o: &ObjectSummary) -> BTreeMap<String, String> {
    doc.element_at(&o.path)
        .and_then(|el| el.child_elements().find(|c| c.name == "LoggerMailer"))
        .map(|lm| {
            lm.attrs
                .iter()
                .map(|a| {
                    (
                        format!("message {}", a.name),
                        crate::xml::unescape(&a.value).into_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The day keywords of the built-in operating modes, by the `N` of
/// their fixed UUID `00000000-0000-000N-15…` — the schedule's own words.
const MODE_DAYS: [&str; 10] = [
    "holiday", "vacation", "dayoff", "mon", "tue", "wed", "thu", "fri", "sat", "sun",
];

/// A DayTimer's switching times, one column per operating mode in
/// `Modes=`: the column's key orders the built-in modes as a schedule
/// names them (holiday … sun, then any other mode by title or UUID), the
/// value is its intervals in source terms — `21:00-24:00 activate`,
/// `06:00-08:00 = 21.5` (a value shows on an analog timer, or wherever it
/// is not a digital `1`), `off` for an empty column. `N=` and
/// `UserModes=` are counts of what the columns hold, so they add nothing
/// here. Not a DayTimer: empty.
fn schedule_columns(doc: &LoxoneDoc, o: &ObjectSummary) -> BTreeMap<(usize, String), String> {
    let mut out = BTreeMap::new();
    if o.block_type != "DayTimer" {
        return out;
    }
    let Some(el) = doc.element_at(&o.path) else {
        return out;
    };
    let analog = el.attr("Analog") == Some("true");
    let modes: Vec<(usize, String)> = el
        .attr_decoded("Modes")
        .unwrap_or_default()
        .split(',')
        .filter(|m| !m.is_empty())
        .map(|m| {
            let builtin = m
                .strip_prefix("00000000-0000-")
                .and_then(|r| r.strip_suffix("-1500000000000000"))
                .and_then(|n| u16::from_str_radix(n, 16).ok())
                .and_then(|n| Some((n as usize, *MODE_DAYS.get((n as usize).checked_sub(1)?)?)));
            match builtin {
                Some((n, day)) => (n, day.to_string()),
                None => {
                    let title = doc
                        .objects()
                        .into_iter()
                        .find(|x| x.uuid == m)
                        .and_then(|x| x.title);
                    (
                        usize::MAX,
                        title.map_or_else(|| m.to_string(), |t| format!("{t:?}")),
                    )
                }
            }
        })
        .collect();
    let mut columns: BTreeMap<(usize, String), Vec<(u32, String)>> =
        modes.iter().map(|m| (m.clone(), Vec::new())).collect();
    let hm = |m: u32| format!("{:02}:{:02}", m / 60, m % 60);
    for e in el.child_elements().filter(|c| c.name == "Entry") {
        let num = |name: &str| e.attr_decoded(name).and_then(|v| v.parse::<u32>().ok());
        let (from, to, ix) = (num("From").unwrap_or(0), num("To").unwrap_or(0), num("Ix"));
        let mut iv = format!("{}-{}", hm(from), hm(to));
        let v = e.attr_decoded("V").unwrap_or_default();
        if analog || v != "1" {
            iv.push_str(&format!(" = {v}"));
        }
        if e.attr("NeedActivate") == Some("true") {
            iv.push_str(" activate");
        }
        let ix = ix.unwrap_or(0) as usize;
        let key = modes
            .get(ix)
            .cloned()
            .unwrap_or_else(|| (usize::MAX, format!("mode #{ix}")));
        columns.entry(key).or_default().push((from, iv));
    }
    for (key, mut ivs) in columns {
        ivs.sort();
        let text = if ivs.is_empty() {
            "off".to_string()
        } else {
            ivs.into_iter()
                .map(|(_, iv)| iv)
                .collect::<Vec<_>>()
                .join(", ")
        };
        out.insert(key, text);
    }
    out
}

/// The schedule changes of one DayTimer, keyed `schedule <days>`: the
/// columns that differ, those with the same before and after grouped
/// under the schedule's day words (`daily`, `weekdays`, `weekend`, then
/// single days). A column one side lacks is `None`.
fn schedule_changes(
    a: &BTreeMap<(usize, String), String>,
    b: &BTreeMap<(usize, String), String>,
) -> Vec<(String, Option<String>, Option<String>)> {
    // The mode columns of one group, and its before and after.
    type Group<'a> = (
        Vec<&'a (usize, String)>,
        Option<&'a String>,
        Option<&'a String>,
    );
    let mut groups: Vec<Group> = Vec::new();
    for key in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
        let (from, to) = (a.get(key), b.get(key));
        if from == to {
            continue;
        }
        match groups.iter_mut().find(|g| g.1 == from && g.2 == to) {
            Some(g) => g.0.push(key),
            None => groups.push((vec![key], from, to)),
        }
    }
    groups
        .into_iter()
        .map(|(keys, from, to)| {
            let mut days: Vec<&str> = keys.iter().map(|(_, d)| d.as_str()).collect();
            for (group, members) in [
                ("daily", &MODE_DAYS[3..10]),
                ("weekdays", &MODE_DAYS[3..8]),
                ("weekend", &MODE_DAYS[8..10]),
            ] {
                if members.iter().all(|m| days.contains(m)) {
                    let at = days.iter().position(|d| d == &members[0]).unwrap_or(0);
                    days.retain(|d| !members.contains(d));
                    days.insert(at.min(days.len()), group);
                }
            }
            (
                format!("schedule {}", days.join(" ")),
                from.cloned(),
                to.cloned(),
            )
        })
        .collect()
}

fn taxonomy_titles(objs: &BTreeMap<String, ObjectSummary>) -> BTreeMap<&str, &str> {
    objs.values()
        .filter(|o| matches!(o.block_type.as_str(), "Place" | "Category"))
        .filter_map(|o| Some((o.uuid.as_str(), o.title.as_deref()?)))
        .collect()
}

/// Compare two documents object-by-object (matched on UUID).
pub fn diff(a: &LoxoneDoc, b: &LoxoneDoc) -> DocDiff {
    let objs_a: BTreeMap<String, ObjectSummary> = a
        .objects()
        .into_iter()
        .map(|o| (o.uuid.clone(), o))
        .collect();
    let objs_b: BTreeMap<String, ObjectSummary> = b
        .objects()
        .into_iter()
        .map(|o| (o.uuid.clone(), o))
        .collect();

    let mut out = DocDiff::default();
    let (names_a, names_b) = (taxonomy_titles(&objs_a), taxonomy_titles(&objs_b));

    for (uuid, ob) in &objs_b {
        if in_access_section(&ob.block_type) {
            continue;
        }
        match objs_a.get(uuid) {
            None => out.added.push(ob.clone()),
            Some(oa) => {
                if oa.title != ob.title {
                    out.renamed.push(Rename {
                        uuid: uuid.clone(),
                        block_type: ob.block_type.clone(),
                        from: oa.title.clone(),
                        to: ob.title.clone(),
                        locale_suspect: locale_suspect(ob),
                    });
                }
                // Def= comparison per port key (ports matched by key: port
                // UUIDs are stable, but keys read better and are unique
                // within a block).
                let pa: BTreeMap<String, Option<String>> = doc_ports(a, oa);
                let pb: BTreeMap<String, Option<String>> = doc_ports(b, ob);
                for (key, def_b) in &pb {
                    let def_a = pa.get(key).cloned().unwrap_or(None);
                    if &def_a != def_b {
                        out.param_changes.push(ParamChange {
                            object_uuid: uuid.clone(),
                            block_type: ob.block_type.clone(),
                            port_key: key.clone(),
                            from: def_a,
                            to: def_b.clone(),
                        });
                    }
                }
                for (key, def_a) in &pa {
                    if !pb.contains_key(key) && def_a.is_some() {
                        out.param_changes.push(ParamChange {
                            object_uuid: uuid.clone(),
                            block_type: ob.block_type.clone(),
                            port_key: key.clone(),
                            from: def_a.clone(),
                            to: None,
                        });
                    }
                }
                for ((attr, va), (_, vb)) in placement(a, oa).into_iter().zip(placement(b, ob)) {
                    if va != vb {
                        let title_in = |names: &BTreeMap<&str, &str>, v: &Option<String>| {
                            v.as_deref()
                                .and_then(|v| names.get(v))
                                .map(|t| t.to_string())
                        };
                        out.placements.push(PlacementChange {
                            object_uuid: uuid.clone(),
                            block_type: ob.block_type.clone(),
                            title: ob.title.clone(),
                            attr,
                            from_title: title_in(&names_a, &va),
                            to_title: title_in(&names_b, &vb),
                            from: va,
                            to: vb,
                        });
                    }
                }
                // Block logic stored as an element attribute — attribute
                // parameters (`Formula=`) and the Automatic Designer rule
                // payload (`APC=`); see `diff_attrs`.
                // Touch Pure Flex tile programs, one change per
                // function.
                let (mut ta, mut tb) = (tile_functions(a, oa), tile_functions(b, ob));
                // …and the message of a `<LoggerMailer>`.
                ta.extend(message_fields(a, oa));
                tb.extend(message_fields(b, ob));
                for key in ta.keys().chain(tb.keys()).collect::<BTreeSet<_>>() {
                    let (from, to) = (ta.get(key), tb.get(key));
                    if from != to {
                        out.param_changes.push(ParamChange {
                            object_uuid: uuid.clone(),
                            block_type: ob.block_type.clone(),
                            port_key: key.clone(),
                            from: from.cloned(),
                            to: to.cloned(),
                        });
                    }
                }
                // A LightController2's moods, one change per mood
                // in source terms, plus their order.
                let (ma, mb) = (
                    crate::moods::mood_fields(a, oa),
                    crate::moods::mood_fields(b, ob),
                );
                for key in ma.keys().chain(mb.keys()).collect::<BTreeSet<_>>() {
                    let (from, to) = (ma.get(key), mb.get(key));
                    if from != to {
                        out.param_changes.push(ParamChange {
                            object_uuid: uuid.clone(),
                            block_type: ob.block_type.clone(),
                            port_key: key.clone(),
                            from: from.cloned(),
                            to: to.cloned(),
                        });
                    }
                }
                // A DayTimer's switching times, by day group.
                for (key, from, to) in
                    schedule_changes(&schedule_columns(a, oa), &schedule_columns(b, ob))
                {
                    out.param_changes.push(ParamChange {
                        object_uuid: uuid.clone(),
                        block_type: ob.block_type.clone(),
                        port_key: key,
                        from,
                        to,
                    });
                }
                for name in diff_attrs(&ob.block_type) {
                    let (va, vb) = (setting_value(a, oa, name), setting_value(b, ob, name));
                    if canonical_attr(name, &va) != canonical_attr(name, &vb) {
                        let (port_key, from) = shown_attr(&ob.block_type, name, va);
                        let (_, to) = shown_attr(&ob.block_type, name, vb);
                        out.param_changes.push(ParamChange {
                            object_uuid: uuid.clone(),
                            block_type: ob.block_type.clone(),
                            port_key,
                            from,
                            to,
                        });
                    }
                }
            }
        }
    }
    for (uuid, oa) in &objs_a {
        if !objs_b.contains_key(uuid) && !in_access_section(&oa.block_type) {
            out.removed.push(oa.clone());
        }
    }
    out.access = crate::access::changes(
        &AccessModel::read(a),
        &AccessModel::read(b),
        &objs_a,
        &objs_b,
    );

    let wires_a: BTreeSet<WireView> = a.wires().into_iter().collect();
    let wires_b: BTreeSet<WireView> = b.wires().into_iter().collect();
    out.wires_added = wires_b.difference(&wires_a).cloned().collect();
    out.wires_removed = wires_a.difference(&wires_b).cloned().collect();
    out
}

/// Stable digest of the projection [`diff`] compares: two documents
/// fingerprint-equal iff their diff is empty apart from locale-suspect
/// renames and the details of users and groups (suspect titles are left
/// out, so re-saving a config under another UI locale does not read as
/// drift; neither do position moves or the save fingerprint, which the
/// projection never sees). Users and groups count as objects here: UUID,
/// type and title. Their credentials, members, rights and last edits stay
/// outside, so existing lockfiles do not read as drift. Recorded in the lockfile
/// at adopt/compile; `loxl drift` recomputes it over a fresh download to
/// answer "did Loxone Config or the app change something?" from one parse, no
/// reference config needed.
pub fn semantic_fingerprint(doc: &LoxoneDoc) -> String {
    fingerprint(doc, FINGERPRINT_VERSION)
}

/// Fingerprint version 9: the current version without a PulseAt's fire time
/// (`Sec=`, #248).
/// `loxl drift` accepts every older version, so an upgrade alone does not
/// read as drift. The next compile records the current version.
#[doc(hidden)] // older forms, for drift checks against old locks
pub fn semantic_fingerprint_v9(doc: &LoxoneDoc) -> String {
    fingerprint(doc, 9)
}

/// Fingerprint version 8: version 9 without the settings of Air and Tree
/// devices (#197).
/// Accepted by `loxl drift` like [`semantic_fingerprint_v9`].
#[doc(hidden)] // older forms, for drift checks against old locks
pub fn semantic_fingerprint_v8(doc: &LoxoneDoc) -> String {
    fingerprint(doc, 8)
}

/// Fingerprint version 7: version 8 without the settings of virtual and Modbus
/// devices and of physical inputs and outputs.
/// Accepted by `loxl drift` like [`semantic_fingerprint_v9`].
#[doc(hidden)] // older forms, for drift checks against old locks
pub fn semantic_fingerprint_v7(doc: &LoxoneDoc) -> String {
    fingerprint(doc, 7)
}

/// Fingerprint version 6: version 7 without LightController2 moods
/// (`<LightscenesC>`).
/// Accepted by `loxl drift` like [`semantic_fingerprint_v9`].
#[doc(hidden)] // older forms, for drift checks against old locks
pub fn semantic_fingerprint_v6(doc: &LoxoneDoc) -> String {
    fingerprint(doc, 6)
}

/// Fingerprint version 5: version 6 without DayTimer schedules (`Modes=` and
/// the `<Entry>` list).
/// Accepted by `loxl drift` like [`semantic_fingerprint_v9`].
#[doc(hidden)] // older forms, for drift checks against old locks
pub fn semantic_fingerprint_v5(doc: &LoxoneDoc) -> String {
    fingerprint(doc, 5)
}

/// Fingerprint version 4: version 5 without messages (`<LoggerMailer>`).
/// Accepted by `loxl drift` like [`semantic_fingerprint_v9`].
#[doc(hidden)] // older forms, for drift checks against old locks
pub fn semantic_fingerprint_v4(doc: &LoxoneDoc) -> String {
    fingerprint(doc, 4)
}

/// Fingerprint version 3: version 4 without Touch Pure Flex tile programs.
/// Accepted by `loxl drift` like [`semantic_fingerprint_v9`].
#[doc(hidden)] // older forms, for drift checks against old locks
pub fn semantic_fingerprint_v3(doc: &LoxoneDoc) -> String {
    fingerprint(doc, 3)
}

/// Fingerprint version 2: version 3 without program text (`Code=` on Code16).
/// Accepted by `loxl drift` like [`semantic_fingerprint_v9`].
#[doc(hidden)] // older forms, for drift checks against old locks
pub fn semantic_fingerprint_v2(doc: &LoxoneDoc) -> String {
    fingerprint(doc, 2)
}

/// Fingerprint version 1: version 2 without room/category bindings.
/// Accepted by `loxl drift` like [`semantic_fingerprint_v9`].
#[doc(hidden)] // older forms, for drift checks against old locks
pub fn semantic_fingerprint_v1(doc: &LoxoneDoc) -> String {
    fingerprint(doc, 1)
}

fn fingerprint(doc: &LoxoneDoc, version: u8) -> String {
    // Length-prefixed fields make the byte stream unambiguous.
    fn field(buf: &mut Vec<u8>, tag: u8, s: &str) {
        buf.push(tag);
        buf.extend_from_slice(&(s.len() as u64).to_le_bytes());
        buf.extend_from_slice(s.as_bytes());
    }
    // The domains keep the crate's old name: lockfiles store these fingerprints.
    let mut buf: Vec<u8> = match version {
        1 => b"lxir-fingerprint-v1".to_vec(),
        2 => b"lxir-fingerprint-v2".to_vec(),
        3 => b"lxir-fingerprint-v3".to_vec(),
        4 => b"lxir-fingerprint-v4".to_vec(),
        5 => b"lxir-fingerprint-v5".to_vec(),
        6 => b"lxir-fingerprint-v6".to_vec(),
        7 => b"lxir-fingerprint-v7".to_vec(),
        8 => b"lxir-fingerprint-v8".to_vec(),
        9 => b"lxir-fingerprint-v9".to_vec(),
        _ => b"lxir-fingerprint-v10".to_vec(),
    };
    let mut objects = doc.objects();
    objects.sort_by(|a, b| a.uuid.cmp(&b.uuid));
    for o in &objects {
        field(&mut buf, b'O', &o.uuid);
        field(&mut buf, b'T', &o.block_type);
        if !locale_suspect(o) {
            match &o.title {
                Some(t) => field(&mut buf, b't', t),
                None => buf.push(b'n'),
            }
        }
        // A port key without Def is indistinguishable from an absent key
        // in the diff, so only Def-carrying ports are hashed.
        for (key, def) in doc_ports(doc, o) {
            if let Some(def) = def {
                field(&mut buf, b'k', &key);
                field(&mut buf, b'd', &def);
            }
        }
        // Same attribute set and canonical form as the diff
        // (`diff_attrs`/`canonical_attr`): a projection extension only
        // moves the fingerprint of documents that carry the new
        // attribute — rule-free configs hash as before.
        for name in diff_attrs_v(&o.block_type, version) {
            if version < 3 && file_params(&o.block_type).contains(&name) {
                continue;
            }
            let value = setting_value(doc, o, name);
            if let Some(v) = canonical_attr(name, &value) {
                field(&mut buf, b'a', name);
                field(&mut buf, b'v', &v);
            }
        }
        if version >= 2 {
            for (attr, v) in placement(doc, o) {
                if let Some(v) = v {
                    field(&mut buf, b'p', attr);
                    field(&mut buf, b'v', &v);
                }
            }
        }
        if version >= 4 {
            for (key, v) in tile_functions(doc, o) {
                field(&mut buf, b'b', &key);
                field(&mut buf, b'v', &v);
            }
        }
        if version >= 5 {
            for (key, v) in message_fields(doc, o) {
                field(&mut buf, b'm', &key);
                field(&mut buf, b'v', &v);
            }
        }
        if version >= 6 {
            for ((_, mode), v) in schedule_columns(doc, o) {
                field(&mut buf, b's', &mode);
                field(&mut buf, b'v', &v);
            }
        }
        if version >= 7 {
            for (key, v) in crate::moods::mood_fields(doc, o) {
                field(&mut buf, b'l', &key);
                field(&mut buf, b'v', &v);
            }
        }
    }
    let wires: BTreeSet<WireView> = doc.wires().into_iter().collect();
    for w in &wires {
        field(&mut buf, b'w', &w.from_port);
        field(&mut buf, b'>', &w.to_port);
    }
    crate::lock::sha256_hex(&buf)
}

/// A program-text change (`Code=`) as a compact line diff instead of
/// two blobs: common leading and trailing lines are cut, the differing
/// middle shows as `-`/`+` lines, at most eight per side, each cut at
/// 100 characters. One contiguous hunk — enough to see what moved
/// without pulling in a diff algorithm.
pub fn text_changes(from: &str, to: &str) -> Vec<String> {
    const SHOWN: usize = 8;
    const WIDTH: usize = 100;
    let a: Vec<&str> = from.split('\n').collect();
    let b: Vec<&str> = to.split('\n').collect();
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (gone, new) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    let mut out = vec![format!(
        "{} -> {} lines; line {} on: {} removed, {} added",
        a.len(),
        b.len(),
        head + 1,
        gone.len(),
        new.len()
    )];
    for (sign, lines) in [('-', gone), ('+', new)] {
        for line in lines.iter().take(SHOWN) {
            let cut: String = line.chars().take(WIDTH).collect();
            let more = line.chars().count() - cut.chars().count();
            if more > 0 {
                out.push(format!("{sign} {cut}… (+{more} chars)"));
            } else {
                out.push(format!("{sign} {cut}"));
            }
        }
        if lines.len() > SHOWN {
            out.push(format!("{sign} … ({} more lines)", lines.len() - SHOWN));
        }
    }
    out
}

fn doc_ports(doc: &LoxoneDoc, obj: &ObjectSummary) -> BTreeMap<String, Option<String>> {
    doc.element_at(&obj.path)
        .map(|el| ports(el).into_iter().map(|p| (p.key, p.def)).collect())
        .unwrap_or_default()
}

/// Heuristic for titles Loxone Config rewrites on locale change: caption
/// folders, built-in system objects, and objects with deterministic
/// (`00000000-…`) system UUIDs.
fn locale_suspect(obj: &ObjectSummary) -> bool {
    obj.uuid.starts_with("00000000-")
        || obj.block_type.ends_with("Caption")
        || matches!(
            obj.block_type.as_str(),
            "Mode"
                | "SysVar"
                | "GlobalStates"
                | "WeatherData"
                | "Calendar"
                | "CalendarEntry"
                | "RemoteControls"
                | "MessageCenter"
                | "AutoPilot"
        )
}

/// The diff as a commit message: a subject line counting the changes
/// (prefixed with the pages when they sit on at most three), then the changes by
/// name — `Type "Title"`, ports as `"Title".Port`, rooms by title — instead
/// of UUIDs. Locale renames are counted, not listed: they are noise
/// ([`Rename::locale_suspect`]). Read-only and deterministic, for `loxl
/// diff --summary`.
pub fn summary(a: &LoxoneDoc, b: &LoxoneDoc, d: &DocDiff) -> String {
    let (va, vb) = (Names::new(a), Names::new(b));
    // Look an object up in the new document first, else the old one.
    let object = |uuid: &str| vb.object(uuid).or_else(|| va.object(uuid));
    let mut pages = BTreeSet::new();
    let mut touch = |names: &Names, uuid: &str| {
        if let Some(p) = names.page_of(uuid) {
            pages.insert(p.to_string());
        }
    };
    let mut body: Vec<(&str, Vec<String>)> = Vec::new();

    let listed = |objs: &[ObjectSummary], names: &Names| -> Vec<String> {
        objs.iter()
            .map(|o| match names.page_of(&o.uuid) {
                Some(p) => format!("{} (page {p:?})", names.name(o)),
                None => names.name(o),
            })
            .collect()
    };
    body.push(("Added", listed(&d.added, &vb)));
    body.push(("Removed", listed(&d.removed, &va)));
    d.added.iter().for_each(|o| touch(&vb, &o.uuid));
    d.removed.iter().for_each(|o| touch(&va, &o.uuid));

    let (locale, renames): (Vec<&Rename>, Vec<&Rename>) =
        d.renamed.iter().partition(|r| r.locale_suspect);
    let title = |t: &Option<String>| t.as_deref().map_or("(none)".into(), |t| format!("{t:?}"));
    body.push((
        "Renamed",
        renames
            .iter()
            .map(|r| {
                touch(&vb, &r.uuid);
                format!("{} {} -> {}", r.block_type, title(&r.from), title(&r.to))
            })
            .collect(),
    ));

    let value = |v: &Option<String>| v.clone().unwrap_or_else(|| "(unset)".into());
    let mut changed = Vec::new();
    for p in &d.param_changes {
        touch(&vb, &p.object_uuid);
        let who = object(&p.object_uuid).unwrap_or_else(|| p.block_type.clone());
        let (from, to) = (p.from.as_deref(), p.to.as_deref());
        let what = if p.port_key == "APC" {
            let n = match (
                from.and_then(crate::autopilot::parse),
                to.and_then(crate::autopilot::parse),
            ) {
                (Some(old), Some(new)) => crate::autopilot::changes(&old, &new).len(),
                _ => 1,
            };
            format!("rule changed ({n} field{})", if n == 1 { "" } else { "s" })
        } else if file_params(&p.block_type).contains(&p.port_key.as_str()) {
            let lines = text_changes(from.unwrap_or(""), to.unwrap_or(""));
            let count = |c: char| lines.iter().skip(1).filter(|l| l.starts_with(c)).count();
            format!(
                "{} changed (-{} +{} lines)",
                p.port_key,
                count('-'),
                count('+')
            )
        } else {
            format!("{}: {} -> {}", p.port_key, value(&p.from), value(&p.to))
        };
        changed.push(format!("{who} {what}"));
    }
    for p in &d.placements {
        touch(&vb, &p.object_uuid);
        let who = object(&p.object_uuid).unwrap_or_else(|| p.block_type.clone());
        let side = |t: &Option<String>, u: &Option<String>| match (t, u) {
            (Some(t), _) => format!("{t:?}"),
            (None, Some(u)) => u.clone(),
            (None, None) => "(none)".into(),
        };
        changed.push(format!(
            "{who} {}: {} -> {}",
            p.attr,
            side(&p.from_title, &p.from),
            side(&p.to_title, &p.to)
        ));
    }
    body.push(("Changed", changed));

    for (heading, list, names) in [
        ("Wires added", &d.wires_added, &vb),
        ("Wires removed", &d.wires_removed, &va),
    ] {
        let mut wires = Vec::new();
        for w in list {
            if let Some((owner, _)) = names.idx.port_owner.get(&w.to_port) {
                touch(names, owner);
            }
            wires.push(format!(
                "{} -> {}",
                names.port(&w.from_port),
                names.port(&w.to_port)
            ));
        }
        body.push((heading, wires));
    }
    // Users and groups name themselves; credentials stay fingerprints.
    body.push((
        "Users and access",
        d.access.iter().map(ToString::to_string).collect(),
    ));

    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let parts: Vec<String> = [
        (d.added.len(), "add", "block", "blocks"),
        (d.removed.len(), "remove", "block", "blocks"),
        (renames.len(), "rename", "block", "blocks"),
        (d.param_changes.len(), "change", "parameter", "parameters"),
        (
            d.placements.len(),
            "rebind",
            "room/category",
            "rooms/categories",
        ),
        (d.wires_added.len(), "add", "wire", "wires"),
        (d.wires_removed.len(), "remove", "wire", "wires"),
        (
            d.access.len(),
            "change",
            "user/access entry",
            "users/access entries",
        ),
    ]
    .into_iter()
    .filter(|(n, ..)| *n > 0)
    .map(|(n, verb, one, many)| format!("{verb} {}", plural(n, one, many)))
    .collect();
    let mut subject = if parts.is_empty() {
        "no semantic change".to_string()
    } else {
        parts.join(", ")
    };
    if (1..=3).contains(&pages.len()) && !parts.is_empty() {
        let pages: Vec<&str> = pages.iter().map(String::as_str).collect();
        subject = format!("{}: {subject}", pages.join(", "));
    }
    let mut out = subject;
    for (heading, lines) in body.iter().filter(|(_, l)| !l.is_empty()) {
        out.push_str(&format!("\n\n{heading}:"));
        for line in lines {
            out.push_str(&format!("\n- {line}"));
        }
    }
    if !locale.is_empty() {
        out.push_str(&format!(
            "\n\n{} locale rename(s) of built-in titles not listed.",
            locale.len()
        ));
    }
    out.push('\n');
    out
}

/// Object, port and page names of one document, for [`summary`].
struct Names {
    objects: BTreeMap<String, ObjectSummary>,
    idx: crate::doc::DocIndex,
    /// Page paths with their titles.
    pages: Vec<(Vec<usize>, String)>,
}

impl Names {
    fn new(doc: &LoxoneDoc) -> Names {
        let objects: BTreeMap<String, ObjectSummary> = doc
            .objects()
            .into_iter()
            .map(|o| (o.uuid.clone(), o))
            .collect();
        let pages = objects
            .values()
            .filter(|o| o.block_type == "Page")
            .map(|o| (o.path.clone(), o.title.clone().unwrap_or_default()))
            .collect();
        Names {
            objects,
            idx: doc.index(),
            pages,
        }
    }

    /// `Type "Title"`, else `Type IName`, else `Type <uuid>`.
    fn name(&self, o: &ObjectSummary) -> String {
        match (&o.title, &o.iname) {
            (Some(t), _) => format!("{} {t:?}", o.block_type),
            (None, Some(i)) => format!("{} {i}", o.block_type),
            (None, None) => format!("{} {}", o.block_type, o.uuid),
        }
    }

    fn object(&self, uuid: &str) -> Option<String> {
        self.objects.get(uuid).map(|o| self.name(o))
    }

    /// `"Title".Port` (falling back like [`Names::name`]), or the raw
    /// port UUID if it belongs to no object.
    fn port(&self, port: &str) -> String {
        let Some((owner, key)) = self.idx.port_owner.get(port) else {
            return port.to_string();
        };
        let label = match self.objects.get(owner) {
            Some(ObjectSummary { title: Some(t), .. }) => format!("{t:?}"),
            Some(ObjectSummary { iname: Some(i), .. }) => i.clone(),
            Some(o) => o.block_type.clone(),
            None => owner.clone(),
        };
        format!("{label}.{key}")
    }

    /// Title of the innermost page containing the object.
    fn page_of(&self, uuid: &str) -> Option<&str> {
        let path = &self.objects.get(uuid)?.path;
        self.pages
            .iter()
            .filter(|(p, _)| path.len() > p.len() && path.starts_with(p))
            .max_by_key(|(p, _)| p.len())
            .map(|(_, t)| t.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(title: &str, def: &str, wired: bool) -> LoxoneDoc {
        doc2(title, title, def, wired)
    }

    fn doc2(or_title: &str, cap_title: &str, def: &str, wired: bool) -> LoxoneDoc {
        let wire = if wired {
            "<In Input=\"00000009-0000-0001-00ff000000000009\"/>"
        } else {
            ""
        };
        let s = format!(
            "<ControlList Version=\"1\" NextObj=\"5\">\r\n\
             \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\">\r\n\
             \t\t<C Type=\"Page\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"P\">\r\n\
             \t\t\t<C Type=\"Or\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"{or_title}\">\r\n\
             \t\t\t\t<Co K=\"I2\" Def=\"{def}\" U=\"00000003-0000-0002-01ff000000000002\">{wire}</Co>\r\n\
             \t\t\t</C>\r\n\
             \t\t\t<C Type=\"ModeCaption\" U=\"00000000-0000-0004-1500000000000000\" Title=\"{cap_title}\"/>\r\n\
             \t\t</C>\r\n\
             \t</C>\r\n\
             </ControlList>\r\n"
        );
        LoxoneDoc::parse(s.as_bytes()).unwrap()
    }

    #[test]
    fn detects_rename_param_and_wire_changes() {
        let a = doc("Alt", "1", false);
        let b = doc("Neu", "2", true);
        let d = diff(&a, &b);
        assert!(d.added.is_empty() && d.removed.is_empty());
        assert_eq!(d.renamed.len(), 2);
        let or = d.renamed.iter().find(|r| r.block_type == "Or").unwrap();
        assert!(!or.locale_suspect);
        let cap = d
            .renamed
            .iter()
            .find(|r| r.block_type == "ModeCaption")
            .unwrap();
        assert!(cap.locale_suspect, "Caption + system UUID → locale noise");
        assert_eq!(d.param_changes.len(), 1);
        assert_eq!(d.param_changes[0].from.as_deref(), Some("1"));
        assert_eq!(d.param_changes[0].to.as_deref(), Some("2"));
        assert_eq!(d.wires_added.len(), 1);
        assert!(d.wires_removed.is_empty());
    }

    #[test]
    fn identical_docs_diff_empty() {
        let a = doc("X", "1", true);
        assert!(diff(&a, &a).is_empty());
    }

    #[test]
    fn autopilot_rule_edits_are_semantic() {
        // An app-edited Automatic Designer rule is a real change: the
        // whole rule lives in the `APC=` attribute, which the projection
        // compares like an attribute parameter.
        let doc_with = |apc: &str| {
            let s = format!(
                "<ControlList Version=\"1\" NextObj=\"5\">\r\n\
                 \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\">\r\n\
                 \t\t<C Type=\"Page\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"P\">\r\n\
                 \t\t\t<C Type=\"AutopilotRule\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"R\" APC=\"{apc}\" STA=\"1\"/>\r\n\
                 \t\t</C>\r\n\
                 \t</C>\r\n\
                 </ControlList>\r\n"
            );
            LoxoneDoc::parse(s.as_bytes()).unwrap()
        };
        let a = doc_with("{&quot;active&quot;:true}");
        let b = doc_with("{&quot;active&quot;:false}");
        let d = diff(&a, &b);
        assert_eq!(d.param_changes.len(), 1);
        assert_eq!(d.param_changes[0].port_key, "APC");
        assert_eq!(
            d.param_changes[0].from.as_deref(),
            Some("{\"active\":true}")
        );
        assert_ne!(semantic_fingerprint(&a), semantic_fingerprint(&b));
        assert!(diff(&a, &a).is_empty());

        // The raw payload text is not writer-stable (raw newlines inside
        // the JSON exist in the wild) — reformatting and key order are
        // not drift, only the parsed value is compared.
        let reflowed = doc_with("{ &quot;active&quot; :\r\ntrue }");
        assert!(diff(&a, &reflowed).is_empty());
        assert_eq!(semantic_fingerprint(&a), semantic_fingerprint(&reflowed));

        // A numeric reference is not a line break to Loxone but literal
        // text, which breaks the JSON — a real change.
        let escaped = doc_with("{ &quot;active&quot; :&#xD;&#xA;true }");
        assert_eq!(diff(&a, &escaped).param_changes.len(), 1);
    }

    #[test]
    fn fingerprint_tracks_the_diff_projection() {
        let base = doc2("X", "Cap", "1", true);
        let fp = semantic_fingerprint(&base);
        assert_eq!(fp.len(), 64);
        assert_eq!(fp, semantic_fingerprint(&doc2("X", "Cap", "1", true)));

        // A locale-suspect rename is save noise, not drift — but every
        // real change in the diff's projection moves the fingerprint.
        let locale = doc2("X", "Betriebsart", "1", true);
        let d = diff(&base, &locale);
        assert_eq!(d.renamed.len(), 1);
        assert!(d.renamed[0].locale_suspect);
        assert_eq!(fp, semantic_fingerprint(&locale));

        assert_ne!(fp, semantic_fingerprint(&doc2("Y", "Cap", "1", true)));
        assert_ne!(fp, semantic_fingerprint(&doc2("X", "Cap", "2", true)));
        assert_ne!(fp, semantic_fingerprint(&doc2("X", "Cap", "1", false)));
    }

    #[test]
    fn day_timer_schedules_read_in_source_terms() {
        // `Modes=` columns by index, `From` absent = 0, a value where the
        // timer is analog, a user mode by its title (lox#162).
        let timer = |entries: &str| {
            let s = format!(
                "<ControlList Version=\"1\" NextObj=\"5\">\r\n\
                 \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\">\r\n\
                 \t\t<C Type=\"Mode\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Party\"/>\r\n\
                 \t\t<C Type=\"Page\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"P\">\r\n\
                 \t\t\t<C Type=\"DayTimer\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"Heizung\" \
                 Analog=\"true\" N=\"3\" Modes=\"00000000-0000-0004-1500000000000000,\
                 00000000-0000-0009-1500000000000000,00000000-0000-000a-1500000000000000,\
                 00000005-0000-0000-ffff000000000001\">\r\n{entries}\
                 \t\t\t</C>\r\n\
                 \t\t</C>\r\n\
                 \t</C>\r\n\
                 </ControlList>\r\n"
            );
            LoxoneDoc::parse(s.as_bytes()).unwrap()
        };
        let a = timer(
            "<Entry To=\"360\" V=\"18\"/><Entry From=\"480\" To=\"1440\" Ix=\"1\" V=\"21\"/>\
             <Entry From=\"480\" To=\"1440\" Ix=\"2\" V=\"21\"/>",
        );
        let b = timer(
            "<Entry To=\"360\" V=\"18\"/><Entry From=\"540\" To=\"1440\" Ix=\"1\" V=\"21\"/>\
             <Entry From=\"540\" To=\"1440\" Ix=\"2\" V=\"21\"/>\
             <Entry From=\"1200\" To=\"1440\" Ix=\"3\" NeedActivate=\"true\" V=\"23\"/>",
        );
        let d = diff(&a, &b);
        let changes: Vec<(&str, Option<&str>, Option<&str>)> = d
            .param_changes
            .iter()
            .map(|p| (p.port_key.as_str(), p.from.as_deref(), p.to.as_deref()))
            .collect();
        assert_eq!(
            changes,
            [
                (
                    "schedule weekend",
                    Some("08:00-24:00 = 21"),
                    Some("09:00-24:00 = 21")
                ),
                (
                    "schedule \"Party\"",
                    Some("off"),
                    Some("20:00-24:00 = 23 activate")
                ),
            ]
        );
        assert_ne!(semantic_fingerprint(&a), semantic_fingerprint(&b));
        assert_eq!(semantic_fingerprint_v5(&a), semantic_fingerprint_v5(&b));

        // Reordering `Modes=` with the entries' indexes is the same
        // schedule.
        let reordered = |doc: &LoxoneDoc| {
            let text = String::from_utf8(doc.to_bytes()).unwrap();
            let text = text
                .replace(
                    "00000000-0000-0004-1500000000000000,00000000-0000-0009",
                    "00000000-0000-0009-1500000000000000,00000000-0000-0004",
                )
                .replace(
                    "<Entry To=\"360\" V=\"18\"/>",
                    "<Entry To=\"360\" Ix=\"1\" V=\"18\"/>",
                )
                .replace("Ix=\"1\" V=\"21\"", "V=\"21\"");
            LoxoneDoc::parse(text.as_bytes()).unwrap()
        };
        assert_ne!(a.to_bytes(), reordered(&a).to_bytes());
        assert!(diff(&a, &reordered(&a)).is_empty());
        assert_eq!(
            semantic_fingerprint(&a),
            semantic_fingerprint(&reordered(&a))
        );
    }

    #[test]
    fn schedule_day_groups() {
        let col = |days: &[usize], v: &str| -> BTreeMap<(usize, String), String> {
            days.iter()
                .map(|&n| ((n, MODE_DAYS[n - 1].to_string()), v.to_string()))
                .collect()
        };
        let all: Vec<usize> = (1..=10).collect();
        let keys = |b: BTreeMap<(usize, String), String>| -> Vec<String> {
            schedule_changes(&col(&all, "off"), &b)
                .into_iter()
                .map(|(k, ..)| k)
                .collect()
        };
        let mut b = col(&[1, 4, 5, 6, 7, 8, 9, 10], "x");
        b.extend(col(&[2, 3], "off"));
        assert_eq!(keys(b), ["schedule holiday daily"]);
        let mut b = col(&[4, 5, 6, 7, 8], "x");
        b.extend(col(&[1, 2, 3, 9, 10], "off"));
        assert_eq!(keys(b), ["schedule weekdays"]);
        let mut b = col(&[4, 6, 9, 10], "x");
        b.extend(col(&[1, 2, 3, 5, 7, 8], "off"));
        assert_eq!(keys(b), ["schedule mon wed weekend"]);
    }

    #[test]
    fn room_and_category_bindings_are_semantic() {
        let doc_in = |room: &str| {
            let s = format!(
                "<ControlList Version=\"1\" NextObj=\"5\">\r\n\
                 \t<C Type=\"Document\" U=\"00000001-0000-0000-ffff000000000001\">\r\n\
                 \t\t<C Type=\"Place\" U=\"00000005-0000-0000-ffff000000000001\" Title=\"Bad\"/>\r\n\
                 \t\t<C Type=\"Place\" U=\"00000006-0000-0000-ffff000000000001\" Title=\"Garten\"/>\r\n\
                 \t\t<C Type=\"LoxAIRDevice\" U=\"00000007-0000-0000-ffff000000000001\" Title=\"Sensor\">\r\n\
                 \t\t\t<IoData Pr=\"{room}\"/>\r\n\
                 \t\t</C>\r\n\
                 \t</C>\r\n\
                 </ControlList>\r\n"
            );
            LoxoneDoc::parse(s.as_bytes()).unwrap()
        };
        let a = doc_in("00000005-0000-0000-ffff000000000001");
        let b = doc_in("00000006-0000-0000-ffff000000000001");
        let d = diff(&a, &b);
        assert!(!d.is_empty());
        assert_eq!(d.placements.len(), 1);
        let p = &d.placements[0];
        assert_eq!((p.attr, p.title.as_deref()), ("room", Some("Sensor")));
        assert_eq!(p.from_title.as_deref(), Some("Bad"));
        assert_eq!(p.to_title.as_deref(), Some("Garten"));

        // v2 sees the move, v1 does not. `loxl drift` accepts a v1
        // fingerprint against the v1 form only.
        assert_ne!(semantic_fingerprint(&a), semantic_fingerprint(&b));
        assert_eq!(semantic_fingerprint_v1(&a), semantic_fingerprint_v1(&b));
        assert_ne!(semantic_fingerprint(&a), semantic_fingerprint_v1(&a));
    }
}
