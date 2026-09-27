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
///
/// The first line (`= …`) sums up the change; then one `# Page` header per
/// config page with its changes (`+` added, `-` removed, `~` changed). Repeated
/// identical lines collapse into one with a `×N` count.
pub fn diff_lines(old: &[u8], new: &[u8]) -> Result<Vec<String>> {
    let a = lxir::LoxoneDoc::parse(old).map_err(|e| anyhow::anyhow!("{}", e))?;
    let b = lxir::LoxoneDoc::parse(new).map_err(|e| anyhow::anyhow!("{}", e))?;
    let d = lxir::diff::diff(&a, &b);
    let (oa, ob) = (a.objects(), b.objects());
    let title = |objs: &[lxir::doc::ObjectSummary]| -> HashMap<String, String> {
        objs.iter()
            .map(|o| {
                let t = o
                    .title
                    .clone()
                    .or(o.iname.clone())
                    .unwrap_or(o.block_type.clone());
                (o.uuid.clone(), t)
            })
            .collect()
    };
    let (ta, tb) = (title(&oa), title(&ob));
    let (ia, ib) = (a.index(), b.index());
    let name = |uuid: &str| {
        tb.get(uuid)
            .or_else(|| ta.get(uuid))
            .cloned()
            .unwrap_or_else(|| uuid.to_string())
    };
    // the page an object sits on: the deepest Page element above it
    let pages = |objs: &[lxir::doc::ObjectSummary]| -> Vec<(Vec<usize>, String)> {
        objs.iter()
            .filter(|o| o.block_type == "Page")
            .map(|o| {
                (
                    o.path.clone(),
                    o.title.clone().unwrap_or_else(|| "Page".into()),
                )
            })
            .collect()
    };
    let (pa, pb) = (pages(&oa), pages(&ob));
    let page_of = |path: &[usize], pages: &[(Vec<usize>, String)]| -> String {
        pages
            .iter()
            .filter(|(p, _)| path.starts_with(p))
            .max_by_key(|(p, _)| p.len())
            .map_or_else(|| "(outside pages)".into(), |(_, t)| t.clone())
    };
    let page_of_uuid = |uuid: &str| -> String {
        match (ib.by_uuid.get(uuid), ia.by_uuid.get(uuid)) {
            (Some(p), _) => page_of(p, &pb),
            (None, Some(p)) => page_of(p, &pa),
            _ => "(outside pages)".into(),
        }
    };
    let owner = |p: &str| {
        ib.port_owner
            .get(p)
            .or_else(|| ia.port_owner.get(p))
            .cloned()
    };
    let port = |p: &str| {
        owner(p)
            .map(|(o, k)| format!("{}.{}", name(&o), k))
            .unwrap_or_else(|| p.to_string())
    };
    let label = |o: &lxir::doc::ObjectSummary| {
        format!(
            "{} ({})",
            o.title.clone().or(o.iname.clone()).unwrap_or_default(),
            o.block_type
        )
    };
    // page → lines, pages in order of first appearance
    let mut by_page: Vec<(String, Vec<String>)> = Vec::new();
    let mut push = |page: String, line: String| match by_page.iter_mut().find(|(p, _)| *p == page) {
        Some((_, v)) => v.push(line),
        None => by_page.push((page, vec![line])),
    };
    for o in &d.added {
        push(page_of(&o.path, &pb), format!("+ block  {}", label(o)));
    }
    for o in &d.removed {
        push(page_of(&o.path, &pa), format!("- block  {}", label(o)));
    }
    for r in &d.renamed {
        push(
            page_of_uuid(&r.uuid),
            format!(
                "~ rename {} → {}{}",
                r.from.clone().unwrap_or_default(),
                r.to.clone().unwrap_or_default(),
                if r.locale_suspect { "  (locale)" } else { "" }
            ),
        );
    }
    for p in &d.param_changes {
        push(
            page_of_uuid(&p.object_uuid),
            format!(
                "~ param  {} · {}  {} → {}",
                name(&p.object_uuid),
                p.port_key,
                p.from.clone().unwrap_or_else(|| "—".into()),
                p.to.clone().unwrap_or_else(|| "—".into())
            ),
        );
    }
    let wire_page = |w: &lxir::doc::WireView| {
        owner(&w.to_port)
            .or_else(|| owner(&w.from_port))
            .map_or_else(|| "(outside pages)".into(), |(o, _)| page_of_uuid(&o))
    };
    for w in &d.wires_added {
        push(
            wire_page(w),
            format!("+ wire   {} → {}", port(&w.from_port), port(&w.to_port)),
        );
    }
    for w in &d.wires_removed {
        push(
            wire_page(w),
            format!("- wire   {} → {}", port(&w.from_port), port(&w.to_port)),
        );
    }
    let mut out = Vec::new();
    if by_page.is_empty() {
        return Ok(out);
    }
    // totals: filled from the collapsed lines, so re-created pairs count once
    let mut tot = [0usize; 8];
    // loose blocks and wires last
    by_page.sort_by_key(|(p, _)| p.starts_with('('));
    for (page, lines) in by_page {
        out.push(format!("# {}", page));
        // collapse repeats, keeping first-appearance order
        let mut seen: Vec<(String, usize)> = Vec::new();
        for l in lines {
            match seen.iter_mut().find(|(s, _)| *s == l) {
                Some((_, n)) => *n += 1,
                None => seen.push((l, 1)),
            }
        }
        // the same thing removed and added again (Config re-created it with
        // new UUIDs): one "re-created" line instead of a + and a − block
        let count = |seen: &[(String, usize)], l: &str| {
            seen.iter().find(|(s, _)| s == l).map_or(0, |(_, n)| *n)
        };
        let mut shown: Vec<(String, usize)> = Vec::new();
        for (l, n) in &seen {
            let Some(rest) = l.strip_prefix("+ ") else {
                if let Some(rest) = l.strip_prefix("- ") {
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
                shown.push((format!("~ {}  re-created", rest), m));
            }
            if *n > m {
                shown.push((l.clone(), n - m));
            }
        }
        for (l, n) in shown {
            // re-created wires follow their blocks: not counted on their own
            let k = if l.ends_with("re-created") {
                if l.starts_with("~ block") { 4 } else { 7 }
            } else if l.starts_with("+ block") {
                0
            } else if l.starts_with("- block") {
                1
            } else if l.starts_with("~ rename") {
                2
            } else if l.starts_with("~ param") {
                3
            } else if l.starts_with("+ wire") {
                5
            } else {
                6
            };
            tot[k] += n;
            out.push(if n > 1 { format!("{}  ×{}", l, n) } else { l });
        }
    }
    let mut parts = Vec::new();
    for (k, what) in ["added", "removed", "renamed", "parameters", "re-created"]
        .iter()
        .enumerate()
    {
        if tot[k] > 0 {
            parts.push(format!("{} {}", tot[k], what));
        }
    }
    if tot[5] + tot[6] > 0 {
        parts.push(format!("wires +{} −{}", tot[5], tot[6]));
    }
    out.insert(0, format!("= {}", parts.join(" · ")));
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
}
