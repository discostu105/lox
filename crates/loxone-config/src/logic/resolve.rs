//! Name resolution of the value positions: a bare name in an argument,
//! an operand, the source of `<-` or a test statement is a `let`
//! constant, a `let` alias of a port (`let aussen = sensor.AQ`), or a
//! block, extern or signal, which stands for its default output
//! (`sonne` is `sonne.Q`; [`crate::port_names::default_output`]).
//!
//! Runs on the expanded module, before desugaring, so a template body's
//! object parameters have become names. The source keeps what the
//! author wrote; only the expanded module carries the resolved ports.

use super::ast::{
    ArgItem, BindingKind, Expr, Item, Module, Operand, PortRef, TestItem, Value, WireDecl,
};
use crate::error::{Error, Result};
use crate::port_names::default_output;
use std::collections::BTreeMap;

/// What a bare name stands for.
enum Named {
    Constant,
    Alias(PortRef),
    Object(String),
}

impl Module {
    /// Resolve the bare names of the expanded module in place.
    pub(crate) fn resolve_names(&mut self) -> Result<()> {
        let mut names: BTreeMap<String, Named> = BTreeMap::new();
        for item in &self.items {
            match item {
                Item::Extern(e) => {
                    names.insert(e.slug.clone(), Named::Object(e.block_type.clone()));
                }
                Item::Block(b) => {
                    names.insert(b.slug.clone(), Named::Object(b.block_type.clone()));
                }
                Item::Signal(x) => {
                    let ty = x.expr.top_block_type().unwrap_or("").to_string();
                    names.insert(x.slug.clone(), Named::Object(ty));
                }
                Item::Let(l) => {
                    names.insert(
                        l.name.clone(),
                        match &l.value {
                            Value::Port(p) => Named::Alias(p.clone()),
                            _ => Named::Constant,
                        },
                    );
                }
                _ => {}
            }
        }
        let mut errors = crate::error::Errors::default();
        let cx = Resolver { names: &names };
        for item in &mut self.items {
            errors.check(cx.item(item));
        }
        errors.finish()
    }
}

struct Resolver<'a> {
    names: &'a BTreeMap<String, Named>,
}

impl Resolver<'_> {
    fn fail<T>(msg: String) -> Result<T> {
        Err(Error::Compile(msg))
    }

    /// The port a bare `name` stands for in `at`: an alias's port, an
    /// object's default output. `Ok(None)` for a constant or an
    /// undeclared name (the check reports that one).
    fn port_of(&self, name: &str, at: &str) -> Result<Option<PortRef>> {
        Ok(match self.names.get(name) {
            Some(Named::Alias(p)) => Some(p.clone()),
            Some(Named::Object(ty)) => match default_output(ty) {
                Some(port) => Some(PortRef {
                    slug: name.to_string(),
                    port: port.to_string(),
                }),
                None => {
                    let outputs: Vec<&str> = crate::connectors::builtin(ty)
                        .unwrap_or_default()
                        .iter()
                        .filter(|s| s.dir == crate::connectors::PortDir::Output)
                        .map(|s| s.name)
                        .collect();
                    let hint = if outputs.is_empty() {
                        String::new()
                    } else {
                        format!(" Its outputs: {}.", outputs.join(", "))
                    };
                    return Self::fail(format!(
                        "`{name}` in `{at}`: a {ty} has no default output, so name the \
                         port (`{name}.<Port>`).{hint}"
                    ));
                }
            },
            Some(Named::Constant) | None => None,
        })
    }

    /// A reference `slug.Port` must not go through an alias.
    fn check_ref(&self, r: &PortRef, at: &str) -> Result<()> {
        if let Some(Named::Alias(p)) = self.names.get(&r.slug)
            && !r.port.is_empty()
        {
            return Self::fail(format!(
                "`{r}` in `{at}`: `{}` is an alias of `{p}`, which has no ports of its own. \
                 Write `{}` or `{}.{}`.",
                r.slug, r.slug, p.slug, r.port
            ));
        }
        Ok(())
    }

    /// A test statement's `slug` or `slug.Port`.
    fn test_ref(&self, r: &mut PortRef, at: &str) -> Result<()> {
        if r.port.is_empty() {
            match self.port_of(&r.slug, at)? {
                Some(p) => *r = p,
                None => {
                    return Self::fail(format!(
                        "`{}` in `{at}` names no block, extern or alias",
                        r.slug
                    ));
                }
            }
        }
        self.check_ref(r, at)
    }

    fn operand(&self, o: &mut Operand, at: &str) -> Result<()> {
        match o {
            Operand::Value(Value::Ref(name)) => {
                if let Some(p) = self.port_of(name, at)? {
                    *o = Operand::Port(p);
                }
            }
            Operand::Port(p) => self.check_ref(p, at)?,
            Operand::Value(_) => {}
        }
        Ok(())
    }

    fn expr(&self, e: &mut Expr, at: &str) -> Result<()> {
        match e {
            Expr::Or(a, b) | Expr::And(a, b) | Expr::Arith { lhs: a, rhs: b, .. } => {
                self.expr(a, at)?;
                self.expr(b, at)
            }
            Expr::Not(x) => self.expr(x, at),
            Expr::Cmp { lhs, rhs, .. } => {
                self.operand(lhs, at)?;
                self.operand(rhs, at)
            }
            Expr::Atom(o) => self.operand(o, at),
        }
    }

    /// A value that must stay a value (`let`, `defaults`, an attribute,
    /// a port assignment): an alias is refused there.
    fn value(&self, v: &Value, at: &str) -> Result<()> {
        if let Value::Ref(name) = v
            && let Some(Named::Alias(p)) = self.names.get(name)
        {
            return Self::fail(format!(
                "`{name}` in `{at}` is an alias of `{p}`, a port, where a value is needed"
            ));
        }
        Ok(())
    }

    fn args(&self, args: &mut [ArgItem], slug: &str) -> Result<()> {
        for arg in args {
            let ArgItem::Binding(b) = arg else { continue };
            let at = format!("{slug}.{}", b.port);
            // A language-level key (`device:`, `mirrors:`, `switches:`)
            // names an object as such, never its output.
            let language_key = b.port.starts_with(|c: char| c.is_ascii_lowercase());
            match &mut b.kind {
                BindingKind::Param(Value::Ref(name)) if !language_key => {
                    if let Some(p) = self.port_of(name, &at)? {
                        b.kind = BindingKind::Wire(p);
                    }
                }
                BindingKind::Param(v) => self.value(v, &at)?,
                BindingKind::Wire(p) => self.check_ref(p, &at)?,
                BindingKind::Expr(e) => self.expr(e, &at)?,
            }
        }
        Ok(())
    }

    fn item(&self, item: &mut Item) -> Result<()> {
        match item {
            Item::Block(b) => {
                let slug = b.slug.clone();
                self.args(&mut b.args, &slug)
            }
            Item::Defaults(d) => {
                let at = format!("defaults {}", d.block_type);
                for (_, v) in d.params() {
                    self.value(v, &at)?;
                }
                Ok(())
            }
            Item::Wire(WireDecl { to, from, .. }) => {
                let at = format!("{to} <- {from}");
                self.check_ref(to, &at)?;
                if from.port.is_empty() {
                    match self.port_of(&from.slug, &at)? {
                        Some(p) => *from = p,
                        None if matches!(self.names.get(&from.slug), Some(Named::Constant)) => {
                            return Self::fail(format!(
                                "`{at}` wires the constant `{}`. To assign a value, write \
                                 `{to} = {}`.",
                                from.slug, from.slug
                            ));
                        }
                        None => {
                            return Self::fail(format!(
                                "`{at}`: `{}` names no block, extern or alias",
                                from.slug
                            ));
                        }
                    }
                }
                self.check_ref(from, &at)
            }
            Item::ExprWire(w) => {
                let at = w.to.to_string();
                self.check_ref(&w.to, &at)?;
                self.expr(&mut w.expr, &at)
            }
            Item::Signal(x) => {
                let at = x.slug.clone();
                self.expr(&mut x.expr, &at)
            }
            Item::Set(s) => {
                let at = s.target.to_string();
                self.check_ref(&s.target, &at)?;
                self.value(&s.value, &at)
            }
            Item::Attr(a) => self.value(&a.value, &a.to_string()),
            Item::Test(t) => {
                let at = format!("test {}", super::ast::quote(&t.name));
                for stmt in &mut t.body {
                    match stmt {
                        TestItem::Inject(s) => {
                            self.test_ref(&mut s.target, &at)?;
                            self.value(&s.value, &at)?;
                        }
                        TestItem::Expect(e) => {
                            self.test_ref(&mut e.port, &at)?;
                            self.value(&e.value, &at)?;
                        }
                        TestItem::Tick(_) | TestItem::Clock(_) | TestItem::Comment(_) => {}
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
