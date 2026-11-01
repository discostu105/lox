//! [`Module::validate`]: name uniqueness, reference resolution, and
//! statement-level consistency, one check per statement kind.

use super::*;

/// One namespace for externs, blocks, and constants (and templates,
/// instances and the named sets).
type Names<'a> = BTreeMap<&'a str, NameKind>;

/// The module-wide view the statement checks share.
struct Scope<'a> {
    names: Names<'a>,
    /// Whether every template and expression has already been lowered into
    /// plain blocks. Test bodies may reference the slugs those lowerings
    /// mint, so their references only resolve on this form.
    flattened: bool,
    /// The one spelling of each operating mode the module names.
    modes: ModeSpellings,
}

/// Statements that may appear once per target, collected across items.
#[derive(Default)]
struct Seen<'a> {
    attrs_seen: BTreeSet<(&'a str, Attr)>,
    tiles_claimed: BTreeSet<&'a str>,
    access_claimed: BTreeSet<&'a str>,
    schedules_claimed: BTreeSet<&'a str>,
    moods_claimed: BTreeSet<&'a str>,
    members_claimed: BTreeSet<&'a str>,
    rules_declared: BTreeSet<&'a str>,
    rule_titles: BTreeSet<&'a str>,
}

fn compile_err<T>(msg: String) -> Result<T> {
    Err(Error::Compile(msg))
}

/// A named signal's inputs are its expression: no wire, expression or
/// value statement targets one of its ports.
fn not_a_signal(names: &Names, target: &PortRef, what: &str) -> Result<()> {
    if names.get(target.slug.as_str()) == Some(&NameKind::Signal) {
        return compile_err(format!(
            "`{target} {what}` targets named signal `{}`, whose inputs are its \
             expression. Change the expression instead.",
            target.slug,
        ));
    }
    Ok(())
}

/// A reference to an object: the slug must name an extern or a block.
fn object_ref(names: &Names, r: &PortRef) -> Result<()> {
    // `instance.block`: resolved by the expansion.
    if let Some((a, _)) = r.slug.split_once('.')
        && names.get(a) == Some(&NameKind::Instance)
    {
        return Ok(());
    }
    match names.get(r.slug.as_str()) {
        None => compile_err(format!(
            "reference to undeclared slug `{}` (in `{r}`)",
            r.slug
        )),
        Some(NameKind::Let) => compile_err(format!(
            "`{}` is a `let` constant, not a block or extern (in `{r}`)",
            r.slug
        )),
        Some(NameKind::Template) => compile_err(format!(
            "`{slug}` is a template, not an object. Instantiate it first \
                     (`<slug> = {slug}(…)`) (in `{r}`)",
            slug = r.slug
        )),
        // An instance names its primary block; the expansion checks it
        // has one.
        Some(NameKind::Instance) => Ok(()),
        Some(NameKind::Tiles) => compile_err(format!(
            "`{}` is a `tiles` set, not an object (in `{r}`)",
            r.slug
        )),
        Some(_) => Ok(()),
    }
}

/// The objects `item` references that `names` does not declare (an
/// instance's `a.b` counts as declared: the expansion resolves it).
fn undeclared(names: &Names, item: &Item) -> Vec<String> {
    let mut out = Vec::new();
    let mut item = item.clone();
    let _ = crate::logic::template::visit_refs(&mut item, &mut |name, usage| {
        if usage == crate::logic::template::RefUse::LockKey {
            return Ok(());
        }
        let declared = match name.split_once('.') {
            Some((a, _)) => names.get(a) == Some(&NameKind::Instance),
            None => names.contains_key(name.as_str()),
        };
        if !declared {
            out.push(name.clone());
        }
        Ok(())
    });
    out
}

impl Module {
    /// Name uniqueness, reference resolution, and statement-level
    /// consistency (no base config needed). Port existence and directions
    /// on managed blocks are checked separately by
    /// [`crate::logic::validate_ports`].
    ///
    /// Statements are checked independently: every failing statement
    /// reports its error ([`Error::Many`] for several). A statement whose
    /// only undeclared names an earlier failing statement already used is
    /// skipped, so one missing declaration is reported once.
    pub fn validate(&self) -> Result<()> {
        let mut errors = crate::error::Errors::default();
        let names = self.name_table(&mut errors);
        errors.check(self.check_scoped_names());
        // Tests are named by display string, in a namespace of their own.
        let mut test_names: BTreeSet<&str> = BTreeSet::new();
        for item in &self.items {
            if let Item::Test(t) = item
                && !test_names.insert(t.name.as_str())
            {
                errors.push(Error::Compile(format!(
                    "duplicate test name {}",
                    quote(&t.name)
                )));
            }
        }
        // Whether every template and expression has already been lowered
        // into plain blocks. Test bodies may reference the slugs those
        // lowerings mint, so their references only resolve on this form.
        let flattened = !self.items.iter().any(|i| match i {
            Item::Template(_) | Item::Instance(_) | Item::ExprWire(_) | Item::Signal(_) => true,
            Item::Block(b) => b.bindings().any(|x| matches!(x.kind, BindingKind::Expr(_))),
            _ => false,
        });
        errors.check(crate::logic::defaults::by_type(&self.items));
        let sc = Scope {
            names,
            flattened,
            modes: ModeSpellings::of([self]),
        };
        let mut seen = Seen::default();
        // Undeclared names of statements that failed.
        let mut reported: BTreeSet<String> = BTreeSet::new();
        for item in &self.items {
            let unknown = undeclared(&sc.names, item);
            if !unknown.is_empty() && unknown.iter().all(|n| reported.contains(n)) {
                continue;
            }
            if errors
                .check(self.check_item(&sc, item, &mut seen))
                .is_none()
            {
                reported.extend(unknown);
            }
        }
        errors.check(self.check_lifecycle(&sc.names));
        errors.finish()
    }

    /// The declared names, each once; a duplicate is an error and keeps
    /// its first declaration.
    fn name_table(&self, errors: &mut crate::error::Errors) -> Names<'_> {
        // One namespace for externs, blocks, and constants.
        let mut names: BTreeMap<&str, NameKind> = BTreeMap::new();
        for item in &self.items {
            let (name, kind) = match item {
                Item::Extern(e) => (e.slug.as_str(), NameKind::Extern),
                Item::Block(b) => (b.slug.as_str(), NameKind::Block),
                Item::Signal(x) => (x.slug.as_str(), NameKind::Signal),
                Item::Let(l) => (l.name.as_str(), NameKind::Let),
                Item::Template(t) => (t.name.as_str(), NameKind::Template),
                Item::Instance(b) => (b.slug.as_str(), NameKind::Instance),
                Item::Tiles(t) => (t.name.as_str(), NameKind::Tiles),
                Item::Schedule(t) => (t.name.as_str(), NameKind::Schedule),
                Item::Moods(t) => (t.name.as_str(), NameKind::Moods),
                _ => continue,
            };
            if names.contains_key(name) {
                errors.push(Error::Compile(format!("duplicate name `{name}`")));
            } else {
                names.insert(name, kind);
            }
        }
        // Rights sets last: a set named like its block is the per-block
        // form of earlier versions, which gets its replacement.
        for a in self.access_decls() {
            let name = a.name.as_str();
            match names.get(name) {
                Some(NameKind::Extern | NameKind::Block) => {
                    let (kw, set) = (a.right.keyword(), format!("{name}_{}", a.right.keyword()));
                    errors.push(Error::Compile(format!(
                        "`{kw} {name}` names {} `{name}`, but {} are a named set the block claims. \
                         Write `{kw} {set}` instead of `{kw} {name}`, and claim it with \
                         `{name}.{kw} = {set}`.",
                        a.right.block_noun(),
                        a.right.rights_noun()
                    )));
                }
                Some(_) => errors.push(Error::Compile(format!("duplicate name `{name}`"))),
                None => {
                    names.insert(name, NameKind::Rights(a.right));
                }
            }
        }
        names
    }

    /// The block type of a managed block, `…` if `slug` declares none.
    fn block_type_of(&self, slug: &str) -> &str {
        self.blocks()
            .find(|b| b.slug == slug)
            .map_or("…", |b| &b.block_type)
    }

    /// A value that is a reference must name a `let` constant.
    fn value_refs(&self, names: &Names, value: &Value) -> Result<()> {
        let Value::Ref(name) = value else {
            return Ok(());
        };
        match names.get(name.as_str()) {
            Some(NameKind::Let) => Ok(()),
            Some(kind) => compile_err(format!(
                "`{name}` is {}, not a `let` constant (quote the value if a \
                     string was intended)",
                kind.describe()
            )),
            None => {
                let hint =
                    crate::logic::validate::suggest(name, self.lets().map(|l| l.name.as_str()));
                compile_err(format!(
                    "undeclared constant `{name}`{hint} (declare it with \
                         `let {name} = …`, or quote the value if a string was intended)"
                ))
            }
        }
    }

    /// Expression operands, shared by `<-` expressions and expression
    /// bindings in argument lists: ports must resolve, constants must be
    /// numbers or `let` references.
    fn check_expr_operands(&self, names: &Names, e: &Expr, sink: &str) -> Result<()> {
        for operand in e.operands() {
            match operand {
                Operand::Port(p) => object_ref(names, p)?,
                Operand::Value(Value::Str(s)) => {
                    return compile_err(format!(
                        "string {} in the expression on `{sink}`. \
                             Expressions compare numbers and ports only",
                        quote(s),
                    ));
                }
                Operand::Value(v) => self.value_refs(names, v)?,
            }
        }
        Ok(())
    }

    /// One statement's checks, by kind.
    fn check_item<'a>(&'a self, sc: &Scope<'a>, item: &'a Item, seen: &mut Seen<'a>) -> Result<()> {
        let names = &sc.names;
        match item {
            Item::Block(b) => {
                if b.block_type == "CalendarEntry" {
                    self.check_calendar_entry(sc, b)?;
                }
                self.check_block(names, b)?;
            }
            Item::Wire(w) => self.check_wire(names, w)?,
            Item::ExprWire(w) => self.check_expr_wire(names, w)?,
            Item::Signal(x) => self.check_expr_operands(names, &x.expr, &x.slug)?,
            Item::Set(s) => self.check_set(names, s)?,
            Item::Attr(a) => self.check_attr(names, a, seen)?,
            Item::Test(t) => self.check_test(sc, t)?,
            Item::Template(t) => check_template(t)?,
            Item::Instance(call) => self.check_instance(names, call)?,
            Item::Defaults(d) => {
                for (_, v) in d.params() {
                    self.value_refs(names, v)?;
                }
            }
            Item::Tiles(t) => self.check_tiles(sc, t)?,
            Item::TilesClaim(c) => self.check_tiles_claim(names, c, seen)?,
            Item::Access(a) => self.check_access(a)?,
            Item::AccessClaim(c) => self.check_access_claim(sc, c, seen)?,
            Item::Schedule(t) => {
                self.check_schedule_days(sc, t)?;
                crate::logic::schedule::program(t)
                    .map_err(|m| Error::Compile(format!("`schedule {}`: {m}", t.name)))?;
            }
            Item::ScheduleClaim(c) => self.check_schedule_claim(sc, c, seen)?,
            Item::Moods(t) => check_moods(t)?,
            Item::MoodsClaim(c) => self.check_moods_claim(sc, c, seen)?,
            Item::MembersClaim(c) => self.check_members_claim(sc, c, seen)?,
            Item::Rule(r) => self.check_rule(sc, r, seen)?,
            Item::Page(p) if p.title.is_empty() => {
                return compile_err("`page \"\"`: the page title must not be empty".into());
            }
            _ => {}
        }
        Ok(())
    }

    /// A block declaration: its parameters, wires and expressions, each
    /// once, and no value next to a wire on one port.
    fn check_block(&self, names: &Names, b: &BlockDecl) -> Result<()> {
        let mut params_seen: BTreeSet<&str> = BTreeSet::new();
        let mut wires_seen: BTreeSet<(&str, &PortRef)> = BTreeSet::new();
        let mut exprs_seen: BTreeSet<(&str, String)> = BTreeSet::new();
        for binding in b.bindings() {
            match &binding.kind {
                BindingKind::Param(v) => {
                    // `mirrors:` on a minted ref names an object,
                    // not a constant — its target is
                    // checked by `validate_ports`. On any other
                    // type the key is a mistake worth a message
                    // clearer than the constant-resolution one.
                    if binding.port == "device" {
                        // The device an I/O hangs below,
                        // checked by `validate_ports`.
                        if !crate::periphery::is_device_binding(&b.block_type, "device") {
                            return compile_err(format!(
                                "`device:` names the device an I/O hangs below, \
                                             but {} has no device (in `{} = {}(…)`)",
                                b.block_type, b.slug, b.block_type
                            ));
                        }
                    } else if binding.port == crate::logic::calendar::SWITCHES {
                        // The mode a calendar entry switches on, checked
                        // by `check_calendar_entry`.
                        if b.block_type != "CalendarEntry" {
                            return compile_err(format!(
                                "`switches:` names the operating mode a calendar entry \
                                 switches on, but {} is no calendar entry (in `{} = {}(…)`)",
                                b.block_type, b.slug, b.block_type
                            ));
                        }
                    } else if binding.port == "tree" && b.block_type == "TreeDevice" {
                        // The Tree extension a created device goes on,
                        // checked when the device is created.
                    } else if binding.port == "mirrors" {
                        if !matches!(
                            b.block_type.as_str(),
                            "InputRef" | "OutputRef" | "OutputRefLM"
                        ) {
                            return compile_err(format!(
                                "`mirrors:` applies to InputRef/\
                                             OutputRef/OutputRefLM only, not {} (in `{} = \
                                             {}(…)`)",
                                b.block_type, b.slug, b.block_type
                            ));
                        }
                    } else {
                        self.value_refs(names, v)?;
                    }
                    if !params_seen.insert(&binding.port) {
                        return compile_err(format!(
                            "block `{}`: duplicate parameter `{}` in `{} = {}(…)`",
                            b.slug, binding.port, b.slug, b.block_type
                        ));
                    }
                }
                BindingKind::Wire(src) => {
                    object_ref(names, src)?;
                    if !wires_seen.insert((&binding.port, src)) {
                        return compile_err(format!(
                            "block `{}`: duplicate wire `{}: {src}` in `{} = {}(…)`",
                            b.slug, binding.port, b.slug, b.block_type
                        ));
                    }
                }
                BindingKind::Expr(e) => {
                    self.check_expr_operands(names, e, &format!("{}.{}", b.slug, binding.port))?;
                    if !exprs_seen.insert((&binding.port, e.to_string())) {
                        return compile_err(format!(
                            "block `{}`: duplicate expression `{}: {e}` in `{} = {}(…)`",
                            b.slug, binding.port, b.slug, b.block_type
                        ));
                    }
                }
            }
        }
        // A value next to a wire on one port: the wire
        // wins and the value is inert, so writing both is a
        // mistake — or a decompiled stale constant restated.
        for binding in b.bindings() {
            let BindingKind::Param(v) = &binding.kind else {
                continue;
            };
            if let Some(w) = b.bindings().find(|w| {
                w.port == binding.port
                    && matches!(w.kind, BindingKind::Wire(_) | BindingKind::Expr(_))
            }) {
                return compile_err(format!(
                    "block `{slug}`: port `{port}` has both a value (`{port}: {v}`) and a wire (`{port}: {wire}`) in `{slug} = {ty}(…)`. Keep one, since a wired input ignores its value.",
                    slug = b.slug,
                    port = binding.port,
                    wire = w.kind,
                    ty = b.block_type,
                ));
            }
        }
        Ok(())
    }

    /// `to <- from` wires an extern port.
    fn check_wire(&self, names: &Names, w: &WireDecl) -> Result<()> {
        object_ref(names, &w.to)?;
        object_ref(names, &w.from)?;
        not_a_signal(names, &w.to, "<- …")?;
        if names.get(w.to.slug.as_str()) == Some(&NameKind::Block) {
            return compile_err(format!(
                "`{to} <- {from}` targets managed block `{slug}`, but `<-` is for extern ports only. \
                             Wire it in the argument list instead: `{port}: {from}` inside \
                             `{slug} = {ty}(…)`",
                to = w.to,
                from = w.from,
                slug = w.to.slug,
                port = w.to.port,
                ty = self.block_type_of(&w.to.slug),
            ));
        }
        Ok(())
    }

    /// `to <- expression` drives an extern port.
    fn check_expr_wire(&self, names: &Names, w: &ExprWireDecl) -> Result<()> {
        object_ref(names, &w.to)?;
        not_a_signal(names, &w.to, "<- …")?;
        if names.get(w.to.slug.as_str()) == Some(&NameKind::Block) {
            return compile_err(format!(
                "`{to} <- …` targets managed block `{slug}`, but `<-` is for extern ports only. \
                             Wire the expression's result in the argument list instead",
                to = w.to,
                slug = w.to.slug,
            ));
        }
        self.check_expr_operands(names, &w.expr, &w.to.to_string())?;
        Ok(())
    }

    /// `target.Port = value` sets an extern port.
    fn check_set(&self, names: &Names, s: &SetDecl) -> Result<()> {
        object_ref(names, &s.target)?;
        not_a_signal(names, &s.target, "= …")?;
        if names.get(s.target.slug.as_str()) == Some(&NameKind::Block) {
            return compile_err(format!(
                "`{target} = …` targets managed block `{slug}`, but port assignment is for extern ports only. \
                             Bind the parameter in the argument list instead: `{port}: …` inside \
                             `{slug} = {ty}(…)`",
                target = s.target,
                slug = s.target.slug,
                port = s.target.port,
                ty = self.block_type_of(&s.target.slug),
            ));
        }
        self.value_refs(names, &s.value)?;
        Ok(())
    }

    /// `slug.title|room|category = value`.
    fn check_attr<'a>(&self, names: &Names, a: &'a AttrDecl, seen: &mut Seen<'a>) -> Result<()> {
        let attrs_seen = &mut seen.attrs_seen;
        let target = format!("{}.{}", a.slug, a.attr);
        match names.get(a.slug.as_str()) {
            Some(NameKind::Extern) => {}
            // A managed block's room and category are the
            // source's when it states them; its title
            // is its label.
            Some(NameKind::Block) if a.attr != Attr::Title => {}
            // A named signal has no call to carry a label: `.title` is
            // its top block's label.
            Some(NameKind::Signal) if a.attr == Attr::Title => {}
            Some(NameKind::Signal) => {
                return compile_err(format!(
                    "`{target} = …` targets named signal `{}`, which takes only a \
                     `.title`. Declare the blocks by hand to set its {}.",
                    a.slug, a.attr
                ));
            }
            Some(NameKind::Block) => {
                return compile_err(format!(
                    "`{target} = …` targets managed block `{}`. Give it a \
                                 label instead (`{} = {}(\"…\", …)`)",
                    a.slug,
                    a.slug,
                    self.block_type_of(&a.slug)
                ));
            }
            _ => object_ref(
                names,
                &PortRef {
                    slug: a.slug.clone(),
                    port: a.attr.name().to_string(),
                },
            )?,
        }
        // An assignment must not rewrite what its own extern is
        // matched by: once deployed, the match would no longer
        // find the object it pinned.
        if let Some(e) = self.externs().find(|e| e.slug == a.slug) {
            let matched_by_it = match a.attr {
                Attr::Title => matches!(e.match_spec, MatchSpec::Title(_)),
                Attr::Room => e.room.is_some(),
                Attr::Category => e.category.is_some(),
            };
            if matched_by_it {
                return compile_err(format!(
                    "`{target} = …` changes what extern `{}` is matched by (`{}`), so the match \
                                 would fail after the push. Pin the extern by `uuid:` or `iname:` instead",
                    a.slug,
                    e.spec()
                ));
            }
        }
        if !attrs_seen.insert((a.slug.as_str(), a.attr)) {
            return compile_err(format!("duplicate assignment `{target} = …`"));
        }
        if let Value::Unit { .. } = a.value {
            return compile_err(format!("`{a}`: a {} is text, not a unit value", a.attr));
        }
        self.value_refs(names, &a.value)?;
        Ok(())
    }

    /// Unlike `Set`, a test may target managed blocks — driving
    /// their inputs (and overriding outputs) is the point. Slug
    /// references are deferred to the flattened view: tests may
    /// name the blocks of template instances (`frost.Q`) and
    /// expression-owned blocks (`…__and2`), which only exist
    /// after expansion and desugaring — compile/check/test all
    /// re-validate that form.
    fn check_test(&self, sc: &Scope, t: &TestDecl) -> Result<()> {
        let (names, flattened) = (&sc.names, sc.flattened);
        for stmt in &t.body {
            let numeric = |target: &dyn fmt::Display, v: &Value| -> Result<()> {
                if let Value::Str(s) = v {
                    return compile_err(format!(
                        "`{target}` gets string {} in test {}. The \
                                     simulator handles numbers only",
                        quote(s),
                        quote(&t.name)
                    ));
                }
                self.value_refs(names, v)
            };
            match stmt {
                TestItem::Inject(s) => {
                    if flattened {
                        object_ref(names, &s.target)?;
                    }
                    numeric(&s.target, &s.value)?;
                }
                TestItem::Expect(e) => {
                    if flattened {
                        object_ref(names, &e.port)?;
                    }
                    numeric(&e.port, &e.value)?;
                }
                TestItem::Tick(tick) => {
                    self.value_refs(names, &tick.amount)?;
                    if let Some(dt) = &tick.dt {
                        self.value_refs(names, dt)?;
                    }
                    tick.count(|name| self.lets().find(|l| l.name == name).map(|l| &l.value))
                        .map_err(|e| Error::Compile(format!("test {}: {e}", quote(&t.name))))?;
                }
                TestItem::Clock(_) | TestItem::Comment(_) => {}
            }
        }
        Ok(())
    }

    /// A template instantiation: a known template, its arguments each once
    /// and of the parameter's kind, every object parameter given.
    fn check_instance(&self, names: &Names, call: &BlockDecl) -> Result<()> {
        let Some(t) = self.items.iter().find_map(|i| match i {
            Item::Template(t) if t.name == call.block_type => Some(t),
            _ => None,
        }) else {
            let hint = crate::logic::validate::suggest(
                &call.block_type,
                self.items.iter().filter_map(|i| match i {
                    Item::Template(t) => Some(t.name.as_str()),
                    _ => None,
                }),
            );
            return compile_err(format!(
                "`{} = {name}(…)` instantiates unknown template `{name}`{hint}",
                call.slug,
                name = call.block_type
            ));
        };
        if call.title.is_some() {
            return compile_err(format!(
                "`{} = {}(…)`: an instantiation takes no label string. \
                             Titles belong on the template's blocks",
                call.slug, call.block_type
            ));
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let body_blocks = t.body_slugs().count();
        for binding in call.bindings() {
            let Some(p) = t.params.iter().find(|p| p.name() == binding.port) else {
                // A binding naming no parameter forwards as a port
                // binding onto the template's primary block;
                // like a block's feeds it may repeat, and the expanded
                // module re-validates its refs.
                if t.primary_block().is_some() {
                    if let BindingKind::Param(v) = &binding.kind {
                        self.value_refs(names, v)?;
                    }
                    continue;
                }
                let hint = crate::logic::validate::suggest(
                    &binding.port,
                    t.params.iter().map(|p| p.name()),
                );
                return compile_err(format!(
                    "`{} = {}(…)`: `{}` names no parameter of `{}`, and \
                                 port bindings forward only to a primary block (this \
                                 body declares {body_blocks} blocks; mark one with \
                                 `-> <block>`){hint}",
                    call.slug, call.block_type, binding.port, call.block_type
                ));
            };
            if !seen.insert(&binding.port) {
                return compile_err(format!(
                    "`{} = {}(…)`: duplicate argument `{}`",
                    call.slug, call.block_type, binding.port
                ));
            }
            match (p, &binding.kind) {
                (TemplateParam::Object { .. }, BindingKind::Param(Value::Ref(_))) => {}
                (TemplateParam::Set { name, kind, .. }, BindingKind::Param(Value::Ref(s))) => {
                    if !self.declares_set(*kind, s) {
                        return compile_err(format!(
                            "`{} = {}(…)`: `{name}: {s}`: `{s}` is not a `{kw}` set \
                             (declare it with `{kw} {s}` … `end`)",
                            call.slug,
                            call.block_type,
                            kw = kind.keyword()
                        ));
                    }
                }
                // `p: licht.lc`, a block of an instance.
                (TemplateParam::Object { .. }, BindingKind::Wire(r))
                    if r.port.starts_with(|c: char| c.is_ascii_lowercase()) => {}
                (TemplateParam::Object { name, .. }, _) => {
                    return compile_err(format!(
                        "`{} = {}(…)`: object parameter `{name}` takes an \
                                     extern, block or instance (`{name}: <slug>`)",
                        call.slug, call.block_type
                    ));
                }
                (TemplateParam::Set { name, .. }, BindingKind::Param(Value::Str(s))) => {
                    return compile_err(format!(
                        "`{} = {}(…)`: a set parameter takes the set's bare name. \
                         Write `{name}: {s}` instead of `{name}: \"{s}\"`.",
                        call.slug, call.block_type
                    ));
                }
                (TemplateParam::Set { name, kind, .. }, _) => {
                    return compile_err(format!(
                        "`{} = {}(…)`: set parameter `{name}` takes the name of a \
                         `{}` set (`{name}: <name>`)",
                        call.slug,
                        call.block_type,
                        kind.keyword()
                    ));
                }
                (TemplateParam::Value { .. }, BindingKind::Param(v)) => {
                    self.value_refs(names, v)?;
                }
                (
                    TemplateParam::Value { name, .. },
                    BindingKind::Wire(_) | BindingKind::Expr(_),
                ) => {
                    return compile_err(format!(
                        "`{} = {}(…)`: value parameter `{name}` takes a \
                                     number, string, or constant, not a port or \
                                     expression",
                        call.slug, call.block_type
                    ));
                }
            }
        }
        for p in &t.params {
            let what = match p {
                TemplateParam::Object { .. } => "object",
                TemplateParam::Set { default: None, .. } => "set",
                _ => continue,
            };
            if !seen.contains(p.name()) {
                return compile_err(format!(
                    "`{} = {}(…)`: {what} parameter `{}` must be given",
                    call.slug,
                    call.block_type,
                    p.name()
                ));
            }
        }
        Ok(())
    }

    /// `to:` targets name blocks, which may be blocks of template
    /// instances (`ost`, `ost.jal`) — an unknown slug is judged on the
    /// flattened view only, like test references.
    fn check_tiles(&self, sc: &Scope, t: &TilesDecl) -> Result<()> {
        let (names, flattened) = (&sc.names, sc.flattened);
        let mut indices: BTreeSet<u32> = BTreeSet::new();
        for tile in t.tiles() {
            let ctx = format!("tiles {}, tile {}", t.name, tile.index);
            if !indices.insert(tile.index) {
                return compile_err(format!(
                    "`tiles {}`: tile {} is programmed twice",
                    t.name, tile.index
                ));
            }
            let program = crate::logic::tiles::program(tile)
                .map_err(|m| Error::Compile(format!("{ctx}: {m}")))?;
            for slug in program.targets() {
                match names.get(slug) {
                    Some(NameKind::Extern | NameKind::Block | NameKind::Signal) => {}
                    None if !flattened => {}
                    None => {
                        let hint = crate::logic::validate::suggest(
                            slug,
                            self.externs()
                                .map(|e| e.slug.as_str())
                                .chain(self.blocks().map(|b| b.slug.as_str())),
                        );
                        return compile_err(format!(
                            "{ctx}: `to: {slug}` names an undeclared slug{hint}"
                        ));
                    }
                    // Resolved by the expansion.
                    Some(NameKind::Instance) => {}
                    Some(kind) => {
                        return compile_err(format!(
                            "{ctx}: `to: {slug}` names {}, not a block",
                            kind.describe()
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// `device.tiles = set` claims a Touch Pure Flex's tiles.
    fn check_tiles_claim<'a>(
        &self,
        names: &Names,
        c: &'a TilesClaim,
        seen: &mut Seen<'a>,
    ) -> Result<()> {
        let tiles_claimed = &mut seen.tiles_claimed;
        match names.get(c.slug.as_str()) {
            Some(NameKind::Extern) => {
                let ty = self
                    .externs()
                    .find(|e| e.slug == c.slug)
                    .map_or("", |e| e.block_type.as_str());
                if ty != "TreeDevice" {
                    return compile_err(format!(
                        "`{c}`: tiles belong to a Touch Pure Flex, a \
                                     `TreeDevice`, but `{}` is declared as `{ty}`",
                        c.slug
                    ));
                }
            }
            Some(kind) => {
                return compile_err(format!(
                    "`{c}`: `{}` is {}. Tiles are claimed on an extern \
                                 Touch Pure Flex (`extern {} = TreeDevice(uuid: \"…\")`)",
                    c.slug,
                    kind.describe(),
                    c.slug
                ));
            }
            None => {
                return compile_err(format!(
                    "`{c}`: undeclared slug `{}`. Declare the device as \
                                 `extern {} = TreeDevice(uuid: \"…\")`",
                    c.slug, c.slug
                ));
            }
        }
        match names.get(c.tiles.as_str()) {
            Some(NameKind::Tiles) => {}
            Some(kind) => {
                return compile_err(format!(
                    "`{c}`: `{}` is {}, not a `tiles` set",
                    c.tiles,
                    kind.describe()
                ));
            }
            None => {
                let hint = crate::logic::validate::suggest(
                    &c.tiles,
                    self.tiles_sets().map(|t| t.name.as_str()),
                );
                return compile_err(format!(
                    "`{c}`: no `tiles {}` set is declared{hint}",
                    c.tiles
                ));
            }
        }
        if !tiles_claimed.insert(&c.slug) {
            return compile_err(format!(
                "duplicate assignment `{}.tiles = …`. A device takes one \
                             tiles set",
                c.slug
            ));
        }
        Ok(())
    }

    /// An `access`/`recipients` set: each principal once, with a schedule
    /// exactly where the right takes one.
    fn check_access(&self, a: &AccessDecl) -> Result<()> {
        let ctx = format!("`{} {}`", a.right.keyword(), a.name);
        let mut principals: BTreeSet<(PrincipalKind, &str)> = BTreeSet::new();
        for g in a.grants() {
            match (a.right, &g.schedule) {
                (RightKind::Access, Some(s)) if !g.principal.is_empty() && !s.is_empty() => {}
                (RightKind::Access, _) => {
                    return compile_err(format!(
                        "{ctx}: `{g}`. Name the {} and the access schedule by \
                                     their titles (`{} \"<title>\": \"<schedule>\"`)",
                        g.kind.as_str(),
                        g.kind.as_str()
                    ));
                }
                (RightKind::Recipients, None) if !g.principal.is_empty() => {}
                (RightKind::Recipients, None) => {
                    return compile_err(format!(
                        "{ctx}: `{g}`. Name the {} by its title",
                        g.kind.as_str()
                    ));
                }
                (RightKind::Recipients, Some(_)) => {
                    return compile_err(format!(
                        "{ctx}: `{g}`: a recipient takes no schedule. Write \
                                     `{} {}`",
                        g.kind.as_str(),
                        quote(&g.principal)
                    ));
                }
            }
            if !principals.insert((g.kind, &g.principal)) {
                return compile_err(format!(
                    "{ctx}: {} {} is listed twice",
                    g.kind.as_str(),
                    quote(&g.principal),
                ));
            }
        }
        Ok(())
    }

    /// `code_touch.access = set` / `notification.recipients = set`
    /// claims a block's access rights or recipients.
    fn check_access_claim<'a>(
        &self,
        sc: &Scope,
        c: &'a AccessClaim,
        seen: &mut Seen<'a>,
    ) -> Result<()> {
        let (names, flattened) = (&sc.names, sc.flattened);
        let (kw, ty_want, noun) = (
            c.right.keyword(),
            c.right.block_type(),
            c.right.block_noun(),
        );
        let rights = c.right.rights_noun();
        let ty = match names.get(c.slug.as_str()) {
            Some(NameKind::Extern) => self
                .externs()
                .find(|e| e.slug == c.slug)
                .map_or("", |e| e.block_type.as_str()),
            // A managed block works as well: an adopted Code Touch keeps
            // its rights in the source.
            Some(NameKind::Block) => self.block_type_of(&c.slug),
            None if !flattened => ty_want,
            Some(kind) => {
                return compile_err(format!(
                    "`{c}`: `{}` is {}. Claim {rights} on {noun} (`{ty_want}`), an extern or \
                     a managed block",
                    c.slug,
                    kind.describe()
                ));
            }
            None => {
                let hint = crate::logic::validate::suggest(
                    &c.slug,
                    self.externs().map(|e| e.slug.as_str()),
                );
                return compile_err(format!(
                    "`{c}`: undeclared slug `{}`{hint}. Declare the block as \
                     `extern {} = {ty_want}(…)`",
                    c.slug, c.slug
                ));
            }
        };
        if ty != ty_want {
            return compile_err(format!(
                "`{c}`: {rights} belong to {noun}, a `{ty_want}`, but `{}` is a `{ty}`",
                c.slug
            ));
        }
        match names.get(c.set.as_str()) {
            Some(NameKind::Rights(r)) if *r == c.right => {}
            Some(kind) => {
                return compile_err(format!(
                    "`{c}`: `{}` is {}, not {} set",
                    c.set,
                    kind.describe(),
                    NameKind::Rights(c.right)
                        .describe()
                        .trim_end_matches(" set")
                ));
            }
            None => {
                let hint = crate::logic::validate::suggest(
                    &c.set,
                    self.access_decls()
                        .filter(|a| a.right == c.right)
                        .map(|a| a.name.as_str()),
                );
                return compile_err(format!("`{c}`: no `{kw} {}` set is declared{hint}", c.set));
            }
        }
        if !seen.access_claimed.insert(&c.slug) {
            return compile_err(format!(
                "duplicate assignment `{}.{kw} = …`. A block takes one `{kw}` set",
                c.slug
            ));
        }
        Ok(())
    }

    /// A calendar entry's mode: `switches:` names one mode, the one way
    /// the module spells it. A built-in mode's raw number (`CalType: 10`)
    /// is refused with its day word; a user mode's (negative) number is
    /// left to the compile, which knows the config's numbers.
    fn check_calendar_entry(&self, sc: &Scope, b: &BlockDecl) -> Result<()> {
        use crate::logic::calendar::{CAL_TYPE, SWITCHES, builtin_value, number_refused};
        let ctx = format!("block `{}`", b.slug);
        for (k, v) in b.params() {
            match (k, v) {
                (CAL_TYPE, Value::Number(n)) => {
                    let Ok(number) = n.parse::<i64>() else {
                        return compile_err(format!(
                            "{ctx}: `{CAL_TYPE}: {n}` is no operating mode. Name the mode with \
                             `{SWITCHES}:`."
                        ));
                    };
                    if number < 0 {
                        continue;
                    }
                    let Some(word) = builtin_value(number) else {
                        return compile_err(format!(
                            "{ctx}: `{CAL_TYPE}: {n}` names no built-in operating mode. Name \
                             the mode with `{SWITCHES}:`: a day word (`heating`), the slug of a \
                             `Mode`, or a mode's quoted title."
                        ));
                    };
                    return compile_err(number_refused(&b.slug, n, &word));
                }
                (CAL_TYPE, _) => {
                    return compile_err(format!(
                        "{ctx}: a calendar entry names its operating mode with `{SWITCHES}:`, \
                         not `{CAL_TYPE}: {v}`."
                    ));
                }
                // The spelling that reads like the date kind, and the date
                // kind given a mode.
                ("mode", _) => {
                    return compile_err(format!(
                        "{ctx}: a calendar entry names its operating mode with `{SWITCHES}:`. \
                         Write `{SWITCHES}: {v}` instead of `mode: {v}`."
                    ));
                }
                ("Mode", Value::Ref(_) | Value::Str(_)) => {
                    return compile_err(format!(
                        "{ctx}: `Mode:` is the kind of date (a number), and `{SWITCHES}:` the \
                         operating mode. Write `{SWITCHES}: {v}` instead of `Mode: {v}`."
                    ));
                }
                (SWITCHES, _) => {}
                _ => continue,
            }
            let fail = |m: String| compile_err(format!("{ctx}: `{SWITCHES}: {v}` {m}"));
            match v {
                Value::Ref(w) if crate::logic::schedule::is_day(w) => {
                    if crate::logic::schedule::builtin_number(w).is_none() {
                        return fail(
                            "names several modes; a calendar entry switches one on (`mon`, \
                             `sat`, …)"
                                .into(),
                        );
                    }
                }
                Value::Ref(slug) => {
                    let ty = match sc.names.get(slug.as_str()) {
                        Some(NameKind::Extern) => self
                            .externs()
                            .find(|e| e.slug == *slug)
                            .map_or("…", |e| e.block_type.as_str()),
                        Some(NameKind::Block) => self.block_type_of(slug),
                        Some(kind) => {
                            return fail(format!(
                                "names {}, not an operating mode",
                                kind.describe()
                            ));
                        }
                        None => {
                            let modes = self
                                .externs()
                                .filter(|e| e.block_type == "Mode")
                                .map(|e| e.slug.as_str())
                                .chain(
                                    self.blocks()
                                        .filter(|b| b.block_type == "Mode")
                                        .map(|b| b.slug.as_str()),
                                );
                            let words: Vec<&str> = crate::logic::schedule::day_keywords()
                                .filter(|w| crate::logic::schedule::builtin_number(w).is_some())
                                .collect();
                            let hint = crate::logic::validate::suggest(
                                slug,
                                words.iter().copied().chain(modes),
                            );
                            return fail(format!(
                                "names no mode{hint}. Name it by a day word ({}), the slug of \
                                 a `Mode` the module declares, or another mode's quoted title \
                                 (`\"Urlaub\"`).",
                                words.join(", ")
                            ));
                        }
                    };
                    if ty != "Mode" && ty != "…" {
                        return fail(format!("names a `{ty}`, not a `Mode`"));
                    }
                }
                Value::Str(t) if t.trim().is_empty() => {
                    return fail("is empty: an operating mode's title cannot be".into());
                }
                Value::Str(_) => {}
                _ => {
                    return fail(
                        "names no operating mode. Name it by a day word (`heating`), the slug \
                         of a `Mode`, or a mode's quoted title."
                            .into(),
                    );
                }
            }
            if let Some(new) = sc.modes.in_calendar(v) {
                return compile_err(format!(
                    "{ctx}: a calendar entry names a built-in mode by its day word and a mode \
                     the module declares by its slug. Write `{SWITCHES}: {new}` instead of \
                     `{SWITCHES}: {v}`."
                ));
            }
        }
        crate::logic::calendar::check_date(self, b)
    }

    /// A schedule's days: a slug names a `Mode` of the module, and each
    /// mode is spelled the one way the module tells.
    fn check_schedule_days(&self, sc: &Scope, t: &ScheduleDecl) -> Result<()> {
        let ctx = format!("`schedule {}`", t.name);
        for line in t.lines() {
            for day in &line.days {
                let Day::Slug(slug) = day else { continue };
                let ty = match sc.names.get(slug.as_str()) {
                    Some(NameKind::Extern) => self
                        .externs()
                        .find(|e| e.slug == *slug)
                        .map_or("…", |e| e.block_type.as_str()),
                    Some(NameKind::Block) => self.block_type_of(slug),
                    None if !sc.flattened => continue,
                    None => {
                        let modes = self
                            .externs()
                            .filter(|e| e.block_type == "Mode")
                            .map(|e| e.slug.as_str())
                            .chain(
                                self.blocks()
                                    .filter(|b| b.block_type == "Mode")
                                    .map(|b| b.slug.as_str()),
                            );
                        return compile_err(format!(
                            "{ctx}: {}",
                            crate::logic::schedule::unknown_day(slug, modes)
                        ));
                    }
                    Some(kind) => {
                        return compile_err(format!(
                            "{ctx}: `{line}`: `{slug}` is {}, not an operating mode",
                            kind.describe()
                        ));
                    }
                };
                if ty != "Mode" && ty != "…" {
                    return compile_err(format!(
                        "{ctx}: `{line}`: `{slug}` is a `{ty}`, not a `Mode`"
                    ));
                }
            }
            let mut canonical = line.clone();
            for day in &mut canonical.days {
                if let Some(d) = sc.modes.in_schedule(day) {
                    *day = d;
                }
            }
            if canonical.days != line.days {
                let (mut new, mut old) = (canonical, line.clone());
                new.comment = None;
                old.comment = None;
                return compile_err(format!(
                    "{ctx}: a schedule names a built-in mode by its day word and a mode the \
                     module declares by its slug. Write `{new}` instead of `{old}`."
                ));
            }
        }
        Ok(())
    }

    /// `timer.schedule = set` claims a DayTimer's schedule.
    fn check_schedule_claim<'a>(
        &self,
        sc: &Scope,
        c: &'a ScheduleClaim,
        seen: &mut Seen<'a>,
    ) -> Result<()> {
        let (names, flattened) = (&sc.names, sc.flattened);
        let schedules_claimed = &mut seen.schedules_claimed;
        match names.get(c.slug.as_str()) {
            Some(NameKind::Extern) => {
                let ty = self
                    .externs()
                    .find(|e| e.slug == c.slug)
                    .map_or("", |e| e.block_type.as_str());
                if ty != "DayTimer" {
                    return compile_err(format!(
                        "`{c}`: a schedule belongs to a `DayTimer`, but `{}` is \
                                     declared as `{ty}`",
                        c.slug
                    ));
                }
            }
            Some(NameKind::Block) => {
                let ty = self.block_type_of(&c.slug);
                if ty != "DayTimer" {
                    return compile_err(format!(
                        "`{c}`: a schedule belongs to a `DayTimer`, but `{}` is \
                                     a `{ty}`",
                        c.slug
                    ));
                }
            }
            None if !flattened => {}
            Some(kind) => {
                return compile_err(format!(
                    "`{c}`: `{}` is {}. A schedule is claimed on a DayTimer \
                                 (an extern or a managed block)",
                    c.slug,
                    kind.describe()
                ));
            }
            None => {
                return compile_err(format!(
                    "`{c}`: undeclared slug `{}`. Declare the timer as \
                                 `extern {} = DayTimer(…)` or as a block",
                    c.slug, c.slug
                ));
            }
        }
        match names.get(c.schedule.as_str()) {
            Some(NameKind::Schedule) => {}
            Some(kind) => {
                return compile_err(format!(
                    "`{c}`: `{}` is {}, not a `schedule`",
                    c.schedule,
                    kind.describe()
                ));
            }
            None => {
                let hint = crate::logic::validate::suggest(
                    &c.schedule,
                    self.schedules().map(|t| t.name.as_str()),
                );
                return compile_err(format!(
                    "`{c}`: no `schedule {}` is declared{hint}",
                    c.schedule
                ));
            }
        }
        if !schedules_claimed.insert(&c.slug) {
            return compile_err(format!(
                "duplicate assignment `{}.schedule = …`. A timer takes one \
                             schedule",
                c.slug
            ));
        }
        Ok(())
    }

    /// `central.members = (a, b)` claims a central block's members: each
    /// one an extern or managed block of the type the central commands.
    fn check_members_claim<'a>(
        &self,
        sc: &Scope,
        c: &'a MembersClaim,
        seen: &mut Seen<'a>,
    ) -> Result<()> {
        let (names, flattened) = (&sc.names, sc.flattened);
        // The declared type of an extern or managed block; `None` for a
        // name a template body takes from its instance.
        let type_of = |slug: &str, what: &str| -> Result<Option<&str>> {
            match names.get(slug) {
                Some(NameKind::Extern) => Ok(Some(
                    self.externs()
                        .find(|e| e.slug == slug)
                        .map_or("", |e| e.block_type.as_str()),
                )),
                Some(NameKind::Block) => Ok(Some(self.block_type_of(slug))),
                None if !flattened => Ok(None),
                Some(kind) => compile_err(format!(
                    "`{c}`: `{slug}` is {}, not {what} (an extern or a managed block)",
                    kind.describe()
                )),
                None => compile_err(format!(
                    "`{c}`: undeclared slug `{slug}`. Declare it as an extern or a block"
                )),
            }
        };
        let member_type = match type_of(&c.slug, "a central block")? {
            Some(ty) => match central_member_type(ty) {
                Some(m) => Some(m),
                None => {
                    return compile_err(format!(
                        "`{c}`: members belong to a `CentralLight` or a `CentralShade`, \
                         but `{}` is a `{ty}`",
                        c.slug
                    ));
                }
            },
            None => None,
        };
        for m in &c.members {
            if *m == c.slug {
                return compile_err(format!("`{c}`: a central block is not its own member"));
            }
            if let (Some(want), Some(ty)) = (member_type, type_of(m, "a block")?)
                && ty != want
            {
                return compile_err(format!(
                    "`{c}`: the members of a {} are `{want}` blocks, but `{m}` is a `{ty}`",
                    self.type_of_central(&c.slug).unwrap_or("central block")
                ));
            }
        }
        if !seen.members_claimed.insert(&c.slug) {
            return compile_err(format!(
                "duplicate assignment `{}.members = …`. Write the whole list once",
                c.slug
            ));
        }
        Ok(())
    }

    /// The declared type of `slug`, extern or managed.
    fn type_of_central(&self, slug: &str) -> Option<&str> {
        self.externs()
            .find(|e| e.slug == slug)
            .map(|e| e.block_type.as_str())
            .or_else(|| {
                self.blocks()
                    .find(|b| b.slug == slug)
                    .map(|b| b.block_type.as_str())
            })
    }

    /// `controller.moods = set` claims a LightController2's moods.
    fn check_moods_claim<'a>(
        &self,
        sc: &Scope,
        c: &'a MoodsClaim,
        seen: &mut Seen<'a>,
    ) -> Result<()> {
        let (names, flattened) = (&sc.names, sc.flattened);
        let moods_claimed = &mut seen.moods_claimed;
        let ty = match names.get(c.slug.as_str()) {
            Some(NameKind::Extern) => self
                .externs()
                .find(|e| e.slug == c.slug)
                .map_or("", |e| e.block_type.as_str()),
            Some(NameKind::Block) => self.block_type_of(&c.slug),
            None if !flattened => "LightController2",
            Some(kind) => {
                return compile_err(format!(
                    "`{c}`: `{}` is {}. Moods are claimed on a \
                                 LightController2 (an extern or a managed block)",
                    c.slug,
                    kind.describe()
                ));
            }
            None => {
                return compile_err(format!(
                    "`{c}`: undeclared slug `{}`. Declare the controller as \
                                 `extern {} = LightController2(…)` or as a block",
                    c.slug, c.slug
                ));
            }
        };
        if ty != "LightController2" {
            return compile_err(format!(
                "`{c}`: moods belong to a `LightController2`, but `{}` is a `{ty}`",
                c.slug
            ));
        }
        match names.get(c.moods.as_str()) {
            Some(NameKind::Moods) => {}
            Some(kind) => {
                return compile_err(format!(
                    "`{c}`: `{}` is {}, not a `moods` set",
                    c.moods,
                    kind.describe()
                ));
            }
            None => {
                let hint = crate::logic::validate::suggest(
                    &c.moods,
                    self.mood_sets().map(|t| t.name.as_str()),
                );
                return compile_err(format!("`{c}`: no `moods {}` is declared{hint}", c.moods));
            }
        }
        if !moods_claimed.insert(&c.slug) {
            return compile_err(format!(
                "duplicate assignment `{}.moods = …`. A controller takes one \
                             mood set",
                c.slug
            ));
        }
        Ok(())
    }

    /// An Automatic Designer rule: its slug or name, then its
    /// statements.
    fn check_rule<'a>(&self, sc: &Scope, r: &'a RuleDecl, seen: &mut Seen<'a>) -> Result<()> {
        let names = &sc.names;
        let Seen {
            rules_declared,
            rule_titles,
            ..
        } = seen;
        let ctx = format!("`rule {}`", r.slug);
        match (&r.title, names.get(r.slug.as_str())) {
            // A titled rule names itself: its slug is
            // its own, like a managed block's.
            (Some(title), None) => {
                if title.trim().is_empty() {
                    return compile_err(format!(
                        "{ctx}: the rule's name is empty. The app lists rules \
                                     by name"
                    ));
                }
                if !rule_titles.insert(title.as_str()) {
                    return compile_err(format!(
                        "{ctx}: another rule is named {title:?}. The name is the \
                                     rule's identity"
                    ));
                }
            }
            (Some(_), Some(kind)) => {
                return compile_err(format!(
                    "{ctx}: `{}` is already {}, and a named rule (`rule {} \"…\"`) declares its own slug. \
                                 For an existing rule's extern, write `rule {}` without the name",
                    r.slug,
                    kind.describe(),
                    r.slug,
                    r.slug
                ));
            }
            (None, Some(NameKind::Extern)) => {
                let ty = self
                    .externs()
                    .find(|e| e.slug == r.slug)
                    .map_or("", |e| e.block_type.as_str());
                if ty != RULE_BLOCK_TYPE {
                    return compile_err(format!(
                        "{ctx}: a rule belongs to an Automatic Designer rule, a \
                                     `{RULE_BLOCK_TYPE}`, but `{}` is declared as `{ty}`",
                        r.slug
                    ));
                }
            }
            (None, Some(kind)) => {
                return compile_err(format!(
                    "{ctx}: `{}` is {}. A rule is declared on an extern \
                                 (`extern {} = {RULE_BLOCK_TYPE}(…)`)",
                    r.slug,
                    kind.describe(),
                    r.slug
                ));
            }
            (None, None) => {
                let hint = crate::logic::validate::suggest(
                    &r.slug,
                    self.externs().map(|e| e.slug.as_str()),
                );
                return compile_err(format!(
                    "{ctx}: undeclared slug `{}`{hint}. Declare the rule as \
                                 `extern {} = {RULE_BLOCK_TYPE}(title: \"…\")`, or name a \
                                 new one: `rule {} \"Name\"`",
                    r.slug, r.slug, r.slug
                ));
            }
        }
        if !rules_declared.insert(&r.slug) {
            return compile_err(format!(
                "{ctx} is declared twice. One declaration holds the whole rule"
            ));
        }
        self.check_rule_body(sc, r, &ctx)
    }

    /// A rule's statements: one `active` line, conditions in groups split
    /// by `or`, actions, and the objects they name.
    fn check_rule_body(&self, sc: &Scope, r: &RuleDecl, ctx: &str) -> Result<()> {
        let (names, flattened) = (&sc.names, sc.flattened);
        let (mut active, mut when, mut then) = (0, 0, 0);
        // A block a condition watches or a mode a line names:
        // declared, and an object (an extern or a managed one).
        let object = |slug: &str, line: &RuleStmtKind, what: &str| -> Result<()> {
            match names.get(slug) {
                Some(NameKind::Extern | NameKind::Block | NameKind::Signal) => Ok(()),
                None if !flattened => Ok(()),
                Some(kind) => compile_err(format!(
                    "{ctx}: `{line}`: `{slug}` is {}, but {what}",
                    kind.describe()
                )),
                None => compile_err(format!("{ctx}: `{line}`: undeclared slug `{slug}`")),
            }
        };
        for st in r.stmts() {
            match &st.kind {
                RuleStmtKind::Active(_) => active += 1,
                RuleStmtKind::Or => {
                    if when == 0 {
                        return compile_err(format!(
                            "{ctx}: `or` needs conditions before it. It starts \
                                         another group of conditions"
                        ));
                    }
                    when = 0;
                }
                RuleStmtKind::Time(_)
                | RuleStmtKind::Weather { .. }
                | RuleStmtKind::Day { .. }
                | RuleStmtKind::Daylight(_)
                | RuleStmtKind::Sun(_)
                | RuleStmtKind::Date(_) => when += 1,
                RuleStmtKind::Notify { .. }
                | RuleStmtKind::Mail { .. }
                | RuleStmtKind::Wait(_)
                | RuleStmtKind::Message { .. } => then += 1,
                RuleStmtKind::State { target, .. } => {
                    when += 1;
                    object(target, &st.kind, "a condition watches a block")?;
                }
                RuleStmtKind::Mode { mode, .. } | RuleStmtKind::ThenMode { mode, .. } => {
                    if matches!(st.kind, RuleStmtKind::Mode { .. }) {
                        when += 1;
                    } else {
                        then += 1;
                    }
                    if let ModeName::Slug(s) = mode {
                        object(s, &st.kind, "a mode is an operating mode object")?;
                    }
                    if let Some(slug) = sc.modes.in_rule(mode) {
                        let mut new = st.kind.clone();
                        if let RuleStmtKind::Mode { mode, .. }
                        | RuleStmtKind::ThenMode { mode, .. } = &mut new
                        {
                            *mode = slug;
                        }
                        return compile_err(format!(
                            "{ctx}: a rule names a mode the module declares by its slug. \
                             Write `{new}` instead of `{}`.",
                            st.kind
                        ));
                    }
                }
                RuleStmtKind::Weekday(days) => {
                    when += 1;
                    let mut seen = BTreeSet::new();
                    for d in days {
                        let Some(ix) = crate::logic::rule::weekdays(d) else {
                            return compile_err(format!(
                                "{ctx}: unknown weekday `{d}`. Days are mon, tue, \
                                             wed, thu, fri, sat, sun, weekdays, weekend, daily"
                            ));
                        };
                        if ix.iter().any(|i| !seen.insert(*i)) {
                            return compile_err(format!("{ctx}: `{}` names a day twice", st.kind));
                        }
                    }
                }
                RuleStmtKind::Then { target, command } => {
                    then += 1;
                    if command.is_empty() {
                        return compile_err(format!(
                            "{ctx}: `{}`. The command is the app's command text \
                                         (`on`, `off`, `pulse`, …)",
                            st.kind
                        ));
                    }
                    match names.get(target.as_str()) {
                        Some(NameKind::Extern | NameKind::Block | NameKind::Signal) => {}
                        None if !flattened => {}
                        Some(kind) => {
                            return compile_err(format!(
                                "{ctx}: `{}`: `{target}` is {}, but a rule sends its \
                                             command to a block (an extern or a managed one)",
                                st.kind,
                                kind.describe()
                            ));
                        }
                        None => {
                            return compile_err(format!(
                                "{ctx}: `{}`: undeclared slug `{target}`",
                                st.kind
                            ));
                        }
                    }
                }
            }
        }
        if active != 1 {
            return compile_err(format!(
                "{ctx}: say once whether the rule is `active` or `inactive` \
                             ({active} lines do)"
            ));
        }
        if when == 0 || then == 0 {
            return compile_err(format!(
                "{ctx}: a rule needs at least one `when` condition and one \
                             `then` action, and every `or` conditions after it"
            ));
        }
        Ok(())
    }

    /// Lifecycle statements must not contradict declarations or each
    /// other.
    fn check_lifecycle(&self, names: &Names) -> Result<()> {
        let mut removed_seen: BTreeMap<&str, ()> = BTreeMap::new();
        for r in self.removed() {
            if let Some(kind) = names.get(r.slug.as_str()) {
                return compile_err(format!(
                    "`removed {slug}` contradicts the declaration of {} `{slug}`. \
                     Delete the `removed` line or the declaration",
                    kind.describe(),
                    slug = r.slug,
                ));
            }
            if removed_seen.insert(&r.slug, ()).is_some() {
                return compile_err(format!("duplicate `removed {}`", r.slug));
            }
        }
        let mut moved_from: BTreeMap<&str, ()> = BTreeMap::new();
        let mut moved_to: BTreeMap<&str, ()> = BTreeMap::new();
        for m in self.moved() {
            if m.from == m.to {
                return compile_err(format!("`moved {0} -> {0}` moves a slug to itself", m.from));
            }
            if let Some(kind) = names.get(m.from.as_str()) {
                return compile_err(format!(
                    "`moved {from} -> {to}` conflicts with the declaration of {} \
                     `{from}`. The old slug must no longer be declared",
                    kind.describe(),
                    from = m.from,
                    to = m.to,
                ));
            }
            if matches!(
                names.get(m.to.as_str()),
                Some(NameKind::Extern | NameKind::Let)
            ) {
                return compile_err(format!(
                    "`moved {} -> {to}`: `{to}` is not a managed block",
                    m.from,
                    to = m.to,
                ));
            }
            if moved_from.insert(&m.from, ()).is_some() {
                return compile_err(format!("duplicate `moved` from `{}`", m.from));
            }
            if moved_to.insert(&m.to, ()).is_some() {
                return compile_err(format!("duplicate `moved` to `{}`", m.to));
            }
            if removed_seen.contains_key(m.from.as_str())
                || removed_seen.contains_key(m.to.as_str())
            {
                return compile_err(format!(
                    "`moved {} -> {}` conflicts with a `removed` of the same slug",
                    m.from, m.to
                ));
            }
        }
        // No chains: a move's target must not be another move's source.
        for m in self.moved() {
            if moved_from.contains_key(m.to.as_str()) {
                return compile_err(format!(
                    "chained `moved` through `{}`. Collapse into one `moved` statement",
                    m.to
                ));
            }
        }
        Ok(())
    }
}

/// A template definition: parameters and body slugs share one
/// template-local namespace; body references are resolved per
/// instantiation, not here.
fn check_template(t: &TemplateDecl) -> Result<()> {
    let mut local: BTreeSet<&str> = BTreeSet::new();
    for p in &t.params {
        if !local.insert(p.name()) {
            return compile_err(format!(
                "template `{}`: duplicate parameter `{}`",
                t.name,
                p.name()
            ));
        }
    }
    for item in &t.body {
        match item {
            Item::Block(b) => {
                if !local.insert(&b.slug) {
                    return compile_err(format!(
                        "template `{}`: duplicate name `{}` in the body",
                        t.name, b.slug
                    ));
                }
                let mut seen: BTreeSet<&str> = BTreeSet::new();
                for binding in b.bindings() {
                    if matches!(binding.kind, BindingKind::Param(_)) && !seen.insert(&binding.port)
                    {
                        return compile_err(format!(
                            "template `{}`: duplicate parameter `{}` in \
                                             `{} = {}(…)`",
                            t.name, binding.port, b.slug, b.block_type
                        ));
                    }
                }
            }
            Item::Signal(x) => {
                if !local.insert(&x.slug) {
                    return compile_err(format!(
                        "template `{}`: duplicate name `{}` in the body",
                        t.name, x.slug
                    ));
                }
            }
            Item::Wire(_)
            | Item::ExprWire(_)
            | Item::Set(_)
            | Item::MoodsClaim(_)
            | Item::MembersClaim(_)
            | Item::ScheduleClaim(_)
            | Item::TilesClaim(_)
            | Item::AccessClaim(_)
            | Item::Comment(_)
            | Item::Gap => {}
            // A body signal's label.
            Item::Attr(a)
                if a.attr == Attr::Title
                    && t.body
                        .iter()
                        .any(|i| matches!(i, Item::Signal(x) if x.slug == a.slug)) => {}
            _ => {
                return compile_err(format!(
                    "template `{}`: a body holds only block declarations, named signals and their titles, wires, port assignments and comments. Move attribute assignments like `x.room = …` to the top level.",
                    t.name
                ));
            }
        }
    }
    Ok(())
}

/// A `moods` set: named moods, each once, a whole-controller setting
/// only first.
fn check_moods(t: &MoodsDecl) -> Result<()> {
    let mut names: BTreeSet<&str> = BTreeSet::new();
    for m in t.moods() {
        let ctx = format!("`moods {}`, mood {}", t.name, quote(&m.name));
        if m.name.is_empty() {
            return compile_err(format!("`moods {}`: a mood needs a name", t.name));
        }
        if !names.insert(&m.name) {
            return compile_err(format!(
                "{ctx}: listed twice. A controller's mood names are unique"
            ));
        }
        let mut circuits: BTreeSet<Option<u8>> = BTreeSet::new();
        for (i, setting) in m.settings.iter().enumerate() {
            if setting.circuit.is_none() && i != 0 {
                return compile_err(format!(
                    "{ctx}: `{setting}` must come first. It sets every \
                                     circuit, and the settings after it name the exceptions"
                ));
            }
            if !circuits.insert(setting.circuit) {
                return compile_err(format!("{ctx}: `{setting}` sets a circuit twice"));
            }
        }
    }
    Ok(())
}
