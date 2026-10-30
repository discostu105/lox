//! LightController2 moods from source (D58).
//!
//! A `<controller>.moods = <set>` claim makes the controller's mood list
//! exactly the set's moods, in order, plus the built-in off mood
//! (`SID="778"`), which every controller keeps and source never lists.
//! A mood keeps its identity by name: one the controller already has
//! keeps its `UUID=`, `SID=`, `CID=` and flags (`Move=` — the motion
//! mood; `ReadOnly=`), only its circuit values are written. A new one is
//! minted the way Loxone Config numbers them — the lowest free `SID` from
//! 1 (776–778 are the built-ins), the lowest free `CID` above the off
//! mood's — and a mood the set leaves out is deleted, unless the
//! controller depends on it: a built-in (`SID` 776/777) or one
//! `<LSConfig>` names (`OpM=`/`ScMv=`) stays and is reported.
//!
//! The circuit values follow each circuit's kind, read off the existing
//! moods ([`crate::moods`]); a claim whose moods already match is a
//! no-op, byte for byte. The lock pins the UUID of each mood the compiler
//! minted until a base carries it ([`crate::lock::Lockfile::moods`]).
//!
//! A controller without moods — one minted from source, before any
//! Loxone Config save — first gets what that save would write: the stock
//! moods "Viel Licht" and "Aus" and their behaviour defaults ([`seed`]).

use super::ast::{Module, MoodLine, MoodsDecl};
use crate::doc::LoxoneDoc;
use crate::error::{Error, Result};
use crate::moods::{MoodValue, Moods, OFF_SID, encode};
use crate::uuid::Minter;
use crate::xml::{Element, Node};
use std::collections::{BTreeMap, BTreeSet};

/// The built-in moods source may list but never drop: the alarm-clock
/// mood (776) and "Viel Licht" (777, the motion mood of every observed
/// controller). 778, the off mood, is never listed at all.
const BUILTIN_SIDS: [&str; 3] = ["776", "777", OFF_SID];

/// Write every claimed mood set onto its controller in the compiled doc.
/// `uuid_of` maps extern and managed slugs to their object UUIDs; new
/// moods take their UUIDs from `minter`.
pub(crate) fn apply(
    doc: &mut LoxoneDoc,
    module: &Module,
    uuid_of: &BTreeMap<String, String>,
    pinned: &BTreeMap<String, BTreeMap<String, String>>,
    minter: &mut Minter,
) -> Result<BTreeMap<String, BTreeMap<String, String>>> {
    let mut minted: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    if module.moods_claims().next().is_none() {
        return Ok(minted);
    }
    let path_of: BTreeMap<String, Vec<usize>> = doc
        .objects()
        .iter()
        .map(|o| (o.uuid.clone(), o.path.clone()))
        .collect();
    let mut claimed: BTreeMap<&str, &str> = BTreeMap::new();
    for c in module.moods_claims() {
        let fail = |m: String| Err(Error::Compile(format!("`{c}`: {m}")));
        let uuid = uuid_of[&c.slug].as_str();
        if let Some(other) = claimed.insert(uuid, &c.slug) {
            return Err(Error::Compile(format!(
                "`{c}` and `{other}.moods = …` claim the moods of the same controller \
                 ({uuid}) — keep one"
            )));
        }
        let set = module
            .mood_sets()
            .find(|s| s.name == c.moods)
            .expect("validate admitted only declared mood sets");
        let el = doc
            .element_at_mut(&path_of[uuid])
            .expect("object path from objects()");
        if el.attr("Type") != Some("LightController2") {
            return fail(format!(
                "{uuid} is a {:?}, not a LightController2",
                el.attr("Type")
            ));
        }
        // A mood new to the controller takes the UUID an earlier compile
        // minted for it, else a fresh one; either way the lock keeps it.
        let lock_entry = minted.entry(uuid.to_string()).or_default();
        let mut mint = |name: &str| {
            let u = pinned
                .get(uuid)
                .and_then(|m| m.get(name))
                .cloned()
                .unwrap_or_else(|| minter.mint_object().to_string());
            lock_entry.insert(name.to_string(), u.clone());
            u
        };
        if Moods::read(el).is_none() {
            seed(el, &mut mint);
        }
        let have = Moods::read(el).expect("seeded");
        let want = values(set, &have).map_err(|m| Error::Compile(format!("`{c}`: {m}")))?;
        rewrite(el, &have, &want, &mut mint).map_err(|m| Error::Compile(format!("`{c}`: {m}")))?;
    }
    minted.retain(|_, m| !m.is_empty());
    Ok(minted)
}

/// What Loxone Config writes into a LightController2 on its first save
/// (oracle session 21, 2026-10-30, Loxone Config 17.2.8.28): the stock
/// moods "Viel Licht" (every circuit on, the motion mood) and "Aus", the
/// mood behaviour, presence and HCL defaults. A controller minted from
/// source has none of it until then; seeding it here lets a claim give
/// the controller its moods straight away. Missing children only — a
/// child already there is the controller's own.
fn seed(el: &mut Element, mint: &mut impl FnMut(&str) -> String) {
    let mut stock = |name: &str, flag: &str, sid: &str, cid: &str, q: &str| {
        let mut e = Element::new("LightsceneC");
        e.set_attr("Name", name);
        e.set_attr("UUID", &mint(name));
        e.set_attr(flag, "true");
        e.set_attr("Outputs", "18");
        e.set_attr("SID", sid);
        e.set_attr("CID", cid);
        for n in 1..=18 {
            e.set_attr(&format!("Q{n}"), q);
        }
        e
    };
    let mut scenes = Element::new("LightscenesC");
    scenes.set_attr("FC", "0040FFFF,008080FF,00FFFF80,0040FF40");
    scenes.set_attr("Outputs", "18");
    scenes.set_attr("Num", "2");
    scenes.push_child(stock("Viel Licht", "Move", "777", "22", "1"));
    scenes.push_child(stock("Aus", "ReadOnly", OFF_SID, "11", "0"));
    let with = |name: &str, attrs: &[(&str, &str)]| {
        let mut e = Element::new(name);
        for (k, v) in attrs {
            e.set_attr(k, v);
        }
        e
    };
    let defaults = [
        scenes,
        with(
            "LSConfig",
            &[("OpM", OFF_SID), ("ScMv", "777"), ("ScAc", "1")],
        ),
        with("IoData", &[("Visu", "true"), ("Rating", "1")]),
        with(
            "PSD",
            &[
                ("As", "-2"),
                ("Ae", "1380"),
                ("Amin", "1"),
                ("Amax", "4"),
                ("Dmin", "30"),
                ("Dmax", "180"),
                ("V", "20"),
            ],
        ),
        with(
            "HCL",
            &[
                ("T", "2"),
                ("M", "0"),
                ("TMIN", "2000"),
                ("TMAX", "6500"),
                ("FROM", "0"),
                ("TO", "0"),
            ],
        ),
        with("COHist", &[("TRK_NE", "20"), ("TRK_MAX", "100")]),
    ];
    for d in defaults {
        if !el.child_elements().any(|c| c.name == d.name) {
            el.push_child(d);
        }
    }
}

/// The set's moods as circuit values for this controller.
fn values<'a>(
    set: &'a MoodsDecl,
    have: &Moods,
) -> std::result::Result<Vec<(&'a str, Vec<u32>)>, String> {
    let prefixes = have.prefixes()?;
    let off = have.moods.iter().find(|m| m.is_off());
    set.moods()
        .map(|m| {
            if let Some(off) = off.filter(|o| o.name == m.name) {
                return Err(format!(
                    "mood \"{}\" is the controller's built-in off mood — it is always there \
                     and always off; leave it out of the set",
                    off.name
                ));
            }
            Ok((m.name.as_str(), circuit_values(m, &prefixes)?))
        })
        .collect()
}

fn circuit_values(m: &MoodLine, prefixes: &[u32]) -> std::result::Result<Vec<u32>, String> {
    let ctx = |s: &dyn std::fmt::Display| format!("mood \"{}\", `{s}`", m.name);
    let mut qs = Vec::with_capacity(prefixes.len());
    let all = m
        .settings
        .iter()
        .find(|s| s.circuit.is_none())
        .map_or(MoodValue::Off, |s| s.value);
    for (i, &prefix) in prefixes.iter().enumerate() {
        let explicit = m.settings.iter().find(|s| s.circuit == Some(i as u8 + 1));
        let q = match explicit {
            Some(s) => {
                encode(prefix, s.value).map_err(|e| format!("{}: AQ{} is {e}", ctx(s), i + 1))?
            }
            // `all` sets the circuits it fits: `all on` switches every
            // switch circuit on and leaves a colour circuit off unless the
            // mood names its colour.
            None => encode(prefix, all).unwrap_or(prefix << 28),
        };
        qs.push(q);
    }
    if let Some(s) = m
        .settings
        .iter()
        .find(|s| s.circuit.is_some_and(|n| usize::from(n) > prefixes.len()))
    {
        return Err(format!(
            "{}: the controller has {} circuits (AQ1 … AQ{})",
            ctx(s),
            prefixes.len(),
            prefixes.len()
        ));
    }
    Ok(qs)
}

/// Make the `<LightscenesC>` hold exactly `want` plus the off mood (and
/// whatever the controller cannot lose); nothing at all when it already
/// does.
fn rewrite(
    el: &mut Element,
    have: &Moods,
    want: &[(&str, Vec<u32>)],
    mint: &mut impl FnMut(&str) -> String,
) -> std::result::Result<(), String> {
    let current: Vec<(&str, &[u32])> = have
        .moods
        .iter()
        .filter(|m| !m.is_off())
        .map(|m| (m.name.as_str(), m.qs.as_slice()))
        .collect();
    let wanted: Vec<(&str, &[u32])> = want.iter().map(|(n, q)| (*n, q.as_slice())).collect();
    if current == wanted {
        return Ok(());
    }

    // `<LSConfig>` names moods by `SID`; read it before borrowing the
    // scenes mutably.
    let lsconfig: BTreeSet<String> = el
        .child_elements()
        .filter(|c| c.name == "LSConfig")
        .flat_map(|c| ["OpM", "ScMv"].into_iter().filter_map(|a| c.attr(a)))
        .map(String::from)
        .collect();
    let scenes = el
        .child_elements_mut()
        .find(|c| c.name == "LightscenesC")
        .expect("Moods::read found it");
    let old: Vec<Element> = scenes
        .child_elements()
        .filter(|c| c.name == "LightsceneC")
        .cloned()
        .collect();
    let is_off = |e: &Element| e.attr("SID") == Some(OFF_SID);
    let names: BTreeSet<&str> = want.iter().map(|(n, _)| *n).collect();
    let survives = |e: &Element| is_off(e) || names.contains(name_of(e).as_str());

    // A mood the controller depends on cannot go.
    for e in old.iter().filter(|e| !survives(e)) {
        let sid = e.attr("SID").unwrap_or("");
        let why = if BUILTIN_SIDS.contains(&sid) {
            "a built-in mood"
        } else if lsconfig.contains(sid) {
            "named by the controller's motion settings (`<LSConfig>`)"
        } else {
            continue;
        };
        return Err(format!(
            "mood \"{}\" is {why} (SID {sid}) and cannot be dropped — keep it in the set \
             (its values are yours to set)",
            name_of(e)
        ));
    }

    // Ids in use by the moods that stay; the built-in SIDs never go to a
    // new mood.
    let mut sids: BTreeSet<u32> = BUILTIN_SIDS.iter().filter_map(|s| s.parse().ok()).collect();
    let mut cids: BTreeSet<u32> = BTreeSet::new();
    for e in old.iter().filter(|e| survives(e)) {
        sids.extend(e.attr("SID").and_then(|s| s.parse::<u32>().ok()));
        cids.extend(e.attr("CID").and_then(|c| c.parse::<u32>().ok()));
    }
    let off_cid: u32 = old
        .iter()
        .find(|e| is_off(e))
        .and_then(|e| e.attr("CID"))
        .and_then(|c| c.parse().ok())
        .unwrap_or(7);

    let outputs = have.outputs.to_string();
    let mut moods: Vec<Element> = Vec::new();
    for (name, qs) in want {
        let mut m = match old.iter().find(|e| !is_off(e) && name_of(e) == *name) {
            Some(e) => e.clone(),
            None => {
                let sid = (1..).find(|s| !sids.contains(s)).expect("unbounded");
                sids.insert(sid);
                let cid = (off_cid + 1..)
                    .find(|c| !cids.contains(c))
                    .expect("unbounded");
                cids.insert(cid);
                let mut m = Element::new("LightsceneC");
                m.set_attr("Name", name);
                m.set_attr("UUID", &mint(name));
                m.set_attr("Outputs", &outputs);
                m.set_attr("SID", &sid.to_string());
                m.set_attr("CID", &cid.to_string());
                m
            }
        };
        for (i, q) in qs.iter().enumerate() {
            m.set_attr(&format!("Q{}", i + 1), &q.to_string());
        }
        moods.push(m);
    }
    // The off mood keeps its place: first stays first, anywhere else
    // goes last — where Loxone Config puts it.
    if let Some(pos) = old.iter().position(is_off) {
        let at = if pos == 0 { 0 } else { moods.len() };
        moods.insert(at, old[pos].clone());
    }
    scenes.set_attr("Num", &moods.len().to_string());
    scenes
        .children
        .retain(|n| !matches!(n, Node::Element(e) if e.name == "LightsceneC"));
    scenes.children.extend(moods.into_iter().map(Node::Element));
    scenes.self_closing = false;
    Ok(())
}

fn name_of(e: &Element) -> String {
    e.attr_decoded("Name").unwrap_or_default().into_owned()
}
