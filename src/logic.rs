//! Thin adapter over `lxir` (§12.1): load a `.Loxone` config, index blocks and
//! wires by UUID, and answer "what is wired around this control?".
//!
//! Hardware never sits on a logic page directly: Loxone Config places an
//! `InputRef`/`OutputRef` there that mirrors the sensor or actuator. Wires are
//! *folded* through those refs, so a light controller's input reads
//! `Eingang 1 · Relais Wohnzimmer` instead of a ref named like the input.
//!
//! Only this file touches the lxir API, so churn in lxir (v0) stays here.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

use anyhow::{Result, bail};
use serde::Serialize;

/// A parameter: an unwired connector with a constant (`Def=`) value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Param {
    pub key: String,
    pub value: String,
    /// The value Loxone Config writes when the block is placed (lxir's
    /// corpus-observed defaults); views hide these unless asked
    pub default: bool,
}

/// One block (`<C>` object) of the config.
#[derive(Debug, Clone, Serialize)]
pub struct Block {
    pub uuid: String,
    #[serde(rename = "type")]
    pub typ: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iname: Option<String>,
    /// The logic page the block is placed on
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    /// The hardware device (Tree/Air device, extension) holding this input or output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Param>,
    /// `InputRef`/`OutputRef`: the object it mirrors
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mirrors: Option<String>,
    #[serde(skip)]
    has_ports: bool,
}

impl Block {
    pub fn is_ref(&self) -> bool {
        matches!(self.typ.as_str(), "InputRef" | "OutputRef")
    }

    /// Where the block lives, without its title: `device · room` for
    /// hardware, `page · room` for logic.
    pub fn context(&self) -> String {
        let place = self
            .device
            .as_deref()
            .or(self.page.as_deref().filter(|_| self.device.is_none()));
        [place, self.room.as_deref()]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// Parameters a reader should see: the non-default ones (or all).
    pub fn shown_params(&self, all: bool) -> impl Iterator<Item = &Param> {
        self.params.iter().filter(move |p| all || !p.default)
    }
}

/// One wire at the center block, seen from the center.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Wire {
    /// Connector key on the center block (`Mv`, `AQ1`)
    pub key: String,
    /// The block at the other end (refs folded: the real sensor/actuator)
    pub other: String,
    /// Connector key on the other block
    pub other_key: String,
    /// UUID of the wire's *source* connector: the streamed state UUID when the
    /// source block has a visualization
    pub source: String,
    /// The `InputRef`/`OutputRef` the wire was folded through
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// Inputs only: the other blocks the same source drives (fan-out)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Neighborhood {
    pub center: Block,
    pub inputs: Vec<Wire>,
    pub outputs: Vec<Wire>,
}

/// One node of a trace: a block reached through `depth` wires.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraceNode {
    pub depth: usize,
    pub block: String,
    /// Connector on this block the wire leaves (upstream) or enters (downstream)
    pub key: String,
    /// Connector on the parent node
    pub parent_key: String,
    /// The wire's source connector (for live values)
    pub source: String,
    /// Already shown above: a feedback loop, not expanded again
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cycle: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageInfo {
    pub title: String,
    pub blocks: usize,
    pub wires: usize,
}

/// A finding of [`Logic::lint`].
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub kind: &'static str,
    /// `warn` or `info`
    pub severity: &'static str,
    pub block: String,
    pub message: String,
}

/// A block as the wiring view names it: what it is and where it lives.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct BlockRef {
    pub uuid: String,
    pub title: String,
    #[serde(rename = "type")]
    pub typ: String,
    /// The connector the wire ends at on this block
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    /// Hardware: the device holding this input or output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

/// A parameter with its value for people (`900 s (15 min)`).
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct ParamView {
    pub key: String,
    pub value: String,
    pub text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
}

/// One wire of [`WiringView`].
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct WireView {
    /// Connector on the center block
    pub key: String,
    /// The block at the other end, hardware refs folded away
    pub block: BlockRef,
    /// Inputs only: other blocks the same source also drives
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<BlockRef>,
}

/// One node of a traced wire path.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct TraceView {
    /// Hops from the center block
    pub depth: usize,
    /// Connector on the parent node (the center block at depth 1)
    pub parent_key: String,
    pub block: BlockRef,
    /// A feedback loop: this block was already shown above
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cycle: bool,
}

/// Everything wired around one block, for `lox config wiring` and MCP.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct WiringView {
    pub block: BlockRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iname: Option<String>,
    pub params: Vec<ParamView>,
    pub inputs: Vec<WireView>,
    pub outputs: Vec<WireView>,
    /// Upstream to the sensors (with a trace)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<Vec<TraceView>>,
    /// Downstream to the actuators (with a trace)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downstream: Option<Vec<TraceView>>,
}

#[derive(Debug)]
pub struct Logic {
    pub blocks: HashMap<String, Block>,
    /// Block UUIDs in document order
    order: Vec<String>,
    /// Page titles in document order
    page_titles: Vec<String>,
    /// connector UUID → (block UUID, key)
    port_owner: HashMap<String, (String, String)>,
    /// sink connector → source connectors
    wires_in: HashMap<String, Vec<String>>,
    /// source connector → sink connectors
    wires_out: HashMap<String, Vec<String>>,
    /// block → its connector UUIDs (document order)
    ports: HashMap<String, Vec<(String, String)>>,
    /// raw wires (source, sink)
    raw_wires: Vec<(String, String)>,
    title_count: HashMap<String, usize>,
    doc: lxir::LoxoneDoc,
    sources: OnceLock<std::result::Result<Vec<(String, String)>, String>>,
}

#[derive(Default)]
struct Index {
    blocks: HashMap<String, Block>,
    order: Vec<String>,
    page_titles: Vec<String>,
    port_owner: HashMap<String, (String, String)>,
    wires_in: HashMap<String, Vec<String>>,
    wires_out: HashMap<String, Vec<String>>,
    ports: HashMap<String, Vec<(String, String)>>,
    raw_wires: Vec<(String, String)>,
    title_count: HashMap<String, usize>,
}

/// Object types that group others without being a device themselves.
fn is_container(typ: &str) -> bool {
    typ.ends_with("Caption") || matches!(typ, "Document" | "Program" | "Page")
}

/// Pure function blocks: a copy of one that drives nothing is dead logic.
const FUNCTION_BLOCKS: &[&str] = &[
    "And",
    "Or",
    "Not",
    "Xor",
    "Equal",
    "NotEqual",
    "Greater",
    "GreaterEqual",
    "Less",
    "LessEqual",
    "Formula",
    "Monoflop",
    "PulseGen",
    "PulseAt",
    "AnalogThresholdTrigger",
    "Add",
    "Subtract",
    "Multiply",
    "Divide",
    "Minimum",
    "Maximum",
];

/// The paired connector on a ref: what comes in on `AI` leaves on `AQ`.
fn ref_pair(key: &str) -> Option<&'static str> {
    Some(match key {
        "AI" => "AQ",
        "AQ" => "AI",
        "I" => "Q",
        "Q" => "I",
        _ => return None,
    })
}

/// The other input of a ref (`AI` ↔ `I`) or the other output (`AQ` ↔ `Q`).
fn ref_sibling(key: &str) -> Option<&'static str> {
    Some(match key {
        "AI" => "I",
        "I" => "AI",
        "AQ" => "Q",
        "Q" => "AQ",
        _ => return None,
    })
}

const FOLD_DEPTH: usize = 8;

impl Logic {
    /// The block owning a connector UUID (e.g. a streamed state UUID).
    #[cfg(test)]
    pub fn owner_of_port(&self, port: &str) -> Option<&str> {
        self.port_owner.get(port).map(|(b, _)| b.as_str())
    }

    pub fn parse(bytes: &[u8]) -> Result<Logic> {
        let doc = lxir::LoxoneDoc::parse(bytes).map_err(|e| anyhow::anyhow!("{}", e))?;
        let objs = doc.objects();
        let by_path: HashMap<&[usize], usize> = objs
            .iter()
            .enumerate()
            .map(|(i, o)| (o.path.as_slice(), i))
            .collect();
        let places: HashMap<&str, String> = objs
            .iter()
            .filter(|o| o.block_type == "Place")
            .map(|o| (o.uuid.as_str(), o.title.clone().unwrap_or_default()))
            .collect();
        let ancestors = |path: &[usize]| -> Vec<usize> {
            (0..path.len())
                .rev()
                .filter_map(|n| by_path.get(&path[..n]).copied())
                .collect()
        };
        // everything but the document itself; it moves in once indexed
        let mut l = Index::default();
        for obj in &objs {
            if obj.block_type == "Page" {
                l.page_titles
                    .push(obj.title.clone().unwrap_or_else(|| "Page".into()));
            }
            if matches!(obj.block_type.as_str(), "Document" | "Page") {
                continue;
            }
            let Some(el) = doc.element_at(&obj.path) else {
                continue;
            };
            let pv = lxir::doc::ports(el);
            let ports: Vec<(String, String)> =
                pv.iter().map(|p| (p.key.clone(), p.uuid.clone())).collect();
            for (k, u) in &ports {
                l.port_owner
                    .insert(u.clone(), (obj.uuid.clone(), k.clone()));
            }
            let params = pv
                .iter()
                .filter(|p| p.inputs.is_empty() && !p.inv)
                .filter_map(|p| {
                    let v = p.def.clone()?;
                    Some(Param {
                        default: lxir::observed_defaults::observed_default(&obj.block_type, &p.key)
                            == Some(v.as_str()),
                        key: p.key.clone(),
                        value: v,
                    })
                })
                .collect();
            let anc = ancestors(&obj.path);
            let page = anc
                .iter()
                .map(|&i| &objs[i])
                .find(|o| o.block_type == "Page")
                .map(|o| o.title.clone().unwrap_or_else(|| "Page".into()));
            let device = if page.is_some() {
                None
            } else {
                let first_caption = anc
                    .first()
                    .map(|&i| objs[i].block_type.as_str())
                    .unwrap_or("");
                anc.iter()
                    .map(|&i| &objs[i])
                    .find_map(|o| {
                        if o.block_type.ends_with("Caption") {
                            return None;
                        }
                        if is_container(&o.block_type) {
                            return Some(None);
                        }
                        // the Miniserver itself: only for its onboard inputs and outputs
                        if o.block_type == "LoxLIVE" {
                            let onboard = matches!(
                                first_caption,
                                "InputCaption"
                                    | "OutputCaption"
                                    | "AnalogInputCaption"
                                    | "AnalogOutputCaption"
                            );
                            return Some(onboard.then(|| o.title.clone().unwrap_or_default()));
                        }
                        Some(o.title.clone())
                    })
                    .flatten()
            };
            let room = el
                .child_elements()
                .find(|c| c.name == "IoData")
                .and_then(|io| io.attr("Pr"))
                .and_then(|pr| places.get(pr).cloned());
            let title = obj
                .title
                .clone()
                .filter(|t| !t.is_empty())
                .or(obj.iname.clone())
                .unwrap_or_else(|| obj.block_type.clone());
            let block = Block {
                uuid: obj.uuid.clone(),
                typ: obj.block_type.clone(),
                title,
                iname: obj.iname.clone(),
                page,
                room,
                device,
                params,
                mirrors: el.attr("Ref").map(str::to_string),
                has_ports: !ports.is_empty(),
            };
            if block.has_ports {
                *l.title_count.entry(block.title.clone()).or_default() += 1;
            }
            l.ports.insert(obj.uuid.clone(), ports);
            l.order.push(obj.uuid.clone());
            l.blocks.insert(obj.uuid.clone(), block);
        }
        for w in doc.wires() {
            l.wires_in
                .entry(w.to_port.clone())
                .or_default()
                .push(w.from_port.clone());
            l.wires_out
                .entry(w.from_port.clone())
                .or_default()
                .push(w.to_port.clone());
            l.raw_wires.push((w.from_port, w.to_port));
        }
        Ok(Logic {
            blocks: l.blocks,
            order: l.order,
            page_titles: l.page_titles,
            port_owner: l.port_owner,
            wires_in: l.wires_in,
            wires_out: l.wires_out,
            ports: l.ports,
            raw_wires: l.raw_wires,
            title_count: l.title_count,
            doc,
            sources: OnceLock::new(),
        })
    }

    // ── Names ───────────────────────────────────────────────────────────────

    /// The block's title, qualified with its device, page or room when another
    /// block has the same title (`Relais 1 (Relais Zählerkasten)`).
    pub fn label(&self, uuid: &str) -> String {
        let Some(b) = self.blocks.get(uuid) else {
            return uuid.to_string();
        };
        let dup = self.title_count.get(&b.title).copied().unwrap_or(0) > 1;
        match b
            .device
            .as_deref()
            .or(b.page.as_deref())
            .or(b.room.as_deref())
        {
            Some(q) if dup && !q.is_empty() && q != b.title => format!("{} ({})", b.title, q),
            _ => b.title.clone(),
        }
    }

    /// `label.Key` of a connector.
    pub fn port_label(&self, port: &str) -> String {
        match self.port_owner.get(port) {
            Some((b, k)) => format!("{}.{}", self.label(b), k),
            None => port.to_string(),
        }
    }

    /// The page a block belongs to for grouping: its page, or the page of the
    /// first block it is wired to.
    fn page_of(&self, uuid: &str) -> Option<&str> {
        self.blocks.get(uuid)?.page.as_deref()
    }

    fn port_of(&self, block: &str, key: &str) -> Option<&str> {
        self.ports
            .get(block)?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, u)| u.as_str())
    }

    fn is_ref_port(&self, port: &str) -> Option<(&str, &str)> {
        let (b, k) = self.port_owner.get(port)?;
        self.blocks
            .get(b)
            .filter(|bl| bl.is_ref())
            .map(|_| (b.as_str(), k.as_str()))
    }

    // ── Folding through refs ────────────────────────────────────────────────

    fn ref_wired_in(&self, block: &str, key: &str) -> Option<&Vec<String>> {
        self.port_of(block, key)
            .and_then(|p| self.wires_in.get(p))
            .filter(|s| !s.is_empty())
    }

    /// The sources feeding a ref's output `key`: its paired input, or the
    /// other input when that one is unwired (Config wires `AI` → `Q` when a
    /// digital page input reads an analog source).
    fn ref_input(&self, block: &str, key: &str) -> Option<&Vec<String>> {
        let pair = ref_pair(key)?;
        self.ref_wired_in(block, pair)
            .or_else(|| self.ref_wired_in(block, ref_sibling(pair)?))
    }

    /// The outputs a ref's input `key` feeds: its paired output, and the other
    /// output when that one's own input is unwired (inverse of `ref_input`).
    fn ref_outputs(&self, block: &str, key: &str) -> Vec<&str> {
        let mut out = Vec::new();
        if let Some(p) = ref_pair(key).and_then(|pk| self.port_of(block, pk)) {
            out.push(p);
        }
        if let Some(sib) = ref_sibling(key)
            && self.ref_wired_in(block, sib).is_none()
            && let Some(p) = ref_pair(sib).and_then(|pk| self.port_of(block, pk))
        {
            out.push(p);
        }
        out
    }

    /// The real source connectors behind `port`: through refs to the sensor.
    fn upstream(&self, port: &str, depth: usize) -> Vec<String> {
        if depth < FOLD_DEPTH
            && let Some((b, k)) = self.is_ref_port(port)
            && let Some(srcs) = self.ref_input(b, k)
        {
            return srcs
                .iter()
                .flat_map(|s| self.upstream(s, depth + 1))
                .collect();
        }
        vec![port.to_string()]
    }

    /// The real sink connectors behind `port`: through refs to the actuator.
    /// A ref whose output drives nothing leads nowhere (lint reports it).
    fn downstream(&self, port: &str, depth: usize) -> Vec<String> {
        if let Some((b, k)) = self.is_ref_port(port) {
            if depth >= FOLD_DEPTH {
                return vec![port.to_string()];
            }
            return self
                .ref_outputs(b, k)
                .into_iter()
                .filter_map(|out| self.wires_out.get(out))
                .flatten()
                .flat_map(|s| self.downstream(s, depth + 1))
                .collect();
        }
        vec![port.to_string()]
    }

    /// Folded sinks of a source connector.
    fn sinks_of(&self, port: &str) -> Vec<String> {
        let mut out = Vec::new();
        for s in self.wires_out.get(port).into_iter().flatten() {
            for d in self.downstream(s, 0) {
                if !out.contains(&d) {
                    out.push(d);
                }
            }
        }
        out
    }

    /// Every wire with refs folded out: (source, sink) connector pairs.
    pub fn folded_wires(&self) -> BTreeSet<(String, String)> {
        let mut out = BTreeSet::new();
        for (from, to) in &self.raw_wires {
            for s in self.upstream(from, 0) {
                for d in self.downstream(to, 0) {
                    if s != d {
                        out.insert((s.clone(), d));
                    }
                }
            }
        }
        out
    }

    /// A ref stands in for the object it mirrors.
    pub fn resolve_center<'a>(&'a self, uuid: &'a str) -> &'a str {
        match self.blocks.get(uuid) {
            Some(b) if b.is_ref() => b
                .mirrors
                .as_deref()
                .filter(|m| self.blocks.contains_key(*m))
                .unwrap_or(uuid),
            _ => uuid,
        }
    }

    /// Inputs and outputs of a block, one hop each way, refs folded.
    pub fn neighborhood(&self, block: &str) -> Option<Neighborhood> {
        let block = self.resolve_center(block);
        let center = self.blocks.get(block)?.clone();
        let mut inputs: Vec<Wire> = Vec::new();
        let mut outputs: Vec<Wire> = Vec::new();
        let via = |raw: &str, folded: &str| -> Option<String> {
            (raw != folded)
                .then(|| self.port_owner.get(raw).map(|(b, _)| b.clone()))
                .flatten()
        };
        for (key, port) in self.ports.get(block).into_iter().flatten() {
            for src in self.wires_in.get(port).into_iter().flatten() {
                for s in self.upstream(src, 0) {
                    let Some((ob, ok)) = self.port_owner.get(&s) else {
                        continue;
                    };
                    let mut also: Vec<String> = Vec::new();
                    for d in self.sinks_of(&s) {
                        if let Some((db, _)) = self.port_owner.get(&d)
                            && db != block
                            && !also.contains(db)
                        {
                            also.push(db.clone());
                        }
                    }
                    let w = Wire {
                        key: key.clone(),
                        other: ob.clone(),
                        other_key: ok.clone(),
                        source: s.clone(),
                        via: via(src, &s),
                        also,
                    };
                    if !inputs
                        .iter()
                        .any(|x| x.key == w.key && x.source == w.source)
                    {
                        inputs.push(w);
                    }
                }
            }
            for sink in self.wires_out.get(port).into_iter().flatten() {
                for d in self.downstream(sink, 0) {
                    let Some((ob, ok)) = self.port_owner.get(&d) else {
                        continue;
                    };
                    let w = Wire {
                        key: key.clone(),
                        other: ob.clone(),
                        other_key: ok.clone(),
                        source: port.clone(),
                        via: via(sink, &d),
                        also: Vec::new(),
                    };
                    if !outputs
                        .iter()
                        .any(|x| x.key == w.key && x.other == w.other && x.other_key == w.other_key)
                    {
                        outputs.push(w);
                    }
                }
            }
        }
        Some(Neighborhood {
            center,
            inputs,
            outputs,
        })
    }

    /// Follow the wires from `block` up to the sensors (`upstream`) or down
    /// to the actuators, `max_depth` hops at most. Feedback loops are cut.
    pub fn trace(&self, block: &str, upstream: bool, max_depth: usize) -> Vec<TraceNode> {
        let block = self.resolve_center(block);
        let mut out = Vec::new();
        let mut seen: HashSet<String> = HashSet::from([block.to_string()]);
        self.trace_rec(block, upstream, 1, max_depth, &mut seen, &mut out);
        out
    }

    fn trace_rec(
        &self,
        block: &str,
        upstream: bool,
        depth: usize,
        max_depth: usize,
        seen: &mut HashSet<String>,
        out: &mut Vec<TraceNode>,
    ) {
        if depth > max_depth || out.len() >= 500 {
            return;
        }
        let Some(n) = self.neighborhood(block) else {
            return;
        };
        let wires = if upstream { n.inputs } else { n.outputs };
        for w in wires {
            let cycle = !seen.insert(w.other.clone());
            out.push(TraceNode {
                depth,
                block: w.other.clone(),
                key: w.other_key.clone(),
                parent_key: w.key.clone(),
                source: w.source.clone(),
                cycle,
            });
            if !cycle {
                self.trace_rec(&w.other, upstream, depth + 1, max_depth, seen, out);
            }
        }
    }

    /// A block as views name it, with the connector a wire ends at.
    pub fn block_ref(&self, uuid: &str, key: Option<&str>) -> BlockRef {
        match self.blocks.get(uuid) {
            Some(b) => BlockRef {
                uuid: b.uuid.clone(),
                title: b.title.clone(),
                typ: b.typ.clone(),
                key: key.map(str::to_string),
                page: b.page.clone(),
                room: b.room.clone(),
                device: b.device.clone(),
            },
            None => BlockRef {
                uuid: uuid.to_string(),
                title: uuid.to_string(),
                typ: String::new(),
                key: key.map(str::to_string),
                page: None,
                room: None,
                device: None,
            },
        }
    }

    /// The wiring of a block as data: parameters (non-default ones unless
    /// `all_params`), inputs, outputs, and optionally traces `depth` hops.
    pub fn wiring_view(
        &self,
        uuid: &str,
        all_params: bool,
        trace: Option<(bool, bool, usize)>,
    ) -> Option<WiringView> {
        let n = self.neighborhood(uuid)?;
        let c = &n.center;
        let wire = |w: &Wire| WireView {
            key: w.key.clone(),
            block: self.block_ref(&w.other, Some(&w.other_key)),
            also: w.also.iter().map(|a| self.block_ref(a, None)).collect(),
        };
        let traced = |up: bool, depth: usize| -> Vec<TraceView> {
            self.trace(&c.uuid, up, depth)
                .into_iter()
                .map(|t| TraceView {
                    depth: t.depth,
                    parent_key: t.parent_key,
                    block: self.block_ref(&t.block, Some(&t.key)),
                    cycle: t.cycle,
                })
                .collect()
        };
        let (up, down) = match trace {
            Some((up, down, depth)) => (
                up.then(|| traced(true, depth)),
                down.then(|| traced(false, depth)),
            ),
            None => (None, None),
        };
        Some(WiringView {
            block: self.block_ref(&c.uuid, None),
            iname: c.iname.clone(),
            params: c
                .shown_params(all_params)
                .map(|p| ParamView {
                    key: p.key.clone(),
                    value: p.value.clone(),
                    text: param_text(&c.typ, &p.key, &p.value),
                    default: p.default,
                })
                .collect(),
            inputs: n.inputs.iter().map(wire).collect(),
            outputs: n.outputs.iter().map(wire).collect(),
            upstream: up,
            downstream: down,
        })
    }

    // ── Browsing ────────────────────────────────────────────────────────────

    /// Blocks a person can look at (no ref plumbing, no empty containers),
    /// in document order.
    pub fn browsable(&self) -> impl Iterator<Item = &Block> {
        self.order
            .iter()
            .filter_map(|u| self.blocks.get(u))
            .filter(|b| b.has_ports && !b.is_ref())
    }

    /// Search titles, inames, types, devices, rooms and pages. Every word of
    /// the query must match; exact titles first.
    pub fn search(&self, query: &str) -> Vec<&Block> {
        let q = query.trim().to_lowercase();
        let words: Vec<&str> = q.split_whitespace().collect();
        let mut hits: Vec<(u8, usize, &Block)> = Vec::new();
        for (i, b) in self.browsable().enumerate() {
            if b.uuid.eq_ignore_ascii_case(&q) {
                hits.push((0, i, b));
                continue;
            }
            let hay = format!(
                "{} {} {} {} {} {}",
                b.title,
                b.iname.as_deref().unwrap_or(""),
                b.typ,
                b.device.as_deref().unwrap_or(""),
                b.room.as_deref().unwrap_or(""),
                b.page.as_deref().unwrap_or("")
            )
            .to_lowercase();
            if !words.iter().all(|w| hay.contains(w)) {
                continue;
            }
            let t = b.title.to_lowercase();
            let rank = if t == q
                || b.iname
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&q))
            {
                1
            } else if t.starts_with(&q) {
                2
            } else if t.contains(&q) {
                3
            } else {
                4
            };
            hits.push((rank, i, b));
        }
        hits.sort_by_key(|(r, i, _)| (*r, *i));
        hits.into_iter().map(|(_, _, b)| b).collect()
    }

    /// One block by name: UUID, `Name [room/page/device]`, exact title or
    /// iname, or a unique substring. Ambiguity is an error listing candidates.
    pub fn resolve(&self, query: &str, room: Option<&str>) -> Result<&Block> {
        let query = query.trim();
        if let Some(b) = self.blocks.get(query) {
            return Ok(self.blocks.get(self.resolve_center(&b.uuid)).unwrap_or(b));
        }
        let (name, qual) = match query.strip_suffix(']').and_then(|q| q.rsplit_once('[')) {
            Some((n, q)) => (n.trim(), Some(q.trim().to_lowercase())),
            None => (query, None),
        };
        let qual = qual.or(room.map(str::to_lowercase));
        let lname = name.to_lowercase();
        let fits = |b: &Block| -> bool {
            let Some(q) = &qual else { return true };
            [&b.room, &b.page, &b.device]
                .into_iter()
                .flatten()
                .any(|s| s.to_lowercase().contains(q.as_str()))
        };
        let cands: Vec<&Block> = self
            .browsable()
            .filter(|b| {
                b.title.to_lowercase().contains(&lname)
                    || b.iname
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
            .filter(|b| fits(b))
            .collect();
        let exact: Vec<&Block> = cands
            .iter()
            .copied()
            .filter(|b| {
                b.title.eq_ignore_ascii_case(name)
                    || b.iname
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
            .collect();
        // logic on a page beats the hardware input of the same name
        let on_page: Vec<&Block> = exact.iter().copied().filter(|b| b.page.is_some()).collect();
        let pick = match (cands.len(), exact.len(), on_page.len()) {
            (1, _, _) => Some(cands[0]),
            (_, 1, _) => Some(exact[0]),
            (_, _, 1) => Some(on_page[0]),
            _ => None,
        };
        if let Some(b) = pick {
            return Ok(b);
        }
        if cands.is_empty() {
            bail!("no block matching '{}' in the config", query);
        }
        let list: Vec<String> = cands
            .iter()
            .take(12)
            .map(|b| format!("  {} — {} · {} · {}", b.title, b.typ, b.context(), b.uuid))
            .collect();
        bail!(
            "'{}' is ambiguous ({} blocks) — qualify it as 'Name [room, page or device]' or use the UUID:\n{}{}",
            query,
            cands.len(),
            list.join("\n"),
            if cands.len() > 12 { "\n  …" } else { "" }
        )
    }

    /// Logic pages with their block and wire counts, in document order.
    pub fn pages(&self) -> Vec<PageInfo> {
        let mut out: Vec<PageInfo> = self
            .page_titles
            .iter()
            .map(|t| PageInfo {
                title: t.clone(),
                blocks: 0,
                wires: 0,
            })
            .collect();
        for b in self.browsable() {
            if let Some(p) = out.iter_mut().find(|p| Some(&p.title) == b.page.as_ref()) {
                p.blocks += 1;
            }
        }
        for (s, d) in self.folded_wires() {
            let page_of_port = |p: &str| self.port_owner.get(p).and_then(|(b, _)| self.page_of(b));
            let page = page_of_port(&d).or_else(|| page_of_port(&s));
            if let Some(p) = out.iter_mut().find(|p| Some(p.title.as_str()) == page) {
                p.wires += 1;
            }
        }
        out
    }

    /// The blocks placed on a page (refs folded away), in document order.
    pub fn page_blocks(&self, page: &str) -> Vec<&Block> {
        self.browsable()
            .filter(|b| b.page.as_deref() == Some(page))
            .collect()
    }

    // ── lxir source view ────────────────────────────────────────────────────

    fn sources(&self) -> std::result::Result<&Vec<(String, String)>, String> {
        self.sources
            .get_or_init(|| {
                let opts = lxir::ir::DecompileOptions::default();
                let (pages, _) =
                    lxir::ir::decompile_pages(&self.doc, &opts).map_err(|e| e.to_string())?;
                Ok(pages
                    .into_iter()
                    .map(|mut p| {
                        self.annotate(&mut p.module);
                        (p.title, p.module.to_text())
                    })
                    .collect())
            })
            .as_ref()
            .map_err(|e| e.clone())
    }

    /// Externs name hardware by its title (`i3`): add where it is.
    fn annotate(&self, m: &mut lxir::ir::Module) {
        use lxir::ir::{Item, MatchSpec};
        for item in &mut m.items {
            let Item::Extern(e) = item else { continue };
            let room_ok = |b: &Block| e.room.is_none() || b.room == e.room;
            let found = match &e.match_spec {
                MatchSpec::Uuid(u) => self.blocks.get(u),
                MatchSpec::IName(n) => self.browsable().find(|b| {
                    b.typ == e.block_type && b.iname.as_deref() == Some(n.as_str()) && room_ok(b)
                }),
                MatchSpec::Title(t) => self
                    .browsable()
                    .find(|b| b.typ == e.block_type && &b.title == t && room_ok(b)),
                MatchSpec::Mirrors(_) => None,
            };
            let Some(b) = found else { continue };
            let mut ctx = Vec::new();
            if !matches!(e.match_spec, MatchSpec::Title(_)) {
                ctx.push(b.title.clone());
            }
            if let Some(d) = &b.device {
                ctx.push(d.clone());
            }
            if let Some(r) = b.room.as_ref().filter(|_| e.room.is_none()) {
                ctx.push(r.clone());
            }
            if ctx.is_empty() {
                continue;
            }
            let ctx = ctx.join(" · ");
            // lxir writes `#` + comment; its own comments bring the space
            e.comment = Some(match e.comment.take() {
                Some(c) if !c.trim().is_empty() && c.trim() != "periphery" => {
                    format!(" {} · {}", c.trim(), ctx)
                }
                _ => format!(" {}", ctx),
            });
        }
    }

    /// One page as lxir source text, hardware annotated with device and room.
    pub fn page_source(&self, page: &str) -> Result<String> {
        let src = self.sources().map_err(|e| anyhow::anyhow!("{}", e))?;
        src.iter()
            .find(|(t, _)| t == page)
            .map(|(_, s)| s.clone())
            .ok_or_else(|| anyhow::anyhow!("page '{}' has no logic lxir can show", page))
    }

    /// Every page with logic as lxir source text.
    pub fn all_sources(&self) -> Result<Vec<(String, String)>> {
        self.sources()
            .cloned()
            .map_err(|e| anyhow::anyhow!("{}", e))
    }

    // ── Lint ────────────────────────────────────────────────────────────────

    fn drives_anything(&self, block: &str) -> bool {
        self.ports
            .get(block)
            .into_iter()
            .flatten()
            .any(|(_, p)| !self.sinks_of(p).is_empty())
    }

    fn has_inputs(&self, block: &str) -> bool {
        self.ports
            .get(block)
            .into_iter()
            .flatten()
            .any(|(_, p)| self.wires_in.get(p).is_some_and(|v| !v.is_empty()))
    }

    /// Advisory checks: logic that drives nothing, inputs nobody reads,
    /// broken or unused refs, duplicate names in a room.
    pub fn lint(&self) -> Vec<Finding> {
        let mut out = Vec::new();
        for uuid in &self.order {
            let b = &self.blocks[uuid];
            if !b.has_ports {
                continue;
            }
            let t = b.typ.as_str();
            if b.page.is_some()
                && FUNCTION_BLOCKS.contains(&t)
                && self.has_inputs(uuid)
                && !self.drives_anything(uuid)
            {
                out.push(Finding {
                    kind: "dead-logic",
                    severity: "warn",
                    block: uuid.clone(),
                    message: format!("{} has inputs but its outputs drive nothing", t),
                });
            }
            if t.starts_with("VirtualIn") && t != "VirtualInCaption" && !self.drives_anything(uuid)
            {
                out.push(Finding {
                    kind: "unused-input",
                    severity: "info",
                    block: uuid.clone(),
                    message: "virtual input not used by any logic (the app may still show it)"
                        .into(),
                });
            }
            if b.is_ref() {
                match b.mirrors.as_deref() {
                    Some(m) if !self.blocks.contains_key(m) => out.push(Finding {
                        kind: "broken-ref",
                        severity: "warn",
                        block: uuid.clone(),
                        message: format!("{} points to an object that no longer exists", t),
                    }),
                    _ if t == "InputRef" && !self.drives_anything(uuid) => out.push(Finding {
                        kind: "unused-ref",
                        severity: "info",
                        block: uuid.clone(),
                        message: "input placed on the page but not wired to anything".into(),
                    }),
                    _ if t == "OutputRef" && !self.has_inputs(uuid) => out.push(Finding {
                        kind: "unused-ref",
                        severity: "info",
                        block: uuid.clone(),
                        message: "output placed on the page but nothing drives it".into(),
                    }),
                    _ => {}
                }
            }
        }
        // the same name twice in one room is ambiguous in the app and for voice
        let mut seen: HashMap<(String, String), Vec<&Block>> = HashMap::new();
        for b in self.browsable().filter(|b| b.page.is_some()) {
            if let Some(r) = &b.room {
                seen.entry((b.title.to_lowercase(), r.clone()))
                    .or_default()
                    .push(b);
            }
        }
        let mut dups: Vec<_> = seen.into_values().filter(|v| v.len() > 1).collect();
        dups.sort_by_key(|v| self.order.iter().position(|u| *u == v[0].uuid));
        for v in dups {
            out.push(Finding {
                kind: "duplicate-name",
                severity: "info",
                block: v[0].uuid.clone(),
                message: format!(
                    "{} blocks named '{}' in room {}",
                    v.len(),
                    v[0].title,
                    v[0].room.as_deref().unwrap_or("")
                ),
            });
        }
        out
    }

    // ── History ─────────────────────────────────────────────────────────────

    fn wire_set(&self, uuid: &str) -> Vec<(bool, String, String, String)> {
        let Some(n) = self.neighborhood(uuid) else {
            return Vec::new();
        };
        n.inputs
            .iter()
            .map(|w| (true, w.key.clone(), w.other.clone(), w.other_key.clone()))
            .chain(
                n.outputs
                    .iter()
                    .map(|w| (false, w.key.clone(), w.other.clone(), w.other_key.clone())),
            )
            .collect()
    }
}

/// What changed about one block between two snapshots, as diff lines.
pub fn block_changes(old: &Logic, new: &Logic, uuid: &str) -> Vec<String> {
    let (a, b) = (old.blocks.get(uuid), new.blocks.get(uuid));
    let mut out = Vec::new();
    match (a, b) {
        (None, None) => return out,
        (None, Some(b)) => {
            out.push(format!(
                "+ block  {} ({}){}",
                b.title,
                b.typ,
                b.page
                    .as_deref()
                    .map(|p| format!(" on page {}", p))
                    .unwrap_or_default()
            ));
            return out;
        }
        (Some(a), None) => {
            out.push(format!("- block  {} ({})", a.title, a.typ));
            return out;
        }
        (Some(a), Some(b)) => {
            if a.title != b.title {
                out.push(format!("~ rename {} → {}", a.title, b.title));
            }
            if a.page != b.page {
                out.push(format!(
                    "~ moved  page {} → {}",
                    a.page.as_deref().unwrap_or("—"),
                    b.page.as_deref().unwrap_or("—")
                ));
            }
            let keys: BTreeSet<&str> = a
                .params
                .iter()
                .chain(b.params.iter())
                .map(|p| p.key.as_str())
                .collect();
            for k in keys {
                let va = a
                    .params
                    .iter()
                    .find(|p| p.key == k)
                    .map(|p| p.value.as_str());
                let vb = b
                    .params
                    .iter()
                    .find(|p| p.key == k)
                    .map(|p| p.value.as_str());
                if va != vb {
                    let f = |v: Option<&str>| {
                        v.map(|v| param_text(&b.typ, k, v))
                            .unwrap_or_else(|| "—".into())
                    };
                    out.push(format!("~ param  {}  {} → {}", k, f(va), f(vb)));
                }
            }
        }
    }
    let (wa, wb) = (old.wire_set(uuid), new.wire_set(uuid));
    let fmt = |l: &Logic, (inp, k, o, ok): &(bool, String, String, String)| {
        if *inp {
            format!("{} ← {}.{}", k, l.label(o), ok)
        } else {
            format!("{} → {}.{}", k, l.label(o), ok)
        }
    };
    let added: Vec<String> = wb
        .iter()
        .filter(|w| !wa.contains(w))
        .map(|w| fmt(new, w))
        .collect();
    let mut removed: Vec<String> = wa
        .iter()
        .filter(|w| !wb.contains(w))
        .map(|w| fmt(old, w))
        .collect();
    for w in added {
        // the other end was re-created with a new UUID: same wire for people
        match removed.iter().position(|r| *r == w) {
            Some(i) => {
                removed.remove(i);
                out.push(format!("~ wire   {}  re-created", w));
            }
            None => out.push(format!("+ wire   {}", w)),
        }
    }
    out.extend(removed.into_iter().map(|w| format!("- wire   {}", w)));
    out
}

// ── Parameter units ─────────────────────────────────────────────────────────

/// Parameters known to be durations in seconds.
fn is_seconds(typ: &str, key: &str) -> bool {
    matches!(
        (typ, key),
        (
            "LightController2",
            "MoveOn" | "MoveIgnore" | "MoveTimeout" | "FadingTime" | "SceneMixTime"
        ) | ("AutoJalousie", "TimeEnd" | "TimeEndDown")
            | (
                "Monoflop" | "PButtonT" | "PulseAt" | "Switch2Button" | "PushButton",
                "Time"
            )
            | ("PulseGen", "TimeHigh" | "TimeLow")
            | ("Alarm", "Delay" | "MaxDurOut")
            | ("SmokeAlarm", "AlarmDelay" | "MaxDuration")
            | ("Irrigation", "MaxR")
            | (
                "HeatIRoomController2",
                "TWin" | "TimeC" | "TimeS" | "TimeMv"
            )
            | ("Intercom", "Timeout")
            | ("NfcCodeTouch" | "AnalogThresholdTrigger", "PulseTime")
    ) || (typ == "Irrigation" && key.starts_with("Tv"))
}

fn trim_num(v: f64) -> String {
    let s = format!("{:.1}", v);
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

/// A parameter value for people: `900` → `900 s (15 min)`.
pub fn param_text(typ: &str, key: &str, value: &str) -> String {
    let Ok(v) = value.parse::<f64>() else {
        return value.to_string();
    };
    if is_seconds(typ, key) {
        return if v >= 3600.0 {
            format!("{} s ({} h)", value, trim_num(v / 3600.0))
        } else if v >= 60.0 {
            format!("{} s ({} min)", value, trim_num(v / 60.0))
        } else {
            format!("{} s", value)
        };
    }
    if typ == "LightController2" && matches!(key, "DayMaxTemp" | "DayMinTemp") {
        return format!("{} K", value);
    }
    value.to_string()
}

// ── Semantic diff ───────────────────────────────────────────────────────────

/// One line of the semantic diff, with the block it is about.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiffLine {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
}

impl From<&str> for DiffLine {
    fn from(s: &str) -> Self {
        DiffLine {
            text: s.to_string(),
            block: None,
        }
    }
}

const PERIPHERY: &str = "(devices & periphery)";

/// Semantic config diff as human-readable lines (System › Config, `lox config
/// diff`, commit messages of `lox config pull`).
///
/// The first line (`= …`) sums up the change; then one `# Page` header per
/// config page with its changes (`+` added, `-` removed, `~` changed). Wires
/// are compared with refs folded, so re-placing an input on a page is not a
/// change and hardware reads as `Input · device`. Repeated identical lines
/// collapse into one with a `×N` count.
pub fn diff(old: &[u8], new: &[u8]) -> Result<Vec<DiffLine>> {
    let a = Logic::parse(old)?;
    let b = Logic::parse(new)?;
    Ok(diff_logic(&a, &b))
}

pub fn diff_lines(old: &[u8], new: &[u8]) -> Result<Vec<String>> {
    Ok(diff(old, new)?.into_iter().map(|l| l.text).collect())
}

pub fn diff_logic(a: &Logic, b: &Logic) -> Vec<DiffLine> {
    let d = lxir::diff::diff(&a.doc, &b.doc);
    let blk = |uuid: &str| b.blocks.get(uuid).or_else(|| a.blocks.get(uuid));
    let page = |uuid: &str| -> String {
        blk(uuid)
            .and_then(|x| x.page.clone())
            .unwrap_or_else(|| PERIPHERY.into())
    };
    let label = |uuid: &str| {
        if b.blocks.contains_key(uuid) {
            b.label(uuid)
        } else {
            a.label(uuid)
        }
    };
    let obj_label = |l: &Logic, o: &lxir::doc::ObjectSummary| -> String {
        let x = l.blocks.get(&o.uuid);
        let mut s = format!(
            "{} ({})",
            x.map(|x| x.title.clone())
                .or(o.title.clone())
                .or(o.iname.clone())
                .unwrap_or_default(),
            o.block_type
        );
        if let Some(dev) = x.and_then(|x| x.device.as_deref()) {
            s.push_str(&format!(" · {}", dev));
        }
        s
    };
    let skip = |typ: &str| matches!(typ, "InputRef" | "OutputRef" | "Document" | "Page");
    // page → lines, pages in order of first appearance
    let mut by_page: Vec<(String, Vec<DiffLine>)> = Vec::new();
    let mut push = |page: String, text: String, block: Option<String>| {
        let line = DiffLine { text, block };
        match by_page.iter_mut().find(|(p, _)| *p == page) {
            Some((_, v)) => v.push(line),
            None => by_page.push((page, vec![line])),
        }
    };
    for o in d.added.iter().filter(|o| !skip(&o.block_type)) {
        push(
            page(&o.uuid),
            format!("+ block  {}", obj_label(b, o)),
            Some(o.uuid.clone()),
        );
    }
    for o in d.removed.iter().filter(|o| !skip(&o.block_type)) {
        push(
            page(&o.uuid),
            format!("- block  {}", obj_label(a, o)),
            Some(o.uuid.clone()),
        );
    }
    for r in d.renamed.iter().filter(|r| !skip(&r.block_type)) {
        push(
            page(&r.uuid),
            format!(
                "~ rename {} → {}{}",
                r.from.clone().unwrap_or_default(),
                r.to.clone().unwrap_or_default(),
                if r.locale_suspect { "  (locale)" } else { "" }
            ),
            Some(r.uuid.clone()),
        );
    }
    for p in d.param_changes.iter().filter(|p| !skip(&p.block_type)) {
        let f = |v: &Option<String>| {
            v.as_deref()
                .map(|v| param_text(&p.block_type, &p.port_key, v))
                .unwrap_or_else(|| "—".into())
        };
        push(
            page(&p.object_uuid),
            format!(
                "~ param  {} · {}  {} → {}",
                label(&p.object_uuid),
                p.port_key,
                f(&p.from),
                f(&p.to)
            ),
            Some(p.object_uuid.clone()),
        );
    }
    // wires, refs folded: re-placed inputs and ref plumbing are no change
    let (wa, wb) = (a.folded_wires(), b.folded_wires());
    let owner = |l: &Logic, p: &str| l.port_owner.get(p).map(|(o, _)| o.clone());
    let wire_line = |l: &Logic, (s, t): &(String, String), sign: char| {
        let (so, to) = (owner(l, s), owner(l, t));
        // grouped under the logic end of the wire
        let anchor = to
            .clone()
            .filter(|o| l.page_of(o).is_some())
            .or(so.clone().filter(|o| l.page_of(o).is_some()))
            .or(to)
            .or(so);
        let pg = anchor
            .as_deref()
            .and_then(|o| l.page_of(o))
            .map(str::to_string)
            .unwrap_or_else(|| PERIPHERY.into());
        (
            pg,
            format!("{} wire   {} → {}", sign, l.port_label(s), l.port_label(t)),
            anchor,
        )
    };
    for w in wb.difference(&wa) {
        let (p, t, o) = wire_line(b, w, '+');
        push(p, t, o);
    }
    for w in wa.difference(&wb) {
        let (p, t, o) = wire_line(a, w, '-');
        push(p, t, o);
    }
    let mut out: Vec<DiffLine> = Vec::new();
    if by_page.is_empty() {
        return out;
    }
    // totals: filled from the collapsed lines, so re-created pairs count once
    let mut tot = [0usize; 8];
    // loose blocks and wires last
    by_page.sort_by_key(|(p, _)| p.starts_with('('));
    for (page, lines) in by_page {
        out.push(DiffLine {
            text: format!("# {}", page),
            block: None,
        });
        // collapse repeats, keeping first-appearance order
        let mut seen: Vec<(DiffLine, usize)> = Vec::new();
        for l in lines {
            match seen.iter_mut().find(|(s, _)| s.text == l.text) {
                Some((_, n)) => *n += 1,
                None => seen.push((l, 1)),
            }
        }
        // the same thing removed and added again (Config re-created it with
        // new UUIDs): one "re-created" line instead of a + and a − block
        let count = |seen: &[(DiffLine, usize)], l: &str| {
            seen.iter()
                .find(|(s, _)| s.text == l)
                .map_or(0, |(_, n)| *n)
        };
        let mut shown: Vec<(DiffLine, usize)> = Vec::new();
        for (l, n) in &seen {
            let Some(rest) = l.text.strip_prefix("+ ") else {
                if let Some(rest) = l.text.strip_prefix("- ") {
                    let m = count(&seen, &format!("+ {}", rest)).min(*n);
                    if *n > m {
                        shown.push((l.clone(), n - m));
                    }
                } else {
                    shown.push((l.clone(), *n));
                }
                continue;
            };
            let m = count(&seen, &format!("- {}", rest)).min(*n);
            if m > 0 {
                shown.push((
                    DiffLine {
                        text: format!("~ {}  re-created", rest),
                        block: l.block.clone(),
                    },
                    m,
                ));
            }
            if *n > m {
                shown.push((l.clone(), n - m));
            }
        }
        for (mut l, n) in shown {
            let t = &l.text;
            // re-created wires follow their blocks: not counted on their own
            let k = if t.ends_with("re-created") {
                if t.starts_with("~ block") { 4 } else { 7 }
            } else if t.starts_with("+ block") {
                0
            } else if t.starts_with("- block") {
                1
            } else if t.starts_with("~ rename") {
                2
            } else if t.starts_with("~ param") {
                3
            } else if t.starts_with("+ wire") {
                5
            } else {
                6
            };
            tot[k] += n;
            if n > 1 {
                l.text = format!("{}  ×{}", l.text, n);
            }
            out.push(l);
        }
    }
    let mut parts = Vec::new();
    for (k, what) in ["added", "removed", "renamed", "parameters", "re-created"]
        .iter()
        .enumerate()
    {
        if tot[k] > 0 {
            let what = if tot[k] == 1 && *what == "parameters" {
                "parameter"
            } else {
                what
            };
            parts.push(format!("{} {}", tot[k], what));
        }
    }
    if tot[5] + tot[6] > 0 {
        parts.push(format!("wires +{} −{}", tot[5], tot[6]));
    }
    if parts.is_empty() {
        // only re-created wires: nothing a person changed
        parts.push("re-created wires only".into());
    }
    out.insert(
        0,
        DiffLine {
            text: format!("= {}", parts.join(" · ")),
            block: None,
        },
    );
    out
}

// the fixtures are the demo house of the TUI
#[cfg(all(test, feature = "tui"))]
mod tests {
    use super::*;
    use crate::tui::demo;
    use crate::tui::model::House;

    fn demo_logic() -> (House, Logic) {
        let st = demo::structure();
        let h = House::from_structure(&st);
        let l = Logic::parse(demo::loxone_xml(&st).as_bytes()).unwrap();
        (h, l)
    }

    #[test]
    fn neighborhood_resolves_control_block_and_live_wires() {
        let (h, l) = demo_logic();
        let c = h.resolve("Hallway light", None).unwrap();
        // control UUID == block UUID
        let n = l.neighborhood(&h.ctrls[c].uuid).unwrap();
        assert_eq!(n.center.title, "Hallway light");
        assert_eq!(n.inputs.len(), 4);
        assert_eq!(n.outputs.len(), 3);
        // live/dashed classification: a source connector is live iff it is a streamed state
        let live: Vec<bool> = n
            .inputs
            .iter()
            .map(|w| h.state_owner.contains_key(&w.source))
            .collect();
        assert_eq!(live, [true, true, false, true]);
        let motion = h.resolve("Motion", None).unwrap();
        let mv = n.inputs.iter().find(|w| w.key == "Mv").unwrap();
        assert_eq!(mv.other, h.ctrls[motion].uuid);
        // a streamed state's owner block is the control
        let spots = h.ctrls[c].subs[0];
        let pos = h.ctrls[spots].state("position").unwrap();
        assert_eq!(l.owner_of_port(pos), Some(h.ctrls[c].uuid.as_str()));
    }

    /// Loxone Config may wire the hardware into a ref's AI while the page wire
    /// leaves from Q: the fold takes the wired sibling input, and the sibling
    /// output only counts while its own input is unwired.
    #[test]
    fn refs_fold_across_analog_and_digital_ports() {
        let st = demo::structure();
        let xml = demo::loxone_xml(&st);
        let at = xml.find("Type=\"InputRef\"").unwrap();
        let aq = at + xml[at..].find("K=\"AQ\"").unwrap();
        let q = format!("{}K=\"Q\"{}", &xml[..aq], &xml[aq + "K=\"AQ\"".len()..]);
        let (h, _) = demo_logic();
        let l = Logic::parse(q.as_bytes()).unwrap();
        let door = h.resolve("Door contact", None).unwrap();
        let dn = l.neighborhood(&h.ctrls[door].uuid).unwrap();
        assert_eq!(dn.inputs.len(), 1, "{:?}", dn.inputs);
        let input = &l.blocks[&dn.inputs[0].other];
        assert_eq!(input.typ, "DigitalIn", "AI → Q still reaches the Miniserver input");
        let hn = l.neighborhood(&input.uuid).unwrap();
        assert_eq!(hn.outputs.len(), 1);
        assert_eq!(hn.outputs[0].other, h.ctrls[door].uuid);
        // the fold is no change: a diff against the AQ version is empty
        assert!(diff(xml.as_bytes(), q.as_bytes()).unwrap().is_empty());
    }

    #[test]
    fn refs_fold_to_hardware_with_device_and_room() {
        let (h, l) = demo_logic();
        let c = h.resolve("Hallway light", None).unwrap();
        let n = l.neighborhood(&h.ctrls[c].uuid).unwrap();
        // AQ1 → OutputRef → the dimmer channel on its Tree device
        let aq1 = n.outputs.iter().find(|w| w.key == "AQ1").unwrap();
        let dimmer = &l.blocks[&aq1.other];
        assert_eq!(dimmer.title, "Spots");
        assert_eq!(dimmer.device.as_deref(), Some("RGBW 24V Dimmer Tree"));
        assert_eq!(dimmer.room.as_deref(), Some("Hallway"));
        assert!(l.blocks[aq1.via.as_ref().unwrap()].is_ref());
        assert_eq!(dimmer.context(), "RGBW 24V Dimmer Tree · Hallway");
        // the center knows its page, room and non-default parameters
        assert_eq!(n.center.page.as_deref(), Some("Hallway"));
        assert_eq!(n.center.room.as_deref(), Some("Hallway"));
        let shown: Vec<&str> = n
            .center
            .shown_params(false)
            .map(|p| p.key.as_str())
            .collect();
        assert_eq!(shown, ["MoveOn", "MoveIgnore"]);
        assert_eq!(n.center.params.len(), 3);
        // the door contact block is fed by a Miniserver input through an InputRef
        let door = h.resolve("Door contact", None).unwrap();
        let dn = l.neighborhood(&h.ctrls[door].uuid).unwrap();
        let input = &l.blocks[&dn.inputs[0].other];
        assert_eq!(input.typ, "DigitalIn");
        assert_eq!(input.device.as_deref(), Some("Demo Miniserver"));
        // centering on the hardware shows the logic it drives, refs folded
        let hn = l.neighborhood(&input.uuid).unwrap();
        assert_eq!(hn.outputs[0].other, h.ctrls[door].uuid);
        // a ref stands in for its target
        let via = dn.inputs[0].via.clone().unwrap();
        assert_eq!(l.resolve_center(&via), input.uuid);
    }

    #[test]
    fn fanout_trace_search_resolve() {
        let (h, l) = demo_logic();
        let c = h.resolve("Hallway light", None).unwrap();
        let n = l.neighborhood(&h.ctrls[c].uuid).unwrap();
        let mv = n.inputs.iter().find(|w| w.key == "Mv").unwrap();
        let also: Vec<&str> = mv.also.iter().map(|u| l.blocks[u].title.as_str()).collect();
        assert_eq!(also, ["Stairs pulse"]);
        // trace down from the motion sensor reaches the dimmer channels
        let motion = h.resolve("Motion", None).unwrap();
        let t = l.trace(&h.ctrls[motion].uuid, false, 4);
        assert!(
            t.iter()
                .any(|n| l.blocks[&n.block].title == "Spots" && n.depth == 2)
        );
        // trace up from the light reaches the Miniserver input behind the door contact
        let up = l.trace(&h.ctrls[c].uuid, true, 4);
        assert!(
            up.iter()
                .any(|n| l.blocks[&n.block].typ == "DigitalIn" && n.depth == 2)
        );
        // search: words match title, type, device, room
        let hits = l.search("dimmer hallway");
        assert!(hits.iter().any(|b| b.title == "Spots"));
        assert_eq!(l.search("Hallway light")[0].title, "Hallway light");
        // resolve: exact, qualified, ambiguous
        assert_eq!(
            l.resolve("hallway light", None).unwrap().typ,
            "LightController2"
        );
        assert_eq!(
            l.resolve("Spots [RGBW]", None).unwrap().device.as_deref(),
            Some("RGBW 24V Dimmer Tree")
        );
        let e = l.resolve("Button", None).unwrap_err().to_string();
        assert!(e.contains("ambiguous"), "{}", e);
        assert!(l.resolve("nope-nothing", None).is_err());
    }

    #[test]
    fn pages_sources_lint() {
        let (_, l) = demo_logic();
        let pages = l.pages();
        let hall = pages.iter().find(|p| p.title == "Hallway").unwrap();
        assert!(hall.blocks >= 5 && hall.wires >= 7, "{:?}", hall);
        let blocks = l.page_blocks("Hallway");
        assert!(blocks.iter().all(|b| !b.is_ref()));
        let src = l.page_source("Hallway").unwrap();
        assert!(src.starts_with("page \"Hallway\""), "{}", src);
        // hardware externs say where they are
        assert!(src.contains("RGBW 24V Dimmer Tree"), "{}", src);
        assert!(l.page_source("No such page").is_err());
        let lint = l.lint();
        let kinds: Vec<(&str, &str)> = lint
            .iter()
            .map(|f| (f.kind, l.blocks[&f.block].title.as_str()))
            .collect();
        assert!(
            kinds.contains(&("dead-logic", "Stairs pulse")),
            "{:?}",
            kinds
        );
        assert!(
            kinds.contains(&("unused-input", "Party mode")),
            "{:?}",
            kinds
        );
    }

    #[test]
    fn params_read_with_units() {
        assert_eq!(
            param_text("LightController2", "MoveOn", "900"),
            "900 s (15 min)"
        );
        assert_eq!(
            param_text("LightController2", "MoveTimeout", "5400"),
            "5400 s (1.5 h)"
        );
        assert_eq!(param_text("Monoflop", "Time", "3"), "3 s");
        assert_eq!(
            param_text("LightController2", "DayMinTemp", "2700"),
            "2700 K"
        );
        assert_eq!(param_text("Formula", "Text", "I1/1000"), "I1/1000");
    }

    #[test]
    fn semantic_diff_lines() {
        let st = demo::structure();
        let a = demo::loxone_xml(&st);
        let b = a.replace("Title=\"Night mode\"", "Title=\"Night mode 2\"");
        let lines = diff_lines(a.as_bytes(), b.as_bytes()).unwrap();
        assert!(
            lines[0].starts_with("= ") && lines[0].contains("1 renamed"),
            "{:?}",
            lines
        );
        assert!(lines[1].starts_with("# "), "page header: {:?}", lines);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("Night mode") && l.contains("Night mode 2"))
        );
        assert!(diff_lines(a.as_bytes(), a.as_bytes()).unwrap().is_empty());
    }

    #[test]
    fn diff_folds_refs_and_formats_params() {
        let st = demo::structure();
        let a = demo::loxone_xml(&st);
        // parameter change with units, and the block it is about
        let b = a.replace("K=\"MoveOn\" Def=\"900\"", "K=\"MoveOn\" Def=\"600\"");
        let d = diff(a.as_bytes(), b.as_bytes()).unwrap();
        let p = d.iter().find(|l| l.text.starts_with("~ param")).unwrap();
        assert!(
            p.text.contains("MoveOn  900 s (15 min) → 600 s (10 min)"),
            "{}",
            p.text
        );
        let (h, l) = demo_logic();
        let c = h.resolve("Hallway light", None).unwrap();
        assert_eq!(p.block.as_deref(), Some(h.ctrls[c].uuid.as_str()));
        // re-placing an OutputRef (new UUIDs, same target) is no change at all
        let n = l.neighborhood(&h.ctrls[c].uuid).unwrap();
        let aq1 = n.outputs.iter().find(|w| w.key == "AQ1").unwrap();
        let r = aq1.via.clone().unwrap();
        let fresh = r.replacen("2e", "3f", 1);
        let b = a.replace(&r, &fresh);
        assert!(diff(a.as_bytes(), b.as_bytes()).unwrap().is_empty());
        // hardware renames are grouped under devices
        let lines = diff_lines(
            a.as_bytes(),
            a.replace("Title=\"Spots\"", "Title=\"Spot\"").as_bytes(),
        )
        .unwrap();
        assert!(
            lines.iter().any(|l| l.contains("rename Spots → Spot")),
            "{:?}",
            lines
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("# (devices & periphery)")),
            "{:?}",
            lines
        );
    }

    #[test]
    fn block_history_changes() {
        let st = demo::structure();
        let a = demo::loxone_xml(&st);
        let b = a
            .replace("K=\"MoveOn\" Def=\"900\"", "K=\"MoveOn\" Def=\"60\"")
            .replace("Title=\"Hallway light\"", "Title=\"Hall light\"");
        let (la, lb) = (
            Logic::parse(a.as_bytes()).unwrap(),
            Logic::parse(b.as_bytes()).unwrap(),
        );
        let (h, _) = demo_logic();
        let c = h.resolve("Hallway light", None).unwrap();
        let ch = block_changes(&la, &lb, &h.ctrls[c].uuid);
        assert_eq!(
            ch,
            [
                "~ rename Hallway light → Hall light",
                "~ param  MoveOn  900 s (15 min) → 60 s (1 min)"
            ]
        );
        assert!(block_changes(&la, &la, &h.ctrls[c].uuid).is_empty());
    }
}
