//! Wires: block argument bindings and `<-` statements, drawn through the
//! page's mirrors where one serves (D34).

use super::*;
use crate::doc::ObjectSummary;
use crate::logic::ast::PortRef;

#[derive(Clone)]
struct MirrorRoute {
    /// Port the rerouted wire uses instead of the named one.
    port: String,
    /// The ref object itself (owner of `port`).
    ref_uuid: String,
    page: Option<String>,
    /// An OutputRef's `AQ`, which feeds the sink it mirrors.
    feeds: Option<String>,
}

/// Mirror routing (D34): a wire may name the mirrored object directly;
/// when the consumer's *own page* carries a ref of it fed from exactly that
/// port, the wire is drawn through the ref — an `AI`-fed mirror serves
/// `AQ`, an `I`-fed mirror serves `Q`. Reuse-only and page-local: no
/// same-page ref, no rerouting (a direct wire is legal, oracle session 6);
/// a ref on another page is someone else's visual. Symmetric for actors: a
/// wire into an extern port that the base feeds from an OutputRef's `AQ`
/// lands on that ref's `AI` instead — the ref on the *writer's* page.
struct Routes<'a> {
    pages_idx: Vec<(&'a [usize], &'a str)>,
    path_by_uuid: BTreeMap<&'a str, &'a [usize]>,
    /// Mirrors of a source port, keyed by that port.
    in_routes: BTreeMap<String, Vec<MirrorRoute>>,
    /// OutputRefs that feed a sink port, keyed by that port.
    out_routes: BTreeMap<String, Vec<MirrorRoute>>,
}

impl<'a> Routes<'a> {
    /// The routes the rebuilt document offers, from its unmanaged refs and
    /// the base's wires.
    fn collect(
        doc: &LoxoneDoc,
        routed_objects: &'a [ObjectSummary],
        managed: &BTreeMap<String, PlannedBlock>,
        all_base_wires: &[WireView],
    ) -> Self {
        let managed_uuids: BTreeSet<&str> = managed.values().map(|p| p.uuid.as_str()).collect();
        let pages_idx: Vec<(&[usize], &str)> = routed_objects
            .iter()
            .filter(|o| o.block_type == "Page")
            .map(|o| (o.path.as_slice(), o.uuid.as_str()))
            .collect();
        let path_by_uuid: BTreeMap<&str, &[usize]> = routed_objects
            .iter()
            .map(|o| (o.uuid.as_str(), o.path.as_slice()))
            .collect();
        let mut in_routes: BTreeMap<String, Vec<MirrorRoute>> = BTreeMap::new();
        let mut aq_of_outputref: BTreeMap<String, MirrorRoute> = BTreeMap::new();
        for o in routed_objects {
            if managed_uuids.contains(o.uuid.as_str())
                || !matches!(
                    o.block_type.as_str(),
                    "InputRef" | "OutputRef" | "OutputRefLM"
                )
            {
                continue;
            }
            let el = doc.element_at(&o.path).expect("path from objects()");
            let ps = ports(el);
            let port = |k: &str| ps.iter().find(|p| p.key == k);
            let page = page_uuid_of(&pages_idx, &o.path);
            if o.block_type == "InputRef" {
                for (fed, serves) in [("AI", "AQ"), ("I", "Q")] {
                    if let (Some(f), Some(s)) = (port(fed), port(serves)) {
                        for src in &f.inputs {
                            in_routes.entry(src.clone()).or_default().push(MirrorRoute {
                                port: s.uuid.clone(),
                                ref_uuid: o.uuid.clone(),
                                page: page.clone(),
                                feeds: None,
                            });
                        }
                    }
                }
            } else if let (Some(aq), Some(ai)) = (port("AQ"), port("AI")) {
                aq_of_outputref.insert(
                    aq.uuid.clone(),
                    MirrorRoute {
                        port: ai.uuid.clone(),
                        ref_uuid: o.uuid.clone(),
                        page,
                        feeds: Some(aq.uuid.clone()),
                    },
                );
            }
        }
        // From the base's wires: a managed sink (a Modbus actuator, D64) lost
        // its `<In>` from the ref with the teardown.
        let mut out_routes: BTreeMap<String, Vec<MirrorRoute>> = BTreeMap::new();
        for w in all_base_wires {
            if let Some(r) = aq_of_outputref.get(&w.from_port) {
                out_routes
                    .entry(w.to_port.clone())
                    .or_default()
                    .push(r.clone());
            }
        }
        Routes {
            pages_idx,
            path_by_uuid,
            in_routes,
            out_routes,
        }
    }

    /// The page the object at `path` sits on.
    fn page_uuid_of(&self, path: &[usize]) -> Option<String> {
        page_uuid_of(&self.pages_idx, path)
    }
}

/// The innermost of `pages` (path, UUID) that contains `path`.
fn page_uuid_of(pages: &[(&[usize], &str)], path: &[usize]) -> Option<String> {
    pages
        .iter()
        .filter(|(p, _)| path.len() > p.len() && path.starts_with(p))
        .max_by_key(|(p, _)| p.len())
        .map(|(_, u)| (*u).to_string())
}

/// Same-page candidates only; several on one page fall back to the
/// previous compile's recorded wire, then refuse.
fn pick_route(
    cands: &[MirrorRoute],
    ctx_page: Option<&String>,
    pinned: &dyn Fn(&MirrorRoute) -> bool,
) -> std::result::Result<Option<MirrorRoute>, Vec<String>> {
    let local: Vec<&MirrorRoute> = cands
        .iter()
        .filter(|c| c.page.as_ref() == ctx_page && ctx_page.is_some())
        .collect();
    match local.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some((*one).clone())),
        many => {
            // Each mirror carries its own output ports (a base where
            // twins share them is refused up front), so several
            // candidates are a real choice.
            let hits: Vec<&&MirrorRoute> = many.iter().filter(|c| pinned(c)).collect();
            match hits.as_slice() {
                [one] => Ok(Some((**one).clone())),
                _ => Err(many.iter().map(|c| c.ref_uuid.clone()).collect()),
            }
        }
    }
}

/// Wires: block argument bindings (sink = the declaring block) and `<-`
/// statements (sink = an extern port), in source order; then the withdrawn
/// extern wires are tombstoned.
pub(super) fn draw_wires(
    doc: &mut LoxoneDoc,
    lock: &mut Lockfile,
    cx: &Rebuild,
    prior: &PriorEdits,
) -> Result<()> {
    let Rebuild {
        module,
        managed,
        snap,
        ..
    } = *cx;
    let PriorEdits {
        old_wires,
        wires_present,
        extern_wire_flags,
        ..
    } = prior;
    let routed_objects = doc.objects();
    let routes = Routes::collect(doc, &routed_objects, managed, &snap.all_base_wires);
    for (from_ref, to_ref) in module.wire_pairs() {
        draw_wire(
            doc,
            lock,
            cx,
            &routes,
            extern_wire_flags,
            &from_ref,
            &to_ref,
        )?;
    }
    lock.extern_wires.sort();
    lock.extern_wires.dedup();
    // A wire this base carried that the source no longer draws was
    // withdrawn — tombstone it (D31) so pre-deployment bases keep losing
    // it instead of passing it through.
    for w in old_wires {
        if wires_present.contains(w) && !lock.extern_wires.contains(w) {
            lock.removed_wires.insert(w.clone());
        }
    }
    Ok(())
}

/// One wire `to_ref <- from_ref`: both ends resolved, routed through a
/// mirror where one serves, drawn, and recorded when its sink is an extern.
fn draw_wire(
    doc: &mut LoxoneDoc,
    lock: &mut Lockfile,
    cx: &Rebuild,
    routes: &Routes,
    extern_wire_flags: &BTreeMap<(String, String), String>,
    from_ref: &PortRef,
    to_ref: &PortRef,
) -> Result<()> {
    let Rebuild {
        managed,
        extern_uuid,
        residue,
        snap,
        mirror_of,
        ..
    } = *cx;
    let BaseSnapshot {
        type_by_uuid,
        base_in_order,
        ..
    } = snap;
    // The reverse half of an API link through a minted `OutputRef`
    // names the ref's `AI` as its source (`api_mirror_entry`); it
    // lands on the block's API connector, `OutputAPI` — never one the
    // builtin table proves output-only.
    let sink_type = managed
        .get(&to_ref.slug)
        .map(|p| p.block_type.as_str())
        .or_else(|| {
            extern_uuid
                .get(&to_ref.slug)
                .and_then(|u| type_by_uuid.get(u))
                .map(|(t, _)| t.as_str())
        });
    let api_sink = to_ref.port == "OutputAPI"
        && !sink_type
            .and_then(builtin)
            .and_then(|specs| specs.iter().find(|s| s.key == "OutputAPI"))
            .is_some_and(|s| s.dir == PortDir::Output);
    let reverse_api = api_sink
        && managed.get(&from_ref.slug).is_some_and(|p| {
            api_mirror_entry(
                &p.block_type,
                &from_ref.port,
                mirror_of
                    .get(&from_ref.slug)
                    .map(|m| m.target_type.as_str()),
            )
        });
    let mut from = resolve_port(
        doc,
        managed,
        extern_uuid,
        &from_ref.slug,
        &from_ref.port,
        if reverse_api {
            PortDir::Input
        } else {
            PortDir::Output
        },
    )?;
    let mut to = resolve_port(
        doc,
        managed,
        extern_uuid,
        &to_ref.slug,
        &to_ref.port,
        PortDir::Input,
    )?;
    // Ref-typed endpoints wire literally (feed wires, explicit ref
    // externs); routing is for wires naming the mirrored object.
    let endpoint_type = |slug: &str, owner_uuid: &str| -> Option<&str> {
        managed
            .get(slug)
            .map(|p| p.block_type.as_str())
            .or_else(|| type_by_uuid.get(owner_uuid).map(|(t, _)| t.as_str()))
    };
    let from_is_ref = endpoint_type(&from_ref.slug, &from.owner_uuid).is_some_and(is_ref_type);
    let to_is_ref = endpoint_type(&to_ref.slug, &to.owner_uuid).is_some_and(is_ref_type);
    if !from_is_ref && !to_is_ref {
        route_through_mirrors(doc, cx, routes, from_ref, to_ref, &mut from, &mut to)?;
    }
    let key = (to.port_uuid.clone(), from.port_uuid.clone());
    let flg = residue
        .get(&to.owner_uuid)
        .and_then(|r| r.wire_flags.get(&key))
        .or_else(|| extern_wire_flags.get(&key));
    add_wire(
        doc,
        &to.owner_uuid,
        &to.port_uuid,
        &from.port_uuid,
        flg.map(String::as_str),
        base_in_order.get(&to.port_uuid).map_or(&[], Vec::as_slice),
    )?;
    if to.is_extern {
        lock.extern_wires.push(LockedWire {
            from: from.port_uuid,
            to: to.port_uuid,
        });
    }
    Ok(())
}

/// Reroutes a wire's ends through the mirrors on the relevant page (D34).
fn route_through_mirrors(
    doc: &mut LoxoneDoc,
    cx: &Rebuild,
    routes: &Routes,
    from_ref: &PortRef,
    to_ref: &PortRef,
    from: &mut ResolvedPort,
    to: &mut ResolvedPort,
) -> Result<()> {
    let Rebuild {
        managed,
        residue,
        snap,
        ..
    } = *cx;
    let BaseSnapshot {
        base_wires,
        base_in_order,
        ..
    } = snap;
    let Routes {
        path_by_uuid,
        in_routes,
        out_routes,
        ..
    } = routes;
    let page_of_endpoint = |slug: &str, owner_uuid: &str| -> Option<String> {
        match managed.get(slug) {
            Some(p) => p.placed.then(|| p.page_uuid.clone()),
            None => path_by_uuid
                .get(owner_uuid)
                .and_then(|p| routes.page_uuid_of(p)),
        }
    };
    let route_err = |which: &str, endpoint: String, refs: Vec<String>| {
        Error::Compile(format!(
            "wire `{to_ref} <- {from_ref}`: several mirrors of {which} \
                     `{endpoint}` are on the consumer's page ({}). Declare the \
                     one you mean (`extern … = \
                     InputRef(mirrors: …)` or a `uuid:` pin) and wire through it.",
            refs.join(", ")
        ))
    };
    if let Some(cands) = in_routes.get(&from.port_uuid) {
        let sink_page = page_of_endpoint(&to_ref.slug, &to.owner_uuid);
        let pin = |c: &MirrorRoute| base_wires.contains(&(c.port.clone(), to.port_uuid.clone()));
        match pick_route(cands, sink_page.as_ref(), &pin) {
            Ok(Some(r)) => {
                from.owner_uuid = r.ref_uuid;
                from.port_uuid = r.port;
            }
            Ok(None) => {}
            Err(refs) => {
                return Err(route_err("source", from_ref.to_string(), refs));
            }
        }
    }
    if let Some(cands) = out_routes.get(&to.port_uuid) {
        let from_page = page_of_endpoint(&from_ref.slug, &from.owner_uuid);
        let pin = |c: &MirrorRoute| base_wires.contains(&(from.port_uuid.clone(), c.port.clone()));
        match pick_route(cands, from_page.as_ref(), &pin) {
            Ok(Some(r)) => {
                // A managed sink is fed by the ref again.
                if let Some(aq) = r.feeds.as_ref().filter(|_| !to.is_extern) {
                    let key = (to.port_uuid.clone(), aq.clone());
                    add_wire(
                        doc,
                        &to.owner_uuid,
                        &to.port_uuid,
                        aq,
                        residue
                            .get(&to.owner_uuid)
                            .and_then(|r| r.wire_flags.get(&key))
                            .map(String::as_str),
                        base_in_order.get(&to.port_uuid).map_or(&[], Vec::as_slice),
                    )?;
                }
                to.owner_uuid = r.ref_uuid;
                to.port_uuid = r.port;
                to.is_extern = true;
            }
            Ok(None) => {}
            Err(refs) => {
                return Err(route_err("sink", to_ref.to_string(), refs));
            }
        }
    }
    // Reverse-recorded distributions (API connectors): there the
    // extern input is the wire's *from* and the managed output its
    // *to* — the OutputRef redirect applies to the source side,
    // still on the writer's page.
    if let Some(cands) = out_routes.get(&from.port_uuid) {
        let writer_page = page_of_endpoint(&to_ref.slug, &to.owner_uuid);
        let pin = |c: &MirrorRoute| base_wires.contains(&(c.port.clone(), to.port_uuid.clone()));
        match pick_route(cands, writer_page.as_ref(), &pin) {
            Ok(Some(r)) => {
                from.owner_uuid = r.ref_uuid;
                from.port_uuid = r.port;
            }
            Ok(None) => {}
            Err(refs) => {
                return Err(route_err("source", from_ref.to_string(), refs));
            }
        }
    }
    Ok(())
}
