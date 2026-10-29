//! Command-line interface over the library, so the IR pipeline is usable
//! without writing Rust: by humans, scripts, and AI agents alike. The
//! `lxir` binary runs it, and so does `lox ir` inside the lox CLI.
//!
//! Every subcommand is a thin wrapper over one public library entry point;
//! nothing here has semantics of its own.

use lxir::ir::{
    CompileOptions, DecompileOptions, DecompileScope, Item, LintFinding, Module, adopt, adopt_one,
    adopt_pages, apply_rekeys, compile, decompile, decompile_pages, lint_dead_outputs, lint_source,
    lint_tiles, lock_rekeys, plan_tests, read_results, rename_slug, slugify, valid_slug,
    validate_ports,
};
use lxir::uuid::parse_serial;
use lxir::{Lockfile, LoxoneDoc, Project};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
lxir — Loxone config-as-code toolchain

USAGE:
  lxir check [--json] [<module.lxir | module-dir>]
        Parse and validate an IR module: syntax, references, and managed
        block types/ports/directions against the builtin table (no base
        config needed; parse errors carry line numbers). --json prints a
        machine-readable result on stdout and still exits 1 on errors.
        With no path: the module of the lox.toml project in the current
        directory.

  lxir fmt [--write | --check] [<module.lxir | module-dir>]
        Print the canonical form. --write rewrites the file(s) in place;
        --check exits 1 if a file is not already canonical. With no
        path: the module of the lox.toml project in the current
        directory.

  lxir compile [<project-dir>]
              [--base <cfg.Loxone>] [--module <m.lxir | module-dir>]
              [--lock <lock.json>] [--out <out.Loxone>]
              [--serial <12-hex>] [--time <unix-seconds>] [--page <title>]
              [--allow-removals] [--accept-version <v>] [--relayout <page>]...
        Compile IR against a base config, updating the lockfile. Inside
        a lox.toml project (see below) every path flag is optional and
        flags override the file; otherwise --base, --module, --lock,
        and --out are required.
        --serial defaults to the project file, then to the lockfile's
        recorded Miniserver serial;
        --time defaults to now (only affects newly minted UUIDs — the
        lockfile pins everything minted before);
        --page defaults to the project file, then to the document's
        first page; a `page \"<Title>\"` statement in the module overrides
        it for the blocks that follow.
        A module directory stands for a multi-file module: all *.lxir
        files inside — subdirectories included — merged in path order
        (one file per page is the convention; a fragment may reference
        sibling-file slugs).
        A base written by a different Loxone release than the lock's
        ConfigVersion pin is refused; after qualifying the release
        (one oracle open+save run), --accept-version <v> re-pins it.
        Placement (D42): a new block goes where its wires want it on its
        page; a placed block keeps its position, and a move made in
        Loxone Config is taken over into the lockfile. --relayout <page>
        (repeatable) places that page's managed blocks afresh — except
        those wired to refs another writer owns, which stay with them.

  lxir test [<project-dir>] [--filter <substr>] [--lox <path>] [--show-spec]
            [--base <cfg.Loxone>] [--module <m.lxir | module-dir>]
            [--lock <lock.json>] [--serial <12-hex>] [--page <title>]
        Run the module's `test \"…\"` … `end` scenarios (D36): compile in
        memory (the lockfile is never written), translate each test into
        a simulator scenario, and execute it with `lox sim run`
        (lox-cli). The lox binary is found via --lox, then the LOX
        environment variable, then PATH. --filter selects tests whose
        name contains the substring; --show-spec prints the generated
        scenario JSON instead of running. Exit 1 when a test fails.

  lxir rename <old-slug> <new-slug> [<project-dir>]
        Rename a module-level name — extern, block, constant, template,
        or instance — across every module file (comments included) and
        rekey the lockfile so every pinned identity survives, synthetic
        slugs from templates and expressions too. Verified before
        anything is written: the baseline lock must be current, and the
        recompiled output must be byte-identical except for Title labels
        the slug itself feeds (auto-labeled blocks, D24 expression
        labels). Needs a lox.toml project (module, lock, base); also
        refreshes the project's out file.

  lxir lint [<module.lxir | module-dir | project-dir>]
        Advisory findings a compile has no business rejecting: unused
        externs and constants, uninstantiated templates — and, inside a
        lox.toml project, managed blocks whose outputs feed nothing in
        the *compiled* config (GUI-drawn wires count as consumers; a
        block that reaches only dead ref plumbing is still dead), and
        Touch Pure Flex commands on any device that reach several
        API-linked blocks because they name none (tile-broadcast) or
        reach none (tile-dead-command). Never
        modifies anything; exit 1 when there are findings — they can be
        deliberate (reference externs kept as documentation, app-visible
        state blocks), so lint is not part of the compile path.

  lxir decompile [--managed-only] [--all-params] [--out-dir <dir>] <cfg.Loxone>
        Print the IR view of a config, grouped into sections headed by
        `page \"<Title>\"` statements (report on stderr). The default
        full view shows every page block and wire — it is for reading,
        not compiling — and elides parameters at their corpus-observed
        GUI default (--all-params shows them; the report counts them).
        --managed-only restricts the view to managed-type blocks and
        what they touch (the adoption subset), keeps every parameter,
        and folds nothing. --out-dir writes one module per logic page
        instead of printing.

  lxir adopt <cfg.Loxone> (--out-module <m.lxir> | --out-dir <dir>) --out-lock <lock.json>
        Move every managed-type block in the config under source control:
        writes the managed-only module plus a lockfile pinning each block's
        existing identity (object/port UUIDs, layout, page), so compiling
        the pair rebuilds the blocks in place instead of minting
        duplicates. --out-dir writes the module as a directory of
        fragments, one file per page (periphery externs in
        _periphery.lxir) — the layout `compile --module <dir>` reads.
        Blocks the rebuild could not reproduce faithfully are skipped
        with a warning and stay unmanaged. Never modifies the config;
        refuses existing outputs.

  lxir adopt <cfg.Loxone> --uuid <uuid> --as <slug> --module <m.lxir | module-dir> --lock <lock.json>
        Incremental form: adopt one existing block (e.g. freshly drawn in
        Loxone Config) into an existing module/lock pair. Appends the
        block's declaration — plus externs for wired neighbors the lock
        does not already pin — to the module (in a directory, to its
        page's fragment) and extends the lockfile. Verified before
        writing: rebuilding with the updated pair must be a semantic
        no-op against the config. A wire into a managed block must
        already be declared in that block's argument list — the error
        says exactly which line to add.

  lxir diff [--exit-code] [--json] <old.Loxone> <new.Loxone>
        Semantic diff: objects, parameters, rooms/categories, wires, and
        a users/access section (users and groups, membership, NFC tags,
        codes, passwords, last edit, access and UI rights). Credentials
        show as names, counts and short fingerprints, never as the stored
        hash. --exit-code exits 1 when the docs differ, users/access
        included. --json prints the diff as one JSON object.

  lxir drift <cfg.Loxone> [--lock <lock.json>]
        Check a config (typically a fresh download) against the semantic
        fingerprint the lockfile recorded at the last adopt/compile.
        Exit 0 = in sync; 1 = another writer changed something since
        (position moves, save noise, and locale renames don't count).
        One parse, no reference config needed — `lxir diff` tells you
        *what* changed, this tells you *whether* cheaply. --lock
        defaults to the lox.toml project's lockfile.

  lxir repair <cfg.Loxone> --out <out.Loxone>
        Write a copy of the config with numeric character references
        (`&#xA;`, also as `&amp;#xA;`) replaced by the characters they
        name, written as Loxone writes them: line breaks raw, `& < > \" '`
        as named entities. Loxone reads such references as literal text,
        so they only come from a foreign XML writer and leave programs,
        rule payloads and descriptions without their line breaks — the
        attributes `drift` notes. Lists every attribute it rewrote; every
        other byte is copied unchanged. Recompile against the repaired
        base before pushing it.

  lxir observe <cfg.Loxone>... [--crosscheck <legacy.json>]...
        Port-direction evidence per block type, as JSON. Multiple configs
        merge into one corpus-level view. --crosscheck compares the
        evidence against a legacy connector database (connector-map.json
        shape: type -> {c: [keys], t: {key: \"I\"|\"O\"}}) and reports
        agreements, conflicts, and coverage gaps per type.

  lxir roundtrip <cfg.Loxone>
        Verify the file re-serializes byte-identically (exit 1 if not).
  lxir render <cfg.Loxone> --page <title> [--lock <lock.json>] [--out <page.svg>]
        Layout metrics of one page — overlaps, backward and long wires,
        crossings, wire length — and, with --out, the page drawn as SVG
        (wires orthogonal, off-page connections as labels; with --lock,
        the lockfile's managed blocks in the compiler's color). Read-only.

PROJECT FILE (lox.toml):
        A directory with a lox.toml is a project — one deployment
        target. Flat `key = \"value\"` lines and # comments; keys:
        base (the deployed .Loxone, required), module (file or
        directory, required), lock (default lxir.lock.json), out
        (default out.Loxone), serial, page. Paths are relative to the
        file. Inside the directory, `lxir compile` needs no flags, and
        check / fmt / drift default to the project's module and lock.
";

type AnyError = Box<dyn std::error::Error>;

/// How help and error hints spell the command: `lxir` for the standalone
/// binary, `lox ir` inside lox.
static PROG: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn prog() -> &'static str {
    PROG.get().map_or("lxir", String::as_str)
}

/// The usage text, spelled for [`prog`].
fn usage() -> String {
    let p = prog();
    USAGE
        .replacen("lxir —", &format!("{p} —"), 1)
        .replace("\n  lxir ", &format!("\n  {p} "))
        .replace("`lxir ", &format!("`{p} "))
}

/// Run one command line (without the program name) and return its exit
/// code: 0 success, 1 a failure or finding (check errors, a diff, drift,
/// a failed test), 2 a usage error. `prog` is the command as the user
/// typed it, for help and hints.
pub fn main_with(prog: &str, args: &[String]) -> ExitCode {
    let _ = PROG.set(prog.to_string());
    let argrefs: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(&argrefs) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[&str]) -> Result<ExitCode, AnyError> {
    match args {
        [] | ["help"] | ["--help"] | ["-h"] => {
            print!("{}", usage());
            Ok(ExitCode::from(if args.is_empty() { 2 } else { 0 }))
        }
        ["check", rest @ ..] => cmd_check(rest),
        ["fmt", rest @ ..] => cmd_fmt(rest),
        ["compile", rest @ ..] => cmd_compile(rest),
        ["test", rest @ ..] => cmd_test(rest),
        ["rename", rest @ ..] => cmd_rename(rest),
        ["lint", rest @ ..] => cmd_lint(rest),
        ["decompile", rest @ ..] => cmd_decompile(rest),
        ["adopt", rest @ ..] => cmd_adopt(rest),
        ["diff", rest @ ..] => cmd_diff(rest),
        ["drift", rest @ ..] => cmd_drift(rest),
        ["repair", rest @ ..] => cmd_repair(rest),
        ["observe", rest @ ..] => cmd_observe(rest),
        ["roundtrip", path] => cmd_roundtrip(path),
        ["render", rest @ ..] => cmd_render(rest),
        [cmd, ..] => {
            eprintln!(
                "unknown or malformed command `{cmd}` — run `{} help`",
                prog()
            );
            Ok(ExitCode::from(2))
        }
    }
}

/// The `*.lxir` files of a module directory — recursive, so a project can
/// split its sources into subdirectories (`rooms/`, `systems/`) — sorted
/// by path (the merge order — semantics don't depend on it, but
/// determinism does). Dot-entries (`.git`, editor droppings) are skipped.
fn module_dir_files(dir: &std::path::Path) -> Result<Vec<PathBuf>, (String, lxir::Error)> {
    fn collect(dir: &std::path::Path, depth: usize, out: &mut Vec<PathBuf>) -> lxir::Result<()> {
        if depth > 16 {
            return Err(lxir::Error::Compile(format!(
                "module directory nested deeper than 16 levels at `{}` — symlink cycle?",
                dir.display()
            )));
        }
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            {
                continue;
            }
            if path.is_dir() {
                collect(&path, depth + 1, out)?;
            } else if path.extension().is_some_and(|x| x == "lxir") {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    collect(dir, 0, &mut files).map_err(|e| (dir.display().to_string(), e))?;
    files.sort();
    if files.is_empty() {
        return Err((
            dir.display().to_string(),
            lxir::Error::Compile("no .lxir files in directory (searched recursively)".into()),
        ));
    }
    Ok(files)
}

/// The module path of the `lox.toml` project in the current directory —
/// what zero-argument `check`/`fmt` operate on.
fn project_module() -> Result<String, AnyError> {
    match Project::find(Path::new(".")) {
        Some(f) => Ok(Project::load(&f)?.module.display().to_string()),
        None => Err("no module given and no lox.toml in the current directory".into()),
    }
}

/// Load a module from one file or a directory of `*.lxir` fragments.
/// Directory fragments parse individually (errors name the file);
/// name resolution runs once on the merged module.
fn load_module(path: &str) -> Result<Module, (String, lxir::Error)> {
    let p = std::path::Path::new(path);
    if p.is_dir() {
        let mut items = Vec::new();
        for f in module_dir_files(p)? {
            let name = f.display().to_string();
            let src =
                std::fs::read_to_string(&f).map_err(|e| (name.clone(), lxir::Error::Io(e)))?;
            let mut fragment = Module::parse_fragment(&src).map_err(|e| (name.clone(), e))?;
            fragment
                .load_files(fragment_dir(&f))
                .map_err(|e| (name.clone(), e))?;
            items.extend(fragment.items);
        }
        let module = Module { items };
        // Validate the expanded view: module-level statements may reference
        // an instance's blocks by their expanded names (D23), which only
        // exist after template expansion — mirroring what compile checks.
        module
            .expand()
            .and_then(|x| x.validate())
            .map_err(|e| (path.to_string(), e))?;
        Ok(module)
    } else {
        let src =
            std::fs::read_to_string(path).map_err(|e| (path.to_string(), lxir::Error::Io(e)))?;
        let mut module = Module::parse(&src).map_err(|e| (path.to_string(), e))?;
        module
            .load_files(fragment_dir(p))
            .map_err(|e| (path.to_string(), e))?;
        Ok(module)
    }
}

/// The directory `file("…")` paths in a `.lxir` file resolve against
/// (D40): the file's own directory.
fn fragment_dir(file: &Path) -> &Path {
    file.parent().unwrap_or(Path::new("."))
}

/// Load a module — one `.lxir` file, or a directory of fragments — the way
/// every command here does (also used by lox's `config push`).
pub fn read_module(path: &str) -> Result<Module, AnyError> {
    load_module(path).map_err(|(p, e)| format!("{p}: {e}").into())
}

/// [`read_module`] without whole-module name resolution: statement-local
/// checks only. The incremental adopt needs this — the manual fix for a
/// wire into a managed sink references the new block's slug *before* the
/// adoption declares it, so the module only resolves after the append.
fn read_module_lenient(path: &str) -> Result<Module, AnyError> {
    let p = std::path::Path::new(path);
    let mut items = Vec::new();
    let files = if p.is_dir() {
        module_dir_files(p).map_err(|(p, e)| format!("{p}: {e}"))?
    } else {
        vec![p.to_path_buf()]
    };
    for f in files {
        let src = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let mut fragment =
            Module::parse_fragment(&src).map_err(|e| format!("{}: {e}", f.display()))?;
        fragment
            .load_files(fragment_dir(&f))
            .map_err(|e| format!("{}: {e}", f.display()))?;
        items.extend(fragment.items);
    }
    Ok(Module { items })
}

fn read_doc(path: &str) -> Result<LoxoneDoc, AnyError> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    Ok(LoxoneDoc::parse(&bytes).map_err(|e| format!("{path}: {e}"))?)
}

fn cmd_check(args: &[&str]) -> Result<ExitCode, AnyError> {
    let (json, path) = match args {
        [] => (false, project_module()?),
        ["--json"] => (true, project_module()?),
        [path] => (false, path.to_string()),
        ["--json", path] | [path, "--json"] => (true, path.to_string()),
        _ => return Err("usage: lxir check [--json] [<module.lxir | module-dir>]".into()),
    };
    let path = path.as_str();
    // Full static validation: parse (syntax + references), then template
    // expansion and expression desugaring, then types, ports, and wire
    // directions against the builtin table. Counts describe the source;
    // the deep checks run on the desugared form (what compile will see).
    let checked = load_module(path).and_then(|m| {
        m.expand()
            .and_then(|x| x.validate().map(|()| x))
            .and_then(|x| x.desugar())
            .and_then(|(x, d)| x.validate().map(|()| (x, d)))
            .and_then(|(x, d)| lxir::ir::validate_ports(&x).map(|()| (x, d)))
            .map(|(x, d)| (m, x, d))
            .map_err(|e| (path.to_string(), e))
    });
    let count =
        |m: &Module, f: fn(&lxir::ir::Item) -> bool| m.items.iter().filter(|i| f(i)).count();
    match checked {
        Ok((m, x, d)) => {
            let templates = count(&m, |i| matches!(i, lxir::ir::Item::Template(_)));
            let instances = count(&m, |i| matches!(i, lxir::ir::Item::Instance(_)));
            if json {
                println!(
                    "{}",
                    serde_json::json!({ "ok": true, "path": path, "counts": {
                        "externs": m.externs().count(), "blocks": m.blocks().count(),
                        "wires": m.wire_pairs().len(), "sets": m.sets().count(),
                        "lets": m.lets().count(), "removed": m.removed().count(),
                        "moved": m.moved().count(),
                        "templates": templates, "instances": instances,
                        "expressions": d.expressions,
                        "expanded_blocks": x.blocks().count(),
                    }})
                );
            } else {
                println!(
                    "OK: {} externs, {} blocks, {} wires, {} sets, {} lets, \
                     {} removed, {} moved",
                    m.externs().count(),
                    m.blocks().count(),
                    m.wire_pairs().len(),
                    m.sets().count(),
                    m.lets().count(),
                    m.removed().count(),
                    m.moved().count()
                );
                if templates + instances > 0 {
                    println!(
                        "    {templates} templates, {instances} instances -> {} blocks expanded",
                        x.blocks().count()
                    );
                }
                if d.expressions > 0 {
                    println!(
                        "    {} expressions -> {} blocks desugared",
                        d.expressions,
                        d.synthetic.len()
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Err((epath, e)) if json => {
            // Structured diagnostics: parse errors carry a 1-based line,
            // semantic errors none. The parser is fail-fast, so there is
            // at most one error per run. `path` names the file the error
            // is in — for a module directory that is the fragment file.
            let line = match &e {
                lxir::Error::IrParse { line, .. } => Some(*line),
                _ => None,
            };
            println!(
                "{}",
                serde_json::json!({ "ok": false, "path": epath, "errors": [
                    { "line": line, "message": e.to_string() },
                ]})
            );
            Ok(ExitCode::FAILURE)
        }
        Err((epath, e)) => Err(format!("{epath}: {e}").into()),
    }
}

fn cmd_fmt(args: &[&str]) -> Result<ExitCode, AnyError> {
    let (mode, path) = match args {
        ["--write"] => ("write", project_module()?),
        ["--check"] => ("check", project_module()?),
        [path] => ("print", path.to_string()),
        ["--write", path] => ("write", path.to_string()),
        ["--check", path] => ("check", path.to_string()),
        _ => {
            return Err("usage: lxir fmt [--write | --check] [<module.lxir | module-dir>]".into());
        }
    };
    let path = path.as_str();
    // Formatting is per file and needs no name resolution (a fragment of
    // a module directory may reference slugs from sibling files), so
    // every target parses as a fragment.
    let targets: Vec<PathBuf> = if std::path::Path::new(path).is_dir() {
        if mode == "print" {
            return Err("fmt on a module directory requires --write or --check".into());
        }
        module_dir_files(std::path::Path::new(path)).map_err(|(p, e)| format!("{p}: {e}"))?
    } else {
        vec![PathBuf::from(path)]
    };
    let mut dirty = false;
    for f in &targets {
        let name = f.display();
        let src = std::fs::read_to_string(f).map_err(|e| format!("{name}: {e}"))?;
        let canonical = Module::parse_fragment(&src)
            .map_err(|e| format!("{name}: {e}"))?
            .to_text();
        match mode {
            "print" => print!("{canonical}"),
            "write" => {
                if src != canonical {
                    std::fs::write(f, &canonical)?;
                }
            }
            _check => {
                if src != canonical {
                    eprintln!(
                        "{name}: not canonical (run `{} fmt --write {name}`)",
                        prog()
                    );
                    dirty = true;
                } else {
                    println!("{name}: canonical");
                }
            }
        }
    }
    if dirty {
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_compile(args: &[&str]) -> Result<ExitCode, AnyError> {
    let mut project_dir: Option<PathBuf> = None;
    let mut base = None;
    let mut module = None;
    let mut lock_path: Option<PathBuf> = None;
    let mut out = None;
    let mut serial = None;
    let mut time = None;
    let mut page = None;
    let mut allow_removals = false;
    let mut accept_version = None;
    let mut relayout = Vec::new();

    let mut it = args.iter();
    while let Some(&flag) = it.next() {
        let mut value = || -> Result<&str, AnyError> {
            it.next()
                .copied()
                .ok_or_else(|| format!("{flag} needs a value").into())
        };
        match flag {
            "--base" => base = Some(value()?.to_string()),
            "--module" => module = Some(value()?.to_string()),
            "--lock" => lock_path = Some(PathBuf::from(value()?)),
            "--out" => out = Some(value()?.to_string()),
            "--serial" => serial = Some(value()?.to_string()),
            "--time" => time = Some(value()?.parse::<i64>()?),
            "--page" => page = Some(value()?.to_string()),
            "--allow-removals" => allow_removals = true,
            "--accept-version" => accept_version = Some(value()?.to_string()),
            "--relayout" => relayout.push(value()?.to_string()),
            dir if !dir.starts_with('-') && project_dir.is_none() => {
                project_dir = Some(PathBuf::from(dir));
            }
            other => return Err(format!("unknown flag `{other}` — run `{} help`", prog()).into()),
        }
    }
    // Project resolution: an explicit <project-dir> must hold a lox.toml;
    // otherwise, if any required path is still unset, the current
    // directory's lox.toml (if there is one) fills the gaps. Flags always
    // win over the file.
    let project = match &project_dir {
        Some(dir) => Some(Project::load(dir)?),
        None if [&base, &module, &out].iter().any(|o| o.is_none()) || lock_path.is_none() => {
            match Project::find(Path::new(".")) {
                Some(f) => Some(Project::load(&f)?),
                None => None,
            }
        }
        None => None,
    };
    let (base, module, lock_path, out) = match &project {
        Some(p) => (
            base.unwrap_or_else(|| p.base.display().to_string()),
            module.unwrap_or_else(|| p.module.display().to_string()),
            lock_path.unwrap_or_else(|| p.lock.clone()),
            out.unwrap_or_else(|| p.out.display().to_string()),
        ),
        None => {
            let (Some(base), Some(module), Some(lock_path), Some(out)) =
                (base, module, lock_path, out)
            else {
                return Err(format!(
                    "compile requires --base, --module, --lock, and --out \
                     (or a lox.toml project — run `{} help`)",
                    prog()
                )
                .into());
            };
            (base, module, lock_path, out)
        }
    };

    let base_doc = read_doc(&base)?;
    let m = read_module(&module)?;
    let mut lock = if lock_path.exists() {
        Lockfile::load(&lock_path)?
    } else {
        Lockfile::new()
    };
    let serial = serial
        .or_else(|| project.as_ref().and_then(|p| p.serial.clone()))
        .or_else(|| lock.target.miniserver_serial.clone())
        .ok_or("no --serial given, none in the project file, and the lockfile has none recorded")?;
    let page = page.or_else(|| project.as_ref().and_then(|p| p.page.clone()));
    let opts = CompileOptions {
        machine: parse_serial(&serial)?,
        accept_version,
        mint_time_unix: time.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        }),
        page_title: page,
        allow_removals,
        relayout,
    };
    let compiled = compile(&base_doc, &m, &mut lock, &opts)?;
    std::fs::write(&out, compiled.to_bytes())?;
    lock.save(&lock_path)?;
    println!(
        "compiled {module} against {base} -> {out} ({} objects, NextObj {}); lock: {}",
        compiled.objects().len(),
        compiled.counters().next_obj,
        lock_path.display()
    );
    // Placement keeps to the sheet where it can (D49); say what is past it.
    let slug_of: std::collections::BTreeMap<&str, &str> = lock
        .objects
        .iter()
        .map(|(slug, o)| (o.uuid.as_str(), slug.as_str()))
        .collect();
    let managed = slug_of.keys().copied().collect();
    for (page, uuids) in lxir::layout::off_sheet(&compiled, &managed) {
        let slugs: Vec<&str> = uuids.iter().map(|u| slug_of[u.as_str()]).collect();
        eprintln!(
            "warning: page {page:?}: {} managed object(s) past the edge of the A3 sheet: {} \
             (`{} compile --relayout {page:?}` lays the page out afresh, on the sheet where \
             there is room)",
            slugs.len(),
            slugs.join(", "),
            prog()
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_test(args: &[&str]) -> Result<ExitCode, AnyError> {
    let mut project_dir: Option<PathBuf> = None;
    let mut base = None;
    let mut module = None;
    let mut lock_path: Option<PathBuf> = None;
    let mut serial = None;
    let mut page = None;
    let mut filter: Option<String> = None;
    let mut lox_flag: Option<String> = None;
    let mut show_spec = false;

    let mut it = args.iter();
    while let Some(&flag) = it.next() {
        let mut value = || -> Result<&str, AnyError> {
            it.next()
                .copied()
                .ok_or_else(|| format!("{flag} needs a value").into())
        };
        match flag {
            "--base" => base = Some(value()?.to_string()),
            "--module" => module = Some(value()?.to_string()),
            "--lock" => lock_path = Some(PathBuf::from(value()?)),
            "--serial" => serial = Some(value()?.to_string()),
            "--page" => page = Some(value()?.to_string()),
            "--filter" => filter = Some(value()?.to_string()),
            "--lox" => lox_flag = Some(value()?.to_string()),
            "--show-spec" => show_spec = true,
            dir if !dir.starts_with('-') && project_dir.is_none() => {
                project_dir = Some(PathBuf::from(dir));
            }
            other => return Err(format!("unknown flag `{other}` — run `{} help`", prog()).into()),
        }
    }
    let project = match &project_dir {
        Some(dir) => Some(Project::load(dir)?),
        None if [&base, &module].iter().any(|o| o.is_none()) || lock_path.is_none() => {
            match Project::find(Path::new(".")) {
                Some(f) => Some(Project::load(&f)?),
                None => None,
            }
        }
        None => None,
    };
    let (base, module_path, lock_path) = match &project {
        Some(p) => (
            base.unwrap_or_else(|| p.base.display().to_string()),
            module.unwrap_or_else(|| p.module.display().to_string()),
            lock_path.unwrap_or_else(|| p.lock.clone()),
        ),
        None => {
            let (Some(base), Some(module), Some(lock_path)) = (base, module, lock_path) else {
                return Err(format!(
                    "test requires --base, --module, and --lock \
                     (or a lox.toml project — run `{} help`)",
                    prog()
                )
                .into());
            };
            (base, module, lock_path)
        }
    };

    let base_doc = read_doc(&base)?;
    let m = read_module(&module_path)?;
    let lock = if lock_path.exists() {
        Lockfile::load(&lock_path)?
    } else {
        Lockfile::new()
    };
    let serial = serial
        .or_else(|| project.as_ref().and_then(|p| p.serial.clone()))
        .or_else(|| lock.target.miniserver_serial.clone())
        .ok_or("no --serial given, none in the project file, and the lockfile has none recorded")?;
    let opts = CompileOptions {
        machine: parse_serial(&serial)?,
        accept_version: None,
        relayout: Vec::new(),
        mint_time_unix: 0,
        page_title: page.or_else(|| project.as_ref().and_then(|p| p.page.clone())),
        allow_removals: false,
    };
    // Compile against a lock COPY — testing never moves the baseline.
    let mut lock_copy = lock;
    let compiled = compile(&base_doc, &m, &mut lock_copy, &opts)?;

    // Tests may reference template-expanded and expression-owned slugs;
    // plan on the same flattened view the compiler used.
    let (flat, _) = m.expand()?.desugar()?;
    let plan = plan_tests(&flat, &lock_copy, &compiled, filter.as_deref())?;
    if plan.runs.is_empty() {
        match &filter {
            Some(f) => println!("no tests match --filter {f}"),
            None => println!("no tests in {module_path}"),
        }
        return Ok(ExitCode::SUCCESS);
    }
    if show_spec {
        println!("{}", plan.specs_json());
        return Ok(ExitCode::SUCCESS);
    }

    // The simulator lives in lox-cli (`lox sim`): --lox flag, LOX env
    // var, then PATH.
    let lox = lox_flag
        .or_else(|| std::env::var("LOX").ok())
        .unwrap_or_else(|| "lox".to_string());
    let probe = std::process::Command::new(&lox)
        .args(["sim", "--help"])
        .output();
    if !probe.map(|o| o.status.success()).unwrap_or(false) {
        return Err(format!(
            "no sim-capable `lox` binary (tried `{lox}`) — install lox-cli \
             (github.com/eisber/lox-cli) or point --lox / the LOX environment \
             variable at it"
        )
        .into());
    }

    let tmp = std::env::temp_dir().join(format!("lxir-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let cfg_path = tmp.join("compiled.Loxone");
    let specs_path = tmp.join("specs.json");
    std::fs::write(&cfg_path, compiled.to_bytes())?;
    std::fs::write(&specs_path, plan.specs_json())?;
    let output = std::process::Command::new(&lox)
        .arg("sim")
        .arg("run")
        .arg(&cfg_path)
        .arg("--sim-file")
        .arg(&specs_path)
        .args(["--output", "json", "--non-interactive"])
        .output();
    let _ = std::fs::remove_dir_all(&tmp);
    let output = output?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Err(format!(
            "`{lox} sim run` produced no output; stderr: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }

    let results = read_results(&stdout, &plan)?;
    let mut failed = 0;
    for r in &results {
        if r.pass {
            println!("test \"{}\" ... ok", r.name);
        } else {
            failed += 1;
            println!("test \"{}\" ... FAILED", r.name);
            for line in &r.failures {
                println!("    {line}");
            }
        }
    }
    println!(
        "{} tests: {} passed, {failed} failed",
        results.len(),
        results.len() - failed
    );
    Ok(if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn cmd_rename(args: &[&str]) -> Result<ExitCode, AnyError> {
    let (old, new, dir) = match args {
        [old, new] => (*old, *new, "."),
        [old, new, dir] if !dir.starts_with('-') => (*old, *new, *dir),
        _ => return Err("usage: lxir rename <old-slug> <new-slug> [<project-dir>]".into()),
    };
    let project_file = Project::find(Path::new(dir)).ok_or_else(|| {
        format!("no lox.toml in `{dir}` — rename needs a project (module, lock, base)")
    })?;
    let project = Project::load(&project_file)?;
    valid_slug(new)?;

    // Fragments load individually so each file can be rewritten in place.
    let files = if project.module.is_dir() {
        module_dir_files(&project.module).map_err(|(p, e)| format!("{p}: {e}"))?
    } else {
        vec![project.module.clone()]
    };
    let mut fragments: Vec<(PathBuf, String, Module)> = Vec::new();
    for f in &files {
        let src = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
        let m = Module::parse_fragment(&src).map_err(|e| format!("{}: {e}", f.display()))?;
        fragments.push((f.clone(), src, m));
    }
    let merge = |frags: &[(PathBuf, String, Module)]| Module {
        items: frags.iter().flat_map(|(_, _, m)| m.items.clone()).collect(),
    };
    let merged_old = merge(&fragments);
    // Validate the expanded view — expanded instance names are legal
    // reference targets at module level (D23).
    merged_old
        .expand()
        .and_then(|x| x.validate())
        .map_err(|e| format!("{}: {e}", project.module.display()))?;

    let declares = |m: &Module, name: &str| {
        m.items.iter().any(|i| match i {
            Item::Extern(e) => e.slug == name,
            Item::Block(b) | Item::Instance(b) => b.slug == name,
            Item::Let(l) => l.name == name,
            Item::Template(t) => t.name == name,
            _ => false,
        })
    };
    if !declares(&merged_old, old) {
        return Err(format!(
            "`{old}` is not declared in {} — rename takes a module-level name \
             (extern, block, constant, template, or instance)",
            project.module.display()
        )
        .into());
    }
    if declares(&merged_old, new) {
        return Err(format!("`{new}` is already declared — pick a fresh name").into());
    }

    for (_, _, m) in &mut fragments {
        rename_slug(m, old, new);
    }
    let merged_new = merge(&fragments);
    merged_new
        .validate()
        .map_err(|e| format!("after rename: {e}"))?;
    let rekeys = lock_rekeys(&merged_old, &merged_new)?;

    // Verification: baseline compile must reproduce the committed lock
    // (otherwise module and lock are out of sync and a rename would bake
    // that confusion in), and the renamed pair must compile to the same
    // bytes — Title labels the slug feeds excepted.
    let base_doc = read_doc(&project.base.display().to_string())?;
    let disk_lock = Lockfile::load(&project.lock)?;
    let serial = project
        .serial
        .clone()
        .or_else(|| disk_lock.target.miniserver_serial.clone())
        .ok_or("no serial in the project file and none recorded in the lockfile")?;
    let opts = CompileOptions {
        machine: parse_serial(&serial)?,
        accept_version: None,
        relayout: Vec::new(),
        // Nothing may mint during a rename (the currency check below
        // guarantees it), so a fixed time keeps the comparison exact.
        mint_time_unix: 0,
        page_title: project.page.clone(),
        allow_removals: false,
    };
    let mut lock_a = disk_lock.clone();
    let out_old = compile(&base_doc, &merged_old, &mut lock_a, &opts)
        .map_err(|e| format!("baseline compile failed — fix that first: {e}"))?;
    if lock_a.to_json() != disk_lock.to_json() {
        return Err(format!(
            "the lockfile is not current (a compile would change it) — \
             run `{} compile`, commit, then rename",
            prog()
        )
        .into());
    }
    let mut lock_new = disk_lock.clone();
    let moved = apply_rekeys(&mut lock_new, &rekeys)?;
    let out_new = compile(&base_doc, &merged_new, &mut lock_new, &opts)?;
    let title_changes = title_only_diff(&out_old.to_bytes(), &out_new.to_bytes())
        .map_err(|e| format!("rename is not cosmetic — nothing written: {e}"))?;

    let mut files_changed = 0;
    for (path, src, m) in &fragments {
        let text = m.to_text();
        if *src != text {
            std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
            files_changed += 1;
        }
    }
    lock_new.save(&project.lock)?;
    std::fs::write(&project.out, out_new.to_bytes())?;
    println!(
        "renamed `{old}` -> `{new}`: {files_changed} file(s) rewritten, \
         {moved} lock entr{} rekeyed, output {}",
        if moved == 1 { "y" } else { "ies" },
        if title_changes == 0 {
            "byte-identical".to_string()
        } else {
            format!("changed in {title_changes} Title label(s) the slug feeds")
        }
    );
    Ok(ExitCode::SUCCESS)
}

/// Count the lines two outputs differ in, requiring every difference to
/// be confined to a `Title="…"` attribute; any other difference is an
/// error describing the first offending pair.
fn title_only_diff(a: &[u8], b: &[u8]) -> Result<usize, String> {
    if a == b {
        return Ok(0);
    }
    let a = String::from_utf8_lossy(a);
    let b = String::from_utf8_lossy(b);
    let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    if la.len() != lb.len() {
        return Err(format!(
            "outputs have different line counts ({} vs {})",
            la.len(),
            lb.len()
        ));
    }
    // XML attribute values escape `"` as `&quot;`, so the closing quote
    // found here always ends the attribute.
    fn strip_title(line: &str) -> Option<String> {
        let start = line.find(" Title=\"")?;
        let rest = &line[start + 8..];
        let end = rest.find('"')?;
        Some(format!("{}{}", &line[..start], &rest[end + 1..]))
    }
    let mut changes = 0;
    for (x, y) in la.iter().zip(&lb) {
        if x == y {
            continue;
        }
        match (strip_title(x), strip_title(y)) {
            (Some(sx), Some(sy)) if sx == sy => changes += 1,
            _ => {
                return Err(format!("first non-Title difference:\n  - {x}\n  + {y}"));
            }
        }
    }
    Ok(changes)
}

fn cmd_lint(args: &[&str]) -> Result<ExitCode, AnyError> {
    let path = match args {
        [] => ".".to_string(),
        [p] if !p.starts_with('-') => p.to_string(),
        _ => return Err("usage: lxir lint [<module.lxir | module-dir | project-dir>]".into()),
    };
    // A project directory gets the full treatment (dead-output analysis
    // needs base + lock); a bare module gets the source-only lints.
    let p = Path::new(&path);
    let project = if p.is_dir() {
        Project::find(p).map(|f| Project::load(&f)).transpose()?
    } else {
        None
    };
    let (module_path, project) = match project {
        Some(prj) => (prj.module.display().to_string(), Some(prj)),
        None => (path.clone(), None),
    };
    let module = read_module(&module_path)?;

    let mut findings = lint_source(&module)?;
    match &project {
        Some(prj) => {
            let base_doc = read_doc(&prj.base.display().to_string())?;
            let lock = Lockfile::load(&prj.lock)?;
            let serial = prj
                .serial
                .clone()
                .or_else(|| lock.target.miniserver_serial.clone())
                .ok_or("no serial in the project file and none recorded in the lockfile")?;
            let opts = CompileOptions {
                machine: parse_serial(&serial)?,
                accept_version: None,
                relayout: Vec::new(),
                mint_time_unix: 0,
                page_title: prj.page.clone(),
                allow_removals: false,
            };
            let mut lock_copy = lock.clone();
            let compiled = compile(&base_doc, &module, &mut lock_copy, &opts)
                .map_err(|e| format!("compile for dead-output analysis failed: {e}"))?;
            findings.extend(lint_dead_outputs(&module, &lock, &compiled)?);
            findings.extend(lint_tiles(&module, &compiled));
        }
        None => eprintln!("note: no lox.toml project — dead-output analysis skipped"),
    }

    if findings.is_empty() {
        println!("lint: clean");
        return Ok(ExitCode::SUCCESS);
    }
    // Name the declaring file where the module is a directory of fragments.
    let locate: Vec<(PathBuf, Module)> = if Path::new(&module_path).is_dir() {
        module_dir_files(Path::new(&module_path))
            .map_err(|(p, e)| format!("{p}: {e}"))?
            .into_iter()
            .filter_map(|f| {
                let src = std::fs::read_to_string(&f).ok()?;
                Some((f, Module::parse_fragment(&src).ok()?))
            })
            .collect()
    } else {
        Vec::new()
    };
    let file_of = |finding: &LintFinding| -> String {
        for (f, m) in &locate {
            let declares = m.items.iter().any(|i| match i {
                Item::Extern(e) => e.slug == finding.slug,
                Item::Block(b) | Item::Instance(b) => b.slug == finding.slug,
                Item::Let(l) => l.name == finding.slug,
                Item::Template(t) => t.name == finding.slug,
                _ => false,
            });
            if declares {
                return format!("{}: ", f.display());
            }
        }
        String::new()
    };
    for finding in &findings {
        println!(
            "{}{}: `{}` — {}",
            file_of(finding),
            finding.kind.label(),
            finding.slug,
            finding.detail
        );
    }
    println!(
        "lint: {} finding{}",
        findings.len(),
        if findings.len() == 1 { "" } else { "s" }
    );
    Ok(ExitCode::FAILURE)
}

fn cmd_decompile(args: &[&str]) -> Result<ExitCode, AnyError> {
    let mut path = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut scope = DecompileScope::Full;
    let mut all_params = false;
    let mut it = args.iter();
    while let Some(&a) = it.next() {
        match a {
            "--managed-only" => scope = DecompileScope::ManagedOnly,
            "--all-params" => all_params = true,
            "--out-dir" => {
                out_dir = Some(PathBuf::from(
                    it.next().copied().ok_or("--out-dir needs a value")?,
                ));
            }
            flag if flag.starts_with("--") => {
                return Err(format!("unknown flag `{flag}` — run `{} help`", prog()).into());
            }
            p if path.is_none() => path = Some(p),
            _ => {
                return Err("usage: lxir decompile [--managed-only] [--all-params] \
                            [--out-dir <dir>] <cfg.Loxone>"
                    .into());
            }
        }
    }
    let Some(path) = path else {
        return Err(
            "usage: lxir decompile [--managed-only] [--all-params] [--out-dir <dir>] \
                    <cfg.Loxone>"
                .into(),
        );
    };
    let doc = read_doc(path)?;
    let opts = DecompileOptions {
        scope,
        all_params,
        ..Default::default()
    };

    let report = if let Some(dir) = out_dir {
        let (pages, report) = decompile_pages(&doc, &opts)?;
        std::fs::create_dir_all(&dir)?;
        for p in &pages {
            let file = dir.join(format!("{}.lxir", p.slug));
            std::fs::write(&file, p.module.to_text())?;
            println!(
                "wrote {} ({} externs, {} blocks, {} wires)",
                file.display(),
                p.module.externs().count(),
                p.module.blocks().count(),
                p.module.wire_pairs().len()
            );
        }
        report
    } else {
        let (module, report) = decompile(&doc, &opts)?;
        print!("{}", module.to_text());
        report
    };
    eprintln!(
        "# {path}: {} managed, {} externs across {} pages, {} raw objects untouched{}",
        report.managed,
        report.externs,
        report.pages,
        report.raw_objects,
        {
            let mut notes = String::new();
            if report.ref_wires_folded > 0 {
                notes.push_str(&format!(
                    ", {} ref plumbing wires folded",
                    report.ref_wires_folded
                ));
            }
            if report.default_params_elided > 0 {
                notes.push_str(&format!(
                    ", {} default params elided",
                    report.default_params_elided
                ));
            }
            notes
        }
    );
    Ok(ExitCode::SUCCESS)
}

fn cmd_adopt(args: &[&str]) -> Result<ExitCode, AnyError> {
    const USAGE: &str = "usage: lxir adopt <cfg.Loxone> \
         (--out-module <m.lxir> | --out-dir <dir>) --out-lock <lock.json>\n\
       lxir adopt <cfg.Loxone> --uuid <uuid> --as <slug> \
         --module <m.lxir | module-dir> --lock <lock.json>";
    let mut path = None;
    let mut out_module: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut out_lock: Option<PathBuf> = None;
    let mut uuid: Option<&str> = None;
    let mut as_slug: Option<&str> = None;
    let mut module_path: Option<&str> = None;
    let mut lock_path: Option<&str> = None;
    let mut it = args.iter();
    while let Some(&a) = it.next() {
        let mut value = || -> Result<&str, AnyError> {
            it.next()
                .copied()
                .ok_or_else(|| format!("{a} needs a value").into())
        };
        match a {
            "--out-module" => out_module = Some(PathBuf::from(value()?)),
            "--out-dir" => out_dir = Some(PathBuf::from(value()?)),
            "--out-lock" => out_lock = Some(PathBuf::from(value()?)),
            "--uuid" => uuid = Some(value()?),
            "--as" => as_slug = Some(value()?),
            "--module" => module_path = Some(value()?),
            "--lock" => lock_path = Some(value()?),
            flag if flag.starts_with("--") => {
                return Err(format!("unknown flag `{flag}` — run `{} help`", prog()).into());
            }
            p if path.is_none() => path = Some(p),
            _ => return Err(USAGE.into()),
        }
    }
    if uuid.is_some() || as_slug.is_some() || module_path.is_some() || lock_path.is_some() {
        let (Some(path), Some(uuid), Some(slug), Some(module_path), Some(lock_path)) =
            (path, uuid, as_slug, module_path, lock_path)
        else {
            return Err(USAGE.into());
        };
        if out_module.is_some() || out_dir.is_some() || out_lock.is_some() {
            return Err(USAGE.into());
        }
        return cmd_adopt_one(path, uuid, slug, module_path, lock_path);
    }
    let (Some(path), Some(out_lock)) = (path, out_lock) else {
        return Err(USAGE.into());
    };
    if out_module.is_some() == out_dir.is_some() {
        return Err(USAGE.into());
    }
    // Adoption is a one-time claim of identity; overwriting an existing
    // module or lock would silently discard identities already pinned.
    for existing in [out_module.as_ref(), out_dir.as_ref(), Some(&out_lock)]
        .into_iter()
        .flatten()
    {
        if existing.exists() {
            return Err(format!(
                "{} already exists — adopt refuses to overwrite (move it away first)",
                existing.display()
            )
            .into());
        }
    }

    let doc = read_doc(path)?;
    let (lock, report) = if let Some(dir) = &out_dir {
        let (fragments, lock, report) = adopt_pages(&doc)?;
        std::fs::create_dir_all(dir)?;
        for (stem, fragment) in &fragments {
            std::fs::write(dir.join(format!("{stem}.lxir")), fragment.to_text())?;
        }
        (lock, report)
    } else {
        let out_module = out_module.as_ref().expect("checked above");
        let (module, lock, report) = adopt(&doc)?;
        std::fs::write(out_module, module.to_text())?;
        (lock, report)
    };
    lock.save(&out_lock)?;
    for r in &report.refused {
        eprintln!("warning: {r}");
    }
    println!(
        "adopted {} blocks ({} externs pinned) across {} pages from {path}{}",
        report.blocks,
        report.externs,
        report.pages,
        if report.refused.is_empty() {
            String::new()
        } else {
            format!(" ({} refused, see warnings)", report.refused.len())
        }
    );
    let module_arg = out_module
        .as_ref()
        .or(out_dir.as_ref())
        .expect("one is set");
    println!(
        "wrote {} and {}\nnext: {p} compile --base {path} --module {} --lock {} \
         --serial <miniserver-serial> --out <out.Loxone>, then `{p} diff {path} \
         <out.Loxone>` — it should be empty",
        module_arg.display(),
        out_lock.display(),
        module_arg.display(),
        out_lock.display(),
        p = prog()
    );
    Ok(ExitCode::SUCCESS)
}

/// The incremental adopt: one block into an existing module/lock pair.
/// Everything is verified in memory — the appended module and extended
/// lock must rebuild the config as a semantic no-op — before any file is
/// touched.
fn cmd_adopt_one(
    path: &str,
    uuid: &str,
    slug: &str,
    module_path: &str,
    lock_path: &str,
) -> Result<ExitCode, AnyError> {
    let doc = read_doc(path)?;
    let module = read_module_lenient(module_path)?;
    let mut lock = Lockfile::load(Path::new(lock_path))?;
    let adopted = adopt_one(&doc, uuid, slug, &module, &mut lock)?;

    let mut merged = module;
    merged.items.extend(adopted.items.iter().cloned());
    merged.validate()?;
    validate_ports(&merged)?;
    // Nothing is minted in a no-op rebuild, so serial and mint time are
    // inert — placeholders keep the verification self-contained.
    let vopts = CompileOptions {
        machine: parse_serial("000000000000")?,
        mint_time_unix: 0,
        page_title: None,
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    };
    let out = compile(&doc, &merged, &mut lock.clone(), &vopts)?;
    let d = lxir::diff::diff(&doc, &out);
    if !d.is_empty() {
        return Err(format!(
            "adoption verification failed — rebuilding with the updated module/lock \
             is not a semantic no-op (nothing was written):\n{d:#?}"
        )
        .into());
    }

    let target = if Path::new(module_path).is_dir() {
        Path::new(module_path).join(format!("{}.lxir", slugify(&adopted.page_title)))
    } else {
        PathBuf::from(module_path)
    };
    let mut text = std::fs::read_to_string(&target).unwrap_or_default();
    if text.is_empty() {
        // A fresh page file opens with its `page` statement (D28), so the
        // adopted blocks stay pinned to the page they were drawn on.
        text = Module {
            items: vec![lxir::ir::Item::Page(lxir::ir::PageDecl {
                title: adopted.page_title.clone(),
                comment: None,
            })],
        }
        .to_text();
    }
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push('\n');
    text.push_str(
        &Module {
            items: adopted.items.clone(),
        }
        .to_text(),
    );
    std::fs::write(&target, text)?;
    lock.save(Path::new(lock_path))?;

    println!(
        "adopted uuid {uuid} as `{slug}` on page \"{}\"{}",
        adopted.page_title,
        if adopted.new_externs.is_empty() {
            String::new()
        } else {
            format!(
                " ({} pinned: {})",
                adopted.new_externs.len(),
                adopted.new_externs.join(", ")
            )
        }
    );
    println!(
        "appended {} statement(s) to {} and updated {lock_path}\n\
         verified: rebuilding with the updated pair is a semantic no-op",
        adopted.items.len(),
        target.display()
    );
    Ok(ExitCode::SUCCESS)
}

fn cmd_diff(args: &[&str]) -> Result<ExitCode, AnyError> {
    const USAGE: &str = "usage: lxir diff [--exit-code] [--json] <old.Loxone> <new.Loxone>";
    let (flags, files): (Vec<&str>, Vec<&str>) = args.iter().partition(|a| a.starts_with("--"));
    let (exit_code, json) = (flags.contains(&"--exit-code"), flags.contains(&"--json"));
    let (old, new) = match (
        files.as_slice(),
        flags.iter().all(|f| ["--exit-code", "--json"].contains(f)),
    ) {
        ([old, new], true) => (*old, *new),
        _ => return Err(USAGE.into()),
    };
    let a = read_doc(old)?;
    let b = read_doc(new)?;
    // The diff keys on UUIDs: twins collapse into one entry, and wires
    // between them read as moves. Say so rather than refuse — diffing a
    // damaged config is how the damage gets investigated.
    for (path, doc) in [(old, &a), (new, &b)] {
        let dups = doc.duplicate_uuids();
        if !dups.is_empty() {
            eprintln!(
                "warning: {path} carries {} duplicate UUID(s) ({}{}) — entries \
                 sharing a UUID are conflated below",
                dups.len(),
                dups.iter().take(3).cloned().collect::<Vec<_>>().join(", "),
                if dups.len() > 3 { ", …" } else { "" }
            );
        }
    }
    let d = lxir::diff::diff(&a, &b);
    if json {
        println!("{}", serde_json::to_string_pretty(&d)?);
        return Ok(if exit_code && !d.is_empty() {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        });
    }

    for o in &d.added {
        println!("+ {} {} {:?}", o.block_type, o.uuid, o.title);
    }
    for o in &d.removed {
        println!("- {} {} {:?}", o.block_type, o.uuid, o.title);
    }
    for r in &d.renamed {
        let tag = if r.locale_suspect { " [locale?]" } else { "" };
        println!(
            "~ {} {} {:?} -> {:?}{tag}",
            r.block_type, r.uuid, r.from, r.to
        );
    }
    for p in &d.param_changes {
        // An Automatic Designer rule payload is a JSON blob — rendered
        // as field-path changes instead of two full blobs.
        if p.port_key == "APC"
            && let (Some(from), Some(to)) = (&p.from, &p.to)
            && let (Some(old), Some(new)) =
                (lxir::autopilot::parse(from), lxir::autopilot::parse(to))
        {
            let lines = lxir::autopilot::changes(&old, &new);
            if !lines.is_empty() {
                let title = b
                    .objects()
                    .into_iter()
                    .chain(a.objects())
                    .find(|o| o.uuid == p.object_uuid)
                    .and_then(|o| o.title);
                println!(
                    "* {} {} {:?} rule changed:",
                    p.block_type, p.object_uuid, title
                );
                for line in lines {
                    println!("    {line}");
                }
                continue;
            }
        }
        // Program text (`Code=`, D40) — a line diff, not two blobs.
        if lxir::connectors::file_params(&p.block_type).contains(&p.port_key.as_str()) {
            println!(
                "* {} {}.{} program changed:",
                p.block_type, p.object_uuid, p.port_key
            );
            let (from, to) = (p.from.as_deref(), p.to.as_deref());
            for line in lxir::diff::text_changes(from.unwrap_or(""), to.unwrap_or("")) {
                println!("    {line}");
            }
            continue;
        }
        // A tile function (D41) is already a readable `name="value"` list.
        if p.port_key.starts_with("tile ") {
            let side = |v: &Option<String>| v.clone().unwrap_or_else(|| "(empty)".into());
            println!(
                "* {} {} {}: {} -> {}",
                p.block_type,
                p.object_uuid,
                p.port_key,
                side(&p.from),
                side(&p.to)
            );
            continue;
        }
        println!(
            "* {} {}.{}: {:?} -> {:?}",
            p.block_type, p.object_uuid, p.port_key, p.from, p.to
        );
    }
    for p in &d.placements {
        // Rooms and categories read by title; the raw UUID only where it
        // resolves to nothing (a dangling binding).
        let name = |title: &Option<String>, uuid: &Option<String>| match (title, uuid) {
            (Some(t), _) => format!("{t:?}"),
            (None, Some(u)) => u.clone(),
            (None, None) => "none".to_string(),
        };
        println!(
            "@ {} {} {:?} {}: {} -> {}",
            p.block_type,
            p.object_uuid,
            p.title,
            p.attr,
            name(&p.from_title, &p.from),
            name(&p.to_title, &p.to)
        );
    }
    for w in &d.wires_added {
        println!("+wire {} -> {}", w.from_port, w.to_port);
    }
    for w in &d.wires_removed {
        println!("-wire {} -> {}", w.from_port, w.to_port);
    }
    // Users and access: `+user "alice" <uuid>`, `~group "Kids" <uuid>: …`.
    for c in &d.access {
        let head = format!(
            "{}{} {} {}",
            c.sign(),
            c.subject.as_str(),
            c.label(),
            c.uuid
        );
        match c.change {
            lxir::access::Change::Added | lxir::access::Change::Removed => println!("{head}"),
            _ => println!("{head}: {}", c.change.describe()),
        }
    }
    let locale_noise = d.renamed.iter().filter(|r| r.locale_suspect).count();
    eprintln!(
        "{} added, {} removed, {} renamed ({locale_noise} locale-suspect), \
         {} param changes, {} room/category changes, {}/{} wires added/removed, \
         {} user/access changes",
        d.added.len(),
        d.removed.len(),
        d.renamed.len(),
        d.param_changes.len(),
        d.placements.len(),
        d.wires_added.len(),
        d.wires_removed.len(),
        d.access.len()
    );
    if exit_code && !d.is_empty() {
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_drift(args: &[&str]) -> Result<ExitCode, AnyError> {
    let (config, lock_path) = match args {
        [config, "--lock", lock] | ["--lock", lock, config] => (*config, lock.to_string()),
        [config] => {
            let Some(f) = Project::find(Path::new(".")) else {
                return Err("no --lock given and no lox.toml in the current directory \
                     (usage: lxir drift <cfg.Loxone> [--lock <lock.json>])"
                    .into());
            };
            (*config, Project::load(&f)?.lock.display().to_string())
        }
        _ => return Err("usage: lxir drift <cfg.Loxone> [--lock <lock.json>]".into()),
    };
    let lock_path = lock_path.as_str();
    let doc = read_doc(config)?;
    let lock = Lockfile::load(Path::new(lock_path))?;
    let Some(recorded) = &lock.target.semantic_fingerprint else {
        return Err(format!(
            "{lock_path} records no semantic fingerprint (it predates the \
             feature) — one adopt or compile establishes the baseline"
        )
        .into());
    };
    // Numeric-reference text is a foreign writer's trace (Loxone reads it
    // literally), in sync or not — a program can sit collapsed in a
    // config that nobody has touched since (docs/loxone-format.md).
    let mut refs = doc.char_ref_attrs();
    // Logic first — a collapsed program or rule payload outranks a
    // garbled description.
    refs.sort_by_key(|(_, attr)| !matches!(attr.as_str(), "Code" | "APC"));
    if !refs.is_empty() {
        let shown: Vec<String> = refs
            .iter()
            .take(5)
            .map(|(who, attr)| format!("{who:?}.{attr}"))
            .collect();
        let more = refs.len().saturating_sub(shown.len());
        println!(
            "note: {config} has numeric character references as literal text in \
             {} attribute(s) ({}{}) — Loxone reads them verbatim, so the \
             line breaks there are gone; `lxir repair` writes them back \
             (docs/loxone-format.md)",
            refs.len(),
            shown.join(", "),
            if more > 0 {
                format!(", +{more} more")
            } else {
                String::new()
            }
        );
    }
    let current = lxir::diff::semantic_fingerprint(&doc);
    if &current == recorded {
        println!("in sync: {config} matches the fingerprint in {lock_path}");
        Ok(ExitCode::SUCCESS)
    } else if lxir::diff::semantic_fingerprint_v3(&doc) == *recorded {
        // Recorded before tile programs joined the projection (D41).
        println!(
            "in sync: {config} matches the fingerprint in {lock_path} (recorded \
             by an older lxir, blind to Touch Pure Flex tile programs — the next \
             compile records the current form)"
        );
        Ok(ExitCode::SUCCESS)
    } else if lxir::diff::semantic_fingerprint_v2(&doc) == *recorded {
        // Recorded before program text joined the projection (D40).
        println!(
            "in sync: {config} matches the fingerprint in {lock_path} (recorded \
             by an older lxir, blind to program code — the next compile records \
             the current form)"
        );
        Ok(ExitCode::SUCCESS)
    } else if lxir::diff::semantic_fingerprint_v1(&doc) == *recorded {
        // Recorded before room/category bindings joined the projection
        // (D39): in sync as far as that form can tell.
        println!(
            "in sync: {config} matches the fingerprint in {lock_path} (recorded \
             by an older lxir, blind to room/category bindings — the next compile \
             records the current form)"
        );
        Ok(ExitCode::SUCCESS)
    } else {
        // Distinguish "another writer changed something" from "the last
        // compile removed blocks and its output has not been pushed yet"
        // (D31): pending tombstones whose objects the config still holds
        // are expected during the compile → push window.
        let present: std::collections::BTreeSet<String> =
            doc.objects().into_iter().map(|o| o.uuid).collect();
        let pending: Vec<&str> = lock
            .removed
            .iter()
            .filter(|(uuid, _)| present.contains(*uuid))
            .map(|(_, t)| t.slug.as_str())
            .collect();
        // Repeated UUIDs never come out of a compile (it refuses them), so
        // a config carrying them points at whatever handled the output
        // after lxir (docs/loxone-format.md names the known producer).
        let dups = doc.duplicate_uuids();
        if !dups.is_empty() {
            println!(
                "note: {config} carries {} duplicate UUID(s), which lxir never \
                 emits — see docs/loxone-format.md for the known producer",
                dups.len()
            );
        }
        if pending.is_empty() {
            println!(
                "drift: {config} no longer matches the fingerprint recorded in \
                 {lock_path} — another writer changed something since the last \
                 adopt/compile; run `{p} diff <last-compiled.Loxone> {config}` \
                 to see what",
                p = prog()
            );
        } else {
            println!(
                "drift: {config} no longer matches the fingerprint recorded in \
                 {lock_path} — pending removal of {} not yet deployed (push the \
                 compiled output, then download); if the config also changed \
                 elsewhere, `{p} diff <last-compiled.Loxone> {config}` shows it",
                pending.join(", "),
                p = prog()
            );
        }
        Ok(ExitCode::FAILURE)
    }
}

fn cmd_observe(args: &[&str]) -> Result<ExitCode, AnyError> {
    use lxir::connectors::{self, LegacyDb, Observations};

    let mut paths: Vec<&str> = Vec::new();
    let mut legacy_paths: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(&a) = it.next() {
        if a == "--crosscheck" {
            legacy_paths.push(it.next().copied().ok_or("--crosscheck needs a value")?);
        } else {
            paths.push(a);
        }
    }
    if paths.is_empty() {
        return Err("usage: lxir observe <cfg.Loxone>... [--crosscheck <legacy.json>]...".into());
    }

    let mut obs = Observations::new();
    for p in &paths {
        connectors::merge(&mut obs, connectors::observe(&read_doc(p)?));
    }

    if legacy_paths.is_empty() {
        println!("{}", serde_json::to_string_pretty(&obs)?);
        return Ok(ExitCode::SUCCESS);
    }

    let mut checks = serde_json::Map::new();
    for lp in legacy_paths {
        let bytes = std::fs::read(lp).map_err(|e| format!("{lp}: {e}"))?;
        let legacy: LegacyDb = serde_json::from_slice(&bytes).map_err(|e| format!("{lp}: {e}"))?;
        let only_corpus: Vec<&String> = obs.keys().filter(|t| !legacy.contains_key(*t)).collect();
        let only_legacy: Vec<&String> = legacy.keys().filter(|t| !obs.contains_key(*t)).collect();
        checks.insert(
            lp.to_string(),
            serde_json::json!({
                "types": connectors::crosscheck(&obs, &legacy),
                "types_only_in_corpus": only_corpus,
                "types_only_in_legacy": only_legacy,
            }),
        );
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "observations": obs,
            "crosscheck": checks,
        }))?
    );
    Ok(ExitCode::SUCCESS)
}

fn cmd_repair(args: &[&str]) -> Result<ExitCode, AnyError> {
    let (config, out) = match args {
        [config, "--out", out] | ["--out", out, config] => (*config, *out),
        _ => return Err("usage: lxir repair <cfg.Loxone> --out <out.Loxone>".into()),
    };
    let input = std::fs::read(config).map_err(|e| format!("{config}: {e}"))?;
    let mut doc = LoxoneDoc::parse(&input).map_err(|e| format!("{config}: {e}"))?;
    let repairs = doc.repair_char_refs();
    for r in &repairs {
        println!(
            "repaired {:?}.{} ({}): {} reference(s)",
            r.object, r.attr, r.uuid, r.replaced
        );
    }
    if repairs.is_empty() {
        std::fs::write(out, &input).map_err(|e| format!("{out}: {e}"))?;
        println!("nothing to repair: {out} is a copy of {config}");
    } else {
        std::fs::write(out, doc.to_bytes()).map_err(|e| format!("{out}: {e}"))?;
        println!("wrote {out}: {} attribute(s) repaired", repairs.len());
    }
    let left = doc.char_ref_attrs();
    if !left.is_empty() {
        let shown: Vec<String> = left
            .iter()
            .map(|(who, attr)| format!("{who:?}.{attr}"))
            .collect();
        println!(
            "note: left as text in {} attribute(s), naming no character an \
             attribute can hold: {}",
            left.len(),
            shown.join(", ")
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_render(args: &[&str]) -> Result<ExitCode, AnyError> {
    let mut config = None;
    let mut page = None;
    let mut lock_path = None;
    let mut out = None;
    let mut it = args.iter();
    while let Some(&flag) = it.next() {
        let mut value = || -> Result<&str, AnyError> {
            it.next()
                .copied()
                .ok_or_else(|| format!("{flag} needs a value").into())
        };
        match flag {
            "--page" => page = Some(value()?.to_string()),
            "--lock" => lock_path = Some(value()?.to_string()),
            "--out" => out = Some(value()?.to_string()),
            path if !path.starts_with('-') && config.is_none() => config = Some(path.to_string()),
            other => return Err(format!("unknown flag `{other}` — run `lxir help`").into()),
        }
    }
    let (Some(config), Some(page)) = (config, page) else {
        return Err("render requires <cfg.Loxone> and --page <title>".into());
    };
    let doc = read_doc(&config)?;
    let uuids = lxir::layout::PageGeometry::pages_titled(&doc, &page);
    let [uuid] = uuids.as_slice() else {
        return Err(format!("{} pages titled {page:?} in {config}", uuids.len()).into());
    };
    let geo = lxir::layout::PageGeometry::of(&doc, uuid).expect("page found by title");
    let managed: std::collections::BTreeSet<String> = match &lock_path {
        Some(p) => Lockfile::load(Path::new(p))?
            .objects
            .values()
            .map(|o| o.uuid.clone())
            .collect(),
        None => Default::default(),
    };
    print!("{}", lxir::render::metrics(&geo));
    if let Some(out) = out {
        std::fs::write(&out, lxir::render::svg(&geo, &managed))?;
        println!("wrote {out}");
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_roundtrip(path: &str) -> Result<ExitCode, AnyError> {
    let input = std::fs::read(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    let doc = LoxoneDoc::parse(&input).map_err(|e| format!("{path}: {e}"))?;
    let output = doc.to_bytes();
    if input == output {
        println!("{path}: byte-identical roundtrip ({} bytes)", input.len());
        Ok(ExitCode::SUCCESS)
    } else {
        let pos = input
            .iter()
            .zip(output.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(input.len().min(output.len()));
        eprintln!(
            "{path}: roundtrip DIVERGES at byte {pos} (in {} bytes, out {} bytes)",
            input.len(),
            output.len()
        );
        Ok(ExitCode::FAILURE)
    }
}
