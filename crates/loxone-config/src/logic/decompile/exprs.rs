//! The way back from an expression's blocks to the expression.
//!
//! The compile desugars `x.I1 <- a.Q and b.AQ >= 5` into an `And` and a
//! `GreaterEqual`, each labeled with its sub-expression. Decompile reads
//! those labels back: a gate whose label parses as the expression of its
//! own type, with sub-blocks that do the same, is written as that
//! expression again.
//!
//! - Into the one port it feeds: an argument binding, or `<-` onto an
//!   extern.
//! - As a named signal (`slug = <expr>`) when it feeds several ports or
//!   none, or when its top block has a title of its own (`slug.title = …`).
//!   A single block with a title of its own stays declared: nothing tells
//!   it from a block drawn in Loxone Config.
//!
//! Only where the round trip is exact. Each candidate is desugared and
//! compared block by block with what the config holds: types, labels,
//! wires and values. A sub-block must feed only its parent, on the same
//! page. Anything else stays declared as before.
//!
//! The labels name the operands (`wassertemp.Q`) and constants
//! (`wunschtemp`). A first pass collects those names; the lift then gives
//! the objects those slugs where they are free, and writes a `let` for each
//! constant.

use super::*;
use crate::logic::ast::{Attr, AttrDecl, Expr, ExprWireDecl, LetDecl, Operand, SignalDecl};
use crate::logic::parser::expr_text;
use crate::logic::template::{RefUse, visit_refs};

/// The gate and comparator types an expression desugars into. A `Formula`
/// is an expression only as a whole binding or signal.
const GATES: &[&str] = &[
    "And",
    "Or",
    "Not",
    "Equal",
    "NotEqual",
    "Greater",
    "GreaterEqual",
    "Less",
    "LessEqual",
];

/// What [`Lift::lift_exprs`] decided.
#[derive(Debug, Default, Clone)]
pub(in crate::logic) struct ExprLift {
    /// Objects an expression stands for: no declaration of their own,
    /// expression-owned in the lock.
    pub(in crate::logic) absorbed: BTreeSet<usize>,
    /// Top blocks written as named signals: the statement and its `.title`.
    pub(super) signals: BTreeMap<usize, Vec<Item>>,
    /// `<-` statements (indexes into `Lift::wires`) whose source is an
    /// expression.
    pub(super) wire_exprs: BTreeMap<usize, Expr>,
    /// The constants the labels name, with the pages that use each.
    pub(super) lets: Vec<(LetDecl, BTreeSet<Option<usize>>)>,
}

impl ExprLift {
    /// The object is written as part of an expression, not declared.
    pub(super) fn replaces(&self, i: usize) -> bool {
        self.absorbed.contains(&i) || self.signals.contains_key(&i)
    }
}

/// Where a block's output goes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Sink {
    /// A port of a managed block.
    Block(usize, String),
    /// A `<-` statement, by index into `Lift::wires`.
    Wire(usize),
}

/// How a candidate expression is written.
enum Shape {
    Inline(usize, String),
    Wire(usize),
    Signal(Option<String>),
}

/// The references to each slug: in total, and per referenced port.
#[derive(Default)]
struct Refs {
    total: BTreeMap<String, usize>,
    port: BTreeMap<(String, String), usize>,
}

/// One candidate's outcome.
struct Found {
    /// The objects in desugar order, each with its slug in the expansion.
    blocks: Vec<(usize, String)>,
    lets: BTreeMap<String, Value>,
    hints: Vec<(usize, String)>,
}

impl Lift {
    /// The names the expression labels give their operands, by object
    /// UUID: what the second scan assigns first.
    pub(super) fn expr_hints(&self) -> BTreeMap<String, String> {
        let mut found: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
        self.walk_candidates(false, &mut |_, _, _, f| {
            for (o, name) in f.hints {
                found.entry(o).or_default().insert(name);
            }
            false
        });
        let mut out = BTreeMap::new();
        let mut given: BTreeSet<String> = BTreeSet::new();
        for (o, names) in found {
            // Two labels that disagree name nothing.
            let mut names = names.into_iter();
            if let (Some(name), None) = (names.next(), names.next())
                && given.insert(name.clone())
            {
                out.insert(self.objects[o].uuid.clone(), name);
            }
        }
        out
    }

    /// Fold the exact gate chains into expressions and signals. Leaves the
    /// lift as it was if the result would not check.
    pub(super) fn lift_exprs(&mut self) {
        let saved = (
            self.block_decls.clone(),
            self.slug_of.clone(),
            self.obj_of_slug.clone(),
        );
        let mut plan: Vec<(usize, Expr, Shape, Found)> = Vec::new();
        let mut lets: BTreeMap<String, Value> = BTreeMap::new();
        self.walk_candidates(true, &mut |r, e, shape, f| {
            // A constant is one value under one name, and the name is free.
            let clash = f.lets.iter().any(|(name, v)| {
                self.names.contains(name) || lets.get(name).is_some_and(|w| w != v)
            });
            if clash {
                return false;
            }
            lets.extend(f.lets.iter().map(|(k, v)| (k.clone(), v.clone())));
            plan.push((r, e.clone(), shape, f));
            true
        });
        if plan.is_empty() {
            return;
        }
        // The backstop: lifting must not make the view check worse. A view
        // of a whole config may already fail (a device it does not lift),
        // so the lifted one only has to fail the same way.
        self.factored = self.block_decls.clone();
        let before = self.verdict();
        let mut lift = ExprLift::default();
        let mut let_pages: BTreeMap<String, BTreeSet<Option<usize>>> = BTreeMap::new();
        for (r, e, shape, found) in plan {
            let page = self.page_of(r);
            for name in found.lets.keys() {
                let_pages.entry(name.clone()).or_default().insert(page);
            }
            for &(o, ref slug) in &found.blocks {
                if matches!(shape, Shape::Signal(_)) && o == r {
                    continue;
                }
                lift.absorbed.insert(o);
                let uuid = self.objects[o].uuid.clone();
                let old =
                    std::mem::replace(self.slug_of.get_mut(&uuid).expect("lifted"), slug.clone());
                self.obj_of_slug.remove(&old);
                self.obj_of_slug.insert(slug.clone(), o);
            }
            match shape {
                Shape::Inline(c, port) => {
                    let decl = self.block_decls.get_mut(&c).expect("managed");
                    for arg in &mut decl.args {
                        if let ArgItem::Binding(b) = arg
                            && b.port == port
                        {
                            b.kind = BindingKind::Expr(e.clone());
                        }
                    }
                }
                Shape::Wire(wi) => {
                    lift.wire_exprs.insert(wi, e);
                }
                Shape::Signal(title) => {
                    let slug = self.slug_of[&self.objects[r].uuid].clone();
                    let mut items = vec![Item::Signal(SignalDecl {
                        slug: slug.clone(),
                        expr: e,
                        comment: None,
                    })];
                    if let Some(t) = title {
                        items.push(Item::Attr(AttrDecl {
                            slug,
                            attr: Attr::Title,
                            value: Value::Str(t),
                            comment: None,
                            bulk: None,
                        }));
                    }
                    lift.signals.insert(r, items);
                }
            }
        }
        lift.lets = lets
            .into_iter()
            .map(|(name, value)| {
                let pages = let_pages.remove(&name).unwrap_or_default();
                (
                    LetDecl {
                        name,
                        value,
                        comment: None,
                    },
                    pages,
                )
            })
            .collect();
        self.exprs = lift;
        self.factored = self.block_decls.clone();
        let after = self.verdict();
        if after.is_err() && after != before {
            (self.block_decls, self.slug_of, self.obj_of_slug) = saved;
            self.exprs = ExprLift::default();
        }
    }

    /// Whether the view as it stands checks, and if not, why.
    fn verdict(&self) -> std::result::Result<(), String> {
        let module = self.single_module();
        module.validate().map_err(|e| e.to_string())?;
        let (m, _) = module.desugar().map_err(|e| e.to_string())?;
        crate::logic::validate_ports(&m).map_err(|e| e.to_string())
    }

    /// Every top of a gate chain, consumers before what feeds them. `f`
    /// gets each one that compiles back exactly (in `strict` mode) or up
    /// to the operand names; it returns whether it takes the candidate,
    /// which then claims its blocks.
    fn walk_candidates(&self, strict: bool, f: &mut dyn FnMut(usize, &Expr, Shape, Found) -> bool) {
        let refs = self.refs();
        let consumers = self.consumers();
        let liftable: Vec<usize> = self
            .managed
            .iter()
            .copied()
            .filter(|&i| {
                let t = self.objects[i].block_type.as_str();
                GATES.contains(&t) || t == "Formula"
            })
            .collect();
        let mut taken: BTreeSet<usize> = BTreeSet::new();
        let mut inlined: BTreeSet<Sink> = BTreeSet::new();
        let mut pending: Vec<usize> = liftable.clone();
        while !pending.is_empty() {
            // A block whose consumers are all settled goes first, so a
            // chain is taken from its top. Feedback falls back to order.
            let k = pending
                .iter()
                .position(|&i| {
                    consumers
                        .get(&i)
                        .into_iter()
                        .flatten()
                        .all(|(s, _)| match s {
                            Sink::Block(c, _) => !pending.contains(c),
                            Sink::Wire(_) => true,
                        })
                })
                .unwrap_or(0);
            let r = pending.remove(k);
            if taken.contains(&r) {
                continue;
            }
            let Some((e, title)) = self.candidate(r, &refs) else {
                continue;
            };
            let Some(shape) = self.shape(r, title, &refs, &consumers, &taken, &inlined) else {
                continue;
            };
            let Some(found) = self.check(r, &e, &shape, &refs, strict) else {
                continue;
            };
            let blocks: Vec<usize> = found.blocks.iter().map(|(o, _)| *o).collect();
            if blocks.iter().any(|o| taken.contains(o)) {
                continue;
            }
            let sink = match &shape {
                Shape::Inline(c, p) => Some(Sink::Block(*c, p.clone())),
                Shape::Wire(wi) => Some(Sink::Wire(*wi)),
                Shape::Signal(_) => None,
            };
            if f(r, &e, shape, found) {
                taken.extend(blocks);
                inlined.extend(sink);
            }
        }
    }

    /// The expression `r` stands for, and the title of its own its top
    /// block has: its label, or for a top block with a title of its own,
    /// the expression its inputs form (with at least one sub-block).
    fn candidate(&self, r: usize, refs: &Refs) -> Option<(Expr, Option<String>)> {
        let ty = self.objects[r].block_type.as_str();
        if let Some(e) = self.label(r) {
            return Some((e, None));
        }
        let title = self.objects[r].title.clone().filter(|t| !t.is_empty())?;
        let decl = &self.block_decls[&r];
        let one = |port: &str| -> Option<&BindingKind> {
            let mut it = decl.bindings().filter(|b| b.port == port);
            let b = it.next()?;
            it.next().is_none().then_some(&b.kind)
        };
        let mut sub = false;
        let mut gate_input = |port: &str| -> Option<Expr> {
            let BindingKind::Wire(src) = one(port)? else {
                return None;
            };
            match self.sub_block(r, port, None, refs) {
                Some(g) => {
                    sub = true;
                    self.label(g)
                }
                None => Some(Expr::Atom(Operand::Port(src.clone()))),
            }
        };
        let e = match ty {
            "And" | "Or" => {
                let (a, b) = (gate_input("I1")?, gate_input("I2")?);
                if ty == "And" {
                    Expr::And(Box::new(a), Box::new(b))
                } else {
                    Expr::Or(Box::new(a), Box::new(b))
                }
            }
            "Not" => Expr::Not(Box::new(gate_input("I")?)),
            // A comparison takes no sub-blocks: on its own it is a block
            // drawn in Loxone Config as much as an expression.
            _ => return None,
        };
        sub.then_some((e, Some(title)))
    }

    /// `o`'s label read as the expression of its own type.
    fn label(&self, o: usize) -> Option<Expr> {
        let e = expr_text(self.objects[o].title.as_deref()?)?;
        (e.top_block_type() == Some(self.objects[o].block_type.as_str())).then_some(e)
    }

    /// The block behind `parent`'s input `port` when it can be folded into
    /// `parent`'s expression: the port's one wire, from the `Q` of a gate
    /// with a label of its own type (`of`, when given), on the same page,
    /// referenced nowhere else.
    fn sub_block(
        &self,
        parent: usize,
        port: &str,
        of: Option<&Expr>,
        refs: &Refs,
    ) -> Option<usize> {
        let mut wires = self.block_decls[&parent]
            .bindings()
            .filter(|b| b.port == port);
        let (Some(b), None) = (wires.next(), wires.next()) else {
            return None;
        };
        let BindingKind::Wire(src) = &b.kind else {
            return None;
        };
        let &g = self.obj_of_slug.get(&src.slug)?;
        let ty = self.objects[g].block_type.as_str();
        let ok = src.port == "Q"
            && GATES.contains(&ty)
            && self.block_decls.contains_key(&g)
            && of.is_none_or(|e| e.top_block_type() == Some(ty))
            && refs.total.get(&src.slug) == Some(&1)
            && self.page_of(g) == self.page_of(parent)
            && self.label(g).is_some();
        ok.then_some(g)
    }

    /// The objects of `e`, rooted at `o`, in desugar order (post-order):
    /// each compound operand is the sub-block at its input.
    fn chain(&self, e: &Expr, o: usize, refs: &Refs, out: &mut Vec<usize>) -> Option<()> {
        let inputs: Vec<(&str, &Expr)> = match e {
            Expr::And(a, b) | Expr::Or(a, b) => vec![("I1", a), ("I2", b)],
            Expr::Not(x) => vec![("I", x)],
            Expr::Cmp { .. } | Expr::Arith { .. } => Vec::new(),
            Expr::Atom(_) => return None,
        };
        for (port, child) in inputs {
            if matches!(child, Expr::Atom(_)) {
                continue;
            }
            let g = self.sub_block(o, port, Some(child), refs)?;
            self.chain(child, g, refs, out)?;
        }
        out.push(o);
        Some(())
    }

    /// How the expression at `r` is written, if it can be.
    fn shape(
        &self,
        r: usize,
        title: Option<String>,
        refs: &Refs,
        consumers: &BTreeMap<usize, Vec<(Sink, String)>>,
        taken: &BTreeSet<usize>,
        inlined: &BTreeSet<Sink>,
    ) -> Option<Shape> {
        let slug = &self.slug_of[&self.objects[r].uuid];
        let out = if self.objects[r].block_type == "Formula" {
            "AQ"
        } else {
            "Q"
        };
        let total = refs.total.get(slug).copied().unwrap_or(0);
        let at_out = refs
            .port
            .get(&(slug.clone(), out.to_string()))
            .copied()
            .unwrap_or(0);
        let cons = consumers.get(&r).map(Vec::as_slice).unwrap_or_default();
        if title.is_none()
            && total == 1
            && let [(sink, port)] = cons
            && port == out
            && !inlined.contains(sink)
        {
            match sink {
                Sink::Block(c, p) => {
                    let ty = self.objects[*c].block_type.as_str();
                    let fan_in = self.block_decls[c]
                        .bindings()
                        .filter(|b| b.port == *p)
                        .count();
                    // A gate consumer may still fold `r` in itself.
                    if !taken.contains(c)
                        && !GATES.contains(&ty)
                        && ty != "Formula"
                        && fan_in == 1
                        && self.page_of(*c) == self.page_of(r)
                    {
                        return Some(Shape::Inline(*c, p.clone()));
                    }
                }
                Sink::Wire(wi) => {
                    let (_, ti, fi) = &self.wires[*wi];
                    if self.page_of(*ti).or_else(|| self.page_of(*fi)) == self.page_of(r) {
                        return Some(Shape::Wire(*wi));
                    }
                }
            }
        }
        // A signal is referenced by its output only.
        (total == at_out).then_some(Shape::Signal(title))
    }

    /// Desugar the candidate and compare it with the config's blocks. In
    /// `strict` mode only an exact match counts; otherwise operands may
    /// name other slugs than the lift's, and those names come back as
    /// hints.
    fn check(&self, r: usize, e: &Expr, shape: &Shape, refs: &Refs, strict: bool) -> Option<Found> {
        let mut order = Vec::new();
        self.chain(e, r, refs, &mut order)?;
        let root_slug = self.slug_of[&self.objects[r].uuid].clone();
        let (item, sink_slug) = match shape {
            Shape::Inline(c, port) => {
                let decl = &self.block_decls[c];
                (
                    Item::Block(BlockDecl {
                        slug: decl.slug.clone(),
                        block_type: decl.block_type.clone(),
                        title: None,
                        args: vec![ArgItem::Binding(Binding {
                            port: port.clone(),
                            kind: BindingKind::Expr(e.clone()),
                            comment: None,
                        })],
                        comment: None,
                        close_comment: None,
                    }),
                    Some(decl.slug.clone()),
                )
            }
            Shape::Wire(wi) => (
                Item::ExprWire(ExprWireDecl {
                    to: self.wires[*wi].0.to.clone(),
                    expr: e.clone(),
                    comment: None,
                }),
                None,
            ),
            Shape::Signal(_) => (
                Item::Signal(SignalDecl {
                    slug: root_slug.clone(),
                    expr: e.clone(),
                    comment: None,
                }),
                None,
            ),
        };
        let mut items = vec![item];
        if let Shape::Signal(Some(t)) = shape {
            items.push(Item::Attr(AttrDecl {
                slug: root_slug,
                attr: Attr::Title,
                value: Value::Str(t.clone()),
                comment: None,
                bulk: None,
            }));
        }
        let (plain, _) = Module { items }.desugar().ok()?;
        let made: Vec<&BlockDecl> = plain
            .blocks()
            .filter(|b| Some(&b.slug) != sink_slug.as_ref())
            .collect();
        if made.len() != order.len() {
            return None;
        }
        let rename: BTreeMap<String, String> = order
            .iter()
            .zip(&made)
            .map(|(&o, d)| (self.slug_of[&self.objects[o].uuid].clone(), d.slug.clone()))
            .collect();
        let mut found = Found {
            blocks: order
                .iter()
                .zip(&made)
                .map(|(&o, d)| (o, d.slug.clone()))
                .collect(),
            lets: BTreeMap::new(),
            hints: Vec::new(),
        };
        for (&o, d) in order.iter().zip(&made) {
            self.same_block(d, o, &rename, strict, &mut found)?;
        }
        Some(found)
    }

    /// Whether the desugared block `d` is object `o` as the lift declares
    /// it. Records the constants and (outside `strict`) the operand names
    /// it takes on the way.
    fn same_block(
        &self,
        d: &BlockDecl,
        o: usize,
        rename: &BTreeMap<String, String>,
        strict: bool,
        found: &mut Found,
    ) -> Option<()> {
        let obj = &self.objects[o];
        let orig = &self.block_decls[&o];
        if d.block_type != obj.block_type
            || d.title != obj.title
            || orig.comment.is_some()
            || orig.close_comment.is_some()
            || orig.args.iter().any(|a| matches!(a, ArgItem::Comment(_)))
        {
            return None;
        }
        let ty = obj.block_type.as_str();
        let empty = Module { items: Vec::new() };
        let resolve = |v: &Value, port: &str| -> Option<String> {
            empty
                .resolve_value_for(v, ty, &d.slug, port)
                .ok()
                .map(|s| s.into_owned())
        };
        let synthetic: BTreeSet<&String> = rename.values().collect();
        let mut theirs: BTreeMap<&str, Vec<&Binding>> = BTreeMap::new();
        for b in orig.bindings() {
            if b.comment.is_some() {
                return None;
            }
            theirs.entry(b.port.as_str()).or_default().push(b);
        }
        let mut ours: BTreeMap<&str, Vec<&Binding>> = BTreeMap::new();
        for b in d.bindings() {
            ours.entry(b.port.as_str()).or_default().push(b);
        }
        for (port, mine) in &ours {
            let [mine] = mine.as_slice() else {
                return None;
            };
            let theirs = match theirs.remove(port).as_deref() {
                Some([t]) => Some(*t),
                None => None,
                Some(_) => return None,
            };
            match (&mine.kind, theirs.map(|t| &t.kind)) {
                (BindingKind::Wire(a), Some(BindingKind::Wire(b))) => {
                    let b_slug = rename.get(&b.slug).unwrap_or(&b.slug);
                    if a.port != b.port {
                        return None;
                    }
                    if a.slug != *b_slug {
                        // An operand the label names otherwise: the
                        // second scan may give it that name.
                        if strict || synthetic.contains(&a.slug) || synthetic.contains(b_slug) {
                            return None;
                        }
                        found
                            .hints
                            .push((*self.obj_of_slug.get(&b.slug)?, a.slug.clone()));
                    }
                }
                (BindingKind::Param(Value::Ref(name)), Some(BindingKind::Param(v))) => {
                    match found.lets.get(name) {
                        Some(w) if w != v => return None,
                        _ => {
                            found.lets.insert(name.clone(), v.clone());
                        }
                    }
                }
                (BindingKind::Param(a), Some(BindingKind::Param(b))) => {
                    if resolve(a, port)? != resolve(b, port)? {
                        return None;
                    }
                }
                // The full view leaves out a value at its usual default.
                (BindingKind::Param(a), None) => {
                    let elided = self.full_view
                        && crate::observed_defaults::observed_default(ty, port)
                            == Some(resolve(a, port)?.as_str());
                    if !elided {
                        return None;
                    }
                }
                _ => return None,
            }
        }
        // Nothing the expression does not bind.
        theirs.is_empty().then_some(())
    }

    /// Who references each slug, from every statement of the view.
    fn refs(&self) -> Refs {
        let mut refs = Refs::default();
        for mut item in self.single_module().items {
            let _ = visit_refs(&mut item, &mut |slug, used| {
                *refs.total.entry(slug.clone()).or_default() += 1;
                if let RefUse::Port(p) = used {
                    *refs.port.entry((slug.clone(), p.to_string())).or_default() += 1;
                }
                Ok(())
            });
        }
        refs
    }

    /// Each managed block's consumers: the ports its outputs feed.
    fn consumers(&self) -> BTreeMap<usize, Vec<(Sink, String)>> {
        let mut out: BTreeMap<usize, Vec<(Sink, String)>> = BTreeMap::new();
        for &c in &self.managed {
            for b in self.block_decls[&c].bindings() {
                if let BindingKind::Wire(src) = &b.kind
                    && let Some(&g) = self.obj_of_slug.get(&src.slug)
                {
                    out.entry(g)
                        .or_default()
                        .push((Sink::Block(c, b.port.clone()), src.port.clone()));
                }
            }
        }
        for (wi, (w, _, fi)) in self.wires.iter().enumerate() {
            out.entry(*fi)
                .or_default()
                .push((Sink::Wire(wi), w.from.port.clone()));
        }
        out
    }
}
