//! Touch Pure Flex tile programs.
//!
//! A Touch Pure Flex is a `TreeDevice` whose `<btns>` child lists its
//! tiles: `<btn i="…">` with three function elements — `f1` (click),
//! `f2` (`bT="1"`, arrow up) and `f3` (`bT="2"`, arrow down). A function
//! carries `api=` (the command, e.g. `SET(As;Co;pulse)`), `cT=` (the
//! status text) and `ctrls=` (comma-separated UUIDs of the blocks the
//! command addresses); docs/loxone-format.md has the format and its
//! evidence. Lox Logic rewrites the functions of existing tiles only — layout,
//! icons and the tile list stay with Loxone Config.
//!
//! A command addresses blocks by abbreviation (`As` = AutoJalousie). The
//! device delivers it through its API connector to the blocks linked
//! there; `ctrls=` narrows it to the listed ones. Without `ctrls=` every
//! linked block of the type acts on it — two shades moving where one was
//! meant. Compile refuses targets that cannot receive a command; lint
//! reports the broadcast and the command nothing receives.

use super::ast::{BindingKind, Module, TileDecl, Value};
use super::lint::{LintFinding, LintKind};
use crate::doc::{LoxoneDoc, ObjectSummary, ports};
use crate::error::{Error, Result};
use crate::xml::{Attr, Element, Node, escape, unescape};
use std::collections::{BTreeMap, BTreeSet};

/// One function slot of a tile, in element order.
#[derive(Debug, Clone, Copy)]
pub struct FunctionSlot {
    /// The source key (`click:`, `up:`, `down:`).
    pub key: &'static str,
    /// The function element (`f1`, `f2`, `f3`).
    pub element: &'static str,
    /// The element's fixed `bT=` (button type), if any.
    pub bt: Option<&'static str>,
}

pub const FUNCTIONS: [FunctionSlot; 3] = [
    FunctionSlot {
        key: "click",
        element: "f1",
        bt: None,
    },
    FunctionSlot {
        key: "up",
        element: "f2",
        bt: Some("1"),
    },
    FunctionSlot {
        key: "down",
        element: "f3",
        bt: Some("2"),
    },
];

/// Command abbreviations whose block type real configs back. A `to:`
/// target of another type is refused.
const KNOWN: &[(&str, &[&str])] = &[("As", &["AutoJalousie"]), ("Lico", &["LightController2"])];

fn known_types(abbr: &str) -> Option<&'static [&'static str]> {
    KNOWN.iter().find(|(a, _)| *a == abbr).map(|(_, t)| *t)
}

/// What one function of a tile is programmed to, as source states it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TileFunction {
    pub api: Option<String>,
    pub text: Option<String>,
    /// `to:` target slugs, in source order.
    pub to: Vec<String>,
}

/// A tile's three functions after defaults and overrides are applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileProgram {
    pub index: u32,
    pub functions: [TileFunction; 3],
}

impl TileProgram {
    pub fn targets(&self) -> impl Iterator<Item = &str> {
        self.functions
            .iter()
            .flat_map(|f| f.to.iter().map(String::as_str))
    }
}

enum Key {
    Command(usize),
    /// `text:` (`None`) or `<fn>_text:`.
    Text(Option<usize>),
    /// `to:` (`None`) or `<fn>_to:`.
    To(Option<usize>),
}

fn parse_key(k: &str) -> Option<Key> {
    let slot = |f: &str| FUNCTIONS.iter().position(|s| s.key == f);
    match k {
        "text" => Some(Key::Text(None)),
        "to" => Some(Key::To(None)),
        _ => match k.split_once('_') {
            None => slot(k).map(Key::Command),
            Some((f, "text")) => slot(f).map(|i| Key::Text(Some(i))),
            Some((f, "to")) => slot(f).map(|i| Key::To(Some(i))),
            Some(_) => None,
        },
    }
}

/// Resolve a `tile` statement into its three functions. Tile-level
/// `text:` and `to:` apply to every function that has a command;
/// `<fn>_text:` / `<fn>_to:` replace them for one function
/// (`<fn>_text: ""` drops the text there). The error is the bare reason;
/// callers add the context.
pub fn program(tile: &TileDecl) -> std::result::Result<TileProgram, String> {
    let mut command: [Option<String>; 3] = Default::default();
    let mut text_all: Option<String> = None;
    let mut text: [Option<String>; 3] = Default::default();
    let mut to_all: Option<Vec<String>> = None;
    let mut to: [Option<Vec<String>>; 3] = Default::default();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for b in tile.bindings() {
        let k = b.port.as_str();
        let Some(key) = parse_key(k) else {
            return Err(format!(
                "unknown key `{k}:`. A tile takes `click:`, `up:`, `down:`, `text:` and `to:`, or `click_`/`up_`/`down_` + `text:`/`to:` for one function."
            ));
        };
        if let Key::To(slot) = key {
            let BindingKind::Param(Value::Ref(slug)) = &b.kind else {
                return Err(format!("`{k}:` takes a block slug (`{k}: <slug>`)"));
            };
            let list = match slot {
                None => to_all.get_or_insert_with(Vec::new),
                Some(i) => to[i].get_or_insert_with(Vec::new),
            };
            if list.contains(slug) {
                return Err(format!("`{k}: {slug}` is given twice"));
            }
            list.push(slug.clone());
            continue;
        }
        if !seen.insert(k) {
            return Err(format!("duplicate key `{k}:`"));
        }
        let value = match &b.kind {
            BindingKind::Param(Value::Str(s)) => s.clone(),
            // A command that reads as a number canonicalizes to one.
            BindingKind::Param(Value::Number(n)) => n.clone(),
            _ => return Err(format!("`{k}:` takes a quoted string (`{k}: \"…\"`)")),
        };
        match key {
            Key::Command(i) => {
                if value.is_empty() {
                    return Err(format!(
                        "`{k}: \"\"` is not a command. Leave the key out to clear the function."
                    ));
                }
                command[i] = Some(value);
            }
            Key::Text(None) => {
                if value.is_empty() {
                    return Err("`text: \"\"` is empty. Leave the key out instead.".into());
                }
                text_all = Some(value);
            }
            Key::Text(Some(i)) => text[i] = Some(value),
            Key::To(_) => unreachable!("handled above"),
        }
    }
    if command.iter().all(Option::is_none) {
        for (given, k) in [(text_all.is_some(), "text"), (to_all.is_some(), "to")] {
            if given {
                return Err(format!(
                    "`{k}:` without a command. Give `click:`, `up:` or `down:`, or write `tile {}()` to clear the tile.",
                    tile.index
                ));
            }
        }
    }
    let mut functions: [TileFunction; 3] = Default::default();
    for (i, slot) in FUNCTIONS.iter().enumerate() {
        let Some(api) = command[i].take() else {
            for (given, what) in [(text[i].is_some(), "text"), (to[i].is_some(), "to")] {
                if given {
                    return Err(format!(
                        "`{key}_{what}:` without `{key}:`. Add the `{key}:` command, since a function without one has no {what}.",
                        key = slot.key
                    ));
                }
            }
            continue;
        };
        functions[i] = TileFunction {
            api: Some(api),
            text: match text[i].take() {
                Some(t) if t.is_empty() => None,
                Some(t) => Some(t),
                None => text_all.clone(),
            },
            to: to[i].take().or_else(|| to_all.clone()).unwrap_or_default(),
        };
    }
    Ok(TileProgram {
        index: tile.index,
        functions,
    })
}

/// The block abbreviations a command addresses: the first argument of
/// every call with at least two (`SET(As;Co;pulse)` → `As`;
/// `WAIT(500)` names none), in order, without repeats.
pub fn abbreviations(api: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for (open, _) in api.match_indices('(') {
        let head = &api[..open];
        let name_len = head
            .bytes()
            .rev()
            .take_while(|b| b.is_ascii_alphanumeric())
            .count();
        if name_len == 0
            || !head[open - name_len..]
                .bytes()
                .any(|b| b.is_ascii_uppercase())
        {
            continue;
        }
        let args = &api[open + 1..];
        let Some(end) = args.find([';', ')']) else {
            continue;
        };
        let abbr = args[..end].trim();
        if args[end..].starts_with(';') && !abbr.is_empty() && !out.contains(&abbr) {
            out.push(abbr);
        }
    }
    out
}

/// The block types `api` can address, when every abbreviation in it is
/// one Lox Logic knows; `None` when any is unknown (or there is none).
fn addressed_types(api: &str) -> Option<Vec<&'static str>> {
    let abbrs = abbreviations(api);
    if abbrs.is_empty() {
        return None;
    }
    let mut types = Vec::new();
    for a in abbrs {
        types.extend(known_types(a)?);
    }
    Some(types)
}

/// The API wiring of one document: which blocks each Touch Pure Flex can
/// reach.
struct ApiLinks<'d> {
    doc: &'d LoxoneDoc,
    objects: Vec<ObjectSummary>,
    by_uuid: BTreeMap<String, usize>,
    /// Port UUID → (owner object index, port key).
    port_owner: BTreeMap<String, (usize, String)>,
    wires: Vec<(String, String)>,
}

impl<'d> ApiLinks<'d> {
    fn new(doc: &'d LoxoneDoc) -> Self {
        let objects = doc.objects();
        let mut by_uuid = BTreeMap::new();
        let mut port_owner = BTreeMap::new();
        for (i, o) in objects.iter().enumerate() {
            by_uuid.insert(o.uuid.clone(), i);
            let el = doc.element_at(&o.path).expect("path from objects()");
            for p in ports(el) {
                port_owner.insert(p.uuid, (i, p.key));
            }
        }
        let wires = doc
            .wires()
            .into_iter()
            .map(|w| (w.from_port, w.to_port))
            .collect();
        ApiLinks {
            doc,
            objects,
            by_uuid,
            port_owner,
            wires,
        }
    }

    fn element(&self, i: usize) -> &'d Element {
        self.doc
            .element_at(&self.objects[i].path)
            .expect("path from objects()")
    }

    /// Blocks API-linked to the device: their `OutputAPI` shares a wire
    /// with the device's `ApiActor.I`, or with the `AI` of an `OutputRef`
    /// mirroring that actor whose `AQ` distributes into its `I` (every
    /// GUI-made ref does; a ref without that leg reaches nothing). Either
    /// direction counts — API distribution wires are recorded in reverse
    /// (docs/loxone-format.md). Object index by UUID.
    fn linked(&self, device: usize) -> BTreeMap<&str, usize> {
        let actors: BTreeSet<&str> = self
            .element(device)
            .child_elements()
            .filter(|c| c.name == "C" && c.attr("Type") == Some("ApiActor"))
            .filter_map(|c| c.attr("U"))
            .collect();
        let owner = |port: &str| {
            self.port_owner
                .get(port)
                .map(|(i, key)| (*i, &self.objects[*i], key.as_str()))
        };
        let distributing: BTreeSet<usize> = self
            .wires
            .iter()
            .filter_map(|(from, to)| {
                let (_, actor, "I") = owner(to)? else {
                    return None;
                };
                let (i, r, "AQ") = owner(from)? else {
                    return None;
                };
                (r.block_type == "OutputRef"
                    && actors.contains(actor.uuid.as_str())
                    && self.element(i).attr("Ref") == Some(actor.uuid.as_str()))
                .then_some(i)
            })
            .collect();
        let entry_port = |port: &str| {
            owner(port).is_some_and(|(i, o, key)| {
                (key == "I" && actors.contains(o.uuid.as_str()))
                    || (key == "AI" && distributing.contains(&i))
            })
        };
        let api_owner = |port: &str| {
            self.port_owner
                .get(port)
                .filter(|(_, key)| key == "OutputAPI")
                .map(|(i, _)| *i)
        };
        let mut out = BTreeMap::new();
        for (from, to) in &self.wires {
            for (entry, other) in [(from, to), (to, from)] {
                if let Some(i) = api_owner(other)
                    && entry_port(entry)
                {
                    out.insert(self.objects[i].uuid.as_str(), i);
                }
            }
        }
        out
    }

    fn describe(&self, i: usize) -> String {
        let o = &self.objects[i];
        match &o.title {
            Some(t) => format!("{} {t:?}", o.block_type),
            None => format!("{} {}", o.block_type, o.uuid),
        }
    }
}

/// Child position (in `children`, as [`LoxoneDoc::element_at`] counts)
/// of the first element named `name`.
fn child_pos(el: &Element, name: &str, attr: Option<(&str, &str)>) -> Option<usize> {
    el.children.iter().position(|n| {
        matches!(n, Node::Element(e) if e.name == name
            && attr.is_none_or(|(k, v)| e.attr(k) == Some(v)))
    })
}

/// A function as it is to be written.
struct FunctionWrite {
    path: Vec<usize>,
    slot: FunctionSlot,
    api: Option<String>,
    text: Option<String>,
    ctrls: Option<String>,
}

/// Write every claimed tiles set onto its device (the compile phase).
/// `uuid_of` maps every extern and managed slug to its UUID in `doc`.
pub(crate) fn apply(
    doc: &mut LoxoneDoc,
    module: &Module,
    uuid_of: &BTreeMap<String, String>,
) -> Result<()> {
    if module.tiles_claims().next().is_none() {
        return Ok(());
    }
    let fail = |m: String| Err(Error::Compile(m));
    let mut writes: Vec<FunctionWrite> = Vec::new();
    {
        let links = ApiLinks::new(doc);
        let mut claimed: BTreeMap<&str, &str> = BTreeMap::new();
        for c in module.tiles_claims() {
            let device_uuid = uuid_of[&c.slug].as_str();
            if let Some(other) = claimed.insert(device_uuid, &c.slug) {
                return fail(format!(
                    "`{c}` and `{other}.tiles = …` claim the tiles of the same device \
                     ({device_uuid}). Keep one."
                ));
            }
            let device = links.by_uuid[device_uuid];
            let device_el = links.element(device);
            let Some(btns_pos) = child_pos(device_el, "btns", None) else {
                return fail(format!(
                    "`{c}`: {} has no Touch Pure Flex tile layout (no `<btns>`). Lay out the tiles in Loxone Config first.",
                    links.describe(device)
                ));
            };
            let Some(Node::Element(btns)) = device_el.children.get(btns_pos) else {
                unreachable!("child_pos found an element")
            };
            let linked = links.linked(device);
            let set = module
                .tiles_sets()
                .find(|t| t.name == c.tiles)
                .expect("validate admitted only declared sets");
            for tile in set.tiles() {
                let ctx = format!("`{c}`, tile {}", tile.index);
                let program = program(tile).map_err(|m| Error::Compile(format!("{ctx}: {m}")))?;
                let index = tile.index.to_string();
                let Some(btn_pos) = child_pos(btns, "btn", Some(("i", &index))) else {
                    let have: Vec<&str> = btns
                        .child_elements()
                        .filter(|e| e.name == "btn")
                        .filter_map(|e| e.attr("i"))
                        .collect();
                    return fail(format!(
                        "{ctx}: {} has no tile {} (it has {}). Add the tile in Loxone Config first.",
                        links.describe(device),
                        tile.index,
                        have.join(", ")
                    ));
                };
                let Some(Node::Element(btn)) = btns.children.get(btn_pos) else {
                    unreachable!("child_pos found an element")
                };
                for (slot, f) in FUNCTIONS.iter().zip(&program.functions) {
                    let ctx = format!("{ctx} {}", slot.key);
                    let Some(f_pos) = child_pos(btn, slot.element, None) else {
                        return fail(format!(
                            "{ctx}: the tile has no `<{}>`, a layout loxl does not know",
                            slot.element
                        ));
                    };
                    let Some(Node::Element(f_el)) = btn.children.get(f_pos) else {
                        unreachable!("child_pos found an element")
                    };
                    if f_el.attr("bT") != slot.bt {
                        return fail(format!(
                            "{ctx}: `<{}>` has bT={:?}, expected {:?}, a layout loxl does not know",
                            slot.element,
                            f_el.attr("bT"),
                            slot.bt
                        ));
                    }
                    let mut ctrls = Vec::new();
                    for slug in &f.to {
                        let uuid = uuid_of[slug].as_str();
                        let Some(&target) = linked.get(uuid) else {
                            return fail(format!(
                                "{ctx}: `to: {slug}` is not API-linked to `{}`. Wire its `OutputAPI` to the device's API connector: `OutputAPI: <api>.I`, an `OutputRef(mirrors: <api>)` whose `AQ` feeds `<api>.I`, or in Loxone Config.",
                                c.slug
                            ));
                        };
                        let ty = links.objects[target].block_type.as_str();
                        if let Some(api) = &f.api
                            && let Some(types) = addressed_types(api)
                            && !types.contains(&ty)
                        {
                            return fail(format!(
                                "{ctx}: `to: {slug}` is a {ty}, but `{api}` addresses {}",
                                types.join("/")
                            ));
                        }
                        ctrls.push(uuid);
                    }
                    let mut path = links.objects[device].path.clone();
                    path.extend([btns_pos, btn_pos, f_pos]);
                    writes.push(FunctionWrite {
                        path,
                        slot: *slot,
                        api: f.api.clone(),
                        text: f.text.clone(),
                        ctrls: (!ctrls.is_empty()).then(|| ctrls.join(",")),
                    });
                }
            }
        }
    }
    for w in writes {
        let el = doc.element_at_mut(&w.path).expect("planned path");
        write_function(el, &w);
    }
    Ok(())
}

/// Rewrite one function element: `api`, `cT`, `bT`, `ctrls` in that
/// order, unknown attributes carried after them. A value that does not
/// change keeps its bytes, and an untouched function keeps its form.
fn write_function(el: &mut Element, w: &FunctionWrite) {
    let known = |name: &str, value: &Option<String>| -> Option<Attr> {
        let v = value.as_ref()?;
        let raw = match el.attr(name) {
            Some(raw) if unescape(raw) == v.as_str() => raw.to_string(),
            _ => escape(v).into_owned(),
        };
        Some(Attr {
            name: name.to_string(),
            value: raw,
        })
    };
    let bt = w.slot.bt.map(|bt| Attr {
        name: "bT".into(),
        value: bt.into(),
    });
    let mut attrs: Vec<Attr> = [
        known("api", &w.api),
        known("cT", &w.text),
        bt,
        known("ctrls", &w.ctrls),
    ]
    .into_iter()
    .flatten()
    .collect();
    attrs.extend(
        el.attrs
            .iter()
            .filter(|a| !matches!(a.name.as_str(), "api" | "cT" | "bT" | "ctrls"))
            .cloned(),
    );
    if attrs != el.attrs {
        el.attrs = attrs;
        // Loxone writes an empty click function open/close
        // (`<f1></f1>`) and every attributed one self-closing.
        if el.children.is_empty() {
            el.self_closing = !el.attrs.is_empty();
        }
    }
}

/// Lint over every Touch Pure Flex in the compiled config, claimed or
/// not: commands that reach several blocks because they name none
/// ([`LintKind::TileBroadcast`]) and commands no linked block receives
/// ([`LintKind::TileDeadCommand`]). Only abbreviations Lox Logic knows are
/// judged. A device reads by its `uuid:`-pinned extern slug when the
/// module has one, by its title otherwise.
pub fn lint_tiles(module: &Module, compiled: &LoxoneDoc) -> Vec<LintFinding> {
    let links = ApiLinks::new(compiled);
    let pinned: BTreeMap<&str, &str> = module
        .externs()
        .filter_map(|e| match &e.match_spec {
            super::ast::MatchSpec::Uuid(u) => Some((u.as_str(), e.slug.as_str())),
            _ => None,
        })
        .collect();
    let mut findings = Vec::new();
    for (device, o) in links.objects.iter().enumerate() {
        let Some(btns) = links
            .element(device)
            .child_elements()
            .find(|c| c.name == "btns")
        else {
            continue;
        };
        let name = pinned
            .get(o.uuid.as_str())
            .map(|s| s.to_string())
            .or_else(|| o.title.clone())
            .unwrap_or_else(|| o.uuid.clone());
        let linked = links.linked(device);
        // Commands no linked block receives, one finding per device and
        // abbreviation: a device's tiles of one type share their cause.
        let mut dead: Vec<(String, &[&str], Vec<String>)> = Vec::new();
        for btn in btns.child_elements().filter(|e| e.name == "btn") {
            let i = btn.attr("i").unwrap_or("?");
            for slot in &FUNCTIONS {
                let Some(f) = btn.child_elements().find(|e| e.name == slot.element) else {
                    continue;
                };
                let Some(api) = f.attr_decoded("api") else {
                    continue;
                };
                let ctrls: BTreeSet<String> = f
                    .attr_decoded("ctrls")
                    .map(|c| {
                        c.split(',')
                            .map(|u| u.trim().to_string())
                            .filter(|u| !u.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                for abbr in abbreviations(&api) {
                    let Some(types) = known_types(abbr) else {
                        continue;
                    };
                    let receivers: Vec<usize> = linked
                        .iter()
                        .filter(|&(uuid, &idx)| {
                            types.contains(&links.objects[idx].block_type.as_str())
                                && (ctrls.is_empty() || ctrls.contains(*uuid))
                        })
                        .map(|(_, &idx)| idx)
                        .collect();
                    let what = format!("tile {i} {}: `{api}`", slot.key);
                    if receivers.is_empty() {
                        let what = if ctrls.is_empty() {
                            what
                        } else {
                            format!("{what} (to its ctrls)")
                        };
                        match dead.iter_mut().find(|(a, _, _)| *a == abbr) {
                            Some((_, _, list)) => list.push(what),
                            None => dead.push((abbr.to_string(), types, vec![what])),
                        }
                    } else if ctrls.is_empty() && receivers.len() >= 2 {
                        let names: Vec<String> =
                            receivers.iter().map(|&r| links.describe(r)).collect();
                        findings.push(LintFinding {
                            kind: LintKind::TileBroadcast,
                            slug: name.clone(),
                            detail: format!(
                                "{what} names no target, so all {} API-linked {} \
                                 blocks act on it ({}). List the intended ones with \
                                 `to:`.",
                                receivers.len(),
                                types.join("/"),
                                names.join(", ")
                            ),
                        });
                    }
                }
            }
        }
        for (abbr, types, list) in dead {
            let types = types.join("/");
            findings.push(LintFinding {
                kind: LintKind::TileDeadCommand,
                slug: name.clone(),
                detail: match list.as_slice() {
                    [one] => format!(
                        "{one} addresses {abbr}, but no {types} is API-linked to the device, so \
                         the command reaches nothing"
                    ),
                    _ => format!(
                        "{} commands address {abbr}, but no {types} is API-linked to the device, \
                         so they reach nothing: {}",
                        list.len(),
                        list.join(", ")
                    ),
                },
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviations_are_first_arguments_of_multi_argument_calls() {
        assert_eq!(abbreviations("SET(As;Co;pulse)"), ["As"]);
        assert_eq!(
            abbreviations("SET(Lico;M+;pulse)&WAIT(500)&MENU(Lico;Mood)"),
            ["Lico"]
        );
        assert_eq!(abbreviations("VALUESELECT(ACc;ϑt;;18;32;1;°C)"), ["ACc"]);
        assert_eq!(
            abbreviations("MENU(Sts;+;+;0:\"Off (x)\";1:\"Low\")"),
            ["Sts"]
        );
        assert!(abbreviations("WAIT(500)").is_empty());
        assert!(abbreviations("=ECHO(As)").is_empty());
    }

    #[test]
    fn addressed_types_need_every_abbreviation_known() {
        assert_eq!(
            addressed_types("SET(As;Co;pulse)"),
            Some(vec!["AutoJalousie"])
        );
        assert_eq!(addressed_types("SET(As;Co)&SET(Xy;Tg)"), None);
        assert_eq!(addressed_types("WAIT(500)"), None);
    }
}
