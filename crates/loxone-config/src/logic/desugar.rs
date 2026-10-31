//! Expression desugaring (D24): `sink.Port <- <expr>` becomes managed
//! gate/comparator blocks plus a plain wire onto the sink.
//!
//! Runs after template expansion and validation, before port validation
//! and compile — everything downstream sees only plain statements. Every
//! operator node becomes one block of a live-verified builtin type:
//! `and`/`or` → `And`/`Or` (fixed 2-input, chains cascade), `not` → `Not`,
//! comparisons → the comparator family. Operand order is preserved
//! (lhs → `Input1`, rhs → `Input2`); constant operands become `Def=`
//! parameters, port operands become wires.
//!
//! Arithmetic (`+ - * /`) is the exception to one-block-per-operator: a
//! maximal arithmetic tree becomes ONE `Formula` block — distinct port
//! operands map to `Input1..Input4` in first-appearance order (a repeated
//! port reuses its input), constants are inlined into the compact formula
//! text (`I1/1000`), and the result is the block's `AQ`.
//!
//! Synthetic slugs are `<sink>_<port>__<op><n>` (post-order, per-operator
//! counter), so an expression's identity is stable while its text is
//! unchanged; editing the expression re-derives the slugs and the compiler
//! auto-removes the orphaned ones (they are marked expression-owned in the
//! lockfile — no `removed` statement needed, because no hand ever wrote
//! those blocks).

use super::ast::*;
use crate::error::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};

/// What [`Module::desugar`] produced — for lock ownership marking and
/// `check` reporting.
#[derive(Debug, Default)]
pub struct DesugarInfo {
    /// Number of expressions desugared — `<-` statements and argument-list
    /// expression bindings alike.
    pub expressions: usize,
    /// Slugs of all synthetic blocks.
    pub synthetic: BTreeSet<String>,
}

impl Module {
    /// Replace every expression — `<-` statements (D24) and argument-list
    /// expression bindings (D26) — with its discrete-backend expansion.
    /// A module without expressions comes back unchanged (cloned).
    pub fn desugar(&self) -> Result<(Module, DesugarInfo)> {
        let mut info = DesugarInfo::default();
        if self.expr_wires().next().is_none()
            && !self.blocks().any(|b| b.expr_bindings().next().is_some())
        {
            return Ok((self.clone(), info));
        }

        // Every declared name — a synthetic slug colliding with one is an
        // error naming the expression, not a bare duplicate-name message.
        let taken: BTreeSet<&str> = self
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Extern(e) => Some(e.slug.as_str()),
                Item::Block(b) | Item::Instance(b) => Some(b.slug.as_str()),
                Item::Let(l) => Some(l.name.as_str()),
                Item::Template(t) => Some(t.name.as_str()),
                Item::Tiles(t) => Some(t.name.as_str()),
                Item::Schedule(t) => Some(t.name.as_str()),
                Item::Moods(t) => Some(t.name.as_str()),
                _ => None,
            })
            .collect();

        // `let` constants, for inlining into formula text.
        let lets: BTreeMap<&str, &Value> =
            self.lets().map(|l| (l.name.as_str(), &l.value)).collect();

        let mut items = Vec::with_capacity(self.items.len());
        let mut minted: Vec<String> = Vec::new();
        // Counters persist across expressions sharing one sink prefix, so
        // two expressions fanning into the same port number on.
        let mut counters: BTreeMap<(String, &'static str), usize> = BTreeMap::new();
        for item in &self.items {
            match item {
                Item::ExprWire(w) => {
                    info.expressions += 1;
                    let mut ctx = Desugar {
                        prefix: format!("{}_{}__", w.to.slug, w.to.port.to_lowercase()),
                        sink: &w.to,
                        counters: &mut counters,
                        out: &mut items,
                        minted: &mut minted,
                        taken: &taken,
                        lets: &lets,
                    };
                    let root = ctx.node(&w.expr)?;
                    items.push(Item::Wire(WireDecl {
                        to: w.to.clone(),
                        from: root,
                        comment: w.comment.clone(),
                    }));
                }
                // A block with expression bindings: each expression's
                // blocks are emitted ahead of the declaration, and the
                // binding becomes a plain wire from the root's `Q`.
                Item::Block(b) if b.expr_bindings().next().is_some() => {
                    let mut args = Vec::with_capacity(b.args.len());
                    for arg in &b.args {
                        let ArgItem::Binding(x) = arg else {
                            args.push(arg.clone());
                            continue;
                        };
                        let BindingKind::Expr(e) = &x.kind else {
                            args.push(arg.clone());
                            continue;
                        };
                        info.expressions += 1;
                        let sink = PortRef {
                            slug: b.slug.clone(),
                            port: x.port.clone(),
                        };
                        let mut ctx = Desugar {
                            prefix: format!("{}_{}__", sink.slug, sink.port.to_lowercase()),
                            sink: &sink,
                            counters: &mut counters,
                            out: &mut items,
                            minted: &mut minted,
                            taken: &taken,
                            lets: &lets,
                        };
                        let root = ctx.node(e)?;
                        args.push(ArgItem::Binding(Binding {
                            port: x.port.clone(),
                            kind: BindingKind::Wire(root),
                            comment: x.comment.clone(),
                        }));
                    }
                    items.push(Item::Block(BlockDecl { args, ..b.clone() }));
                }
                other => items.push(other.clone()),
            }
        }
        info.synthetic = minted.into_iter().collect();
        Ok((Module { items }, info))
    }
}

struct Desugar<'a> {
    prefix: String,
    sink: &'a PortRef,
    /// Per-(sink, operator) counters (`__ge1`, `__ge2`, …), shared across
    /// the module's expressions.
    counters: &'a mut BTreeMap<(String, &'static str), usize>,
    out: &'a mut Vec<Item>,
    minted: &'a mut Vec<String>,
    taken: &'a BTreeSet<&'a str>,
    /// Module `let` constants — arithmetic inlines them into the formula
    /// text, so they must resolve to plain numbers here.
    lets: &'a BTreeMap<&'a str, &'a Value>,
}

impl Desugar<'_> {
    /// Emit the blocks for one node, post-order; the returned port is the
    /// node's boolean output.
    fn node(&mut self, e: &Expr) -> Result<PortRef> {
        match e {
            Expr::And(a, b) | Expr::Or(a, b) => {
                let (op, ty) = if matches!(e, Expr::And(..)) {
                    ("and", "And")
                } else {
                    ("or", "Or")
                };
                let i1 = self.node(a)?;
                let i2 = self.node(b)?;
                self.emit(op, ty, e, vec![wire("I1", i1), wire("I2", i2)], "Q")
            }
            Expr::Not(x) => {
                let i = self.node(x)?;
                self.emit("not", "Not", e, vec![wire("I", i)], "Q")
            }
            Expr::Cmp { op, lhs, rhs } => {
                if !matches!(lhs, Operand::Port(_)) && !matches!(rhs, Operand::Port(_)) {
                    return Err(Error::Compile(format!(
                        "`{lhs} {} {rhs}` in the expression on `{}` compares two \
                         constants. Fold it by hand",
                        op.symbol(),
                        self.sink,
                    )));
                }
                let (opname, ty) = match op {
                    CmpOp::Ge => ("ge", "GreaterEqual"),
                    CmpOp::Gt => ("gt", "Greater"),
                    CmpOp::Le => ("le", "LessEqual"),
                    CmpOp::Lt => ("lt", "Less"),
                    CmpOp::Eq => ("eq", "Equal"),
                    CmpOp::Ne => ("ne", "NotEqual"),
                };
                self.emit(
                    opname,
                    ty,
                    e,
                    vec![operand("Input1", lhs), operand("Input2", rhs)],
                    "Q",
                )
            }
            Expr::Arith { .. } => self.formula(e),
            Expr::Atom(Operand::Port(p)) => Ok(p.clone()),
            Expr::Atom(Operand::Value(v)) => Err(Error::Compile(format!(
                "`{v}` in the expression on `{}` cannot drive a gate input, since gates \
                 take boolean ports. Write a comparison like `x.AQ >= {v}`",
                self.sink,
            ))),
        }
    }

    /// One `Formula` block for a maximal arithmetic tree.
    fn formula(&mut self, e: &Expr) -> Result<PortRef> {
        let mut inputs: Vec<PortRef> = Vec::new();
        for o in e.operands() {
            if let Operand::Port(p) = o
                && !inputs.contains(p)
            {
                inputs.push(p.clone());
            }
        }
        if inputs.is_empty() {
            return Err(Error::Compile(format!(
                "`{e}` in the expression on `{}` computes on constants only. \
                 Fold it by hand",
                self.sink,
            )));
        }
        if inputs.len() > 4 {
            return Err(Error::Compile(format!(
                "`{e}` on `{}` reads {} distinct ports, but a `Formula` block takes \
                 at most 4 inputs. Split the expression or declare the block by \
                 hand",
                self.sink,
                inputs.len(),
            )));
        }
        let text = self.render(e, &inputs)?;
        let mut args = vec![ArgItem::Binding(Binding {
            port: "Formula".to_string(),
            kind: BindingKind::Param(Value::Str(text)),
            comment: None,
        })];
        for (i, p) in inputs.iter().enumerate() {
            args.push(wire(&format!("Input{}", i + 1), p.clone()));
        }
        self.emit("f", "Formula", e, args, "AQ")
    }

    /// The Loxone formula text: compact (no spaces), port operands as
    /// `I<n>`, constants inlined, parens only where precedence needs them.
    fn render(&self, e: &Expr, inputs: &[PortRef]) -> Result<String> {
        Ok(match e {
            Expr::Arith { op, lhs, rhs } => {
                let l = self.render_child(lhs, e, false, inputs)?;
                let r = self.render_child(rhs, e, true, inputs)?;
                format!("{l}{}{r}", op.symbol())
            }
            Expr::Atom(Operand::Port(p)) => {
                let i = inputs.iter().position(|x| x == p).expect("collected above") + 1;
                format!("I{i}")
            }
            Expr::Atom(Operand::Value(v)) => {
                let n = self.constant(v)?;
                // `I1*(-5)` — a bare `-` mid-formula would read as binary.
                if n.starts_with('-') {
                    format!("({n})")
                } else {
                    n
                }
            }
            other => {
                return Err(Error::Compile(format!(
                    "`{other}` mixes boolean logic into the arithmetic on `{}`, which Lox Logic \
                     does not support yet. Compute the boolean part in its own block",
                    self.sink,
                )));
            }
        })
    }

    fn render_child(
        &self,
        child: &Expr,
        parent: &Expr,
        right: bool,
        inputs: &[PortRef],
    ) -> Result<String> {
        let s = self.render(child, inputs)?;
        // Same minimal-paren rule as the canonical text form: equal
        // precedence needs parens on the right only (left-associative).
        let needs = if right {
            child.prec() <= parent.prec()
        } else {
            child.prec() < parent.prec()
        };
        Ok(if needs { format!("({s})") } else { s })
    }

    /// A constant operand as formula text — plain numbers only; `let`
    /// references are inlined (their values are always literals).
    fn constant(&self, v: &Value) -> Result<String> {
        match v {
            Value::Number(n) => Ok(n.clone()),
            Value::Ref(name) => match self.lets.get(name.as_str()) {
                Some(Value::Number(n)) => Ok(n.clone()),
                Some(other) => Err(Error::Compile(format!(
                    "`{name}` in the arithmetic on `{}` is `{other}`. Formulas \
                     compute on plain numbers",
                    self.sink,
                ))),
                None => Err(Error::Compile(format!(
                    "unknown constant `{name}` in the arithmetic on `{}`",
                    self.sink,
                ))),
            },
            Value::Unit { .. } => Err(Error::Compile(format!(
                "`{v}` in the arithmetic on `{}` carries a unit, but formulas \
                 compute on plain numbers. Write the value in the port's base \
                 unit instead",
                self.sink,
            ))),
            Value::Str(s) => Err(Error::Compile(format!(
                "`\"{s}\"` in the arithmetic on `{}` is a string. Formulas \
                 compute on plain numbers",
                self.sink,
            ))),
            Value::File { .. } => Err(Error::Compile(format!(
                "`{v}` in the arithmetic on `{}` is a file. Formulas compute \
                 on plain numbers",
                self.sink,
            ))),
            Value::Clock(_) => Err(Error::Compile(format!(
                "`{v}` in the arithmetic on `{}` is a time of day. Formulas \
                 compute on plain numbers",
                self.sink,
            ))),
            Value::Bool(_) => Err(Error::Compile(format!(
                "`{v}` in the arithmetic on `{}` is a digital value, but formulas \
                 compute on plain numbers. Write `1` or `0`",
                self.sink,
            ))),
        }
    }

    fn emit(
        &mut self,
        op: &'static str,
        ty: &str,
        node: &Expr,
        args: Vec<ArgItem>,
        out_port: &str,
    ) -> Result<PortRef> {
        let n = self.counters.entry((self.prefix.clone(), op)).or_insert(0);
        *n += 1;
        let slug = format!("{}{op}{}", self.prefix, n);
        if self.taken.contains(slug.as_str()) || self.minted.contains(&slug) {
            return Err(Error::Compile(format!(
                "`{slug}` is claimed by the expression on `{}` (its blocks are named \
                 `{}<op><n>`). Rename the declared `{slug}`",
                self.sink, self.prefix,
            )));
        }
        self.out.push(Item::Block(BlockDecl {
            slug: slug.clone(),
            block_type: ty.to_string(),
            // The canvas shows the block type; the label carries the
            // sub-expression so the rule stays readable in Loxone Config.
            title: Some(node.to_string()),
            args,
            comment: None,
            close_comment: None,
        }));
        self.minted.push(slug.clone());
        Ok(PortRef {
            slug,
            port: out_port.to_string(),
        })
    }
}

fn wire(port: &str, src: PortRef) -> ArgItem {
    ArgItem::Binding(Binding {
        port: port.to_string(),
        kind: BindingKind::Wire(src),
        comment: None,
    })
}

fn operand(port: &str, o: &Operand) -> ArgItem {
    ArgItem::Binding(Binding {
        port: port.to_string(),
        kind: match o {
            Operand::Port(p) => BindingKind::Wire(p.clone()),
            Operand::Value(v) => BindingKind::Param(v.clone()),
        },
        comment: None,
    })
}
