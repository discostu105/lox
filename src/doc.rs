//! Semantic read layer over a `.Loxone` document: objects, ports, wires,
//! counters. Everything here is a *view* over the lossless [`crate::xml`]
//! tree — mutations go through the tree so unknown content is never touched.

use crate::error::{Error, Result};
use crate::xml::{Element, Node, XmlDocument};
use std::collections::BTreeMap;

/// The `Next*` counters on the `<ControlList>` root. Loxone Config bumps
/// these on save; they must never decrease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counters {
    pub next_obj: u64,
    pub next_const: u64,
    pub next_note: u64,
    pub next_mem: u64,
}

/// One `<C>` object, summarized. `path` addresses the element inside the
/// XML tree (child indexes from the root, counting all nodes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectSummary {
    pub path: Vec<usize>,
    pub uuid: String,
    pub block_type: String,
    pub title: Option<String>,
    pub iname: Option<String>,
}

/// One `<Co>` connector of a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortView {
    pub key: String,
    pub uuid: String,
    /// Decoded `Def=` value (a parameter default), if present.
    pub def: Option<String>,
    /// Source port UUIDs of incoming wires (`<In Input=…/>` children).
    pub inputs: Vec<String>,
    /// The connector carries the GUI's `Inv=` input-inversion flag and is
    /// therefore **GUI-owned** (design decision D20): the rebuild carries
    /// the whole `<Co>` verbatim — flag, `Def=`, wires — and the IR is
    /// refused from wiring or setting it. Unwired, the idiom means
    /// "constant 1" (how the GUI encodes e.g. an enabled `Remanence`
    /// checkbox); wired, the wire's meaning is inverted and must not be
    /// restated in source.
    pub inv: bool,
}

/// A resolved wire: source and sink port UUIDs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WireView {
    pub from_port: String,
    pub to_port: String,
}

/// GUI-owned display attributes on block elements (design decision D19):
/// carried forward verbatim through a rebuild, allowed by adoption. They
/// follow `WF=` in element order. Evidence per entry (real config +
/// 2026-08-25 oracle runs): `Tp=` Memory subtype, `Sun=` AutoJalousie,
/// `SpStates=` visualization-state UUID lists (PushButton, AutoJalousie).
/// `NDOC=` block documentation text and the `StatsType=`/`StatsAutoDel=`
/// statistics settings (PushButton "WW-Boost" / two Memory blocks in the
/// real config). Growing this list takes evidence — an attribute is only
/// added here if re-emitting it verbatim cannot contradict what the
/// source expresses (which is why `Inv=`, input inversion, must never
/// appear here).
pub const GUI_OWNED_ATTRS: &[&str] = &[
    "Tp",
    "Sun",
    "SpStates",
    "NDOC",
    "StatsType",
    "StatsAutoDel",
    // PulseAt trigger config (2026-08-25 corpus): `Sec=` fire time in
    // seconds since midnight, `Typ=`/`AutP=` mode flags. Understood well
    // enough to carry, not yet to author — a future probe can promote
    // `Sec` to an attribute parameter.
    "Sec",
    "Typ",
    "AutP",
    // DayTimer schedule config: `Analog=`/`DefValue=`/`On=`/`Off=`
    // output behavior, `N=` count of `<Entry>` children (carried
    // together with them), `Modes=`/`UserModes=` operating-mode UUID
    // gating, `Desc=` description text. Like the `<Entry>` schedule
    // itself: GUI-authored logic the IR cannot express, gating *when*
    // the block runs — never modifying what a declared wire or param
    // means.
    "Analog",
    "DefValue",
    "N",
    "Modes",
    "On",
    "Off",
    "UserModes",
    "Desc",
    // The 2026-08-25 admission batch (LightController2, Switch2Button,
    // CentralShade/CentralLight, Code16). Corpus survey: each name below
    // appears on exactly one managed type — no cross-type collisions.
    // LightController2 per-circuit GUI state: `NameAI<n>=`/`CapAI<n>=`
    // circuit names and capability flags, `PresM<n>=` presence-mode
    // membership, `COName=` combined-output name, `T5P=` T5 pairing,
    // `uuidSeqencing=`/`uuidSeqenceIx=` scene-sequencing state. Only the
    // observed indexes are listed (0–4 / 0–2 / 0–4) — a sixth lighting
    // circuit would surface `NameAI5` as a refusal and gets added on that
    // evidence.
    "NameAI0",
    "NameAI1",
    "NameAI2",
    "NameAI3",
    "NameAI4",
    "CapAI0",
    "CapAI1",
    "CapAI2",
    "PresM0",
    "PresM1",
    "PresM2",
    "PresM3",
    "PresM4",
    "COName",
    "T5P",
    "uuidSeqencing",
    "uuidSeqenceIx",
    // CentralShade/CentralLight `rec=`: the UUID list of the blocks the
    // central controls, edited in the GUI's central dialog. Same class as
    // `Modes=`/`SpStates=` — a UUID-list the IR cannot express, gating
    // *which blocks* the GUI addresses, never what a declared wire or
    // param means.
    "rec",
    // Code16 (the PicoC program block): `Code=` is the program source,
    // `Task=` its scheduling mode. GUI-authored logic exactly like the
    // DayTimer `<Entry>` schedule — carried verbatim, never authored.
    // Promoting `Code` to an attribute parameter (the `Formula=`
    // precedent) is future work.
    "Code",
    "Task",
];

/// GUI-owned child elements of block elements (D19), same contract as
/// [`GUI_OWNED_ATTRS`]; they follow the `<Co>` children in element order.
/// `<IoData>` carries the visualization/room/category binding (`Visu=`,
/// `Pr=` place, `Cr=` category), `<Display>`/`<PSD>` visualization
/// settings, `<COHist>` AutoJalousie history settings, `<Entry>` the
/// DayTimer schedule entries (authored in the GUI's schedule editor —
/// real logic the IR cannot express, owned by the GUI like a room
/// binding). The four LightController2 subtrees are the complete child
/// inventory of all 45 corpus instances beyond `IoData`/`PSD`/`COHist`,
/// all GUI-authored — the DayTimer-`<Entry>` precedent exactly:
/// `<LightscenesC>` holds the lighting-scene definitions (`<LightsceneC>`
/// moods with per-circuit Q values, authored in the GUI's/app's scene
/// editor), `<LSConfig>` the scene-behavior settings (`OpM=`/`ScMv=`
/// reference the scenes' SIDs), `<HCL>` the human-centric-lighting
/// color-temperature curve, and `<SeqConf>` RGB color sequences (nested
/// `<seq0>`/`<Sequence>`/`<Col>`, carried wholesale).
pub const GUI_OWNED_CHILDREN: &[&str] = &[
    "IoData",
    "Display",
    "PSD",
    "COHist",
    "Entry",
    "LightscenesC",
    "LSConfig",
    "HCL",
    "SeqConf",
];

/// Lookup tables built in one pass over the document.
#[derive(Debug, Default)]
pub struct DocIndex {
    /// Object UUID → path of its `<C>` element.
    pub by_uuid: BTreeMap<String, Vec<usize>>,
    /// Port UUID → (owner object UUID, port key).
    pub port_owner: BTreeMap<String, (String, String)>,
    /// Port UUIDs of GUI-owned (`Inv=`-carrying) connectors — see
    /// [`PortView::inv`]. Wires into them are GUI content, never lifted.
    pub inv_ports: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoxoneDoc {
    pub xml: XmlDocument,
}

impl LoxoneDoc {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let xml = XmlDocument::parse(bytes)?;
        if xml.root.name != "ControlList" {
            return Err(Error::Structure(format!(
                "expected <ControlList> root, found <{}>",
                xml.root.name
            )));
        }
        Ok(LoxoneDoc { xml })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.xml.to_bytes()
    }

    pub fn counters(&self) -> Counters {
        let get = |name: &str| {
            self.xml
                .root
                .attr(name)
                .and_then(|v| v.parse().ok())
                .unwrap_or(1)
        };
        Counters {
            next_obj: get("NextObj"),
            next_const: get("NextConst"),
            next_note: get("NextNote"),
            next_mem: get("NextMem"),
        }
    }

    pub fn set_counters(&mut self, c: Counters) {
        let root = &mut self.xml.root;
        root.set_attr("NextObj", &c.next_obj.to_string());
        root.set_attr("NextConst", &c.next_const.to_string());
        root.set_attr("NextNote", &c.next_note.to_string());
        root.set_attr("NextMem", &c.next_mem.to_string());
    }

    /// The `<C Type="Document">` element.
    pub fn document(&self) -> Option<&Element> {
        self.xml
            .root
            .child_elements()
            .find(|e| e.name == "C" && e.attr("Type") == Some("Document"))
    }

    /// `ConfigVersion` of the document (e.g. `"17010727"`).
    pub fn config_version(&self) -> Option<String> {
        self.document()
            .and_then(|d| d.attr_decoded("ConfigVersion"))
            .map(|v| v.into_owned())
    }

    /// All `<C>` objects in document order (recursive).
    pub fn objects(&self) -> Vec<ObjectSummary> {
        let mut out = Vec::new();
        collect_objects(&self.xml.root, &mut Vec::new(), &mut out);
        out
    }

    pub fn element_at(&self, path: &[usize]) -> Option<&Element> {
        let mut cur = &self.xml.root;
        for &i in path {
            match cur.children.get(i)? {
                Node::Element(e) => cur = e,
                Node::Text(_) => return None,
            }
        }
        Some(cur)
    }

    pub fn element_at_mut(&mut self, path: &[usize]) -> Option<&mut Element> {
        let mut cur = &mut self.xml.root;
        for &i in path {
            match cur.children.get_mut(i)? {
                Node::Element(e) => cur = e,
                Node::Text(_) => return None,
            }
        }
        Some(cur)
    }

    /// Build the UUID lookup tables.
    pub fn index(&self) -> DocIndex {
        let mut idx = DocIndex::default();
        for obj in self.objects() {
            let el = self.element_at(&obj.path).expect("path from objects()");
            for port in ports(el) {
                if port.inv {
                    idx.inv_ports.insert(port.uuid.clone());
                }
                idx.port_owner
                    .insert(port.uuid.clone(), (obj.uuid.clone(), port.key.clone()));
            }
            idx.by_uuid.insert(obj.uuid.clone(), obj.path);
        }
        idx
    }

    /// All wires in the document, as (source port, sink port) UUID pairs.
    pub fn wires(&self) -> Vec<WireView> {
        let mut out = Vec::new();
        for obj in self.objects() {
            let el = self.element_at(&obj.path).expect("path from objects()");
            for port in ports(el) {
                for src in port.inputs {
                    out.push(WireView {
                        from_port: src,
                        to_port: port.uuid.clone(),
                    });
                }
            }
        }
        out
    }

    /// UUIDs carried by more than one `<C>` object or `<Co>` connector,
    /// sorted. Identity is the UUID, so a sound document has none
    /// ([loxone-format.md](../docs/loxone-format.md)); `<SObj>` entries
    /// reuse their object's UUID by design and are not counted.
    pub fn duplicate_uuids(&self) -> Vec<String> {
        fn walk<'a>(el: &'a Element, seen: &mut BTreeMap<&'a str, usize>) {
            for child in el.child_elements() {
                if matches!(child.name.as_str(), "C" | "Co")
                    && let Some(u) = child.attr("U")
                {
                    *seen.entry(u).or_default() += 1;
                }
                walk(child, seen);
            }
        }
        let mut seen = BTreeMap::new();
        walk(&self.xml.root, &mut seen);
        seen.into_iter()
            .filter(|&(_, n)| n > 1)
            .map(|(u, _)| u.to_string())
            .collect()
    }

    /// Refuse a document with duplicate UUIDs ([`Self::duplicate_uuids`]):
    /// every UUID-keyed step — extern matching, wiring, the semantic diff —
    /// would silently pick one twin. `role` names the document in the
    /// error ("base", "config").
    pub fn require_unique_uuids(&self, role: &str) -> Result<()> {
        let dups = self.duplicate_uuids();
        if dups.is_empty() {
            return Ok(());
        }
        Err(Error::Structure(format!(
            "{role} carries {} duplicate UUID(s) ({}{}) — two objects or \
             connectors share one identity, so wiring to either is \
             ambiguous. Open and save the config once in Loxone Config \
             (it keeps the first occurrence and re-mints the rest), then use \
             that save; docs/loxone-format.md names the known cause",
            dups.len(),
            dups.iter().take(3).cloned().collect::<Vec<_>>().join(", "),
            if dups.len() > 3 { ", …" } else { "" }
        )))
    }

    /// Remove the `<C>` object with this UUID from wherever it is in the
    /// tree. Returns the removed element.
    pub fn remove_by_uuid(&mut self, uuid: &str) -> Option<Element> {
        remove_object(&mut self.xml.root, uuid)
    }

    /// The `<C Type="Page">` element to place blocks on: by title when given,
    /// otherwise the first page in the document.
    pub fn page_path(&self, title: Option<&str>) -> Option<Vec<usize>> {
        self.objects()
            .into_iter()
            .find(|o| {
                o.block_type == "Page"
                    && match title {
                        Some(t) => o.title.as_deref() == Some(t),
                        None => true,
                    }
            })
            .map(|o| o.path)
    }
}

fn collect_objects(el: &Element, path: &mut Vec<usize>, out: &mut Vec<ObjectSummary>) {
    for (i, node) in el.children.iter().enumerate() {
        let Node::Element(child) = node else { continue };
        path.push(i);
        if child.name == "C"
            && let (Some(ty), Some(uuid)) = (child.attr("Type"), child.attr("U"))
        {
            out.push(ObjectSummary {
                path: path.clone(),
                uuid: uuid.to_string(),
                block_type: ty.to_string(),
                title: child.attr_decoded("Title").map(|t| t.into_owned()),
                iname: child.attr_decoded("IName").map(|t| t.into_owned()),
            });
        }
        collect_objects(child, path, out);
        path.pop();
    }
}

/// The `<Co>` connectors of a block element.
pub fn ports(el: &Element) -> Vec<PortView> {
    el.child_elements()
        .filter(|c| c.name == "Co")
        .filter_map(|co| {
            Some(PortView {
                key: co.attr_decoded("K")?.into_owned(),
                uuid: co.attr("U")?.to_string(),
                def: co.attr_decoded("Def").map(|d| d.into_owned()),
                inputs: co
                    .child_elements()
                    .filter(|i| i.name == "In")
                    .filter_map(|i| i.attr("Input").map(str::to_string))
                    .collect(),
                inv: co.attr("Inv").is_some(),
            })
        })
        .collect()
}

fn remove_object(el: &mut Element, uuid: &str) -> Option<Element> {
    let pos = el
        .children
        .iter()
        .position(|n| matches!(n, Node::Element(c) if c.name == "C" && c.attr("U") == Some(uuid)));
    if let Some(i) = pos {
        let Node::Element(removed) = el.children.remove(i) else {
            unreachable!()
        };
        // An element we emptied serializes self-closing, as Loxone writes
        // emptied containers. Only here: elements that were *always* empty
        // keep whatever form the source had (real configs contain
        // non-self-closing `<IoData></IoData>`).
        if el.children.is_empty() {
            el.self_closing = true;
        }
        return Some(removed);
    }
    for child in el.child_elements_mut() {
        if let Some(removed) = remove_object(child, uuid) {
            return Some(removed);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "<ControlList Version=\"1\" NextObj=\"5\" NextConst=\"1\" NextNote=\"1\" NextMem=\"1\">\r\n\
        \t<C Type=\"Document\" V=\"17010727\" U=\"00000001-0000-0000-ffff000000000001\" Title=\"T\" ConfigVersion=\"17010727\">\r\n\
        \t\t<C Type=\"Page\" V=\"175\" U=\"00000002-0000-0000-ffff000000000001\" Title=\"P1\">\r\n\
        \t\t\t<C Type=\"Or\" V=\"175\" U=\"00000003-0000-0000-ffff000000000001\" Title=\"O1\" Nio=\"3\">\r\n\
        \t\t\t\t<Co K=\"I1\" Nc=\"1\" U=\"00000003-0000-0001-00ff000000000002\">\r\n\
        \t\t\t\t\t<In Input=\"00000004-0000-0001-00ff000000000003\"/>\r\n\
        \t\t\t\t</Co>\r\n\
        \t\t\t\t<Co K=\"I2\" Def=\"1\" U=\"00000003-0000-0002-01ff000000000002\"/>\r\n\
        \t\t\t\t<Co K=\"Q\" U=\"00000003-0000-0003-02ff000000000002\"/>\r\n\
        \t\t\t</C>\r\n\
        \t\t</C>\r\n\
        \t</C>\r\n\
        </ControlList>\r\n";

    #[test]
    fn objects_ports_wires() {
        let doc = LoxoneDoc::parse(FIXTURE.as_bytes()).unwrap();
        let objs = doc.objects();
        assert_eq!(objs.len(), 3);
        assert_eq!(doc.counters().next_obj, 5);
        assert_eq!(doc.config_version().as_deref(), Some("17010727"));

        let or = objs.iter().find(|o| o.block_type == "Or").unwrap();
        let el = doc.element_at(&or.path).unwrap();
        let ports = ports(el);
        assert_eq!(ports.len(), 3);
        assert_eq!(ports[1].def.as_deref(), Some("1"));

        let wires = doc.wires();
        assert_eq!(wires.len(), 1);
        assert_eq!(wires[0].to_port, "00000003-0000-0001-00ff000000000002");

        let idx = doc.index();
        assert_eq!(
            idx.port_owner.get("00000003-0000-0003-02ff000000000002"),
            Some(&(
                "00000003-0000-0000-ffff000000000001".to_string(),
                "Q".to_string()
            ))
        );
    }

    #[test]
    fn remove_and_page() {
        let mut doc = LoxoneDoc::parse(FIXTURE.as_bytes()).unwrap();
        assert!(doc.page_path(Some("P1")).is_some());
        assert!(doc.page_path(Some("Nope")).is_none());
        let removed = doc
            .remove_by_uuid("00000003-0000-0000-ffff000000000001")
            .unwrap();
        assert_eq!(removed.attr("Type"), Some("Or"));
        assert_eq!(doc.objects().len(), 2);
    }
}
