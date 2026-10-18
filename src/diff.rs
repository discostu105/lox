//! Semantic diff between two `.Loxone` documents, keyed by UUID.
//!
//! Its main job is telling *real* edits apart from noise, in particular the
//! locale renames: saving a config in a differently-localized Loxone Config
//! renames every built-in object (Modes, weather fields, caption folders …)
//! into the UI language — 111 renames in one observed save with zero
//! semantic change. [`Rename::locale_suspect`] flags those.

use crate::connectors::attr_params;
use crate::doc::{LoxoneDoc, ObjectSummary, WireView, ports};
use std::collections::{BTreeMap, BTreeSet};

/// Element attributes the diff projection compares for a block type: the
/// language-surface attribute parameters (`Formula=`), plus `APC=` on
/// `AutopilotRule` — the app-owned Automatic Designer rule payload.
/// `APC` is *not* an `attr_params` entry because it is not language
/// surface (the app and the Miniserver own those blocks; lxir never
/// writes them), but it is block logic stored in an attribute all the
/// same, and a diff blind to it would call an edited rule "semantically
/// empty" — the exact hole D18 closed for `Formula=`.
fn diff_attrs(block_type: &str) -> impl Iterator<Item = &'static str> {
    attr_params(block_type)
        .iter()
        .copied()
        .chain((block_type == "AutopilotRule").then_some("APC"))
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocDiff {
    pub added: Vec<ObjectSummary>,
    pub removed: Vec<ObjectSummary>,
    pub renamed: Vec<Rename>,
    pub param_changes: Vec<ParamChange>,
    /// Room/category binding changes (D39).
    pub placements: Vec<PlacementChange>,
    pub wires_added: Vec<WireView>,
    pub wires_removed: Vec<WireView>,
}

impl DocDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.renamed.is_empty()
            && self.param_changes.is_empty()
            && self.placements.is_empty()
            && self.wires_added.is_empty()
            && self.wires_removed.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
/// where every binding lives (docs/loxone-format.md). Since lxir can
/// rewrite them (D39), they are part of the projection: a diff blind to
/// them would call a moved device semantically empty.
fn placement(doc: &LoxoneDoc, o: &ObjectSummary) -> [(&'static str, Option<String>); 2] {
    let io = doc
        .element_at(&o.path)
        .and_then(|el| el.child_elements().find(|c| c.name == "IoData"));
    let get = |name: &str| io.and_then(|io| io.attr_decoded(name).map(|v| v.into_owned()));
    [("room", get("Pr")), ("category", get("Cr"))]
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
                for name in diff_attrs(&ob.block_type) {
                    let attr = |doc: &LoxoneDoc, o: &ObjectSummary| {
                        doc.element_at(&o.path)
                            .and_then(|el| el.attr_decoded(name).map(|v| v.into_owned()))
                    };
                    let (va, vb) = (attr(a, oa), attr(b, ob));
                    if canonical_attr(name, &va) != canonical_attr(name, &vb) {
                        out.param_changes.push(ParamChange {
                            object_uuid: uuid.clone(),
                            block_type: ob.block_type.clone(),
                            port_key: name.to_string(),
                            from: va,
                            to: vb,
                        });
                    }
                }
            }
        }
    }
    for (uuid, oa) in &objs_a {
        if !objs_b.contains_key(uuid) {
            out.removed.push(oa.clone());
        }
    }

    let wires_a: BTreeSet<WireView> = a.wires().into_iter().collect();
    let wires_b: BTreeSet<WireView> = b.wires().into_iter().collect();
    out.wires_added = wires_b.difference(&wires_a).cloned().collect();
    out.wires_removed = wires_a.difference(&wires_b).cloned().collect();
    out
}

/// Stable digest of exactly the projection [`diff`] compares: two
/// documents fingerprint-equal iff their diff is empty apart from
/// locale-suspect renames (suspect titles are left out, so re-saving a
/// config under another UI locale does not read as drift; neither do
/// position moves or the save fingerprint, which the projection never
/// sees). Recorded in the lockfile at adopt/compile; `lxir drift`
/// recomputes it over a fresh download to answer "did another writer
/// change something?" from one parse, no reference config needed.
pub fn semantic_fingerprint(doc: &LoxoneDoc) -> String {
    fingerprint(doc, true)
}

/// The fingerprint as lxir wrote it before room/category bindings joined
/// the projection (D39) — recorded in every lock compiled until then.
/// `lxir drift` accepts it so an upgrade alone does not read as drift; the
/// next compile records the current form.
pub fn semantic_fingerprint_v1(doc: &LoxoneDoc) -> String {
    fingerprint(doc, false)
}

fn fingerprint(doc: &LoxoneDoc, with_placement: bool) -> String {
    // Length-prefixed fields make the byte stream unambiguous.
    fn field(buf: &mut Vec<u8>, tag: u8, s: &str) {
        buf.push(tag);
        buf.extend_from_slice(&(s.len() as u64).to_le_bytes());
        buf.extend_from_slice(s.as_bytes());
    }
    let mut buf: Vec<u8> = if with_placement {
        b"lxir-fingerprint-v2".to_vec()
    } else {
        b"lxir-fingerprint-v1".to_vec()
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
        for name in diff_attrs(&o.block_type) {
            let value = doc
                .element_at(&o.path)
                .and_then(|el| el.attr_decoded(name).map(|v| v.into_owned()));
            if let Some(v) = canonical_attr(name, &value) {
                field(&mut buf, b'a', name);
                field(&mut buf, b'v', &v);
            }
        }
        if with_placement {
            for (attr, v) in placement(doc, o) {
                if let Some(v) = v {
                    field(&mut buf, b'p', attr);
                    field(&mut buf, b'v', &v);
                }
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
        let reflowed = doc_with("{ &quot;active&quot; :&#10;true }");
        assert!(diff(&a, &reflowed).is_empty());
        assert_eq!(semantic_fingerprint(&a), semantic_fingerprint(&reflowed));
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

        // v2 sees the move; v1 (what older lxir recorded, D21) does not —
        // `lxir drift` accepts a v1 fingerprint against the v1 form only.
        assert_ne!(semantic_fingerprint(&a), semantic_fingerprint(&b));
        assert_eq!(semantic_fingerprint_v1(&a), semantic_fingerprint_v1(&b));
        assert_ne!(semantic_fingerprint(&a), semantic_fingerprint_v1(&a));
    }
}
