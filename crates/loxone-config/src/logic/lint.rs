//! Advisory lints — findings a compile has no business rejecting but a
//! human wants to see: names that serve nothing, blocks whose outputs
//! feed nothing.
//!
//! Two layers. [`lint_source`] needs only the module: unused externs and
//! constants, uninstantiated templates, values outside a port's
//! documented range. [`lint_dead_outputs`] needs the
//! compiled result too, because a managed block's consumers may live
//! outside the source — GUI-drawn wires survive in the compiled config,
//! so only there is "nothing reads this block" a real statement. Both are
//! advisory: a finding can be deliberate (reference externs kept as
//! documentation, registers kept for later), which is why lint reports
//! and never blocks a compile. [`finish_lint`] applies the
//! `# lint: allow <kind>` comments, groups a device's unused inputs into
//! one finding and reports allows that cover nothing.

use super::ast::{
    ArgItem, Binding, BindingKind, Item, MatchSpec, Module, TestItem, Value, duration_value,
};
use crate::connectors::PortDir;
use crate::doc::{LoxoneDoc, ports};
use crate::error::Result;
use crate::lock::Lockfile;
use crate::port_meta::port_meta;
use crate::xml::Element;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintKind {
    UnusedExtern,
    UnusedLet,
    UnusedTemplate,
    DeadOutputs,
    /// A Touch Pure Flex command that names no target while several
    /// API-linked blocks answer to its abbreviation.
    TileBroadcast,
    /// A Touch Pure Flex command no API-linked block receives.
    TileDeadCommand,
    /// A user with an NFC tag or access code and no access right on any
    /// NFC Code Touch.
    NoAccessRight,
    /// A parameter value outside the port's documented range.
    OutOfRange,
    /// Page blocks above their file's first `page` statement in a
    /// directory module whose other files use `page`.
    DefaultPage,
    /// A `defaults` value no block takes: every block of the type binds
    /// the port itself, or the module has no block of the type.
    UnusedDefault,
    /// A bare number on a port documented in seconds or minutes that a
    /// unit suffix writes exactly (`Time: 300` is `5min`).
    UnitSuffix,
    /// A `# lint: allow` comment that covers no finding.
    UnusedAllow,
}

impl LintKind {
    pub fn label(self) -> &'static str {
        match self {
            LintKind::UnusedExtern => "unused-extern",
            LintKind::UnusedLet => "unused-let",
            LintKind::UnusedTemplate => "unused-template",
            LintKind::DeadOutputs => "dead-outputs",
            LintKind::TileBroadcast => "tile-broadcast",
            LintKind::TileDeadCommand => "tile-dead-command",
            LintKind::NoAccessRight => "no-access-right",
            LintKind::OutOfRange => "out-of-range",
            LintKind::DefaultPage => "default-page",
            LintKind::UnusedDefault => "unused-default",
            LintKind::UnitSuffix => "unit-suffix",
            LintKind::UnusedAllow => "unused-allow",
        }
    }

    /// Every kind.
    pub const ALL: [LintKind; 12] = [
        LintKind::UnusedExtern,
        LintKind::UnusedLet,
        LintKind::UnusedTemplate,
        LintKind::DeadOutputs,
        LintKind::TileBroadcast,
        LintKind::TileDeadCommand,
        LintKind::NoAccessRight,
        LintKind::OutOfRange,
        LintKind::DefaultPage,
        LintKind::UnusedDefault,
        LintKind::UnitSuffix,
        LintKind::UnusedAllow,
    ];

    /// The kind with this label (`dead-outputs`).
    pub fn from_label(label: &str) -> Option<LintKind> {
        LintKind::ALL.into_iter().find(|k| k.label() == label)
    }
}

#[derive(Debug, Clone)]
pub struct LintFinding {
    pub kind: LintKind,
    pub slug: String,
    pub detail: String,
}

/// Bare numbers on ports documented in seconds or minutes, rewritten in
/// `module` with their unit suffix: block and `defaults` parameters,
/// template bodies included. The spelling is decompile's: the largest
/// unit that writes the value as a whole number, and only when the
/// compile reproduces the number exactly (`300` → `5min`, `0.02` →
/// `20ms`). One finding per rewritten value. The compiled config does
/// not change.
pub fn fix_unit_suffixes(module: &mut Module) -> Vec<LintFinding> {
    fn fix(slug: &str, block_type: &str, args: &mut [ArgItem], out: &mut Vec<LintFinding>) {
        for arg in args {
            let ArgItem::Binding(Binding {
                port,
                kind: BindingKind::Param(value),
                ..
            }) = arg
            else {
                continue;
            };
            let Value::Number(n) = &*value else { continue };
            let Some(new) = duration_value(block_type, port, n) else {
                continue;
            };
            out.push(LintFinding {
                kind: LintKind::UnitSuffix,
                slug: slug.to_string(),
                detail: format!(
                    "`{port}: {n}` reads better with its unit. Write `{port}: {new}` instead of \
                     `{port}: {n}`."
                ),
            });
            *value = new;
        }
    }
    fn walk(items: &mut [Item], out: &mut Vec<LintFinding>) {
        for item in items {
            match item {
                Item::Block(b) => fix(&b.slug, &b.block_type, &mut b.args, out),
                Item::Defaults(d) => fix(&d.block_type, &d.block_type, &mut d.args, out),
                Item::Template(t) => walk(&mut t.body, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&mut module.items, &mut out);
    out
}

/// `defaults` values that reach no block: the blocks of the type, as
/// expanded without the defaults, all bind the port.
fn unused_defaults(module: &Module) -> Result<Vec<LintFinding>> {
    if !module.items.iter().any(|i| matches!(i, Item::Defaults(_))) {
        return Ok(Vec::new());
    }
    let bare = Module {
        items: module
            .items
            .iter()
            .filter(|i| !matches!(i, Item::Defaults(_)))
            .cloned()
            .collect(),
    }
    .expand()?;
    let mut out = Vec::new();
    for item in &module.items {
        let Item::Defaults(d) = item else { continue };
        // What each block of the type binds; a hardware declaration is
        // lowered, its settings and port values apart.
        let blocks: Vec<BTreeSet<&str>> = bare
            .blocks()
            .filter(|b| b.block_type == d.block_type)
            .map(|b| b.bindings().map(|x| x.port.as_str()).collect())
            .chain(
                bare.owned()
                    .filter(|o| o.block_type == d.block_type)
                    .map(|o| {
                        o.settings
                            .iter()
                            .map(|(k, _)| k.as_str())
                            .chain(
                                bare.sets()
                                    .filter(|s| s.target.slug == o.slug)
                                    .map(|s| s.target.port.as_str()),
                            )
                            .collect()
                    }),
            )
            .collect();
        for (port, v) in d.params() {
            if blocks.iter().any(|b| !b.contains(port)) {
                continue;
            }
            out.push(LintFinding {
                kind: LintKind::UnusedDefault,
                slug: d.block_type.clone(),
                detail: if blocks.is_empty() {
                    format!("`defaults {}(…)`: no block of the type", d.block_type)
                } else {
                    format!(
                        "`defaults {}({port}: {v})`: every block of the type binds `{port}` \
                         itself",
                        d.block_type
                    )
                },
            });
            if blocks.is_empty() {
                break;
            }
        }
    }
    Ok(out)
}

/// Source-only lints: declared names nothing references. Templates are
/// checked before expansion (an uninstantiated template disappears in the
/// expanded view); externs and constants after expansion and desugaring,
/// so template-body captures and expression operands count as uses.
pub fn lint_source(module: &Module) -> Result<Vec<LintFinding>> {
    let mut findings = Vec::new();

    let instantiated: BTreeSet<&str> = module
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Instance(call) => Some(call.block_type.as_str()),
            _ => None,
        })
        .collect();
    for item in &module.items {
        if let Item::Template(t) = item
            && !instantiated.contains(t.name.as_str())
        {
            findings.push(LintFinding {
                kind: LintKind::UnusedTemplate,
                slug: t.name.clone(),
                detail: "declared but never instantiated".into(),
            });
        }
    }

    findings.extend(unused_defaults(module)?);
    findings.extend(fix_unit_suffixes(&mut module.clone()));

    let (flat, _) = module.expand()?.desugar()?;
    let mut object_refs: BTreeSet<&str> = BTreeSet::new();
    let mut let_refs: BTreeSet<&str> = BTreeSet::new();
    for item in &flat.items {
        match item {
            Item::Wire(w) => {
                object_refs.insert(&w.from.slug);
                object_refs.insert(&w.to.slug);
            }
            Item::Block(b) => {
                for (_, src) in b.input_wires() {
                    object_refs.insert(&src.slug);
                }
                for (k, v) in b.params() {
                    if let Value::Ref(name) = v {
                        // `mirrors:` on a minted ref names an object;
                        // every other `Ref` value is a `let` reference.
                        if (k == "mirrors"
                            && matches!(
                                b.block_type.as_str(),
                                "InputRef" | "OutputRef" | "OutputRefLM"
                            ))
                            || crate::periphery::is_device_binding(&b.block_type, k)
                        {
                            object_refs.insert(name);
                        } else {
                            let_refs.insert(name);
                        }
                    }
                }
            }
            Item::Set(s) => {
                object_refs.insert(&s.target.slug);
                if let Value::Ref(name) = &s.value {
                    let_refs.insert(name);
                }
            }
            Item::Attr(a) => {
                object_refs.insert(&a.slug);
                if let Value::Ref(name) = &a.value {
                    let_refs.insert(name);
                }
            }
            Item::Extern(e) => {
                if let MatchSpec::Mirrors(target) = &e.match_spec {
                    object_refs.insert(target);
                }
            }
            // A tile target and a tiles claim use their slugs.
            Item::Tiles(t) => {
                for tile in t.tiles() {
                    for b in tile.bindings() {
                        if let BindingKind::Param(Value::Ref(slug)) = &b.kind {
                            object_refs.insert(slug);
                        }
                    }
                }
            }
            Item::TilesClaim(c) => {
                object_refs.insert(&c.slug);
            }
            // Access rights use the Code Touch they are on.
            Item::AccessClaim(c) => {
                object_refs.insert(&c.slug);
            }
            Item::ScheduleClaim(c) => {
                object_refs.insert(&c.slug);
            }
            Item::MoodsClaim(c) => {
                object_refs.insert(&c.slug);
            }
            // A rule uses its own block, its action targets, the blocks
            // its conditions watch and the modes it names.
            Item::Rule(r) => {
                object_refs.insert(&r.slug);
                object_refs.extend(r.slugs());
            }
            // A test driving or asserting a port is a use.
            Item::Test(t) => {
                for stmt in &t.body {
                    let (port, value) = match stmt {
                        TestItem::Inject(s) => (&s.target, &s.value),
                        TestItem::Expect(e) => (&e.port, &e.value),
                        _ => continue,
                    };
                    object_refs.insert(&port.slug);
                    if let Value::Ref(name) = value {
                        let_refs.insert(name);
                    }
                }
            }
            _ => {}
        }
    }

    for item in &flat.items {
        match item {
            // A hardware declaration owns what it states; it needs no use.
            Item::Extern(e)
                if !object_refs.contains(e.slug.as_str())
                    && !flat.owned().any(|o| o.slug == e.slug) =>
            {
                findings.push(LintFinding {
                    kind: LintKind::UnusedExtern,
                    slug: e.slug.clone(),
                    detail: format!("extern {} is never referenced", e.block_type),
                });
            }
            Item::Let(l) if !let_refs.contains(l.name.as_str()) => {
                findings.push(LintFinding {
                    kind: LintKind::UnusedLet,
                    slug: l.name.clone(),
                    detail: "constant is never referenced".into(),
                });
            }
            _ => {}
        }
    }

    // Documented ranges: advisory, because the documentation is the
    // weakest evidence tier — Loxone Config itself clamps or accepts.
    let extern_types: BTreeMap<&str, &str> = flat
        .externs()
        .map(|e| (e.slug.as_str(), e.block_type.as_str()))
        .collect();
    let mut values: Vec<(&str, &str, &str, &Value)> = Vec::new();
    for item in &flat.items {
        match item {
            Item::Block(b) => {
                for (k, v) in b.params() {
                    values.push((&b.slug, &b.block_type, k, v));
                }
            }
            Item::Set(s) => {
                if let Some(t) = extern_types.get(s.target.slug.as_str()) {
                    values.push((&s.target.slug, t, &s.target.port, &s.value));
                }
            }
            _ => {}
        }
    }
    for (slug, block_type, port, value) in values {
        let Some(meta) = port_meta(block_type, port) else {
            continue;
        };
        let Ok(v) = flat.resolve_value_for(value, block_type, slug, port) else {
            continue;
        };
        let Ok(x) = v.parse::<f64>() else {
            continue;
        };
        let below = meta
            .min
            .and_then(|m| m.parse::<f64>().ok())
            .filter(|m| x < *m);
        let above = meta
            .max
            .and_then(|m| m.parse::<f64>().ok())
            .filter(|m| x > *m);
        if below.is_some() || above.is_some() {
            let unit = meta.unit.map(|u| format!(" {u}")).unwrap_or_default();
            let range = format!("{} … {}", meta.min.unwrap_or("−∞"), meta.max.unwrap_or("∞"));
            findings.push(LintFinding {
                kind: LintKind::OutOfRange,
                slug: slug.to_string(),
                detail: format!("{port} = {v}{unit} is outside the documented range {range}{unit}"),
            });
        }
    }
    findings.extend(lint_default_page(module, &flat));
    Ok(findings)
}

/// A file of a directory module whose blocks come before its first `page`
/// statement, while another file names a page: those blocks go to the
/// default page (a `page` governs only its own file). Usually the file
/// lacks its `page` line. One finding per file, named by its first
/// declaration. `flat` is `module` expanded and desugared: both keep the
/// fragment boundaries, so their file segments align.
fn lint_default_page(module: &Module, flat: &Module) -> Vec<LintFinding> {
    fn segments(m: &Module) -> Vec<&[Item]> {
        m.items.split(|i| matches!(i, Item::FragmentEnd)).collect()
    }
    let (src, flat) = (segments(module), segments(flat));
    let paged = |seg: &[Item]| seg.iter().any(|i| matches!(i, Item::Page(_)));
    if src.len() < 2 || !src.iter().any(|s| paged(s)) || src.len() != flat.len() {
        return Vec::new();
    }
    let mut findings = Vec::new();
    // The page in force at the end of the files so far: what the blocks
    // above a file's first `page` would take if `page` carried over.
    let mut previous: Option<&str> = None;
    for (s, f) in src.iter().zip(&flat) {
        let before = previous;
        if let Some(t) = s.iter().rev().find_map(|i| match i {
            Item::Page(p) => Some(p.title.as_str()),
            _ => None,
        }) {
            previous = Some(t);
        }
        // Placed blocks (not object-tree items) before the first `page`.
        let unpaged: Vec<&str> = f
            .iter()
            .take_while(|i| !matches!(i, Item::Page(_)))
            .filter_map(|i| match i {
                Item::Block(b) if !crate::connectors::object_tree(&b.block_type) => {
                    Some(b.slug.as_str())
                }
                _ => None,
            })
            .collect();
        let Some(&first) = unpaged.first() else {
            continue;
        };
        let named = s
            .iter()
            .take_while(|i| !matches!(i, Item::Page(_)))
            .find_map(|i| match i {
                Item::Block(b) if !crate::connectors::object_tree(&b.block_type) => {
                    Some(b.slug.as_str())
                }
                Item::Instance(b)
                    if unpaged.iter().any(|u| {
                        u.strip_prefix(b.slug.as_str())
                            .is_some_and(|r| r.starts_with('_'))
                    }) =>
                {
                    Some(b.slug.as_str())
                }
                Item::Signal(x) => Some(x.slug.as_str()),
                _ => None,
            })
            .unwrap_or(first);
        let n = unpaged.len();
        let fix = match before {
            // The language server's quick fix inserts exactly this line.
            Some(t) => format!(
                "Start the file with `page {}` (the page of the file before it) or its own page.",
                Value::Str(t.to_string())
            ),
            None => "Start the file with its `page \"…\"` line.".to_string(),
        };
        findings.push(LintFinding {
            kind: LintKind::DefaultPage,
            slug: named.to_string(),
            detail: format!(
                "{n} block{} before any `page` statement in its file go{} to the default page, \
                 while other files name their page. {fix}",
                if n == 1 { "" } else { "s" },
                if n == 1 { "es" } else { "" },
            ),
        });
    }
    findings
}

/// Blocks that serve nothing, checked against the *compiled* config, so
/// wires drawn in Loxone Config count. A block is flagged when nothing
/// draws from any of its ports, or when everything reachable from them is
/// `InputRef`/`OutputRef` plumbing that itself feeds nothing (a mirror of
/// a dead signal is still dead). Not flagged, because something uses them
/// that a wire does not show:
///
/// - expression blocks (they feed their sink by construction), central
///   blocks (`rec=`), `Code16` programs, and family parents (Modbus server
///   and device, virtual HTTP/UDP input, virtual output);
/// - operating modes and calendar entries: schedules and the Automatic
///   Designer read them by mode number;
/// - a value the app shows (`<IoData Visu="true">` with
///   `<Display StateOnly="true">`, an info-only control) or a block that
///   records a statistic (`StatsType=`). A switch or a controller in the
///   app is still flagged: it has to drive something;
/// - a block an energy flow monitor reads (`<nd ctrlUuid=>`);
/// - a block that logs, mails or notifies (a `<LoggerMailer>` child), and
///   whatever feeds one.
///
/// A periphery output (`ModbusAActor`, `VirtualOutCmd`) has no outputs to
/// draw from: it is flagged when nothing drives it.
pub fn lint_dead_outputs(
    module: &Module,
    lock: &Lockfile,
    compiled: &LoxoneDoc,
) -> Result<Vec<LintFinding>> {
    let (flat, _) = module.expand()?.desugar()?;

    let objects = compiled.objects();
    let mut port_owner: BTreeMap<String, usize> = BTreeMap::new();
    // Refs that only pass a signal on; a ref that sends a message acts.
    let mut plumbing = vec![false; objects.len()];
    let mut messages = vec![false; objects.len()];
    // Objects an energy flow monitor reads: its `<nd ctrlUuid=>` nodes.
    let mut efm_reads: BTreeSet<String> = BTreeSet::new();
    for (i, obj) in objects.iter().enumerate() {
        let el = compiled.element_at(&obj.path).expect("path from objects()");
        messages[i] = el.child_elements().any(|c| c.name == "LoggerMailer");
        plumbing[i] = matches!(
            obj.block_type.as_str(),
            "InputRef" | "OutputRef" | "OutputRefLM"
        ) && !messages[i];
        if obj.block_type == "EFM" {
            collect_efm_nodes(el, &mut efm_reads);
        }
        for port in ports(el) {
            port_owner.insert(port.uuid.clone(), i);
        }
    }
    // feeds: source object -> sink objects (through wires).
    let mut feeds: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    let mut fed: BTreeSet<usize> = BTreeSet::new();
    for wire in compiled.wires() {
        if let (Some(&from), Some(&to)) = (
            port_owner.get(&wire.from_port),
            port_owner.get(&wire.to_port),
        ) {
            feeds.entry(from).or_default().insert(to);
            fed.insert(to);
        }
    }
    let uuid_index: BTreeMap<&str, usize> = objects
        .iter()
        .enumerate()
        .map(|(i, o)| (o.uuid.as_str(), i))
        .collect();

    let mut findings = Vec::new();
    for item in &flat.items {
        let Item::Block(b) = item else { continue };
        let Some(entry) = lock.objects.get(&b.slug) else {
            continue; // never compiled — nothing to judge yet
        };
        if entry.expr_owned {
            continue;
        }
        let Some(&start) = uuid_index.get(entry.uuid.as_str()) else {
            continue;
        };
        let el = compiled
            .element_at(&objects[start].path)
            .expect("path from objects()");
        let block_type = objects[start].block_type.as_str();
        // Side-channel consumers wires cannot see: central blocks command
        // their targets through the `rec=` uuid list, and a `Code16`
        // program acts through its code (network calls, app interaction).
        // Parents of a family (the Modbus server and device, the virtual
        // HTTP/UDP input and output) have no ports of their own: their children
        // carry the signals. Modes and calendar entries take effect by
        // their mode number.
        if el.attr("rec").is_some_and(|v| !v.is_empty())
            || messages[start]
            || efm_reads.contains(entry.uuid.as_str())
            || matches!(
                block_type,
                "Code16"
                    | "ModbusServer"
                    | "ModbusDev"
                    | "VirtualHttpIn"
                    | "VirtualUdpIn"
                    | "VirtualOut"
                    | "Mode"
                    | "CalendarEntry"
            )
        {
            continue;
        }
        // Read in the app: a value it shows (an info-only control, its
        // `<Display StateOnly="true">`) or a statistic it records. A switch
        // or a controller in the app still has to drive something.
        let shown = el
            .child_elements()
            .any(|c| c.name == "IoData" && c.attr("Visu") == Some("true"));
        let info_only = el
            .child_elements()
            .any(|c| c.name == "Display" && c.attr("StateOnly") == Some("true"));
        let statistic = el.attr("StatsType").is_some_and(|v| !v.is_empty());
        if statistic || (shown && info_only) {
            continue;
        }
        // A periphery output is driven, not read.
        if let Some(p) = crate::periphery::periphery(block_type)
            && !p.ports.iter().any(|s| s.dir == PortDir::Output)
        {
            if !fed.contains(&start) {
                findings.push(LintFinding {
                    kind: LintKind::DeadOutputs,
                    slug: b.slug.clone(),
                    detail: "an output nothing drives: no wire into it (compiled config, GUI \
                             wires included)"
                        .into(),
                });
            }
            continue;
        }
        // Forward reachability; dead when it never leaves ref plumbing.
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut queue: VecDeque<usize> = feeds
            .get(&start)
            .map(|next| next.iter().copied().collect())
            .unwrap_or_default();
        let mut alive = false;
        while let Some(i) = queue.pop_front() {
            if !seen.insert(i) {
                continue;
            }
            if !plumbing[i] {
                alive = true;
                break;
            }
            if let Some(next) = feeds.get(&i) {
                queue.extend(next.iter().copied());
            }
        }
        if alive {
            continue;
        }
        let mut detail = if seen.is_empty() {
            "no wire draws from any of its ports (compiled config, GUI wires included)".to_string()
        } else {
            "feeds only ref plumbing that itself feeds nothing".to_string()
        };
        if shown {
            detail.push_str(". It is in the app, but its outputs reach nothing");
        }
        findings.push(LintFinding {
            kind: LintKind::DeadOutputs,
            slug: b.slug.clone(),
            detail,
        });
    }
    Ok(findings)
}

/// The `ctrlUuid=` of every `<nd>` node below an energy flow monitor.
fn collect_efm_nodes(el: &Element, out: &mut BTreeSet<String>) {
    for c in el.child_elements() {
        if c.name == "nd"
            && let Some(u) = c.attr("ctrlUuid")
        {
            out.insert(u.to_string());
        }
        if c.name == "root" || c.name == "nd" {
            collect_efm_nodes(c, out);
        }
    }
}

/// A `# lint: allow <kind>…` comment line and the declaration below it.
#[derive(Debug)]
struct Allow {
    /// The comment as written, without the `#`.
    text: String,
    /// The declaration's name; `None` when no declaration follows.
    name: Option<String>,
    /// The finding slugs it covers: the name, or for a statement in a
    /// template body that name in every instance. `true` also covers
    /// `<slug>_…`, the blocks of a template instance.
    targets: Vec<(String, bool)>,
    kinds: Vec<LintKind>,
    used: bool,
}

impl Allow {
    fn covers(&self, slug: &str) -> bool {
        self.targets.iter().any(|(t, prefix)| {
            slug == t
                || (*prefix
                    && slug
                        .strip_prefix(t.as_str())
                        .is_some_and(|r| r.starts_with('_')))
        })
    }
}

/// The lint kinds a `# lint: allow …` comment names, `None` when the
/// comment is not one. Kinds are separated by spaces or commas; the first
/// word that is not a kind starts the free-text reason.
fn parse_allow(comment: &str) -> Option<Vec<LintKind>> {
    let rest = comment.trim().strip_prefix("lint:")?.trim_start();
    let rest = rest.strip_prefix("allow")?;
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    Some(
        rest.split([' ', '\t', ','])
            .filter(|w| !w.is_empty())
            .map_while(LintKind::from_label)
            .collect(),
    )
}

/// Whether a whole-line comment (text after the `#`) is a `# lint: allow`
/// comment; `loxl fmt` keeps one right above its statement.
pub(crate) fn is_allow_comment(comment: &str) -> bool {
    parse_allow(comment).is_some()
}

/// The slugs an allow covers, each with whether it also covers `slug_*`.
type Targets = Vec<(String, bool)>;

/// The `# lint: allow` comments of a module, each with the declaration
/// right below it (other comment lines may come between).
fn collect_allows(module: &Module) -> Vec<Allow> {
    let mut instances: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for item in &module.items {
        if let Item::Instance(call) = item {
            instances
                .entry(call.block_type.as_str())
                .or_default()
                .push(call.slug.as_str());
        }
    }
    fn walk(items: &[Item], scope: &dyn Fn(&str, bool) -> Targets, out: &mut Vec<Allow>) {
        let mut pending: Vec<Allow> = Vec::new();
        for item in items {
            let decl = match item {
                Item::Comment(c) => {
                    if let Some(kinds) = parse_allow(c) {
                        pending.push(Allow {
                            text: c.trim().to_string(),
                            name: None,
                            targets: Vec::new(),
                            kinds,
                            used: false,
                        });
                    }
                    continue;
                }
                Item::Extern(e) => Some((e.slug.as_str(), false)),
                Item::Block(b) => Some((b.slug.as_str(), false)),
                Item::Instance(b) => Some((b.slug.as_str(), true)),
                Item::Signal(s) => Some((s.slug.as_str(), false)),
                Item::Let(l) => Some((l.name.as_str(), false)),
                Item::Template(t) => Some((t.name.as_str(), false)),
                Item::Defaults(d) => Some((d.block_type.as_str(), false)),
                _ => None,
            };
            for mut a in pending.drain(..) {
                if let Some((name, prefix)) = decl {
                    a.name = Some(name.to_string());
                    a.targets = scope(name, prefix);
                }
                out.push(a);
            }
        }
        out.append(&mut pending);
    }
    let mut out = Vec::new();
    walk(
        &module.items,
        &|name, prefix| vec![(name.to_string(), prefix)],
        &mut out,
    );
    for item in &module.items {
        if let Item::Template(t) = item {
            let of = instances.get(t.name.as_str()).cloned().unwrap_or_default();
            let mut body = Vec::new();
            walk(
                &t.body,
                &|name, prefix| of.iter().map(|i| (format!("{i}_{name}"), prefix)).collect(),
                &mut body,
            );
            // A template no one instantiates has no findings to cover.
            out.extend(body.into_iter().filter(|_| !of.is_empty()));
        }
    }
    out
}

/// Lint kinds only a project (a compiled config) can report.
fn needs_project(kind: LintKind) -> bool {
    matches!(
        kind,
        LintKind::DeadOutputs
            | LintKind::TileBroadcast
            | LintKind::TileDeadCommand
            | LintKind::NoAccessRight
    )
}

/// The last step of a lint run over `module`: drops the findings a
/// `# lint: allow <kind>` comment above their declaration covers, reports
/// the allow comments that cover nothing (`unused-allow`), and merges the
/// `dead-outputs` findings of a device's inputs into one finding on the
/// device. `project` says whether the compiled-config lints ran: without
/// them, an allow of their kinds is not judged.
pub fn finish_lint(
    module: &Module,
    findings: Vec<LintFinding>,
    project: bool,
) -> Result<Vec<LintFinding>> {
    let mut allows = collect_allows(module);
    // The device each periphery input hangs below, and how many inputs
    // each device has.
    let (flat, _) = module.expand()?.desugar()?;
    let mut device_of: BTreeMap<&str, &str> = BTreeMap::new();
    let mut inputs: BTreeMap<&str, usize> = BTreeMap::new();
    for b in flat.blocks() {
        let Some(p) = crate::periphery::periphery(&b.block_type) else {
            continue;
        };
        if !p.ports.iter().any(|s| s.dir == PortDir::Output) {
            continue;
        }
        if let Some((_, Value::Ref(dev))) = b
            .params()
            .find(|(k, _)| crate::periphery::is_device_binding(&b.block_type, k))
        {
            device_of.insert(b.slug.as_str(), dev.as_str());
            *inputs.entry(dev.as_str()).or_default() += 1;
        }
    }

    let mut kept = Vec::new();
    for f in findings {
        let device = (f.kind == LintKind::DeadOutputs)
            .then(|| device_of.get(f.slug.as_str()).copied())
            .flatten();
        let mut allowed = false;
        for a in allows.iter_mut().filter(|a| a.kinds.contains(&f.kind)) {
            if a.covers(&f.slug) || device.is_some_and(|d| a.covers(d)) {
                a.used = true;
                allowed = true;
            }
        }
        if !allowed {
            kept.push(f);
        }
    }

    // A device's dead inputs, two or more, as one finding on the device.
    let mut dead: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for f in &kept {
        if f.kind == LintKind::DeadOutputs
            && let Some(d) = device_of.get(f.slug.as_str())
        {
            dead.entry(d).or_default().push(f.slug.clone());
        }
    }
    let sizes: BTreeMap<&str, usize> = dead.iter().map(|(d, n)| (*d, n.len())).collect();
    let mut out = Vec::new();
    for f in kept {
        let device = (f.kind == LintKind::DeadOutputs)
            .then(|| device_of.get(f.slug.as_str()).copied())
            .flatten();
        let Some(d) = device.filter(|d| sizes[d] >= 2) else {
            out.push(f);
            continue;
        };
        let Some(names) = dead.remove(d) else {
            continue; // reported with the device's first input
        };
        const SHOWN: usize = 8;
        let mut list: Vec<String> = names.iter().take(SHOWN).map(|n| format!("`{n}`")).collect();
        if names.len() > SHOWN {
            list.push(format!("and {} more", names.len() - SHOWN));
        }
        out.push(LintFinding {
            kind: LintKind::DeadOutputs,
            slug: d.to_string(),
            detail: format!(
                "{} of its {} inputs feed nothing (compiled config, GUI wires included): {}",
                names.len(),
                inputs[d],
                list.join(", ")
            ),
        });
    }

    for a in allows.into_iter().filter(|a| !a.used) {
        if !project && a.kinds.iter().any(|k| needs_project(*k)) {
            continue;
        }
        let detail = if a.kinds.is_empty() {
            format!(
                "`# {}` names no lint kind. Name one after `allow`, e.g. `# lint: allow \
                 dead-outputs`.",
                a.text
            )
        } else if a.name.is_none() {
            format!(
                "`# {}` stands above no declaration. Put it right above the block, extern or \
                 instance it is for.",
                a.text
            )
        } else {
            format!(
                "`# {}` covers no finding. Remove it, or check that it names the right kind.",
                a.text
            )
        };
        out.push(LintFinding {
            kind: LintKind::UnusedAllow,
            slug: a.name.unwrap_or_default(),
            detail,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_lints_find_unused_names() {
        let src = "\
extern used = VirtualIn(iname: \"VI1\")
extern lonely = VirtualIn(iname: \"VI2\")
extern sink = VirtualIn(iname: \"VI3\")

let schwelle = 28
let verwaist = 5

template nie_benutzt(x: VirtualIn)
\tg = And(
\t\tI1: x.Q,
\t)
end

gate = GreaterEqual(
\tInput1: used.Q,
\tInput2: schwelle,
)

sink.I <- gate.Q
";
        let m = Module::parse(src).unwrap();
        let findings = lint_source(&m).unwrap();
        let got: Vec<(&str, &str)> = findings
            .iter()
            .map(|f| (f.kind.label(), f.slug.as_str()))
            .collect();
        assert!(got.contains(&("unused-extern", "lonely")), "{got:?}");
        assert!(got.contains(&("unused-let", "verwaist")), "{got:?}");
        assert!(got.contains(&("unused-template", "nie_benutzt")), "{got:?}");
        assert!(!got.iter().any(|(_, s)| *s == "used"), "{got:?}");
        assert!(!got.iter().any(|(_, s)| *s == "schwelle"), "{got:?}");
        assert!(!got.iter().any(|(_, s)| *s == "sink"), "{got:?}");
    }

    #[test]
    fn allow_comments_name_kinds_then_a_reason() {
        use LintKind::*;
        assert_eq!(
            parse_allow(" lint: allow dead-outputs"),
            Some(vec![DeadOutputs])
        );
        assert_eq!(
            parse_allow(" lint: allow dead-outputs, unused-extern for the wall tablet"),
            Some(vec![DeadOutputs, UnusedExtern])
        );
        // A misspelt kind ends the list: it is reason text.
        assert_eq!(parse_allow(" lint: allow dead-output"), Some(vec![]));
        assert_eq!(parse_allow(" lint: allow"), Some(vec![]));
        assert_eq!(parse_allow(" lint: allowed"), None);
        assert_eq!(parse_allow(" Lüftung: allow dead-outputs"), None);
        assert_eq!(parse_allow(" see lint: allow dead-outputs"), None);
        for k in LintKind::ALL {
            assert_eq!(LintKind::from_label(k.label()), Some(k));
        }
    }

    #[test]
    fn mirrors_target_counts_as_use() {
        let src = "\
extern status = VirtualIn(iname: \"VI1\")
extern status_ref = InputRef(mirrors: status)
extern sink = VirtualIn(iname: \"VI2\")

status_ref.AI <- status.AQ
sink.I <- status_ref.Q
";
        let m = Module::parse(src).unwrap();
        let findings = lint_source(&m).unwrap();
        assert!(findings.is_empty(), "{findings:?}");
    }
}
