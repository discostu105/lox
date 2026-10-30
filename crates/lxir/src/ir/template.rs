//! Template expansion (D23): a pure source-to-source pass replacing
//! `template` declarations and their instantiations with plain statements
//! before anything downstream runs. Body slug `b` in instance `sued`
//! becomes `sued_b`, object parameters substitute to the passed slugs,
//! value parameters substitute like shadowing `let`s, and everything else
//! passes through untouched — the lockfile, compiler, and diff only ever
//! see the expanded form, so an instance's blocks are pinned exactly like
//! hand-written ones.

use crate::error::{Error, Result};
use crate::ir::ast::{
    ArgItem, Binding, BindingKind, BlockDecl, Expr, ExprWireDecl, Item, Module, MoodsClaim,
    Operand, PortRef, SetDecl, TemplateDecl, TemplateParam, Value, WireDecl,
};
use std::collections::BTreeMap;

/// Rewrite an expression's operands through the instance's slug/value
/// maps: port operands like wire refs, constant operands like values.
fn map_expr(
    e: &Expr,
    map_ref: &impl Fn(&PortRef) -> PortRef,
    map_value: &impl Fn(&Value) -> Result<Value>,
) -> Result<Expr> {
    let operand = |o: &Operand| -> Result<Operand> {
        Ok(match o {
            Operand::Port(p) => Operand::Port(map_ref(p)),
            Operand::Value(v) => Operand::Value(map_value(v)?),
        })
    };
    Ok(match e {
        Expr::Or(a, b) => Expr::Or(
            Box::new(map_expr(a, map_ref, map_value)?),
            Box::new(map_expr(b, map_ref, map_value)?),
        ),
        Expr::And(a, b) => Expr::And(
            Box::new(map_expr(a, map_ref, map_value)?),
            Box::new(map_expr(b, map_ref, map_value)?),
        ),
        Expr::Not(x) => Expr::Not(Box::new(map_expr(x, map_ref, map_value)?)),
        Expr::Cmp { op, lhs, rhs } => Expr::Cmp {
            op: *op,
            lhs: operand(lhs)?,
            rhs: operand(rhs)?,
        },
        Expr::Arith { op, lhs, rhs } => Expr::Arith {
            op: *op,
            lhs: Box::new(map_expr(lhs, map_ref, map_value)?),
            rhs: Box::new(map_expr(rhs, map_ref, map_value)?),
        },
        Expr::Atom(o) => Expr::Atom(operand(o)?),
    })
}

impl Module {
    /// The module with every template instantiated: `Template` items drop
    /// (they declare nothing), each `Instance` becomes its expansion, all
    /// other items pass through. A module without templates expands to a
    /// plain clone. Name resolution of the result is the caller's job
    /// ([`Module::validate`]) — expansion only checks what it needs.
    pub fn expand(&self) -> Result<Module> {
        if !self
            .items
            .iter()
            .any(|i| matches!(i, Item::Template(_) | Item::Instance(_)))
        {
            return Ok(self.clone());
        }
        let mut templates: BTreeMap<&str, &TemplateDecl> = BTreeMap::new();
        for item in &self.items {
            if let Item::Template(t) = item
                && templates.insert(&t.name, t).is_some()
            {
                return Err(Error::Compile(format!("duplicate template `{}`", t.name)));
            }
        }
        let mut items = Vec::new();
        for item in &self.items {
            match item {
                Item::Template(_) => {}
                Item::Instance(call) => {
                    let Some(t) = templates.get(call.block_type.as_str()) else {
                        return Err(Error::Compile(format!(
                            "`{} = {name}(…)` instantiates unknown template `{name}`",
                            call.slug,
                            name = call.block_type
                        )));
                    };
                    items.extend(self.instantiate(t, call)?);
                }
                other => items.push(other.clone()),
            }
        }
        Ok(Module { items })
    }

    fn instantiate(&self, t: &TemplateDecl, call: &BlockDecl) -> Result<Vec<Item>> {
        let ctx = format!("in `{} = {}(…)`", call.slug, call.block_type);
        let fail = |m: String| Error::Compile(format!("{ctx}: {m}"));

        // Bind arguments: object parameters to the passed slugs (type
        // annotation checked where the slug's declaration is visible),
        // value parameters to the given value or their default.
        let mut objects: BTreeMap<&str, &str> = BTreeMap::new();
        let mut values: BTreeMap<&str, Value> = t
            .params
            .iter()
            .filter_map(|p| match p {
                TemplateParam::Value { name, default } => Some((name.as_str(), default.clone())),
                TemplateParam::Object { .. } => None,
            })
            .collect();
        // A call-site binding that names no template parameter forwards as a
        // port binding onto the body's single block — an instance is a
        // composite block, so its call reads like a block declaration with
        // the shared parameters factored away (feeds may repeat, exactly as
        // in a block's argument list). Bodies with several blocks cannot
        // take forwards yet (a qualified form is deferred with D23's other
        // extensions).
        let mut forwards: Vec<Binding> = Vec::new();
        for binding in call.bindings() {
            let Some(p) = t.params.iter().find(|p| p.name() == binding.port) else {
                let body_blocks: Vec<&str> = t
                    .body
                    .iter()
                    .filter_map(|i| match i {
                        Item::Block(b) => Some(b.slug.as_str()),
                        _ => None,
                    })
                    .collect();
                if body_blocks.len() == 1 {
                    forwards.push(binding.clone());
                    continue;
                }
                return Err(fail(format!(
                    "`{}` names no parameter of `{}`, and port bindings forward \
                     only when the template body declares exactly one block \
                     (this one declares {})",
                    binding.port,
                    t.name,
                    body_blocks.len()
                )));
            };
            match (p, &binding.kind) {
                (TemplateParam::Object { name, block_type }, BindingKind::Param(Value::Ref(s))) => {
                    if let Some(declared) = self.declared_type(s) {
                        if declared != block_type {
                            return Err(fail(format!(
                                "`{s}` is a {declared}, but parameter `{name}` \
                                 expects a {block_type}"
                            )));
                        }
                    } else if self.items.iter().any(|i| {
                        matches!(i, Item::Instance(other) if other.slug == *s)
                            || matches!(i, Item::Template(other) if other.name == *s)
                    }) {
                        return Err(fail(format!(
                            "`{s}` is a template or instance, not an object — pass one \
                             of an instance's blocks by its expanded name (`{s}_<block>`)"
                        )));
                    }
                    objects.insert(name, s);
                }
                (TemplateParam::Object { name, .. }, _) => {
                    return Err(fail(format!(
                        "object parameter `{name}` takes an extern or block slug"
                    )));
                }
                (TemplateParam::Value { name, .. }, BindingKind::Param(v)) => {
                    values.insert(name, v.clone());
                }
                (
                    TemplateParam::Value { name, .. },
                    BindingKind::Wire(_) | BindingKind::Expr(_),
                ) => {
                    return Err(fail(format!(
                        "value parameter `{name}` takes a number, string, or constant"
                    )));
                }
            }
        }
        for p in &t.params {
            if let TemplateParam::Object { name, .. } = p
                && !objects.contains_key(name.as_str())
            {
                return Err(fail(format!("object parameter `{name}` must be given")));
            }
        }

        let body_slugs: Vec<&str> = t
            .body
            .iter()
            .filter_map(|i| match i {
                Item::Block(b) => Some(b.slug.as_str()),
                _ => None,
            })
            .collect();
        let map_slug = |s: &str| -> String {
            if body_slugs.contains(&s) {
                format!("{}_{s}", call.slug)
            } else if let Some(o) = objects.get(s) {
                (*o).to_string()
            } else {
                s.to_string()
            }
        };
        let map_ref = |r: &PortRef| PortRef {
            slug: map_slug(&r.slug),
            port: r.port.clone(),
        };
        let map_value = |v: &Value| -> Result<Value> {
            if let Value::Ref(n) = v {
                if let Some(val) = values.get(n.as_str()) {
                    return Ok(val.clone());
                }
                if objects.contains_key(n.as_str()) {
                    return Err(fail(format!(
                        "object parameter `{n}` used as a value — reference one of its \
                         ports (`{n}.<Port>`) or a value parameter instead"
                    )));
                }
            }
            Ok(v.clone())
        };

        // A body title may interpolate value parameters: `{titel}` becomes
        // the parameter's string at expansion, so instances carry their own
        // app-visible titles. A brace-wrapped slug naming no value parameter
        // is an error (typo guard); braces around anything not slug-shaped
        // pass through verbatim.
        let map_title = |title: &str| -> Result<String> {
            let mut out = String::with_capacity(title.len());
            let mut rest = title;
            while let Some(open) = rest.find('{') {
                out.push_str(&rest[..open]);
                let tail = &rest[open + 1..];
                let placeholder = tail.find('}').map(|close| &tail[..close]).filter(|name| {
                    name.starts_with(|c: char| c.is_ascii_lowercase())
                        && name
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                });
                let Some(name) = placeholder else {
                    out.push('{');
                    rest = tail;
                    continue;
                };
                match values.get(name) {
                    Some(Value::Str(s)) => out.push_str(s),
                    Some(Value::Ref(r)) => {
                        match self.lets().find(|l| l.name == *r).map(|l| &l.value) {
                            Some(Value::Str(s)) => out.push_str(s),
                            _ => {
                                return Err(fail(format!(
                                    "title placeholder `{{{name}}}`: `{r}` is not a \
                                     string constant"
                                )));
                            }
                        }
                    }
                    Some(_) => {
                        return Err(fail(format!(
                            "title placeholder `{{{name}}}` needs a string value"
                        )));
                    }
                    None => {
                        return Err(fail(format!(
                            "title placeholder `{{{name}}}` names no value parameter"
                        )));
                    }
                }
                rest = &tail[name.len() + 1..];
            }
            out.push_str(rest);
            Ok(out)
        };

        let mut out = Vec::new();
        for item in &t.body {
            match item {
                Item::Block(b) => {
                    let mut args = Vec::with_capacity(b.args.len());
                    for arg in &b.args {
                        match arg {
                            ArgItem::Binding(x) => args.push(ArgItem::Binding(Binding {
                                port: x.port.clone(),
                                kind: match &x.kind {
                                    BindingKind::Param(v) => BindingKind::Param(map_value(v)?),
                                    BindingKind::Wire(r) => BindingKind::Wire(map_ref(r)),
                                    BindingKind::Expr(e) => {
                                        BindingKind::Expr(map_expr(e, &map_ref, &map_value)?)
                                    }
                                },
                                comment: x.comment.clone(),
                            })),
                            ArgItem::Comment(c) => args.push(ArgItem::Comment(c.clone())),
                        }
                    }
                    // Forwarded port bindings pass through verbatim — they
                    // are written in module scope at the call site, not in
                    // the template's parameter scope. Only reachable when
                    // the body declares exactly one block.
                    args.extend(forwards.iter().cloned().map(ArgItem::Binding));
                    out.push(Item::Block(BlockDecl {
                        slug: format!("{}_{}", call.slug, b.slug),
                        block_type: b.block_type.clone(),
                        title: b.title.as_deref().map(map_title).transpose()?,
                        args,
                        comment: b.comment.clone(),
                        close_comment: b.close_comment.clone(),
                    }));
                }
                Item::Wire(w) => out.push(Item::Wire(WireDecl {
                    to: map_ref(&w.to),
                    from: map_ref(&w.from),
                    comment: w.comment.clone(),
                })),
                Item::ExprWire(w) => out.push(Item::ExprWire(ExprWireDecl {
                    to: map_ref(&w.to),
                    expr: map_expr(&w.expr, &map_ref, &map_value)?,
                    comment: w.comment.clone(),
                })),
                Item::Set(s) => out.push(Item::Set(SetDecl {
                    target: map_ref(&s.target),
                    value: map_value(&s.value)?,
                    comment: s.comment.clone(),
                })),
                // A mood claim (D58) on a body block: the set is named
                // directly, or by a value parameter so each instance can
                // pass its own (`stimmungen = "standard"`, overridden with
                // `stimmungen: "kueche"`).
                Item::MoodsClaim(c) => {
                    let moods = match values.get(c.moods.as_str()) {
                        Some(Value::Ref(name) | Value::Str(name)) => name.clone(),
                        Some(other) => {
                            return Err(fail(format!(
                                "`{c}`: parameter `{}` passes `{other}`, not the name of a \
                                 `moods` set",
                                c.moods
                            )));
                        }
                        None => c.moods.clone(),
                    };
                    out.push(Item::MoodsClaim(MoodsClaim {
                        slug: map_slug(&c.slug),
                        moods,
                        comment: c.comment.clone(),
                    }));
                }
                // Template-source commentary, not per-instance content.
                Item::Comment(_) => {}
                other => {
                    return Err(fail(format!(
                        "template `{}` carries an unexpected {other:?} in its body",
                        t.name
                    )));
                }
            }
        }
        Ok(out)
    }

    /// The declared type of a top-level extern or managed block, if the
    /// slug names one.
    fn declared_type(&self, slug: &str) -> Option<&str> {
        self.items.iter().find_map(|i| match i {
            Item::Extern(e) if e.slug == slug => Some(e.block_type.as_str()),
            Item::Block(b) if b.slug == slug => Some(b.block_type.as_str()),
            _ => None,
        })
    }
}
