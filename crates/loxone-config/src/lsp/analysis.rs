//! What the language server knows about a module, independent of the
//! protocol: diagnostics, declarations, hover text and completions.
//! Positions are (line, byte column); the server converts them.
//!
//! The AST carries no source positions, so declarations are found
//! lexically — the language is line-oriented (language.md), and a
//! declaration always opens its line. An error message names what it is
//! about in backticks; the first name that is declared somewhere places
//! the diagnostic.

use crate::connectors::{BUILTIN_TYPES, PortDir, builtin};
use crate::doc::{LoxoneDoc, ports};
use crate::logic::{
    CompileOptions, LintFinding, Module, compile, lint_access, lint_dead_outputs, lint_source,
    lint_tiles, resolve_extern_uuids, validate_ports,
};
use crate::port_meta::port_meta;
use crate::{Error, Lockfile, Project};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub severity: Severity,
    pub message: String,
}

/// A place in a file: line and byte range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub file: PathBuf,
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

/// A name a statement declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decl {
    pub slug: String,
    /// `extern`, `block`, `instance`, `let`, `template`, `schedule`,
    /// `access`, `tiles`, `rule` or `moods`
    pub kind: &'static str,
    /// The block type (`extern`/`block`) or template (`instance`)
    pub ty: Option<String>,
    pub span: Span,
}

/// The files that make one module, and the project that compiles it.
#[derive(Debug, Clone)]
pub struct Unit {
    pub files: Vec<PathBuf>,
    /// The module is a directory of fragments
    pub fragments: bool,
    pub project: Option<Project>,
}

/// A file a rename rewrites: its text now and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdit {
    pub file: PathBuf,
    pub old: String,
    pub new: String,
}

/// The `X` of a diagnostic's "did you mean `X`?", when it is a name.
pub fn suggestion(message: &str) -> Option<&str> {
    let rest = &message[message.find("did you mean `")? + "did you mean `".len()..];
    let fix = &rest[..rest.find('`')?];
    is_name(fix).then_some(fix)
}

/// The `(new, old)` of a diagnostic's "write `NEW` instead of `OLD`" —
/// the exact replacement a v1 spelling names.
pub fn replacement(message: &str) -> Option<(&str, &str)> {
    let at = message
        .find("Write `")
        .or_else(|| message.find("write `"))?;
    let rest = &message[at + "write `".len()..];
    let (new, rest) = rest.split_once("` instead of `")?;
    let old = &rest[..rest.find('`')?];
    (!old.is_empty()).then_some((new, old))
}

/// `s` is one identifier.
pub fn is_name(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && c.all(|c| c.is_alphanumeric() || c == '_')
}

/// The declaration kinds rename and references take.
const RENAMEABLE: &[&str] = &["extern", "block", "instance", "let", "template"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    /// `port`, `type`, `template`, or a [`Decl::kind`]
    pub kind: &'static str,
    pub detail: Option<String>,
}

type BaseCache = Option<(PathBuf, SystemTime, Rc<LoxoneDoc>)>;

/// The open documents (their unsaved text wins over the disk) and what
/// the last analysis resolved.
#[derive(Default)]
pub struct Workspace {
    open: BTreeMap<PathBuf, String>,
    base: RefCell<BaseCache>,
    /// Per module path: the extern UUIDs of the last module that resolved,
    /// so hover still knows them while the text does not parse.
    resolved: RefCell<BTreeMap<PathBuf, BTreeMap<String, String>>>,
}

impl Workspace {
    pub fn open(&mut self, path: PathBuf, text: String) {
        self.open.insert(path, text);
    }

    pub fn close(&mut self, path: &Path) {
        self.open.remove(path);
    }

    /// The text of `path`: the editor's if open, else the disk's.
    pub fn text(&self, path: &Path) -> Option<String> {
        match self.open.get(path) {
            Some(t) => Some(t.clone()),
            None => std::fs::read_to_string(path).ok(),
        }
    }

    /// The module `path` belongs to: the module of the nearest `lox.toml`
    /// above it when that module is or contains the file, else the file
    /// on its own.
    pub fn unit(&self, path: &Path) -> Unit {
        let alone = Unit {
            files: vec![path.to_path_buf()],
            fragments: false,
            project: None,
        };
        let Some(file) = path.ancestors().skip(1).find_map(Project::find) else {
            return alone;
        };
        let Ok(project) = Project::load(&file) else {
            return alone;
        };
        let module = &project.module;
        if module.is_dir() {
            if !same_or_inside(path, module) {
                return alone;
            }
            let mut files = crate::cli::module_dir_files(module).unwrap_or_default();
            // A new file the editor has not saved yet.
            if !files.iter().any(|f| same_file(f, path)) {
                files.push(path.to_path_buf());
                files.sort();
            }
            // Open documents under their editor path, so diagnostics land
            // on the URI the editor knows.
            for f in &mut files {
                if let Some(p) = self.open.keys().find(|p| same_file(p, f)) {
                    *f = p.clone();
                }
            }
            Unit {
                files,
                fragments: true,
                project: Some(project),
            }
        } else if same_file(module, path) {
            Unit {
                files: vec![path.to_path_buf()],
                fragments: false,
                project: Some(project),
            }
        } else {
            alone
        }
    }

    /// Every declaration of the unit, files in order.
    pub fn decls(&self, unit: &Unit) -> Vec<Decl> {
        unit.files
            .iter()
            .flat_map(|f| {
                let text = self.text(f).unwrap_or_default();
                scan(&text, f)
            })
            .collect()
    }

    /// The diagnostics of every file of `unit` (an empty list clears a
    /// file). `deep` also compiles against the project's base config —
    /// extern resolution, the lockfile, dead outputs — without writing
    /// anything.
    pub fn diagnostics(&self, unit: &Unit, deep: bool) -> BTreeMap<PathBuf, Vec<Diagnostic>> {
        let mut out: BTreeMap<PathBuf, Vec<Diagnostic>> =
            unit.files.iter().map(|f| (f.clone(), Vec::new())).collect();
        let decls = self.decls(unit);
        let report = |e: &Error, file: &Path, out: &mut BTreeMap<PathBuf, Vec<Diagnostic>>| {
            let (span, message) = match e {
                Error::Parse { line, msg } => {
                    let n = line.saturating_sub(1);
                    let text = self.text(file).unwrap_or_default();
                    let len = super::text::line(&text, n).len();
                    (span(file, n, 0, len), msg.clone())
                }
                _ => (
                    self.locate(&e.to_string(), &decls, unit, file),
                    message_of(e),
                ),
            };
            push(out, span, Severity::Error, message);
        };

        let mut items = Vec::new();
        let mut failed = false;
        for f in &unit.files {
            let text = self.text(f).unwrap_or_default();
            match Module::parse_fragment(&text).and_then(|mut m| {
                m.load_files(f.parent().unwrap_or(Path::new(".")))?;
                Ok(m)
            }) {
                Ok(m) => items.extend(m.items),
                Err(e) => {
                    report(&e, f, &mut out);
                    failed = true;
                }
            }
        }
        if failed {
            return out;
        }
        let module = Module { items };
        let first = unit.files[0].clone();
        let checked = module
            .expand()
            .and_then(|x| x.validate().map(|()| x))
            .and_then(|x| x.desugar())
            .and_then(|(x, _)| x.validate().map(|()| x))
            .and_then(|x| validate_ports(&x));
        if let Err(e) = checked {
            report(&e, &first, &mut out);
            return out;
        }
        let mut findings = lint_source(&module).unwrap_or_default();

        if let Some(project) = &unit.project
            && let Some(base) = self.base_doc(&project.base)
        {
            let lock = if project.lock.exists() {
                Lockfile::load(&project.lock).ok()
            } else {
                Some(Lockfile::new())
            };
            if let Some(lock) = lock {
                if let Ok(r) = resolve_extern_uuids(&base, &module, &lock) {
                    self.resolved.borrow_mut().insert(project.module.clone(), r);
                }
                if deep {
                    let opts = CompileOptions {
                        machine: project
                            .serial
                            .clone()
                            .or_else(|| lock.target.miniserver_serial.clone())
                            .and_then(|s| crate::uuid::parse_serial(&s).ok())
                            .unwrap_or_default(),
                        mint_time_unix: 0,
                        page_title: project.page.clone(),
                        allow_removals: false,
                        accept_version: None,
                        relayout: Vec::new(),
                    };
                    match compile(&base, &module, &mut lock.clone(), &opts) {
                        Ok(compiled) => {
                            findings.extend(
                                lint_dead_outputs(&module, &lock, &compiled).unwrap_or_default(),
                            );
                            findings.extend(lint_tiles(&module, &compiled));
                            findings.extend(lint_access(&compiled));
                        }
                        Err(e) => report(&e, &first, &mut out),
                    }
                }
            }
        }
        for f in findings {
            let span = self.lint_span(&f, &decls, unit, &first);
            push(
                &mut out,
                span,
                Severity::Warning,
                format!("{}: {}", f.kind.label(), f.detail),
            );
        }
        out
    }

    /// The base config of a project, parsed once per modification time.
    fn base_doc(&self, path: &Path) -> Option<Rc<LoxoneDoc>> {
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
        if let Some((p, t, doc)) = &*self.base.borrow()
            && p == path
            && *t == mtime
        {
            return Some(doc.clone());
        }
        let doc = Rc::new(LoxoneDoc::parse(&std::fs::read(path).ok()?).ok()?);
        *self.base.borrow_mut() = Some((path.to_path_buf(), mtime, doc.clone()));
        Some(doc)
    }

    /// Where an error message points. Of the names in backticks: a use
    /// `slug.port` of two of them (`extern \`w\` has no port \`Qx\``),
    /// else a name inside the declaration of another (an argument key),
    /// else the declaration of the first declared one, else the first use
    /// of one, else the top of `fallback`.
    fn locate(&self, message: &str, decls: &[Decl], unit: &Unit, fallback: &Path) -> Span {
        let names: Vec<&str> = message
            .split('`')
            .skip(1)
            .step_by(2)
            .map(|n| n.split('.').next().unwrap_or(n))
            .collect();
        let declared: Vec<&Decl> = names
            .iter()
            .filter_map(|n| decls.iter().find(|d| d.slug == *n))
            .collect();
        for d in &declared {
            for other in names.iter().filter(|n| **n != d.slug) {
                if let Some(s) = self.port_use(&d.slug, other, unit) {
                    return s;
                }
                if let Some(s) = self.inside_decl(d, other) {
                    return s;
                }
            }
        }
        if let Some(d) = declared.first() {
            return d.span.clone();
        }
        for n in &names {
            if let Some(s) = self.first_use(n, unit) {
                return s;
            }
        }
        span(fallback, 0, 0, 0)
    }

    /// The first `slug.port` in the unit, as the span of `port`.
    fn port_use(&self, slug: &str, port: &str, unit: &Unit) -> Option<Span> {
        unit.files.iter().find_map(|f| {
            let text = self.text(f)?;
            text.split('\n').enumerate().find_map(|(n, line)| {
                let ws: Vec<(usize, usize)> = words(code(line)).collect();
                ws.windows(2).find_map(|w| {
                    let ((s0, e0), (s1, e1)) = (w[0], w[1]);
                    (&line[s0..e0] == slug
                        && s1 == e0 + 1
                        && &line[e0..s1] == "."
                        && &line[s1..e1] == port)
                        .then(|| span(f, n, s1, e1))
                })
            })
        })
    }

    /// `name` within the statement `d` declares (to its closing `)`).
    fn inside_decl(&self, d: &Decl, name: &str) -> Option<Span> {
        let text = self.text(&d.span.file)?;
        for (n, line) in text.split('\n').enumerate().skip(d.span.line).take(64) {
            let code = code(line);
            let from = if n == d.span.line { d.span.end } else { 0 };
            if let Some((s, e)) = words(code).find(|&(s, e)| s >= from && &code[s..e] == name) {
                return Some(span(&d.span.file, n, s, e));
            }
            // A one-line statement ends on its line, a call that spans lines
            // at the `)` that opens a line.
            let ends = if n == d.span.line {
                code.matches('(').count() <= code.matches(')').count()
            } else {
                code.trim_start().starts_with(')')
            };
            if ends {
                break;
            }
        }
        None
    }

    fn lint_span(&self, f: &LintFinding, decls: &[Decl], unit: &Unit, fallback: &Path) -> Span {
        match decls.iter().find(|d| d.slug == f.slug) {
            Some(d) => d.span.clone(),
            None => self
                .first_use(&f.slug, unit)
                .unwrap_or_else(|| span(fallback, 0, 0, 0)),
        }
    }

    fn first_use(&self, name: &str, unit: &Unit) -> Option<Span> {
        if name.is_empty() {
            return None;
        }
        unit.files.iter().find_map(|f| {
            let text = self.text(f)?;
            text.split('\n').enumerate().find_map(|(n, line)| {
                words(code(line))
                    .find(|&(s, e)| &line[s..e] == name)
                    .map(|(s, e)| span(f, n, s, e))
            })
        })
    }

    /// The module-level name at (`line`, `byte`) in `path` that rename
    /// and references take, with its range: a constant, extern, block,
    /// template or instance declared outside a template body.
    pub fn name_at(&self, path: &Path, line: usize, byte: usize) -> Option<(String, Span)> {
        let text = self.text(path)?;
        let l = super::text::line(&text, line);
        let Word::Name { name, start, end } = word_at(l, byte)? else {
            return None;
        };
        if l[end..].trim_start().starts_with(':') && !l[end..].trim_start().starts_with("::") {
            return None; // an argument key
        }
        if local_in(&text, line, name) {
            return None;
        }
        let unit = self.unit(path);
        let decls = self.decls(&unit);
        decls.iter().find(|d| {
            d.slug == name && RENAMEABLE.contains(&d.kind) && {
                let t = self.text(&d.span.file).unwrap_or_default();
                !in_template_body(&t, d.span.line)
            }
        })?;
        Some((name.to_string(), span(path, line, start, end)))
    }

    /// Every use of the module-level name at (`line`, `byte`) across the
    /// module, its declaration included: the name outside strings and
    /// comments, not as a port (`x.name`) or an argument key (`name:`),
    /// and not inside a template body that declares a name of its own
    /// (a parameter or a block) by it.
    pub fn references(&self, path: &Path, line: usize, byte: usize) -> Vec<Span> {
        let Some((name, _)) = self.name_at(path, line, byte) else {
            return Vec::new();
        };
        let unit = self.unit(path);
        let mut out = Vec::new();
        for f in &unit.files {
            let text = self.text(f).unwrap_or_default();
            let bodies = template_bodies(&text);
            for (n, l) in text.split('\n').enumerate() {
                if bodies
                    .iter()
                    .any(|(lines, locals)| lines.contains(&n) && locals.contains(&name))
                {
                    continue;
                }
                let c = code(l);
                for (s, e) in words(c) {
                    let after = c[e..].trim_start();
                    if c[s..e] == name
                        && !c[..s].ends_with('.')
                        && !(after.starts_with(':') && !after.starts_with("::"))
                    {
                        out.push(span(f, n, s, e));
                    }
                }
            }
        }
        out
    }

    /// The files a rename of the name at (`line`, `byte`) to `new`
    /// changes, with their current and new text. In a project, `loxl
    /// rename`'s verified plan: the fragments and the lockfile (the
    /// compiled output is left to the next compile). A file on its own:
    /// the file.
    pub fn rename(
        &self,
        path: &Path,
        line: usize,
        byte: usize,
        new: &str,
    ) -> Result<Vec<FileEdit>, String> {
        let (old, _) = self
            .name_at(path, line, byte)
            .ok_or("not a name rename takes (a constant, extern, block, template or instance)")?;
        let unit = self.unit(path);
        let sources: Vec<(PathBuf, String)> = unit
            .files
            .iter()
            .map(|f| (f.clone(), self.text(f).unwrap_or_default()))
            .collect();
        let current = |f: &Path| {
            sources
                .iter()
                .find(|(p, _)| p == f)
                .map(|(_, t)| t.clone())
                .unwrap_or_default()
        };
        if let Some(project) = &unit.project {
            let r =
                crate::cli::plan_rename(project, &sources, &old, new).map_err(|e| e.to_string())?;
            let mut out: Vec<FileEdit> = r
                .files
                .into_iter()
                .map(|(file, text)| FileEdit {
                    old: current(&file),
                    file,
                    new: text,
                })
                .collect();
            let lock = r.lock.to_json();
            let disk = std::fs::read_to_string(&project.lock).unwrap_or_default();
            if lock != disk {
                out.push(FileEdit {
                    file: project.lock.clone(),
                    old: self.text(&project.lock).unwrap_or(disk),
                    new: lock,
                });
            }
            return Ok(out);
        }
        crate::logic::valid_slug(new).map_err(|e| message_of(&e))?;
        let text = current(path);
        let mut m = Module::parse_fragment(&text).map_err(|e| e.to_string())?;
        if scan(&text, path).iter().any(|d| d.slug == new) {
            return Err(format!("`{new}` is already declared. Pick another name."));
        }
        crate::logic::rename_slug(&mut m, &old, new);
        m.validate()
            .map_err(|e| format!("after rename: {}", message_of(&e)))?;
        Ok(vec![FileEdit {
            file: path.to_path_buf(),
            old: text,
            new: m.to_text(),
        }])
    }

    /// The declaration of the name at (`line`, `byte`) in `path`.
    pub fn definition(&self, path: &Path, line: usize, byte: usize) -> Option<Span> {
        let text = self.text(path)?;
        let word = word_at(super::text::line(&text, line), byte)?;
        let unit = self.unit(path);
        let decls = self.decls(&unit);
        find_decl(&decls, word.slug(), path).map(|d| d.span.clone())
    }

    /// Hover text (Markdown) for the name at (`line`, `byte`).
    pub fn hover(&self, path: &Path, line: usize, byte: usize) -> Option<(String, Span)> {
        let text = self.text(path)?;
        let l = super::text::line(&text, line);
        let word = word_at(l, byte)?;
        let unit = self.unit(path);
        let decls = self.decls(&unit);
        let (start, end) = word.range();
        let here = span(path, line, start, end);
        let Some(decl) = find_decl(&decls, word.slug(), path) else {
            // A block type: its ports.
            return match word {
                Word::Name { name, .. } if builtin(name).is_some() => {
                    Some((type_hover(name), here))
                }
                _ => None,
            };
        };
        if let Word::Port { port, .. } = word {
            return Some((self.port_hover(&unit, decl, port)?, here));
        }
        let decl_text = super::text::line(&self.text(&decl.span.file)?, decl.span.line)
            .trim()
            .to_string();
        let mut md = format!("```loxl\n{decl_text}\n```\n");
        if let Some(obj) = self.object(&unit, decl) {
            md.push_str(&obj);
        }
        Some((md, here))
    }

    /// The base object a declaration stands for: an extern's resolved
    /// object, a managed block's pinned one.
    fn object(&self, unit: &Unit, decl: &Decl) -> Option<String> {
        let project = unit.project.as_ref()?;
        let uuid = match decl.kind {
            "extern" => self
                .resolved
                .borrow()
                .get(&project.module)?
                .get(&decl.slug)?
                .clone(),
            "block" => {
                let lock = Lockfile::load(&project.lock).ok()?;
                match lock.objects.get(&decl.slug) {
                    Some(o) => o.uuid.clone(),
                    None => return Some("\nNot compiled yet: the next compile creates it.".into()),
                }
            }
            _ => return None,
        };
        let base = self.base_doc(&project.base)?;
        Some(match describe(&base, &uuid) {
            Some(d) => format!("\n{d}\n\n`{uuid}`"),
            None => format!("\n`{uuid}` (not in the base config)"),
        })
    }

    fn port_hover(&self, unit: &Unit, decl: &Decl, port: &str) -> Option<String> {
        let ty = decl.ty.as_deref()?;
        if let Some(spec) = builtin(ty).and_then(|s| s.iter().find(|p| p.key == port)) {
            let mut md = format!("**{ty}.{port}** — {}", dir_name(spec.dir));
            if let Some(m) = port_meta(ty, port) {
                for (k, v) in [
                    ("label", m.label),
                    ("unit", m.unit),
                    ("min", m.min),
                    ("max", m.max),
                    ("default", m.default),
                ] {
                    if let Some(v) = v {
                        md.push_str(&format!("\n- {k}: `{v}`"));
                    }
                }
            }
            return Some(md);
        }
        self.base_ports(unit, decl)?
            .iter()
            .any(|k| k == port)
            .then(|| format!("**{ty}.{port}** — a connector of the base object"))
    }

    /// An extern's connectors in the base config.
    fn base_ports(&self, unit: &Unit, decl: &Decl) -> Option<Vec<String>> {
        let project = unit.project.as_ref()?;
        let uuid = self
            .resolved
            .borrow()
            .get(&project.module)?
            .get(&decl.slug)?
            .clone();
        let base = self.base_doc(&project.base)?;
        let path = base.index().by_uuid.get(&uuid)?.clone();
        Some(
            ports(base.element_at(&path)?)
                .into_iter()
                .map(|p| p.key)
                .collect(),
        )
    }

    /// Completions at (`line`, `byte`): the ports after `slug.`, the block
    /// types after `name =`, the declared names elsewhere.
    pub fn completion(&self, path: &Path, line: usize, byte: usize) -> Vec<Completion> {
        let Some(text) = self.text(path) else {
            return Vec::new();
        };
        let l = super::text::line(&text, line);
        let byte = byte.min(l.len());
        let before = &l[..byte];
        let partial = before.len()
            - before
                .chars()
                .rev()
                .take_while(|c| is_ident(*c))
                .map(char::len_utf8)
                .sum::<usize>();
        let unit = self.unit(path);
        let decls = self.decls(&unit);
        if before[..partial].ends_with('.') {
            let slug_end = partial - 1;
            let slug_start = before[..slug_end]
                .char_indices()
                .rev()
                .take_while(|(_, c)| is_ident(*c))
                .last()
                .map_or(slug_end, |(i, _)| i);
            let slug = &before[slug_start..slug_end];
            let Some(decl) = find_decl(&decls, slug, path) else {
                return Vec::new();
            };
            return self.port_completions(&unit, decl);
        }
        if is_type_position(&before[..partial]) {
            let mut out: Vec<Completion> = BUILTIN_TYPES
                .iter()
                .map(|t| Completion {
                    label: t.to_string(),
                    kind: "type",
                    detail: None,
                })
                .collect();
            out.extend(
                decls
                    .iter()
                    .filter(|d| d.kind == "template")
                    .map(|d| Completion {
                        label: d.slug.clone(),
                        kind: "template",
                        detail: None,
                    }),
            );
            return out;
        }
        let mut seen = std::collections::BTreeSet::new();
        decls
            .iter()
            .filter(|d| d.kind != "template" && seen.insert(d.slug.clone()))
            .map(|d| Completion {
                label: d.slug.clone(),
                kind: d.kind,
                detail: d.ty.clone(),
            })
            .collect()
    }

    fn port_completions(&self, unit: &Unit, decl: &Decl) -> Vec<Completion> {
        let ty = decl.ty.as_deref().unwrap_or("");
        let detail = |key: &str, dir: Option<PortDir>| {
            let label = port_meta(ty, key).and_then(|m| m.label);
            match (dir, label) {
                (Some(d), Some(l)) => Some(format!("{} ({l})", dir_name(d))),
                (Some(d), None) => Some(dir_name(d).to_string()),
                (None, _) => None,
            }
        };
        let spec = builtin(ty);
        let keys: Vec<String> = match (decl.kind, self.base_ports(unit, decl)) {
            ("extern", Some(keys)) => keys,
            _ => spec
                .map(|s| s.iter().map(|p| p.key.to_string()).collect())
                .unwrap_or_default(),
        };
        keys.into_iter()
            .map(|key| {
                let dir = spec.and_then(|s| s.iter().find(|p| p.key == key).map(|p| p.dir));
                Completion {
                    detail: detail(&key, dir),
                    label: key,
                    kind: "port",
                }
            })
            .collect()
    }

    /// The declarations of one file, for the outline.
    pub fn symbols(&self, path: &Path) -> Vec<Decl> {
        scan(&self.text(path).unwrap_or_default(), path)
    }
}

/// `text` in canonical form (`loxl fmt`), if it parses.
pub fn format(text: &str) -> Option<String> {
    Module::parse_fragment(text).ok().map(|m| m.to_text())
}

fn push(
    out: &mut BTreeMap<PathBuf, Vec<Diagnostic>>,
    span: Span,
    severity: Severity,
    message: String,
) {
    out.entry(span.file).or_default().push(Diagnostic {
        line: span.line,
        start: span.start,
        end: span.end,
        severity,
        message,
    });
}

/// The message without the error-kind prefix the CLI shows.
fn message_of(e: &Error) -> String {
    match e {
        Error::Compile(m) => m.clone(),
        other => other.to_string(),
    }
}

fn span(file: &Path, line: usize, start: usize, end: usize) -> Span {
    Span {
        file: file.to_path_buf(),
        line,
        start,
        end,
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

fn same_or_inside(path: &Path, dir: &Path) -> bool {
    if path.starts_with(dir) {
        return true;
    }
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    // The file may not exist yet: compare its directory.
    let parent = path
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_default();
    parent.starts_with(&dir)
}

/// The declaration of `slug`, the one in `path` first.
fn find_decl<'d>(decls: &'d [Decl], slug: &str, path: &Path) -> Option<&'d Decl> {
    decls
        .iter()
        .find(|d| d.slug == slug && d.span.file == path)
        .or_else(|| decls.iter().find(|d| d.slug == slug))
}

fn dir_name(d: PortDir) -> &'static str {
    match d {
        PortDir::Input => "input",
        PortDir::Output => "output",
        PortDir::Param => "parameter",
        PortDir::Api => "api",
    }
}

fn type_hover(ty: &str) -> String {
    let mut md = format!("**{ty}**\n");
    if let Some(s) = crate::blockdoc::summary(ty) {
        md.push_str(&format!("\n{s}\n"));
    }
    if let Some(url) = crate::blockdoc::kb_url(ty) {
        md.push_str(&format!("\n[Loxone documentation]({url})\n"));
    }
    for p in builtin(ty).unwrap_or_default() {
        md.push_str(&format!("\n- `{}` {}", p.key, dir_name(p.dir)));
        if let Some(l) = port_meta(ty, p.key).and_then(|m| m.label) {
            md.push_str(&format!(" ({l})"));
        }
    }
    md
}

/// Type, title, room and page of the object `uuid` in `doc`.
fn describe(doc: &LoxoneDoc, uuid: &str) -> Option<String> {
    let index = doc.index();
    let path = index.by_uuid.get(uuid)?;
    let el = doc.element_at(path)?;
    let title_of = |u: &str| {
        let p = index.by_uuid.get(u)?;
        doc.element_at(p)?
            .attr_decoded("Title")
            .map(|t| t.into_owned())
    };
    let mut parts = vec![format!(
        "`{}` \"{}\"",
        el.attr("Type").unwrap_or("?"),
        el.attr_decoded("Title").unwrap_or_default()
    )];
    if let Some(room) = el
        .child_elements()
        .find(|c| c.name == "IoData")
        .and_then(|io| io.attr("Pr"))
        .and_then(title_of)
    {
        parts.push(format!("room \"{room}\""));
    }
    if let Some(page) = (0..path.len()).rev().find_map(|k| {
        let e = doc.element_at(&path[..k])?;
        (e.name == "C" && e.attr("Type") == Some("Page"))
            .then(|| e.attr_decoded("Title").unwrap_or_default().into_owned())
    }) {
        parts.push(format!("page \"{page}\""));
    }
    Some(parts.join(", "))
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The part of `line` before a comment.
fn code(line: &str) -> &str {
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if in_str => escaped = true,
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

/// The identifiers of `code` outside strings, as byte ranges.
fn words(code: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut out = Vec::new();
    let mut in_str = false;
    let mut escaped = false;
    let mut start: Option<usize> = None;
    for (i, c) in code.char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        if is_ident(c) {
            start.get_or_insert(i);
            continue;
        }
        if let Some(s) = start.take() {
            out.push((s, i));
        }
        if c == '"' {
            in_str = true;
        }
    }
    if let Some(s) = start {
        out.push((s, code.len()));
    }
    out.into_iter()
        .filter(|&(s, _)| !code[s..].starts_with(|c: char| c.is_ascii_digit()))
}

/// The name under the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Word<'a> {
    Name {
        name: &'a str,
        start: usize,
        end: usize,
    },
    /// `slug.port`, the cursor on the port
    Port {
        slug: &'a str,
        port: &'a str,
        start: usize,
        end: usize,
    },
}

impl Word<'_> {
    fn slug(&self) -> &str {
        match self {
            Word::Name { name, .. } => name,
            Word::Port { slug, .. } => slug,
        }
    }

    fn range(&self) -> (usize, usize) {
        match *self {
            Word::Name { start, end, .. } | Word::Port { start, end, .. } => (start, end),
        }
    }
}

fn word_at(line: &str, byte: usize) -> Option<Word<'_>> {
    let code = code(line);
    let list: Vec<(usize, usize)> = words(code).collect();
    let i = list.iter().position(|&(s, e)| s <= byte && byte <= e)?;
    let (s, e) = list[i];
    if s > 0 && code[..s].ends_with('.') && i > 0 && list[i - 1].1 == s - 1 {
        let (ps, pe) = list[i - 1];
        return Some(Word::Port {
            slug: &code[ps..pe],
            port: &code[s..e],
            start: s,
            end: e,
        });
    }
    Some(Word::Name {
        name: &code[s..e],
        start: s,
        end: e,
    })
}

/// The template bodies of `text`: the lines from `template` to its `end`,
/// and the names the template declares (its parameters and blocks).
fn template_bodies(text: &str) -> Vec<(std::ops::Range<usize>, Vec<String>)> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let mut n = 0;
    while n < lines.len() {
        let c = code(lines[n]);
        let Some(rest) = c.trim_start().strip_prefix("template ") else {
            n += 1;
            continue;
        };
        let start = n;
        let mut locals: Vec<String> = Vec::new();
        // `template name(p: Type, q = 3)`: the word before a `:` or `=`.
        if let Some((_, params)) = rest.split_once('(') {
            let params = params.split(')').next().unwrap_or("");
            for p in params.split(',') {
                if let Some((name, _)) = p.split_once([':', '=']) {
                    locals.push(name.trim().to_string());
                }
            }
        }
        n += 1;
        while n < lines.len() && code(lines[n]).trim() != "end" {
            n += 1;
        }
        let body = lines[start + 1..n.min(lines.len())].join("\n");
        locals.extend(scan(&body, Path::new("")).into_iter().map(|d| d.slug));
        out.push((start..n + 1, locals));
        n += 1;
    }
    out
}

fn in_template_body(text: &str, line: usize) -> bool {
    template_bodies(text)
        .iter()
        .any(|(lines, _)| lines.start < line && lines.contains(&line))
}

/// `name` at `line` is a template's own (a parameter or a block of the
/// body it is in).
fn local_in(text: &str, line: usize, name: &str) -> bool {
    template_bodies(text)
        .iter()
        .any(|(lines, locals)| lines.contains(&line) && locals.iter().any(|l| l == name))
}

/// `before` ends where a block type or template name goes: `name = `.
fn is_type_position(before: &str) -> bool {
    let t = before.trim_start();
    let t = t.strip_prefix("extern ").unwrap_or(t);
    let Some((name, rest)) = t.split_once('=') else {
        return false;
    };
    let name = name.trim();
    !name.is_empty()
        && name.chars().all(is_ident)
        && rest.trim().is_empty()
        && !rest.starts_with('=')
}

const KEYWORD_DECLS: &[&str] = &[
    "extern", "let", "template", "schedule", "access", "tiles", "rule", "moods",
];

/// The declarations in `text` (of `file`).
pub fn scan(text: &str, file: &Path) -> Vec<Decl> {
    let mut out = Vec::new();
    for (n, line) in text.split('\n').enumerate() {
        let code = code(line);
        let ws: Vec<(usize, usize)> = words(code).collect();
        let word = |i: usize| ws.get(i).map(|&(s, e)| &code[s..e]);
        let after = |i: usize| ws.get(i).map_or("", |&(_, e)| code[e..].trim_start());
        let Some(first) = word(0) else { continue };
        if code[..ws[0].0].trim() != "" {
            continue;
        }
        let decl = |i: usize, kind: &'static str, ty: Option<&str>| {
            let (s, e) = ws[i];
            Decl {
                slug: code[s..e].to_string(),
                kind,
                ty: ty.map(str::to_string),
                span: span(file, n, s, e),
            }
        };
        if let Some(kw) = KEYWORD_DECLS.iter().find(|k| **k == first)
            && ws.len() > 1
            && code[ws[0].1..ws[1].0].trim().is_empty()
        {
            let ty = (*kw == "extern" && after(1).starts_with('='))
                .then(|| word(2))
                .flatten();
            out.push(decl(1, kw, ty));
            continue;
        }
        // `name = Type(` or `name = template(`.
        let rest = after(0);
        if rest.starts_with('=')
            && !rest.starts_with("==")
            && let Some(callee) = word(1)
            && after(1).starts_with('(')
            && code[ws[0].1..ws[1].0].trim() == "="
        {
            let kind = if callee.starts_with(|c: char| c.is_ascii_uppercase()) {
                "block"
            } else {
                "instance"
            };
            out.push(decl(0, kind, Some(callee)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"# Pool
let solltemp = 27
extern wasser = VirtualIn(iname: "VI1")  # Wasser
page "Pool"
heizen = Less(
	"Wasser # kein Kommentar",
	Input1: wasser.Q,
	Input2: solltemp,
)
pumpe.On <- heizen.Q
template licht(t: Switch)
	g = Or(I1: t.Q)
end
a = licht(t: pumpe)
x = heizen.Q == 1
"#;

    #[test]
    fn references_skip_ports_keys_and_template_locals() {
        let src = "let t = 3\nextern pumpe = Switch(title: \"P\")\n\
                   template licht(t: Switch, n = 2)\n\tg = Or(I1: t.Q)\nend\n\
                   a = licht(t: pumpe)\nb = Less(Input1: pumpe.t, Input2: t)\n";
        let path = Path::new("/nowhere/a.loxl");
        let mut ws = Workspace::default();
        ws.open(path.to_path_buf(), src.to_string());
        let lines = |line, byte| -> Vec<(usize, usize)> {
            ws.references(path, line, byte)
                .iter()
                .map(|s| (s.line, s.start))
                .collect()
        };
        // `t` the constant: not the template's parameter, not the
        // instance's argument key `t:`, not the port `pumpe.t`.
        assert_eq!(lines(0, 4), [(0, 4), (6, 34)]);
        // The parameter is the template's own: no rename.
        assert!(ws.name_at(path, 3, 12).is_none());
        assert!(ws.name_at(path, 3, 1).is_none());
        assert_eq!(lines(1, 8), [(1, 7), (5, 13), (6, 17)]);
        let edits = ws.rename(path, 0, 4, "schwelle").unwrap();
        assert!(
            edits[0].new.contains("Input2: schwelle"),
            "{}",
            edits[0].new
        );
        assert!(edits[0].new.contains("t.Q"), "{}", edits[0].new);
    }

    #[test]
    fn declarations_are_found_lexically() {
        let d = scan(SRC, Path::new("a.loxl"));
        let got: Vec<(&str, &str, Option<&str>, usize)> = d
            .iter()
            .map(|d| (d.slug.as_str(), d.kind, d.ty.as_deref(), d.span.line))
            .collect();
        assert_eq!(
            got,
            [
                ("solltemp", "let", None, 1),
                ("wasser", "extern", Some("VirtualIn"), 2),
                ("heizen", "block", Some("Less"), 4),
                ("licht", "template", None, 10),
                ("g", "block", Some("Or"), 11),
                ("a", "instance", Some("licht"), 13),
            ]
        );
        assert_eq!((d[1].span.start, d[1].span.end), (7, 13));
    }

    #[test]
    fn the_word_under_the_cursor_knows_ports_strings_and_comments() {
        let l = "\tInput1: wasser.Q, # wasser.Q";
        let at = |s: &str| word_at(l, l.find(s).unwrap());
        assert_eq!(
            at("wasser"),
            Some(Word::Name {
                name: "wasser",
                start: 9,
                end: 15
            })
        );
        assert!(matches!(
            at("Q"),
            Some(Word::Port {
                slug: "wasser",
                port: "Q",
                ..
            })
        ));
        assert_eq!(word_at(l, l.len() - 2), None);
        assert_eq!(word_at("\t\"Wasser\",", 3), None);
    }

    #[test]
    fn type_positions() {
        assert!(is_type_position("heizen = "));
        assert!(is_type_position("extern w = "));
        assert!(!is_type_position("x.On = "));
        assert!(!is_type_position("x == "));
        assert!(!is_type_position("\tInput1: "));
    }
}
