//! Thin adapter over `lxir` (§12.1): load a `.Loxone` config, index blocks and
//! wires by UUID, and answer "what is wired around this control?".
//!
//! Only this file touches the lxir API, so churn in lxir (v0) stays here.

use std::collections::HashMap;

use anyhow::Result;

/// One block (`<C>` object) of the config.
#[derive(Debug, Clone)]
pub struct Block {
    pub typ: String,
    pub title: String,
}

/// One wire at the center block, seen from the center.
#[derive(Debug, Clone, PartialEq)]
pub struct Wire {
    /// Connector key on the center block (`Mv`, `AQ1`)
    pub key: String,
    /// The block at the other end
    pub other: String,
    /// Connector key on the other block
    pub other_key: String,
    /// UUID of the wire's *source* connector: the streamed state UUID when the
    /// source block has a visualization
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct Neighborhood {
    pub center: Block,
    pub inputs: Vec<Wire>,
    pub outputs: Vec<Wire>,
}

#[derive(Debug, Default)]
pub struct Logic {
    pub blocks: HashMap<String, Block>,
    /// connector UUID → (block UUID, key)
    port_owner: HashMap<String, (String, String)>,
    /// sink connector → source connectors
    wires_in: HashMap<String, Vec<String>>,
    /// source connector → sink connectors
    wires_out: HashMap<String, Vec<String>>,
    /// block → its connector UUIDs (document order)
    ports: HashMap<String, Vec<(String, String)>>,
    pub wire_count: usize,
}

impl Logic {
    /// The block owning a connector UUID (e.g. a streamed state UUID).
    #[cfg(test)]
    pub fn owner_of_port(&self, port: &str) -> Option<&str> {
        self.port_owner.get(port).map(|(b, _)| b.as_str())
    }

    pub fn parse(bytes: &[u8]) -> Result<Logic> {
        let doc = lxir::LoxoneDoc::parse(bytes).map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut l = Logic::default();
        for obj in doc.objects() {
            if matches!(obj.block_type.as_str(), "Document" | "Page") {
                continue;
            }
            let Some(el) = doc.element_at(&obj.path) else {
                continue;
            };
            let ports: Vec<(String, String)> = lxir::doc::ports(el)
                .into_iter()
                .map(|p| (p.key, p.uuid))
                .collect();
            for (k, u) in &ports {
                l.port_owner
                    .insert(u.clone(), (obj.uuid.clone(), k.clone()));
            }
            l.ports.insert(obj.uuid.clone(), ports);
            l.blocks.insert(
                obj.uuid.clone(),
                Block {
                    typ: obj.block_type.clone(),
                    title: obj
                        .title
                        .clone()
                        .or(obj.iname.clone())
                        .unwrap_or_else(|| obj.block_type.clone()),
                },
            );
        }
        for w in doc.wires() {
            l.wires_in
                .entry(w.to_port.clone())
                .or_default()
                .push(w.from_port.clone());
            l.wires_out.entry(w.from_port).or_default().push(w.to_port);
            l.wire_count += 1;
        }
        Ok(l)
    }

    /// Inputs and outputs of a block, one hop each way.
    pub fn neighborhood(&self, block: &str) -> Option<Neighborhood> {
        let center = self.blocks.get(block)?.clone();
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for (key, port) in self.ports.get(block).into_iter().flatten() {
            for src in self.wires_in.get(port).into_iter().flatten() {
                if let Some((ob, ok)) = self.port_owner.get(src) {
                    inputs.push(Wire {
                        key: key.clone(),
                        other: ob.clone(),
                        other_key: ok.clone(),
                        source: src.clone(),
                    });
                }
            }
            for sink in self.wires_out.get(port).into_iter().flatten() {
                if let Some((ob, ok)) = self.port_owner.get(sink) {
                    outputs.push(Wire {
                        key: key.clone(),
                        other: ob.clone(),
                        other_key: ok.clone(),
                        source: port.clone(),
                    });
                }
            }
        }
        Some(Neighborhood {
            center,
            inputs,
            outputs,
        })
    }
}

/// Semantic config diff as human-readable lines (for System › Config).
pub fn diff_lines(old: &[u8], new: &[u8]) -> Result<Vec<String>> {
    let a = lxir::LoxoneDoc::parse(old).map_err(|e| anyhow::anyhow!("{}", e))?;
    let b = lxir::LoxoneDoc::parse(new).map_err(|e| anyhow::anyhow!("{}", e))?;
    let d = lxir::diff::diff(&a, &b);
    let title = |doc: &lxir::LoxoneDoc| -> HashMap<String, String> {
        doc.objects()
            .into_iter()
            .map(|o| {
                let t = o.title.or(o.iname).unwrap_or(o.block_type);
                (o.uuid, t)
            })
            .collect()
    };
    let (ta, tb) = (title(&a), title(&b));
    let (ia, ib) = (a.index(), b.index());
    let name = |uuid: &str| {
        tb.get(uuid)
            .or_else(|| ta.get(uuid))
            .cloned()
            .unwrap_or_else(|| uuid.to_string())
    };
    let port = |p: &str| {
        ib.port_owner
            .get(p)
            .or_else(|| ia.port_owner.get(p))
            .map(|(o, k)| format!("{}.{}", name(o), k))
            .unwrap_or_else(|| p.to_string())
    };
    let mut out = Vec::new();
    for o in &d.added {
        out.push(format!(
            "+ block  {} ({})",
            o.title.clone().unwrap_or_default(),
            o.block_type
        ));
    }
    for o in &d.removed {
        out.push(format!(
            "- block  {} ({})",
            o.title.clone().unwrap_or_default(),
            o.block_type
        ));
    }
    for r in &d.renamed {
        out.push(format!(
            "~ rename {} → {}{}",
            r.from.clone().unwrap_or_default(),
            r.to.clone().unwrap_or_default(),
            if r.locale_suspect { "  (locale)" } else { "" }
        ));
    }
    for p in &d.param_changes {
        out.push(format!(
            "~ param  {} · {}  {} → {}",
            name(&p.object_uuid),
            p.port_key,
            p.from.clone().unwrap_or_else(|| "—".into()),
            p.to.clone().unwrap_or_else(|| "—".into())
        ));
    }
    for w in &d.wires_added {
        out.push(format!(
            "+ wire   {} → {}",
            port(&w.from_port),
            port(&w.to_port)
        ));
    }
    for w in &d.wires_removed {
        out.push(format!(
            "- wire   {} → {}",
            port(&w.from_port),
            port(&w.to_port)
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::demo;
    use crate::tui::model::House;

    #[test]
    fn neighborhood_resolves_control_block_and_live_wires() {
        let st = demo::structure();
        let h = House::from_structure(&st);
        let l = Logic::parse(demo::loxone_xml(&st).as_bytes()).unwrap();
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

    #[test]
    fn semantic_diff_lines() {
        let st = demo::structure();
        let a = demo::loxone_xml(&st);
        let b = a.replace("Title=\"Night mode\"", "Title=\"Night mode 2\"");
        let lines = diff_lines(a.as_bytes(), b.as_bytes()).unwrap();
        assert!(
            lines
                .iter()
                .any(|l| l.contains("Night mode") && l.contains("Night mode 2"))
        );
        assert!(diff_lines(a.as_bytes(), a.as_bytes()).unwrap().is_empty());
    }
}
