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
    ArgItem, Binding, BindingKind, BlockDecl, Expr, ExprWireDecl, Item, MatchSpec, ModeName,
    Module, MoodsClaim, Operand, PortRef, RuleItem, RuleStmtKind, ScheduleClaim, SetDecl, SetKind,
    SignalDecl, TemplateDecl, TemplateParam, TestItem, TilesClaim, TilesItem, Value, WireDecl,
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
        f(&mut r.slug, RefUse::Port(&r.port))
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
        Item::Extern(e) => {
            if let MatchSpec::Mirrors(target) = &mut e.match_spec {
                f(target, RefUse::Object)?;
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
        Item::Let(_)
        | Item::Template(_)
        | Item::Instance(_)
        | Item::Page(_)
        | Item::Access(_)
        | Item::Schedule(_)
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
                if let Operand::Port(p) = o {
                    f(&mut p.slug, RefUse::Port(&p.port))?;
                }
            }
            Ok(())
        }
        Expr::Arith { lhs, rhs, .. } => {
            visit_expr(lhs, f)?;
            visit_expr(rhs, f)
        }
        Expr::Atom(Operand::Port(p)) => f(&mut p.slug, RefUse::Port(&p.port)),
        Expr::Atom(Operand::Value(_)) => Ok(()),
    }
}

/// The module's template instances, for resolving scoped names.
struct Scopes<'m> {
    /// Instance slug → its template.
    instances: BTreeMap<&'m str, &'m TemplateDecl>,
    /// Expanded slug `<instance>_<block>` → (instance, block): the
    /// spelling source may no longer use.
    expanded: BTreeMap<String, (&'m str, &'m str)>,
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
        for item in &module.items {
            if let Item::Instance(call) = item
                && let Some(t) = templates.get(call.block_type.as_str())
            {
                instances.insert(call.slug.as_str(), *t);
                for b in t.body_slugs() {
                    expanded.insert(format!("{}_{b}", call.slug), (call.slug.as_str(), b));
                }
            }
        }
        Scopes {
            instances,
            expanded,
        }
    }

    /// How source writes `instance_block`: `instance` for the primary
    /// block, `instance.block` otherwise.
    fn spelling(&self, instance: &str, block: &str) -> String {
        let t = self.instances[instance];
        if t.primary_block() == Some(block) {
            instance.to_string()
        } else {
            format!("{instance}.{block}")
        }
    }

    /// The expanded slug a referenced name stands for: `a.b` is body
    /// block `b` of instance `a`, an instance alone its primary block.
    /// The expanded spelling itself (`a_b`) is refused with the one to
    /// write; a lockfile key is only joined (`a.b` → `a_b`) — what it
    /// names may be gone from the source.
    fn resolve(&self, name: &str, usage: RefUse) -> Result<String> {
        let at = |n: &str| match usage {
            RefUse::Port(p) => format!("{n}.{p}"),
            _ => n.to_string(),
        };
        let fail = |m: String| Err(Error::Compile(m));
        if let Some((a, b)) = name.split_once('.') {
            if usage == RefUse::LockKey {
                return Ok(format!("{a}_{b}"));
            }
            let Some(t) = self.instances.get(a) else {
                return fail(format!(
                    "`{}`: `{a}` is not a template instance, so `{a}.{b}` names no block",
                    at(name)
                ));
            };
            if b.contains('.') {
                return fail(format!(
                    "`{}`: templates do not nest. Name a block as `<instance>.<block>`",
                    at(name)
                ));
            }
            if !t.body_slugs().any(|s| s == b) {
                return fail(format!(
                    "`{}`: `{a}` (a `{}` instance) has no block `{b}`. Its blocks: {}",
                    at(name),
                    t.name,
                    t.body_slugs().collect::<Vec<_>>().join(", ")
                ));
            }
            if t.primary_block() == Some(b) {
                return fail(format!(
                    "`{b}` is the primary block of `{}`, so the instance names it. Write `{}` \
                     instead of `{}`.",
                    t.name,
                    at(a),
                    at(name)
                ));
            }
            return Ok(format!("{a}_{b}"));
        }
        if usage == RefUse::LockKey {
            return Ok(name.to_string());
        }
        if let Some(t) = self.instances.get(name) {
            return match t.primary_block() {
                Some(p) => Ok(format!("{name}_{p}")),
                None => fail(format!(
                    "`{}`: `{name}` is a `{}` instance, and that template has no primary \
                     block. Name the block (`{}`), or mark one primary with `template {}(…) \
                     -> <block>`. Its blocks: {}",
                    at(name),
                    t.name,
                    at(&format!("{name}.<block>")),
                    t.name,
                    t.body_slugs().collect::<Vec<_>>().join(", ")
                )),
            };
        }
        if let Some((a, b)) = self.expanded.get(name) {
            return fail(format!(
                "source names a block of a template instance by its instance. Write `{}` \
                 instead of `{}`.",
                at(&self.spelling(a, b)),
                at(name)
            ));
        }
        Ok(name.to_string())
    }

    /// The declared type of an instance's body block, by expanded slug.
    fn block_type(&self, expanded: &str) -> Option<&'m str> {
        let (a, b) = self.expanded.get(expanded)?;
        self.instances[a].body.iter().find_map(|i| match i {
            Item::Block(x) if x.slug == *b => Some(x.block_type.as_str()),
            _ => None,
        })
    }
}

/// Rewrite an expression's operands through the instance's slug/value
/// maps: port operands like wire refs, constant operands like values.
fn map_expr(
    e: &Expr,
    map_ref: &impl Fn(&PortRef) -> Result<PortRef>,
    map_value: &impl Fn(&Value) -> Result<Value>,
) -> Result<Expr> {
    let operand = |o: &Operand| -> Result<Operand> {
        Ok(match o {
            Operand::Port(p) => Operand::Port(map_ref(p)?),
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
    /// other items pass through with their scoped names (`a.b`, an
    /// instance for its primary block) resolved to expanded slugs. Name
    /// resolution of the result is the caller's job ([`Module::validate`])
    /// — expansion only checks what it needs.
    pub fn expand(&self) -> Result<Module> {
        let mut templates: BTreeMap<&str, &TemplateDecl> = BTreeMap::new();
        for item in &self.items {
            if let Item::Template(t) = item {
                if templates.insert(&t.name, t).is_some() {
                    return Err(Error::Compile(format!("duplicate template `{}`", t.name)));
                }
                if let Some(p) = &t.primary
                    && !t.body_slugs().any(|s| s == p)
                {
                    return Err(Error::Compile(format!(
                        "template `{}`: `-> {p}` names no block of the body. Its blocks: {}",
                        t.name,
                        t.body_slugs().collect::<Vec<_>>().join(", ")
                    )));
                }
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
                return Err(Error::Compile(format!("duplicate name `{name}`")));
            }
        }
        let scopes = Scopes::of(self);
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
                    items.extend(self.instantiate(&scopes, t, call)?);
                }
                other => {
                    let mut other = other.clone();
                    visit_refs(&mut other, &mut |name, usage| {
                        *name = scopes.resolve(name, usage)?;
                        Ok(())
                    })?;
                    items.push(other);
                }
            }
        }
        Ok(Module { items })
    }

    /// The scoped-name errors [`Module::expand`] would report (`a_b` for
    /// `a.b`, an unknown block, an instance without a primary block),
    /// without expanding: the unexpanded module's validation runs it first,
    /// so those references get their own message, not "undeclared".
    pub(crate) fn check_scoped_names(&self) -> Result<()> {
        let scopes = Scopes::of(self);
        if scopes.instances.is_empty() {
            return Ok(());
        }
        for item in &self.items {
            let mut item = match item {
                Item::Template(_) | Item::Instance(_) => continue,
                other => other.clone(),
            };
            visit_refs(&mut item, &mut |name, usage| {
                scopes.resolve(name, usage).map(|_| ())
            })?;
        }
        Ok(())
    }

    /// Whether the module declares a `kind` set named `name`.
    pub(crate) fn declares_set(&self, kind: SetKind, name: &str) -> bool {
        self.items.iter().any(|i| match (kind, i) {
            (SetKind::Moods, Item::Moods(m)) => m.name == name,
            (SetKind::Schedule, Item::Schedule(x)) => x.name == name,
            (SetKind::Tiles, Item::Tiles(x)) => x.name == name,
            _ => false,
        })
    }

    /// How source spells the expanded slug `slug`: `instance.block`, or
    /// the instance alone for its template's primary block. Any
    /// other slug is spelled as it is.
    pub fn spelling(&self, slug: &str) -> String {
        let scopes = Scopes::of(self);
        match scopes.expanded.get(slug) {
            Some((a, b)) => scopes.spelling(a, b),
            None => slug.to_string(),
        }
    }

    /// Rewrite the references of `items` (generated against the expanded
    /// module, as adopt does) into this module's spellings.
    pub fn respell(&self, items: &mut [Item]) {
        let scopes = Scopes::of(self);
        for item in items {
            let _ = visit_refs(item, &mut |name, usage| {
                if usage != RefUse::LockKey
                    && let Some((a, b)) = scopes.expanded.get(name.as_str())
                {
                    *name = scopes.spelling(a, b);
                }
                Ok(())
            });
        }
    }

    fn instantiate(
        &self,
        scopes: &Scopes,
        t: &TemplateDecl,
        call: &BlockDecl,
    ) -> Result<Vec<Item>> {
        let ctx = format!("in `{} = {}(…)`", call.slug, call.block_type);
        let fail = |m: String| Error::Compile(format!("{ctx}: {m}"));

        // Bind arguments: object parameters to the passed slugs (type
        // annotation checked where the slug's declaration is visible),
        // set parameters to the passed set, value parameters to the given
        // value or their default.
        let mut objects: BTreeMap<&str, String> = BTreeMap::new();
        let mut sets: BTreeMap<&str, (SetKind, String)> = BTreeMap::new();
        let mut values: BTreeMap<&str, Value> = BTreeMap::new();
        for p in &t.params {
            match p {
                TemplateParam::Value { name, default } => {
                    values.insert(name, default.clone());
                }
                TemplateParam::Set {
                    name,
                    kind,
                    default: Some(d),
                } => {
                    sets.insert(name, (*kind, d.clone()));
                }
                TemplateParam::Set { default: None, .. } | TemplateParam::Object { .. } => {}
            }
        }
        // A call-site binding that names no template parameter forwards as
        // a port binding onto the template's primary block — an instance
        // is a composite block, so its call reads like a block declaration
        // with the shared parameters factored away (feeds may repeat,
        // exactly as in a block's argument list).
        let primary = t.primary_block();
        let mut forwards: Vec<Binding> = Vec::new();
        for binding in call.bindings() {
            let Some(p) = t.params.iter().find(|p| p.name() == binding.port) else {
                // A named signal takes no port bindings: its inputs are
                // its expression.
                if let Some(p) = primary
                    && t.body
                        .iter()
                        .any(|i| matches!(i, Item::Signal(x) if x.slug == p))
                {
                    return Err(fail(format!(
                        "`{}` names no parameter of `{}`. Its primary block is the named \
                         signal `{p}`, which takes no port bindings.",
                        binding.port, t.name
                    )));
                }
                if primary.is_some() {
                    let mut forward = binding.clone();
                    let mut args = [ArgItem::Binding(forward.clone())];
                    visit_args(&mut args, &mut |name, usage| {
                        *name = scopes.resolve(name, usage)?;
                        Ok(())
                    })?;
                    if let [ArgItem::Binding(b)] = args {
                        forward = b;
                    }
                    forwards.push(forward);
                    continue;
                }
                return Err(fail(format!(
                    "`{}` names no parameter of `{}`. Port bindings forward to the \
                     template's primary block, and `{}` has none: its body declares {} \
                     blocks. Mark one with `template {}(…) -> <block>`.",
                    binding.port,
                    t.name,
                    t.name,
                    t.body_slugs().count(),
                    t.name
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
                    if self
                        .items
                        .iter()
                        .any(|i| matches!(i, Item::Template(other) if other.name == s))
                    {
                        return Err(fail(format!(
                            "`{s}` is a template, not an object. Pass an extern, a block or an instance."
                        )));
                    }
                    let s = scopes.resolve(&s, RefUse::Object)?;
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
                (TemplateParam::Value { name, .. }, _) => match &binding.kind {
                    BindingKind::Param(v) => {
                        values.insert(name, v.clone());
                    }
                    BindingKind::Wire(_) | BindingKind::Expr(_) => {
                        return Err(fail(format!(
                            "value parameter `{name}` takes a number, string, or constant"
                        )));
                    }
                },
            }
        }
        for p in &t.params {
            match p {
                TemplateParam::Object { name, .. } if !objects.contains_key(name.as_str()) => {
                    return Err(fail(format!("object parameter `{name}` must be given")));
                }
                TemplateParam::Set { name, .. } if !sets.contains_key(name.as_str()) => {
                    return Err(fail(format!("set parameter `{name}` must be given")));
                }
                _ => {}
            }
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

        let body_slugs: Vec<&str> = t.body_slugs().collect();
        let map_slug = |s: &str| -> Result<String> {
            if body_slugs.contains(&s) {
                Ok(format!("{}_{s}", call.slug))
            } else if let Some(o) = objects.get(s) {
                Ok(o.clone())
            } else {
                scopes.resolve(s, RefUse::Object)
            }
        };
        let map_ref = |r: &PortRef| -> Result<PortRef> {
            let slug = if body_slugs.contains(&r.slug.as_str()) || objects.contains_key(&*r.slug) {
                map_slug(&r.slug)?
            } else {
                scopes.resolve(&r.slug, RefUse::Port(&r.port))?
            };
            Ok(PortRef {
                slug,
                port: r.port.clone(),
            })
        };
        let map_value = |v: &Value| -> Result<Value> {
            if let Value::Ref(n) = v {
                if let Some(val) = values.get(n.as_str()) {
                    return Ok(val.clone());
                }
                if objects.contains_key(n.as_str()) {
                    return Err(fail(format!(
                        "object parameter `{n}` is used as a value. Reference one of its ports (`{n}.<Port>`) or use a value parameter."
                    )));
                }
            }
            Ok(v.clone())
        };
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
            if let Some(TemplateParam::Value { default, .. }) =
                t.params.iter().find(|p| p.name() == set)
            {
                let default = match default {
                    Value::Str(s) => s.clone(),
                    other => other.to_string(),
                };
                return Err(Error::Compile(format!(
                    "template `{}`: a `{kw}` set is passed by a set parameter. Write \
                     `{set}: {kw} = {default}` instead of `{set} = \"{default}\"`.",
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
                                    BindingKind::Wire(r) => BindingKind::Wire(map_ref(r)?),
                                    BindingKind::Expr(e) => {
                                        BindingKind::Expr(map_expr(e, &map_ref, &map_value)?)
                                    }
                                },
                                comment: x.comment.clone(),
                            })),
                            ArgItem::Comment(c) => args.push(ArgItem::Comment(c.clone())),
                        }
                    }
                    // Forwarded port bindings land on the primary block,
                    // resolved in module scope at the call site — never in
                    // the template's parameter scope.
                    if primary == Some(b.slug.as_str()) {
                        args.extend(forwards.iter().cloned().map(ArgItem::Binding));
                    }
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
                    to: map_ref(&w.to)?,
                    from: map_ref(&w.from)?,
                    comment: w.comment.clone(),
                })),
                Item::ExprWire(w) => out.push(Item::ExprWire(ExprWireDecl {
                    to: map_ref(&w.to)?,
                    expr: map_expr(&w.expr, &map_ref, &map_value)?,
                    comment: w.comment.clone(),
                })),
                // A named signal is a body-local name like a block: its
                // slug (and so its sub-blocks' prefix) is the instance's.
                Item::Signal(x) => out.push(Item::Signal(SignalDecl {
                    slug: map_slug(&x.slug)?,
                    expr: map_expr(&x.expr, &map_ref, &map_value)?,
                    comment: x.comment.clone(),
                })),
                Item::Set(s) => out.push(Item::Set(SetDecl {
                    target: map_ref(&s.target)?,
                    value: map_value(&s.value)?,
                    comment: s.comment.clone(),
                    bulk: s.bulk,
                })),
                // Claims on a body block or an object parameter: the set is named directly, or by a set parameter
                // so each instance can pass its own (`stimmungen: moods =
                // standard`, overridden with `stimmungen: kueche`).
                Item::MoodsClaim(c) => out.push(Item::MoodsClaim(MoodsClaim {
                    slug: map_slug(&c.slug)?,
                    moods: map_set(&c.to_string(), &c.moods, SetKind::Moods)?,
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
