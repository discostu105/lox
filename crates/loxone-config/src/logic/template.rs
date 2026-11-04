//! Template expansion: a pure source-to-source pass replacing
//! `template` declarations and their instantiations with plain statements
//! before anything downstream runs. Body slug `b` in instance `sued`
//! becomes `sued_b` — source spells it `sued.b`, or `sued` alone for the
//! template's primary block — object and set parameters substitute to
//! the passed names, value parameters substitute like shadowing `let`s,
//! and everything else passes through untouched. The lockfile, compiler,
//! and diff only ever see the expanded form, so an instance's blocks are
//! pinned exactly like hand-written ones.

use crate::error::{Error, Result};
use crate::logic::ast::{
    AccessClaim, ArgItem, AttrDecl, Binding, BindingKind, BlockDecl, Day, Expr, ExprWireDecl, Item,
    MatchSpec, MembersClaim, ModeName, Module, MoodsClaim, Operand, PortRef, RuleItem,
    RuleStmtKind, ScheduleClaim, ScheduleItem, SetDecl, SetKind, SignalDecl, TemplateDecl,
    TemplateParam, TestItem, TilesClaim, TilesItem, Value, WireDecl,
};
use std::collections::BTreeMap;

/// How a name is used where [`visit_refs`] finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefUse<'a> {
    /// `name.Port`: the object behind a port reference.
    Port(&'a str),
    /// A bare object (`to: name`, `then name on`, a claim's target,
    /// `moved … -> name`).
    Object,
    /// A bare name where a port may stand (an argument, an operand, the
    /// source of `<-`): the object's default output.
    Bare,
    /// A lockfile key (`removed name`, `moved name -> …`): it may name
    /// something the source no longer declares.
    LockKey,
}

/// Call `f` on every name a statement *references* (never one it
/// declares): port references, expression operands, claim targets,
/// tile targets, rule targets, test ports, `removed`/`moved` keys.
/// Template bodies and instance calls are left to the expansion, which
/// knows their parameters.
pub(crate) fn visit_refs(
    item: &mut Item,
    f: &mut impl FnMut(&mut String, RefUse) -> Result<()>,
) -> Result<()> {
    let port = |r: &mut PortRef, f: &mut dyn FnMut(&mut String, RefUse) -> Result<()>| {
        if r.port.is_empty() {
            f(&mut r.slug, RefUse::Bare)
        } else {
            f(&mut r.slug, RefUse::Port(&r.port))
        }
    };
    match item {
        Item::Block(b) => visit_args(&mut b.args, f)?,
        Item::Wire(w) => {
            port(&mut w.to, f)?;
            port(&mut w.from, f)?;
        }
        Item::ExprWire(w) => {
            port(&mut w.to, f)?;
            visit_expr(&mut w.expr, f)?;
        }
        Item::Signal(x) => visit_expr(&mut x.expr, f)?,
        Item::Set(s) => port(&mut s.target, f)?,
        Item::Attr(a) => f(&mut a.slug, RefUse::Object)?,
        Item::TilesClaim(c) => f(&mut c.slug, RefUse::Object)?,
        Item::ScheduleClaim(c) => f(&mut c.slug, RefUse::Object)?,
        Item::MoodsClaim(c) => f(&mut c.slug, RefUse::Object)?,
        Item::MembersClaim(c) => {
            f(&mut c.slug, RefUse::Object)?;
            for m in &mut c.members {
                f(m, RefUse::Object)?;
            }
        }
        Item::AccessClaim(c) => f(&mut c.slug, RefUse::Object)?,
        Item::Extern(e) => {
            if let MatchSpec::Mirrors(target) = &mut e.match_spec {
                f(target, RefUse::Object)?;
            }
            visit_args(&mut e.args, f)?;
        }
        // A `let` alias names a port.
        Item::Let(l) => {
            if let Value::Port(r) = &mut l.value {
                port(r, f)?;
            }
        }
        Item::Removed(r) => f(&mut r.slug, RefUse::LockKey)?,
        Item::Moved(m) => {
            f(&mut m.from, RefUse::LockKey)?;
            f(&mut m.to, RefUse::Object)?;
        }
        Item::Tiles(t) => {
            for stmt in &mut t.body {
                if let TilesItem::Tile(tile) = stmt {
                    for arg in &mut tile.args {
                        if let ArgItem::Binding(b) = arg
                            && (b.port == "to" || b.port.ends_with("_to"))
                            && let BindingKind::Param(Value::Ref(target)) = &mut b.kind
                        {
                            f(target, RefUse::Object)?;
                        }
                    }
                }
            }
        }
        Item::Test(t) => {
            for stmt in &mut t.body {
                match stmt {
                    TestItem::Inject(s) => port(&mut s.target, f)?,
                    TestItem::Expect(e) => port(&mut e.port, f)?,
                    _ => {}
                }
            }
        }
        Item::Rule(r) => {
            for stmt in &mut r.body {
                if let RuleItem::Stmt(st) = stmt {
                    match &mut st.kind {
                        RuleStmtKind::Then { target, .. } => f(target, RefUse::Object)?,
                        RuleStmtKind::State { target, state, .. } => {
                            f(target, RefUse::Port(state))?
                        }
                        RuleStmtKind::Mode {
                            mode: ModeName::Slug(target),
                            ..
                        }
                        | RuleStmtKind::ThenMode {
                            mode: ModeName::Slug(target),
                            ..
                        } => f(target, RefUse::Object)?,
                        _ => {}
                    }
                }
            }
        }
        Item::Schedule(s) => {
            for line in &mut s.body {
                if let ScheduleItem::Line(l) = line {
                    for d in &mut l.days {
                        if let Day::Slug(slug) = d {
                            f(slug, RefUse::Object)?;
                        }
                    }
                }
            }
        }
        Item::Defaults(_)
        | Item::Template(_)
        | Item::Instance(_)
        | Item::Page(_)
        | Item::FragmentEnd
        | Item::Gap
        | Item::Owned(_)
        | Item::Access(_)
        | Item::Moods(_)
        | Item::Comment(_) => {}
    }
    Ok(())
}

fn visit_args(
    args: &mut [ArgItem],
    f: &mut dyn FnMut(&mut String, RefUse) -> Result<()>,
) -> Result<()> {
    for arg in args {
        if let ArgItem::Binding(b) = arg {
            match &mut b.kind {
                BindingKind::Wire(r) => f(&mut r.slug, RefUse::Port(&r.port))?,
                BindingKind::Expr(e) => visit_expr(e, f)?,
                // A minted ref's `mirrors:` names an object.
                BindingKind::Param(Value::Ref(target)) if b.port == "mirrors" => {
                    f(target, RefUse::Object)?
                }
                // A bare name on a port: a constant, or an object whose
                // default output the binding wires.
                BindingKind::Param(Value::Ref(target))
                    if b.port.starts_with(|c: char| c.is_ascii_uppercase()) =>
                {
                    f(target, RefUse::Bare)?
                }
                // A calendar entry's `switches:` names a `Mode` by slug
                // (a day word is no name).
                BindingKind::Param(Value::Ref(target))
                    if b.port == crate::logic::calendar::SWITCHES
                        && !crate::logic::schedule::is_day(target) =>
                {
                    f(target, RefUse::Object)?
                }
                BindingKind::Param(_) => {}
            }
        }
    }
    Ok(())
}

fn visit_expr(e: &mut Expr, f: &mut dyn FnMut(&mut String, RefUse) -> Result<()>) -> Result<()> {
    match e {
        Expr::Or(a, b) | Expr::And(a, b) => {
            visit_expr(a, f)?;
            visit_expr(b, f)
        }
        Expr::Not(x) => visit_expr(x, f),
        Expr::Cmp { lhs, rhs, .. } => {
            for o in [lhs, rhs] {
                match o {
                    Operand::Port(p) => f(&mut p.slug, RefUse::Port(&p.port))?,
                    Operand::Value(Value::Ref(name)) => f(name, RefUse::Bare)?,
                    Operand::Arith(x) => visit_expr(x, f)?,
                    Operand::Value(_) => {}
                }
            }
            Ok(())
        }
        Expr::Arith { lhs, rhs, .. } => {
            visit_expr(lhs, f)?;
            visit_expr(rhs, f)
        }
        Expr::Atom(Operand::Port(p)) => f(&mut p.slug, RefUse::Port(&p.port)),
        Expr::Atom(Operand::Value(Value::Ref(name))) => f(name, RefUse::Bare),
        Expr::Atom(Operand::Arith(x)) => visit_expr(x, f),
        Expr::Atom(Operand::Value(_)) => Ok(()),
    }
}

/// The names of a template body being expanded: its nested instances,
/// and every name the body declares itself (blocks, signals, constants,
/// parameters), which shadows a module name of the same spelling.
#[derive(Default)]
struct Local<'m> {
    instances: BTreeMap<&'m str, &'m TemplateDecl>,
    shadowed: std::collections::BTreeSet<&'m str>,
}

/// What an expanded slug stands for: the spelled instance path
/// (`a`, `a.inner`), the body block, and its type when it is a block.
struct Expanded<'m> {
    path: String,
    block: &'m str,
    block_type: Option<&'m str>,
}

/// The module's templates and instances, for resolving scoped names.
struct Scopes<'m> {
    templates: BTreeMap<&'m str, &'m TemplateDecl>,
    /// Top-level instance slug → its template.
    instances: BTreeMap<&'m str, &'m TemplateDecl>,
    /// Expanded slug `<instance>_<block>` (nested: `<a>_<inner>_<block>`)
    /// → what it stands for: the spelling source may no longer use.
    expanded: BTreeMap<String, Expanded<'m>>,
}

impl<'m> Scopes<'m> {
    fn of(module: &'m Module) -> Scopes<'m> {
        let templates: BTreeMap<&str, &TemplateDecl> = module
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Template(t) => Some((t.name.as_str(), t)),
                _ => None,
            })
            .collect();
        let mut instances = BTreeMap::new();
        let mut expanded = BTreeMap::new();
        fn register<'m>(
            prefix: &str,
            path: &str,
            t: &'m TemplateDecl,
            templates: &BTreeMap<&'m str, &'m TemplateDecl>,
            expanded: &mut BTreeMap<String, Expanded<'m>>,
            depth: usize,
        ) {
            if depth > 8 {
                return;
            }
            for item in &t.body {
                match item {
                    Item::Block(b) => {
                        expanded.insert(
                            format!("{prefix}_{}", b.slug),
                            Expanded {
                                path: path.to_string(),
                                block: &b.slug,
                                block_type: Some(&b.block_type),
                            },
                        );
                    }
                    Item::Signal(x) => {
                        expanded.insert(
                            format!("{prefix}_{}", x.slug),
                            Expanded {
                                path: path.to_string(),
                                block: &x.slug,
                                block_type: None,
                            },
                        );
                    }
                    Item::Instance(inner) => {
                        if let Some(it) = templates.get(inner.block_type.as_str()) {
                            register(
                                &format!("{prefix}_{}", inner.slug),
                                &format!("{path}.{}", inner.slug),
                                it,
                                templates,
                                expanded,
                                depth + 1,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        for item in &module.items {
            if let Item::Instance(call) = item
                && let Some(t) = templates.get(call.block_type.as_str())
            {
                instances.insert(call.slug.as_str(), *t);
                register(&call.slug, &call.slug, t, &templates, &mut expanded, 0);
            }
        }
        Scopes {
            templates,
            instances,
            expanded,
        }
    }

    /// How source writes an expanded block: `instance.block`, or
    /// `a.inner.block` for a block of a nested instance.
    fn spelling(&self, path: &str, block: &str) -> String {
        format!("{path}.{block}")
    }

    /// The template at the end of an instance path (`a`, `a.inner`),
    /// `locals` naming the instances of the template body being
    /// expanded. `None` when the path names no instance.
    fn template_at(&self, locals: &Local<'m>, path: &str) -> Option<&'m TemplateDecl> {
        let mut parts = path.split('.');
        let first = parts.next()?;
        let mut t = match locals.instances.get(first) {
            Some(t) => *t,
            None if locals.shadowed.contains(first) => return None,
            None => *self.instances.get(first)?,
        };
        for part in parts {
            let inner = t.body_instances().find(|b| b.slug == part)?;
            t = self.templates.get(inner.block_type.as_str())?;
        }
        Some(t)
    }

    /// The body port an instance's output stands for: `a.Q` is the
    /// output `Q` of `a`'s template, written back to `a.<block>.<Port>`;
    /// an empty port is the first output. `None` when `path` names no
    /// instance (a block, an extern, a constant).
    fn output_target(&self, locals: &Local<'m>, path: &str, port: &str) -> Result<Option<PortRef>> {
        let Some(t) = self.template_at(locals, path) else {
            return Ok(None);
        };
        let out = if port.is_empty() {
            t.outputs.first().ok_or_else(|| {
                Error::Compile(format!(
                    "`{path}` is a `{}` instance, and that template declares no outputs. \
                     Declare them: `template {}(…) -> (Q: <block>.Q)`",
                    t.name, t.name
                ))
            })?
        } else if port.starts_with(|c: char| c.is_ascii_lowercase()) {
            // `a.block`: a block of the instance, not an output.
            return Ok(None);
        } else {
            t.output(port).ok_or_else(|| {
                let outputs: Vec<&str> = t.outputs.iter().map(|o| o.name.as_str()).collect();
                Error::Compile(if outputs.is_empty() {
                    format!(
                        "`{path}.{port}`: `{}` declares no outputs. Declare them: \
                         `template {}(…) -> ({port}: <block>.{port})`",
                        t.name, t.name
                    )
                } else {
                    format!(
                        "`{path}.{port}`: `{}` has no output `{port}`. Its outputs: {}",
                        t.name,
                        outputs.join(", ")
                    )
                })
            })?
        };
        let target = PortRef {
            slug: format!("{path}.{}", out.port.slug),
            port: out.port.port.clone(),
        };
        // An output that reads a nested instance's output: follow it.
        Ok(Some(
            self.output_target(locals, &target.slug, &target.port)?
                .unwrap_or(target),
        ))
    }

    /// The expanded slug a referenced name stands for: `a.b` is body
    /// block `b` of instance `a` (`a.inner.b` one of a nested instance).
    /// The expanded spelling itself (`a_b`) is refused with the one to
    /// write; a lockfile key is only joined (`a.b` → `a_b`) — what it
    /// names may be gone from the source.
    fn resolve(&self, name: &str, usage: RefUse) -> Result<String> {
        let at = |n: &str| match usage {
            RefUse::Port(p) => format!("{n}.{p}"),
            _ => n.to_string(),
        };
        let fail = |m: String| Err(Error::Compile(m));
        let blocks = |t: &TemplateDecl| -> String {
            t.body_slugs()
                .chain(t.body_instances().map(|b| b.slug.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        if let Some((a, rest)) = name.split_once('.') {
            if usage == RefUse::LockKey {
                return Ok(name.replace('.', "_"));
            }
            let Some(mut t) = self.instances.get(a).copied() else {
                return fail(format!(
                    "`{}`: `{a}` is not a template instance, so `{name}` names no block",
                    at(name)
                ));
            };
            let parts: Vec<&str> = rest.split('.').collect();
            for (i, part) in parts.iter().enumerate() {
                let last = i + 1 == parts.len();
                if last && t.body_slugs().any(|s| s == *part) {
                    return Ok(name.replace('.', "_"));
                }
                if let Some(inner) = t.body_instances().find(|b| b.slug == *part) {
                    if last {
                        return fail(format!(
                            "`{}`: `{name}` is a `{}` instance inside `{a}`. Name one of its \
                             blocks (`{name}.<block>`) or one of its outputs (`{name}.<Output>`)",
                            at(name),
                            inner.block_type
                        ));
                    }
                    let Some(it) = self.templates.get(inner.block_type.as_str()) else {
                        return fail(format!(
                            "`{}`: unknown template `{}`",
                            at(name),
                            inner.block_type
                        ));
                    };
                    t = it;
                    continue;
                }
                return fail(format!(
                    "`{}`: `{a}` (a `{}` instance) has no block `{part}`. Its blocks: {}",
                    at(name),
                    t.name,
                    blocks(t)
                ));
            }
            return Ok(name.replace('.', "_"));
        }
        if usage == RefUse::LockKey {
            return Ok(name.to_string());
        }
        if let Some(t) = self.instances.get(name) {
            return fail(match usage {
                RefUse::Port(p) => format!(
                    "`{name}.{p}`: `{}` has no output `{p}`. Its outputs: {}",
                    t.name,
                    t.outputs
                        .iter()
                        .map(|o| o.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                _ => format!(
                    "`{name}` is a `{}` instance. Name one of its blocks (`{name}.<block>`); \
                     its outputs are {}",
                    t.name,
                    if t.outputs.is_empty() {
                        "none".to_string()
                    } else {
                        t.outputs
                            .iter()
                            .map(|o| format!("`{name}.{}`", o.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                ),
            });
        }
        if let Some(x) = self.expanded.get(name) {
            return fail(format!(
                "source names a block of a template instance by its instance. Write `{}` \
                 instead of `{}`.",
                at(&self.spelling(&x.path, x.block)),
                at(name)
            ));
        }
        Ok(name.to_string())
    }

    /// The declared type of an instance's body block, by expanded slug.
    fn block_type(&self, expanded: &str) -> Option<&'m str> {
        self.expanded.get(expanded)?.block_type
    }

    /// Rewrite every reference to an instance's output in `item` to the
    /// body port it stands for (`a.Q` → `a.<block>.Q`, a bare `a` to its
    /// first output), so the scoped-name resolution sees blocks only.
    fn rewrite_outputs(&self, locals: &Local<'m>, item: &mut Item) -> Result<()> {
        let port = |r: &mut PortRef| -> Result<()> {
            if let Some(target) = self.output_target(locals, &r.slug, &r.port)? {
                *r = target;
            }
            Ok(())
        };
        let kind = |k: &mut BindingKind, pascal: bool| -> Result<()> {
            match k {
                // `In: a.m.i`, a nested instance by its path: its first
                // output (the lexer reads the last segment as a port).
                BindingKind::Wire(r)
                    if pascal && r.port.starts_with(|c: char| c.is_ascii_lowercase()) =>
                {
                    let name = format!("{}.{}", r.slug, r.port);
                    if let Some(target) = self.output_target(locals, &name, "")? {
                        *k = BindingKind::Wire(target);
                    }
                    Ok(())
                }
                BindingKind::Wire(r) => port(r),
                BindingKind::Expr(e) => self.rewrite_expr(locals, e),
                BindingKind::Param(Value::Ref(n)) if pascal => {
                    if let Some(target) = self.output_target(locals, n, "")? {
                        *k = BindingKind::Wire(target);
                    }
                    Ok(())
                }
                BindingKind::Param(_) => Ok(()),
            }
        };
        let args = |args: &mut [ArgItem]| -> Result<()> {
            for arg in args {
                if let ArgItem::Binding(b) = arg {
                    let pascal = b.port.starts_with(|c: char| c.is_ascii_uppercase());
                    kind(&mut b.kind, pascal)?;
                }
            }
            Ok(())
        };
        match item {
            Item::Block(b) => args(&mut b.args),
            // A nested instance's input parameter takes a source too: a
            // bare name there may be an instance's first output.
            Item::Instance(b) => {
                let inputs: Vec<String> = self
                    .templates
                    .get(b.block_type.as_str())
                    .map(|t| {
                        t.params
                            .iter()
                            .filter(|p| matches!(p, TemplateParam::Input { .. }))
                            .map(|p| p.name().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                for arg in &mut b.args {
                    if let ArgItem::Binding(x) = arg {
                        let source = x.port.starts_with(|c: char| c.is_ascii_uppercase())
                            || inputs.contains(&x.port);
                        kind(&mut x.kind, source)?;
                    }
                }
                Ok(())
            }
            Item::Extern(e) => args(&mut e.args),
            Item::Wire(w) => {
                port(&mut w.to)?;
                port(&mut w.from)
            }
            Item::ExprWire(w) => {
                port(&mut w.to)?;
                self.rewrite_expr(locals, &mut w.expr)
            }
            Item::Signal(x) => self.rewrite_expr(locals, &mut x.expr),
            Item::Set(s) => port(&mut s.target),
            Item::Let(l) => match &mut l.value {
                Value::Port(r) => port(r),
                _ => Ok(()),
            },
            Item::Test(t) => {
                for stmt in &mut t.body {
                    match stmt {
                        TestItem::Inject(s) => port(&mut s.target)?,
                        TestItem::Expect(e) => port(&mut e.port)?,
                        _ => {}
                    }
                }
                Ok(())
            }
            Item::Rule(r) => {
                for stmt in &mut r.body {
                    if let RuleItem::Stmt(st) = stmt
                        && let RuleStmtKind::State { target, state, .. } = &mut st.kind
                        && let Some(t) = self.output_target(locals, target, state)?
                    {
                        *target = t.slug;
                        *state = t.port;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn rewrite_expr(&self, locals: &Local<'m>, e: &mut Expr) -> Result<()> {
        let operand = |o: &mut Operand| -> Result<()> {
            match o {
                Operand::Port(p) => {
                    if let Some(t) = self.output_target(locals, &p.slug, &p.port)? {
                        *p = t;
                    }
                }
                Operand::Value(Value::Ref(n)) => {
                    if let Some(t) = self.output_target(locals, n, "")? {
                        *o = Operand::Port(t);
                    }
                }
                Operand::Arith(x) => self.rewrite_expr(locals, x)?,
                Operand::Value(_) => {}
            }
            Ok(())
        };
        match e {
            Expr::Or(a, b) | Expr::And(a, b) | Expr::Arith { lhs: a, rhs: b, .. } => {
                self.rewrite_expr(locals, a)?;
                self.rewrite_expr(locals, b)
            }
            Expr::Not(x) => self.rewrite_expr(locals, x),
            Expr::Cmp { lhs, rhs, .. } => {
                operand(lhs)?;
                operand(rhs)
            }
            Expr::Atom(o) => operand(o),
        }
    }
}

/// How a template's input parameter is fed at the call: what the body's
/// binding of it becomes.
#[derive(Clone)]
enum Fed {
    Wire(PortRef),
    /// A bare object name: its default output.
    Object(String),
    Expr(Expr),
}

/// Rewrite an expression's operands through the instance's slug/value
/// maps: port operands like wire refs, constant operands like values,
/// and a bare input parameter by what the call feeds it.
fn map_expr(
    e: &Expr,
    map_ref: &impl Fn(&PortRef) -> Result<PortRef>,
    map_value: &impl Fn(&Value) -> Result<Value>,
    map_input: &impl Fn(&str) -> Result<Option<Vec<Fed>>>,
) -> Result<Expr> {
    let rec = |x: &Expr| map_expr(x, map_ref, map_value, map_input);
    let fed_one = |name: &str, what: &str| -> Result<Option<Fed>> {
        let Some(fed) = map_input(name)? else {
            return Ok(None);
        };
        match fed.as_slice() {
            [one] => Ok(Some(one.clone())),
            [] => Err(Error::Compile(format!(
                "input `{name}` is not given, but {what} reads it"
            ))),
            _ => Err(Error::Compile(format!(
                "input `{name}` is given several sources, but {what} reads one"
            ))),
        }
    };
    let operand = |o: &Operand| -> Result<Operand> {
        Ok(match o {
            Operand::Port(p) => Operand::Port(map_ref(p)?),
            Operand::Value(Value::Ref(n)) => match fed_one(n, "a comparison")? {
                Some(Fed::Wire(r)) => Operand::Port(r),
                Some(Fed::Object(obj)) => Operand::Value(Value::Ref(obj)),
                Some(Fed::Expr(_)) => {
                    return Err(Error::Compile(format!(
                        "input `{n}` is given an expression, but a comparison reads a port"
                    )));
                }
                None => Operand::Value(map_value(&Value::Ref(n.clone()))?),
            },
            Operand::Value(v) => Operand::Value(map_value(v)?),
            Operand::Arith(x) => Operand::Arith(Box::new(rec(x)?)),
        })
    };
    Ok(match e {
        Expr::Or(a, b) => Expr::Or(Box::new(rec(a)?), Box::new(rec(b)?)),
        Expr::And(a, b) => Expr::And(Box::new(rec(a)?), Box::new(rec(b)?)),
        Expr::Not(x) => Expr::Not(Box::new(rec(x)?)),
        Expr::Cmp { op, lhs, rhs } => Expr::Cmp {
            op: *op,
            lhs: operand(lhs)?,
            rhs: operand(rhs)?,
        },
        Expr::Arith { op, lhs, rhs } => Expr::Arith {
            op: *op,
            lhs: Box::new(rec(lhs)?),
            rhs: Box::new(rec(rhs)?),
        },
        // A bare input parameter as a gate operand: the fed expression
        // takes its place.
        Expr::Atom(Operand::Value(Value::Ref(n))) => match fed_one(n, "an expression")? {
            Some(Fed::Wire(r)) => Expr::Atom(Operand::Port(r)),
            Some(Fed::Object(obj)) => Expr::Atom(Operand::Value(Value::Ref(obj))),
            Some(Fed::Expr(x)) => x,
            None => Expr::Atom(operand(&Operand::Value(Value::Ref(n.clone())))?),
        },
        Expr::Atom(o) => Expr::Atom(operand(o)?),
    })
}

impl Module {
    /// The module with every template instantiated: `Template` items drop
    /// (they declare nothing), each `Instance` becomes its expansion, all
    /// other items pass through with their scoped names (`a.b`, an
    /// instance's output `a.Q`) resolved to expanded slugs. Name
    /// resolution of the result is the caller's job ([`Module::validate`])
    /// — expansion only checks what it needs.
    pub fn expand(&self) -> Result<Module> {
        // An extern's port bindings are the statements they stand for.
        if self.externs().any(|e| !e.args.is_empty()) {
            return self.lowered().expand();
        }
        let mut errors = crate::error::Errors::default();
        let mut seen: BTreeMap<&str, ()> = BTreeMap::new();
        for item in &self.items {
            if let Item::Template(t) = item
                && seen.insert(&t.name, ()).is_some()
            {
                errors.push(Error::Compile(format!("duplicate template `{}`", t.name)));
            }
        }
        // An instance's name now names its blocks (`a.b`, `a.Port`), so it
        // shares the module's one namespace.
        let mut names: BTreeMap<&str, ()> = BTreeMap::new();
        for item in &self.items {
            let name = match item {
                Item::Extern(e) => e.slug.as_str(),
                Item::Block(b) | Item::Instance(b) => b.slug.as_str(),
                Item::Signal(x) => x.slug.as_str(),
                Item::Let(l) => l.name.as_str(),
                Item::Template(t) => t.name.as_str(),
                Item::Tiles(t) => t.name.as_str(),
                Item::Schedule(t) => t.name.as_str(),
                Item::Moods(t) => t.name.as_str(),
                _ => continue,
            };
            if names.insert(name, ()).is_some() && matches!(item, Item::Instance(_)) {
                errors.push(Error::Compile(format!("duplicate name `{name}`")));
            }
        }
        let defaults = errors
            .check(super::defaults::by_type(&self.items))
            .unwrap_or_default();
        let scopes = Scopes::of(self);
        let no_locals = Local::default();
        let mut items = Vec::new();
        for item in &self.items {
            match item {
                // Applied below, to the expanded blocks.
                Item::Template(_) | Item::Defaults(_) => {}
                Item::Instance(call) => {
                    let Some(t) = scopes.templates.get(call.block_type.as_str()) else {
                        errors.push(Error::Compile(format!(
                            "`{} = {name}(…)` instantiates unknown template `{name}`",
                            call.slug,
                            name = call.block_type
                        )));
                        continue;
                    };
                    let mut call = Item::Instance(call.clone());
                    if errors
                        .check(scopes.rewrite_outputs(&no_locals, &mut call))
                        .is_none()
                    {
                        continue;
                    }
                    let Item::Instance(call) = call else {
                        unreachable!()
                    };
                    if let Some(x) =
                        errors.check(self.instantiate(&scopes, t, &call, &call.slug, false))
                    {
                        items.extend(x);
                    }
                }
                other => {
                    let mut other = other.clone();
                    let resolved = scopes
                        .rewrite_outputs(&no_locals, &mut other)
                        .and_then(|()| {
                            visit_refs(&mut other, &mut |name, usage| {
                                *name = scopes.resolve(name, usage)?;
                                Ok(())
                            })
                        });
                    if errors.check(resolved).is_some() {
                        items.push(other);
                    }
                }
            }
        }
        errors.finish()?;
        super::defaults::apply(&mut items, &defaults);
        let mut module = Module { items };
        // `b.In1 <- a` on a managed block is an argument of `b`.
        module.fold_remote_bindings();
        module.resolve_names()?;
        super::hardware::lower(&mut module.items)?;
        Ok(module)
    }

    /// The scoped-name errors [`Module::expand`] would report (`a_b` for
    /// `a.b`, an unknown block, an unknown output), without expanding:
    /// the unexpanded module's validation runs it first, so those
    /// references get their own message, not "undeclared".
    pub(crate) fn check_scoped_names(&self) -> Result<()> {
        let scopes = Scopes::of(self);
        if scopes.instances.is_empty() {
            return Ok(());
        }
        let no_locals = Local::default();
        let mut errors = crate::error::Errors::default();
        for item in &self.items {
            let mut item = match item {
                Item::Template(_) => continue,
                other => other.clone(),
            };
            let checked = scopes
                .rewrite_outputs(&no_locals, &mut item)
                .and_then(|()| {
                    if matches!(item, Item::Instance(_)) {
                        return Ok(());
                    }
                    visit_refs(&mut item, &mut |name, usage| {
                        scopes.resolve(name, usage).map(|_| ())
                    })
                });
            errors.check(checked);
        }
        errors.finish()
    }

    /// Whether the module declares a `kind` set named `name`.
    pub(crate) fn declares_set(&self, kind: SetKind, name: &str) -> bool {
        self.items.iter().any(|i| match (kind, i) {
            (SetKind::Moods, Item::Moods(m)) => m.name == name,
            (SetKind::Schedule, Item::Schedule(x)) => x.name == name,
            (SetKind::Tiles, Item::Tiles(x)) => x.name == name,
            (_, Item::Access(a)) => a.right.set_kind() == kind && a.name == name,
            _ => false,
        })
    }

    /// How source spells the expanded slug `slug`: `instance.block`
    /// (`a.inner.block` under a nested instance). Any other slug is
    /// spelled as it is.
    pub fn spelling(&self, slug: &str) -> String {
        let scopes = Scopes::of(self);
        match scopes.expanded.get(slug) {
            Some(x) => scopes.spelling(&x.path, x.block),
            None => slug.to_string(),
        }
    }

    /// How source spells an expanded port: an instance's output by its
    /// name (`hitze.Q` for `hitze_alarm.Q` under `-> (Q: alarm.Q)`),
    /// any other port by its block (`hitze.alarm.On`).
    pub fn spell_port(&self, port: &PortRef) -> PortRef {
        let scopes = Scopes::of(self);
        let Some(x) = scopes.expanded.get(&port.slug) else {
            return port.clone();
        };
        let by_output = scopes
            .template_at(&Local::default(), &x.path)
            .and_then(|t| t.output(&port.port))
            .is_some_and(|o| o.port.slug == x.block && o.port.port == port.port);
        PortRef {
            slug: if by_output {
                x.path.clone()
            } else {
                scopes.spelling(&x.path, x.block)
            },
            port: port.port.clone(),
        }
    }

    /// [`Module::expand`], then `f` on the expanded module, whose errors
    /// name blocks as the source does ([`Module::respell_error`]). The
    /// expansion's own errors already do, and some name the expanded
    /// spelling on purpose ("write `kueche.lc` instead of `kueche_lc`").
    pub fn with_expanded<T>(&self, f: impl FnOnce(Module) -> Result<T>) -> Result<T> {
        let expanded = self.expand()?;
        f(expanded).map_err(|e| self.respell_error(e))
    }

    /// `e` naming blocks the way this (unexpanded) module's source does:
    /// every expanded slug of a template instance (`kueche_lc`) in a
    /// compile or lockfile error becomes its spelling (`kueche.lc`). The
    /// passes after expansion only see expanded slugs; this is the one
    /// place their messages are translated back.
    pub fn respell_error(&self, e: Error) -> Error {
        let scopes = Scopes::of(self);
        if scopes.expanded.is_empty() {
            return e;
        }
        let respell = |msg: String| respell_text(&msg, &scopes);
        match e {
            Error::Many(list) => {
                Error::Many(list.into_iter().map(|e| self.respell_error(e)).collect())
            }
            Error::Compile(m) => Error::Compile(respell(m)),
            Error::Lock(m) => Error::Lock(respell(m)),
            Error::NoMatch { slug, spec } => Error::NoMatch {
                slug: respell(slug),
                spec: respell(spec),
            },
            other => other,
        }
    }

    /// Rewrite the references of `items` (generated against the expanded
    /// module, as adopt does) into this module's spellings.
    pub fn respell(&self, items: &mut [Item]) {
        let scopes = Scopes::of(self);
        for item in items {
            let _ = visit_refs(item, &mut |name, usage| {
                if usage != RefUse::LockKey
                    && let Some(x) = scopes.expanded.get(name.as_str())
                {
                    // `a_alarm.Q` is `a.Q` where the template declares
                    // the output `Q: alarm.Q`; otherwise `a.alarm.Q`.
                    let no_locals = Local::default();
                    if let RefUse::Port(p) = usage
                        && let Some(t) = scopes.template_at(&no_locals, &x.path)
                        && t.output(p)
                            .is_some_and(|o| o.port.slug == x.block && o.port.port == p)
                    {
                        *name = x.path.clone();
                    } else {
                        *name = scopes.spelling(&x.path, x.block);
                    }
                }
                Ok(())
            });
        }
    }

    /// The items of one instance: the body with every body name
    /// prefixed by `prefix` (the instance's expanded slug), the
    /// parameters substituted. `call` is the call with its outputs
    /// rewritten; `expanded` says whether its argument names are
    /// expanded slugs already (a nested call).
    fn instantiate(
        &self,
        scopes: &Scopes,
        t: &TemplateDecl,
        call: &BlockDecl,
        prefix: &str,
        expanded: bool,
    ) -> Result<Vec<Item>> {
        let ctx = format!(
            "in `{} = {}(…)`",
            self.spelling(&call.slug),
            call.block_type
        );
        let fail = |m: String| Error::Compile(format!("{ctx}: {m}"));
        // A nested call's argument names are expanded slugs already,
        // mapped by the enclosing body; a top-level call's are source
        // spellings to resolve.
        let resolve = |name: &str, usage: RefUse| -> Result<String> {
            if expanded {
                Ok(name.to_string())
            } else {
                scopes.resolve(name, usage)
            }
        };

        // Bind arguments: object parameters to the passed slugs (type
        // annotation checked where the slug's declaration is visible),
        // set parameters to the passed set, value parameters to the given
        // value or their default, inputs to what the call feeds them.
        let mut objects: BTreeMap<&str, String> = BTreeMap::new();
        let mut sets: BTreeMap<&str, (SetKind, String)> = BTreeMap::new();
        let mut values: BTreeMap<&str, Value> = BTreeMap::new();
        let mut inputs: BTreeMap<&str, Vec<Fed>> = BTreeMap::new();
        for p in &t.params {
            match p {
                TemplateParam::Value {
                    name,
                    default: Some(d),
                    ..
                } => {
                    values.insert(name, d.clone());
                }
                TemplateParam::Set {
                    name,
                    kind,
                    default: Some(d),
                } => {
                    sets.insert(name, (*kind, d.clone()));
                }
                TemplateParam::Input { name } => {
                    inputs.insert(name, Vec::new());
                }
                _ => {}
            }
        }
        // The label string binds the parameter named `label`.
        if let Some(title) = &call.title {
            match t.param("label") {
                Some(TemplateParam::Value {
                    kind: super::ast::ValueKind::Str,
                    ..
                }) => {
                    values.insert("label", Value::Str(title.clone()));
                }
                _ => {
                    return Err(fail(format!(
                        "`{}` takes no label string: declare `label: string` in the template \
                         and use `{{label}}` in a block title",
                        t.name
                    )));
                }
            }
        }
        let constant = |n: &str| self.lets().find(|l| l.name == n).map(|l| &l.value);
        for binding in call.bindings() {
            if binding.port == "label" && call.title.is_some() {
                return Err(fail(
                    "duplicate argument `label`: the label string binds it".into(),
                ));
            }
            let Some(p) = t.param(&binding.port) else {
                let hint = if binding.port.starts_with(|c: char| c.is_ascii_uppercase()) {
                    format!(
                        ". A call binds the template's parameters only; to feed a port of \
                         the body, declare `<name>: input` in `template {}(…)` and bind it \
                         there (`{}: <name>`)",
                        t.name, binding.port
                    )
                } else {
                    crate::logic::validate::suggest(
                        &binding.port,
                        t.params.iter().map(|p| p.name()),
                    )
                };
                return Err(fail(format!(
                    "`{}` names no parameter of `{}`{hint}",
                    binding.port, t.name
                )));
            };
            // An object named like a port reference: `p: licht.lc`.
            let object = match &binding.kind {
                BindingKind::Param(Value::Ref(s)) => Some(s.clone()),
                BindingKind::Wire(r) if r.port.starts_with(|c: char| c.is_ascii_lowercase()) => {
                    Some(format!("{}.{}", r.slug, r.port))
                }
                _ => None,
            };
            match (p, object) {
                (TemplateParam::Object { name, block_type }, Some(s)) => {
                    if scopes.templates.contains_key(s.as_str()) {
                        return Err(fail(format!(
                            "`{s}` is a template, not an object. Pass an extern, a block or an instance."
                        )));
                    }
                    let s = resolve(&s, RefUse::Object)?;
                    if let Some(declared) = self.declared_type(&s).or(scopes.block_type(&s))
                        && block_type != "any"
                        && declared != block_type
                    {
                        return Err(fail(format!(
                            "`{}` is a {declared}, but parameter `{name}` expects a \
                             {block_type}",
                            self.spelling(&s)
                        )));
                    }
                    objects.insert(name, s);
                }
                (TemplateParam::Object { name, .. }, None) => {
                    return Err(fail(format!(
                        "object parameter `{name}` takes an extern, block or instance"
                    )));
                }
                (TemplateParam::Set { name, kind, .. }, Some(s)) if !s.contains('.') => {
                    sets.insert(name, (*kind, s));
                }
                (TemplateParam::Set { name, kind, .. }, _) => {
                    return Err(fail(match &binding.kind {
                        BindingKind::Param(Value::Str(s)) => format!(
                            "a set parameter takes the set's bare name. Write `{name}: {s}` \
                             instead of `{name}: \"{s}\"`."
                        ),
                        _ => format!(
                            "set parameter `{name}` takes the name of a `{}` set",
                            kind.keyword()
                        ),
                    }));
                }
                (TemplateParam::Value { name, kind, .. }, _) => match &binding.kind {
                    BindingKind::Param(v) => {
                        let given = match v {
                            Value::Ref(c) => constant(c).and_then(super::ast::ValueKind::of),
                            other => super::ast::ValueKind::of(other),
                        };
                        if let Some(k) = given
                            && k != *kind
                        {
                            return Err(fail(format!(
                                "`{name}: {v}`: `{name}` takes {}, not {}",
                                kind.example(),
                                k.example()
                            )));
                        }
                        values.insert(name, v.clone());
                    }
                    BindingKind::Wire(_) | BindingKind::Expr(_) => {
                        return Err(fail(format!(
                            "value parameter `{name}` takes {}, not a port or expression",
                            kind.example()
                        )));
                    }
                },
                (TemplateParam::Input { name }, _) => {
                    let fed = match &binding.kind {
                        BindingKind::Wire(r) => Fed::Wire(PortRef {
                            slug: resolve(&r.slug, RefUse::Port(&r.port))?,
                            port: r.port.clone(),
                        }),
                        BindingKind::Param(Value::Ref(s)) if constant(s).is_none() => {
                            Fed::Object(resolve(s, RefUse::Bare)?)
                        }
                        BindingKind::Param(v) => {
                            return Err(fail(format!(
                                "input `{name}` takes a source (a block, extern, `<slug>.<Port>` \
                                 or an expression), not the value `{v}`"
                            )));
                        }
                        BindingKind::Expr(e) => {
                            let mut e = e.clone();
                            visit_expr(&mut e, &mut |n, usage| {
                                *n = resolve(n, usage)?;
                                Ok(())
                            })?;
                            Fed::Expr(e)
                        }
                    };
                    inputs.entry(name).or_default().push(fed);
                }
            }
        }
        for p in &t.params {
            let (name, what) = match p {
                TemplateParam::Object { name, .. } if !objects.contains_key(name.as_str()) => {
                    (name, "object")
                }
                TemplateParam::Set { name, .. } if !sets.contains_key(name.as_str()) => {
                    (name, "set")
                }
                TemplateParam::Value { name, .. } if !values.contains_key(name.as_str()) => {
                    (name, "value")
                }
                _ => continue,
            };
            return Err(fail(format!("{what} parameter `{name}` must be given")));
        }
        // A passed or default set must be declared, of the parameter's kind.
        for (name, (kind, s)) in &sets {
            if !self.declares_set(*kind, s) {
                return Err(fail(format!(
                    "`{name}: {s}`: `{s}` is not a `{kw}` set (declare it with `{kw} {s}` … \
                     `end`)",
                    kw = kind.keyword()
                )));
            }
        }

        // The body's own names: blocks and signals, nested instances,
        // local constants and aliases.
        let body_slugs: Vec<&str> = t.body_slugs().collect();
        let locals = Local {
            instances: t
                .body_instances()
                .filter_map(|b| {
                    scopes
                        .templates
                        .get(b.block_type.as_str())
                        .map(|it| (b.slug.as_str(), *it))
                })
                .collect(),
            shadowed: t
                .body_slugs()
                .chain(t.params.iter().map(|p| p.name()))
                .chain(t.body.iter().filter_map(|i| match i {
                    Item::Let(l) => Some(l.name.as_str()),
                    _ => None,
                }))
                .collect(),
        };
        let mut aliases: BTreeMap<&str, PortRef> = BTreeMap::new();
        for item in &t.body {
            if let Item::Let(l) = item {
                match &l.value {
                    Value::Port(r) => {
                        aliases.insert(&l.name, r.clone());
                    }
                    v => {
                        values.insert(&l.name, v.clone());
                    }
                }
            }
        }
        let is_local = |s: &str| {
            let head = s.split('.').next().unwrap_or(s);
            body_slugs.contains(&head) || locals.instances.contains_key(head)
        };
        let map_slug = |s: &str| -> Result<String> {
            if is_local(s) {
                Ok(format!("{prefix}_{}", s.replace('.', "_")))
            } else if let Some(o) = objects.get(s) {
                Ok(o.clone())
            } else {
                scopes.resolve(s, RefUse::Object)
            }
        };
        let map_ref_inner = |r: &PortRef, depth: usize| -> Result<PortRef> {
            if r.port.is_empty()
                && let Some(a) = aliases.get(r.slug.as_str())
                && depth == 0
            {
                // A bare alias: the port it names, itself mapped.
                return Ok(PortRef {
                    slug: if is_local(&a.slug) {
                        format!("{prefix}_{}", a.slug.replace('.', "_"))
                    } else if let Some(o) = objects.get(a.slug.as_str()) {
                        o.clone()
                    } else {
                        scopes.resolve(&a.slug, RefUse::Port(&a.port))?
                    },
                    port: a.port.clone(),
                });
            }
            let slug = if is_local(&r.slug) {
                format!("{prefix}_{}", r.slug.replace('.', "_"))
            } else if let Some(o) = objects.get(r.slug.as_str()) {
                o.clone()
            } else {
                scopes.resolve(&r.slug, RefUse::Port(&r.port))?
            };
            Ok(PortRef {
                slug,
                port: r.port.clone(),
            })
        };
        let map_ref = |r: &PortRef| map_ref_inner(r, 0);
        let map_value = |v: &Value| -> Result<Value> {
            if let Value::Ref(n) = v {
                if let Some(val) = values.get(n.as_str()) {
                    return Ok(val.clone());
                }
                // An object parameter in a value position: the passed
                // name, which resolution reads as its default output.
                if let Some(o) = objects.get(n.as_str()) {
                    return Ok(Value::Ref(o.clone()));
                }
                if let Some(a) = aliases.get(n.as_str()) {
                    return Ok(Value::Port(map_ref_inner(a, 1)?));
                }
                // A body block by its bare name: its default output.
                if is_local(n) {
                    return Ok(Value::Ref(format!("{prefix}_{}", n.replace('.', "_"))));
                }
            }
            Ok(v.clone())
        };
        let map_input = |n: &str| -> Result<Option<Vec<Fed>>> { Ok(inputs.get(n).cloned()) };
        // A claim in the body names its set directly or by a set parameter
        // of the claim's kind.
        let map_set = |claim: &str, set: &str, kind: SetKind| -> Result<String> {
            if let Some((k, bound)) = sets.get(set) {
                if *k != kind {
                    return Err(fail(format!(
                        "`{claim}`: parameter `{set}` passes a `{}` set, not a `{}` set",
                        k.keyword(),
                        kind.keyword()
                    )));
                }
                return Ok(bound.clone());
            }
            if let Some(TemplateParam::Value {
                default: Some(default),
                ..
            }) = t.param(set)
            {
                let default = match default {
                    Value::Str(s) => s.clone(),
                    other => other.to_string(),
                };
                return Err(Error::Compile(format!(
                    "template `{}`: a `{kw}` set is passed by a set parameter. Write \
                     `{set}: {kw} = {default}` instead of `{set}: string = \"{default}\"`.",
                    t.name,
                    kw = kind.keyword()
                )));
            }
            Ok(set.to_string())
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
                    Some(Value::Ref(r)) => match constant(r) {
                        Some(Value::Str(s)) => out.push_str(s),
                        _ => {
                            return Err(fail(format!(
                                "title placeholder `{{{name}}}`: `{r}` is not a \
                                 string constant"
                            )));
                        }
                    },
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
        // The arguments of a body block or nested instance: an input
        // parameter on a port becomes what the call feeds it (nothing,
        // when the input is not given).
        let map_args = |args: &[ArgItem]| -> Result<Vec<ArgItem>> {
            let mut out = Vec::with_capacity(args.len());
            for arg in args {
                let ArgItem::Binding(x) = arg else {
                    out.push(arg.clone());
                    continue;
                };
                if let BindingKind::Param(Value::Ref(n)) = &x.kind
                    && let Some(fed) = inputs.get(n.as_str())
                {
                    for f in fed {
                        out.push(ArgItem::Binding(Binding {
                            port: x.port.clone(),
                            kind: match f {
                                Fed::Wire(r) => BindingKind::Wire(r.clone()),
                                Fed::Object(o) => BindingKind::Param(Value::Ref(o.clone())),
                                Fed::Expr(e) => BindingKind::Expr(e.clone()),
                            },
                            comment: x.comment.clone(),
                        }));
                    }
                    continue;
                }
                out.push(ArgItem::Binding(Binding {
                    port: x.port.clone(),
                    kind: match &x.kind {
                        BindingKind::Param(v) => BindingKind::Param(map_value(v)?),
                        BindingKind::Wire(r) => BindingKind::Wire(map_ref(r)?),
                        BindingKind::Expr(e) => {
                            BindingKind::Expr(map_expr(e, &map_ref, &map_value, &map_input)?)
                        }
                    },
                    comment: x.comment.clone(),
                }));
            }
            Ok(out)
        };

        let mut out = Vec::new();
        for item in &t.body {
            // A nested instance's outputs read as the body ports they
            // stand for.
            let mut item = item.clone();
            scopes.rewrite_outputs(&locals, &mut item)?;
            match &item {
                Item::Block(b) => {
                    out.push(Item::Block(BlockDecl {
                        slug: format!("{prefix}_{}", b.slug),
                        block_type: b.block_type.clone(),
                        title: b.title.as_deref().map(map_title).transpose()?,
                        args: map_args(&b.args)?,
                        comment: b.comment.clone(),
                        close_comment: b.close_comment.clone(),
                    }));
                }
                Item::Instance(inner) => {
                    let Some(it) = scopes.templates.get(inner.block_type.as_str()) else {
                        return Err(fail(format!(
                            "`{} = {}(…)` instantiates unknown template `{}`",
                            inner.slug, inner.block_type, inner.block_type
                        )));
                    };
                    let nested = BlockDecl {
                        slug: format!("{prefix}_{}", inner.slug),
                        block_type: inner.block_type.clone(),
                        title: inner.title.as_deref().map(map_title).transpose()?,
                        args: map_args(&inner.args)?,
                        comment: inner.comment.clone(),
                        close_comment: inner.close_comment.clone(),
                    };
                    out.extend(self.instantiate(scopes, it, &nested, &nested.slug, true)?);
                }
                Item::Wire(w) => out.push(Item::Wire(WireDecl {
                    to: map_ref(&w.to)?,
                    from: map_ref(&w.from)?,
                    comment: w.comment.clone(),
                })),
                Item::ExprWire(w) => out.push(Item::ExprWire(ExprWireDecl {
                    to: map_ref(&w.to)?,
                    expr: map_expr(&w.expr, &map_ref, &map_value, &map_input)?,
                    comment: w.comment.clone(),
                })),
                // A named signal is a body-local name like a block: its
                // slug (and so its sub-blocks' prefix) is the instance's.
                Item::Signal(x) => out.push(Item::Signal(SignalDecl {
                    slug: map_slug(&x.slug)?,
                    expr: map_expr(&x.expr, &map_ref, &map_value, &map_input)?,
                    comment: x.comment.clone(),
                })),
                // A body signal's label, interpolated like a block's.
                Item::Attr(a) => out.push(Item::Attr(AttrDecl {
                    slug: map_slug(&a.slug)?,
                    value: match &a.value {
                        Value::Str(s) => Value::Str(map_title(s)?),
                        v => map_value(v)?,
                    },
                    ..a.clone()
                })),
                Item::Set(s) => out.push(Item::Set(SetDecl {
                    target: map_ref(&s.target)?,
                    value: map_value(&s.value)?,
                    comment: s.comment.clone(),
                    bulk: s.bulk,
                })),
                // Claims on a body block or an object parameter: the set is
                // named directly, or by a set parameter so each instance
                // can pass its own (`stimmungen: moods = standard`,
                // overridden with `stimmungen: kueche`).
                Item::MoodsClaim(c) => out.push(Item::MoodsClaim(MoodsClaim {
                    slug: map_slug(&c.slug)?,
                    moods: map_set(&c.to_string(), &c.moods, SetKind::Moods)?,
                    comment: c.comment.clone(),
                })),
                Item::MembersClaim(c) => out.push(Item::MembersClaim(MembersClaim {
                    slug: map_slug(&c.slug)?,
                    members: c
                        .members
                        .iter()
                        .map(|m| map_slug(m))
                        .collect::<Result<_>>()?,
                    comment: c.comment.clone(),
                })),
                Item::ScheduleClaim(c) => out.push(Item::ScheduleClaim(ScheduleClaim {
                    slug: map_slug(&c.slug)?,
                    schedule: map_set(&c.to_string(), &c.schedule, SetKind::Schedule)?,
                    comment: c.comment.clone(),
                })),
                Item::TilesClaim(c) => out.push(Item::TilesClaim(TilesClaim {
                    slug: map_slug(&c.slug)?,
                    tiles: map_set(&c.to_string(), &c.tiles, SetKind::Tiles)?,
                    comment: c.comment.clone(),
                })),
                Item::AccessClaim(c) => out.push(Item::AccessClaim(AccessClaim {
                    slug: map_slug(&c.slug)?,
                    right: c.right,
                    set: map_set(&c.to_string(), &c.set, c.right.set_kind())?,
                    comment: c.comment.clone(),
                })),
                // Template-source commentary and the body's own constants,
                // not per-instance content.
                Item::Comment(_) | Item::Gap | Item::Let(_) => {}
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

/// [`Module::respell_error`] on one message: each name inside a backtick
/// span (a maximal `[a-z0-9_]` run, not after a `.`) that is an expanded
/// instance slug is replaced by its spelling. Synthetic names
/// (`kueche_lc__and1`) and ports are longer runs or follow a dot, so they
/// stay.
fn respell_text(msg: &str, scopes: &Scopes) -> String {
    let mut out = String::with_capacity(msg.len());
    for (i, part) in msg.split('`').enumerate() {
        if i > 0 {
            out.push('`');
        }
        if i % 2 == 0 {
            out.push_str(part);
            continue;
        }
        let bytes = part.as_bytes();
        let mut k = 0;
        while k < bytes.len() {
            let is_name = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_';
            if is_name(bytes[k]) && (k == 0 || !(is_name(bytes[k - 1]) || bytes[k - 1] == b'.')) {
                let end = (k..bytes.len())
                    .find(|&j| !is_name(bytes[j]))
                    .unwrap_or(bytes.len());
                let word = &part[k..end];
                match scopes.expanded.get(word) {
                    Some(x) => out.push_str(&scopes.spelling(&x.path, x.block)),
                    None => out.push_str(word),
                }
                k = end;
            } else {
                // Copy one whole character (messages are UTF-8).
                let ch = part[k..].chars().next().expect("in bounds");
                out.push(ch);
                k += ch.len_utf8();
            }
        }
    }
    out
}
