//! Lowering of the two binding forms into one: an extern's argument list
//! (`extern jal = AutoJalousie(title: "Süd", AutoShade: beschatten,
//! TargetPos: 70)`) becomes the wire and assignment statements the
//! compile tracks, and a remote statement on a managed block
//! (`b.In1 <- a`, `b.Time = 5s`) becomes an argument of that block.
//!
//! The source keeps what the author wrote; `fmt` prints it back. The
//! lowered module is what the checks, the expansion and the compile see,
//! so "port P of X is fed by Y" has one meaning however it is spelled.

use super::ast::{
    ArgItem, Binding, BindingKind, ExprWireDecl, Item, Module, PortRef, SetDecl, Value, WireDecl,
};
use std::collections::{BTreeMap, BTreeSet};

impl Module {
    /// The module with every extern's port bindings turned into `<-`
    /// and `=` statements right after the declaration.
    pub fn lowered(&self) -> Module {
        let mut m = self.clone();
        m.lower_extern_args();
        m
    }

    /// Turn every extern's port bindings into statements (in place).
    pub(crate) fn lower_extern_args(&mut self) {
        let constants: BTreeSet<String> = self
            .lets()
            .filter(|l| !matches!(l.value, Value::Port(_)))
            .map(|l| l.name.clone())
            .collect();
        let mut out = Vec::with_capacity(self.items.len());
        for item in std::mem::take(&mut self.items) {
            let Item::Extern(mut e) = item else {
                out.push(item);
                continue;
            };
            let args = std::mem::take(&mut e.args);
            let slug = e.slug.clone();
            out.push(Item::Extern(e));
            for arg in args {
                let ArgItem::Binding(b) = arg else { continue };
                let to = PortRef {
                    slug: slug.clone(),
                    port: b.port,
                };
                out.push(match b.kind {
                    BindingKind::Wire(from) => Item::Wire(WireDecl {
                        to,
                        from,
                        comment: b.comment,
                    }),
                    BindingKind::Expr(expr) => Item::ExprWire(ExprWireDecl {
                        to,
                        expr,
                        comment: b.comment,
                    }),
                    // A bare name that is no constant: a block, extern or
                    // alias, standing for its default output.
                    BindingKind::Param(Value::Ref(name)) if !constants.contains(&name) => {
                        Item::Wire(WireDecl {
                            to,
                            from: PortRef {
                                slug: name,
                                port: String::new(),
                            },
                            comment: b.comment,
                        })
                    }
                    BindingKind::Param(value) => Item::Set(SetDecl {
                        target: to,
                        value,
                        comment: b.comment,
                        bulk: None,
                    }),
                });
            }
        }
        self.items = out;
    }

    /// Move every `<-`, expression wire and `=` whose sink is a managed
    /// block of this (expanded) module into that block's argument list.
    pub(crate) fn fold_remote_bindings(&mut self) {
        let blocks: BTreeMap<String, usize> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| match item {
                Item::Block(b) => Some((b.slug.clone(), i)),
                _ => None,
            })
            .collect();
        if blocks.is_empty() {
            return;
        }
        let mut folded: Vec<(usize, Binding)> = Vec::new();
        let mut keep = Vec::with_capacity(self.items.len());
        for item in std::mem::take(&mut self.items) {
            let (sink, binding) = match &item {
                Item::Wire(w) if blocks.contains_key(&w.to.slug) => (
                    &w.to,
                    Binding {
                        port: w.to.port.clone(),
                        kind: BindingKind::Wire(w.from.clone()),
                        comment: w.comment.clone(),
                    },
                ),
                Item::ExprWire(w) if blocks.contains_key(&w.to.slug) => (
                    &w.to,
                    Binding {
                        port: w.to.port.clone(),
                        kind: BindingKind::Expr(w.expr.clone()),
                        comment: w.comment.clone(),
                    },
                ),
                Item::Set(s) if blocks.contains_key(&s.target.slug) => (
                    &s.target,
                    Binding {
                        port: s.target.port.clone(),
                        kind: BindingKind::Param(s.value.clone()),
                        comment: s.comment.clone(),
                    },
                ),
                _ => {
                    keep.push(item);
                    continue;
                }
            };
            folded.push((blocks[&sink.slug], binding));
        }
        // Indexes shift as items leave: fold by the block's slug.
        let mut by_slug: BTreeMap<String, Vec<Binding>> = BTreeMap::new();
        let slugs: Vec<String> = blocks.keys().cloned().collect();
        for (i, b) in folded {
            let slug = slugs.iter().find(|s| blocks[*s] == i).expect("indexed");
            by_slug.entry(slug.clone()).or_default().push(b);
        }
        for item in &mut keep {
            if let Item::Block(b) = item
                && let Some(more) = by_slug.remove(&b.slug)
            {
                b.args.extend(more.into_iter().map(ArgItem::Binding));
            }
        }
        self.items = keep;
    }
}
