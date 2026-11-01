//! Command-line interface over the library, so the Lox Logic pipeline is usable
//! without writing Rust: by humans, scripts, and AI agents alike. The
//! `loxl` binary runs it, and so does `lox logic` inside the lox CLI.
//!
//! Every subcommand is a thin wrapper over one public library entry point;
//! nothing here has semantics of its own.

use loxone_config::logic::{
    CompileOptions, DecompileOptions, DecompileScope, Item, LintFinding, LintKind, Module,
    TestPlan, TestResult, adopt, adopt_one, adopt_pages, apply_rekeys, compile, decompile,
    decompile_pages, finish_lint, fix_implicit_zeros, fix_unit_suffixes_text, implicit_zeros,
    lint_access, lint_circuits, lint_dead_outputs, lint_implicit_zero, lint_source, lint_tiles,
    lock_rekeys, lox_specs, plan_tests, read_results, rename_slug, run_tests, slugify, trace_tests,
    valid_slug, validate_ports,
};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc, Project};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// One command's help: [`Command::usage`] lines, a one-line summary (also
/// its line in the overview), then the options and notes.
struct Command {
    name: &'static str,
    usage: &'static [&'static str],
    summary: &'static str,
    details: &'static str,
}

const COMMANDS: &[Command] = &[
    Command {
        name: "check",
        usage: &["loxl check [--json] [<module.loxl | module-dir>]"],
        summary: "Check a module for errors",
        details: r#"Options:
  --json  Print the result as JSON. The exit code is still 1 on errors.

Checks syntax, references, block types, ports and wire directions. No base config is needed.
Reports every independent error at once (at most 50), each with its file and line, sorted.
JSON: {"ok": false, "path": <first error's file>, "errors": [{"path", "line", "message"}, ...]},
plus "truncated": <count> when errors were left out.
With no path, checks the module of the lox.toml project in the current directory.
"#,
    },
    Command {
        name: "fmt",
        usage: &["loxl fmt [--write | --check] [<module.loxl | module-dir>]"],
        summary: "Format a module",
        details: r#"Options:
  --write  Rewrite the files in place
  --check  Exit 1 if a file is not formatted

Without --write or --check, prints the formatted module. A module directory needs one of them.
With --write or --check and no path, formats the module of the lox.toml project.
"#,
    },
    Command {
        name: "lsp",
        usage: &["loxl lsp [--stdio]"],
        summary: "Run the language server for editors",
        details: r#"Speaks LSP on stdin and stdout. It offers check and lint diagnostics, go to definition,
hover, completion, formatting and an outline.
In a lox.toml project, opening or saving a file also compiles the module in memory.
"#,
    },
    Command {
        name: "compile",
        usage: &["loxl compile [<project-dir>] [<options>]"],
        summary: "Compile a module into a config and update the lockfile",
        details: r#"Options:
  --base <cfg.Loxone>      Config to compile into
  --module <m.loxl | dir>  Module file, or a directory of .loxl files
  --lock <lock.json>       Lockfile to read and update. Created if missing.
  --out <out.Loxone>       Where to write the compiled config
  --serial <12-hex>        Miniserver serial (default: lox.toml, then the lockfile)
  --time <unix-seconds>    Creation time for new UUIDs (default: now)
  --page <title>           Page for blocks before a file's first `page` statement
                           (default: lox.toml, then the config's first page)
  --allow-removals         Delete managed blocks that are missing from the module
  --accept-version <v>     Accept a base saved by a different Loxone Config release
  --relayout <page>        Re-place this page's managed blocks (repeatable)

In a lox.toml project the paths come from the project file, and flags override it.
Outside a project, --base, --module, --lock and --out are required.
A module directory merges all .loxl files in it and its subdirectories, in path order.
Blocks keep their place between compiles. A move made in Loxone Config is kept.

Examples:
  loxl compile
  loxl compile --relayout "Beschattung"
  loxl compile --base haus.Loxone --module logic/ --lock logic.lock.json --out out.Loxone
"#,
    },
    Command {
        name: "test",
        usage: &["loxl test [<project-dir>] [<options>]"],
        summary: "Run the module's test scenarios in the simulator",
        details: r#"Options:
  --filter <text>  Run only the tests whose name contains the text
  --trace          Print each test cycle by cycle: what it sets, port changes, each check
  --lox <path>     Run the tests with `lox sim`, using the lox binary at <path>
  --show-spec      With --lox, print the scenario JSON instead of running it
  --base, --module, --lock, --serial, --page
                   As for compile (default: from lox.toml)

The module is compiled in memory. The lockfile is not written.
A test that uses a block the simulator does not model reports ERROR and names the block.
Exit 1 when a test fails.
"#,
    },
    Command {
        name: "rename",
        usage: &["loxl rename <old-slug> <new-slug> [<project-dir>]"],
        summary: "Rename a name across the module and the lockfile",
        details: r#"Renames an extern, block, constant, template or instance in every module file, comments included.
The lockfile keeps every identity, so the compiled config only changes in labels that show the name.
Nothing is written unless the lockfile is current and the recompiled config matches.
Needs a lox.toml project. Also rewrites the project's out file.
"#,
    },
    Command {
        name: "lint",
        usage: &["loxl lint [--fix] [--summary] [<module.loxl | module-dir | project-dir>]"],
        summary: "Report unused and dead parts of a module",
        details: r#"On any module, reports unused externs and constants, templates nothing uses, values
outside a port's documented range and bare time values that read better with their unit
(`unit-suffix`: `Delay: 10800` is `Delay: 3h`; also extern port assignments and template
parameters). In a directory module it reports a file whose
blocks come before its first `page` statement while other files name their page (those blocks go
to the default page).
In a lox.toml project it also reports:
  - managed blocks whose outputs feed nothing in the compiled config (blocks shown in the app,
    modes and blocks that send messages count as used; a device's unused inputs are one finding)
  - Touch Pure Flex commands that reach several blocks, or none
  - users whose NFC tag or access code opens no NFC Code Touch
  - existing blocks with a port at 0 only because nothing sets it, where a new block would get
    Loxone Config's default (`implicit-zero`: an adopted `Monoflop` without `Time` pulses 0 s)

Findings are grouped by kind, each kind with its count.

A finding you mean to keep: put `# lint: allow <kind>` on the line above its declaration (the
block, extern, instance or device). Text after the kind is free. An allow that covers nothing is
reported (`unused-allow`).

Options:
  --fix       write the unit suffixes the `unit-suffix` findings name, in place (the rest of
              the file stays as written), and `Port: 0` for the `implicit-zero` findings (a
              file that gets one is written in canonical form, as `loxl fmt` writes it). The
              compile output does not change; in a project, lint compiles before and after
              and writes nothing if the two differ.
  --summary   print only the count of each kind

The path defaults to the current directory. Without --fix, lint never changes a file.
Exit 1 when there are findings. Some can be deliberate, so compile does not run lint.
"#,
    },
    Command {
        name: "decompile",
        usage: &["loxl decompile [--managed-only] [--all-params] [--out-dir <dir>] <cfg.Loxone>"],
        summary: "Print a config as Lox Logic",
        details: r#"Options:
  --managed-only   Show only the blocks Lox Logic manages and what they touch, with every parameter
  --all-params     Also show parameters left at their default
  --out-dir <dir>  Write one module file per page instead of printing

The output has a `page "<Title>"` section per page. A report goes to stderr.
The full view is for reading. To put blocks under source control, use `loxl adopt`.
"#,
    },
    Command {
        name: "adopt",
        usage: &[
            "loxl adopt <cfg.Loxone> (--out-module <m.loxl> | --out-dir <dir>) --out-lock <lock.json>",
            "loxl adopt <cfg.Loxone> --uuid <uuid> --as <slug> --module <m.loxl | dir> --lock <lock.json>",
        ],
        summary: "Put existing blocks of a config under source control",
        details: r#"The first form adopts every block in the config that Lox Logic can manage:
  --out-module <m.loxl>    Write the module to one file
  --out-dir <dir>          Write the module as one file per page, device externs in _periphery.loxl
  --out-lock <lock.json>   Write the lockfile with each block's UUIDs, layout and page

Compiling the pair rebuilds the blocks in place, without duplicates.
Blocks that cannot be rebuilt exactly are skipped with a warning.
adopt never changes the config and does not overwrite existing files.

The second form adds one block, for example one just drawn in Loxone Config:
  --uuid <uuid>            The block to adopt
  --as <slug>              Its name in the module
  --module <m.loxl | dir>  Module to add it to (in a directory, to its page's file)
  --lock <lock.json>       Lockfile to extend

Externs for its wired neighbors are added too.
Nothing is written unless the rebuilt config is unchanged.
A wire into a managed block must already be in that block's arguments.
The error names the line to add.
"#,
    },
    Command {
        name: "diff",
        usage: &["loxl diff [--exit-code] [--json | --summary] <old.Loxone> <new.Loxone>"],
        summary: "Show what changed between two configs",
        details: r#"Options:
  --exit-code  Exit 1 when the configs differ
  --json       Print the diff as one JSON object
  --summary    Print the diff as a commit message: a subject line, then the changes

Compares objects, parameters, rooms and categories, schedules, wires, users and access rights.
Passwords and codes show as names, counts and short hashes, never as stored.
"#,
    },
    Command {
        name: "drift",
        usage: &["loxl drift <cfg.Loxone> [--lock <lock.json>]"],
        summary: "Check whether a config changed since the last compile",
        details: r#"Options:
  --lock <lock.json>  Lockfile to compare against (default: the lox.toml project's)

Exit 0 when in sync, also for the config the last compile started from (its output is not
pushed yet). Exit 1 when the config changed since the last adopt or compile.
Moved blocks and the changes Loxone Config makes on every save don't count.
Run `loxl diff` to see what changed.
"#,
    },
    Command {
        name: "coverage",
        usage: &["loxl coverage [--json] [<project-dir> | <cfg.Loxone> [--lock <lock.json>]]"],
        summary: "Show how much of a config the module manages",
        details: r#"Options:
  --json              Print the report as JSON
  --lock <lock.json>  Lockfile to count against. Without one, every object counts as untouched.

Counts the objects of each type, on pages and in the periphery, as managed, extern or untouched.
For each type it says whether Lox Logic can manage it, only adopt it, or only reference it.
It lists the blocks `loxl adopt --uuid` could take over next.
For managed types it lists the attributes the compile copies from the base, not the source.
The default is the lox.toml project in the current directory.
"#,
    },
    Command {
        name: "repair",
        usage: &["loxl repair <cfg.Loxone> --out <out.Loxone>"],
        summary: "Restore line breaks that another tool escaped",
        details: r#"Options:
  --out <out.Loxone>  Where to write the repaired copy

Replaces numeric character references such as `&#xA;` with the characters they stand for.
Loxone reads such references as literal text, so programs and descriptions lose their line breaks.
Lists every attribute it changed. All other bytes stay the same.
Recompile against the repaired base before you push it.
"#,
    },
    Command {
        name: "scan",
        usage: &[
            "loxl scan [--json] <cfg.Loxone | dir>...",
            "loxl scan --redact <cfg.Loxone> --out <copy.Loxone>",
        ],
        summary: "Find personal data in configs, or redact it",
        details: r#"Options:
  --json                 Print the findings as JSON
  --redact <cfg.Loxone>  Write a redacted copy instead of scanning
  --out <copy.Loxone>    Where --redact writes the copy

Finds addresses, coordinates, names, mail addresses, phone numbers, access codes, credentials,
the remote-access address, MAC and public IP addresses, and hardware serials.
It shows where each one is, never the value. Directories are searched for *.Loxone files.
Exit 1 when it finds any, so it can guard against committing a real config.

--redact replaces each finding with a placeholder, and hardware serials with a counter.
Serials inside UUIDs are replaced too.
The copy is for sharing in an issue or a test, never for uploading. Exit 1 when a finding is left.
"#,
    },
    Command {
        name: "observe",
        usage: &["loxl observe <cfg.Loxone>... [--crosscheck <file>]..."],
        summary: "Collect the port directions of each block type from configs",
        details: r#"Options:
  --crosscheck <file>  Compare against a connector database (repeatable)

Prints JSON. Several configs merge into one view.
--crosscheck reports agreements, conflicts and gaps per type. The file is either:
  - a connector map: type -> {c: [keys], t: {key: "I" | "O"}}
  - Loxone's documented connectors: type -> {connectors: [{name, type}]}
"#,
    },
    Command {
        name: "types",
        usage: &["loxl types [<Type> | --search <text>] [--json]"],
        summary: "List the block types Lox Logic can create",
        details: r#"Options:
  --search <text>  List the types whose name or purpose mentions the text
  --json           Print as JSON

Without a type, prints one line per type.
With a type, prints its purpose, its Loxone documentation page and its ports.
For each port: direction, unit, canvas label, documented range and default, and the default
Loxone Config sets. A value with a unit is converted: `2h` is `120` on a minutes port.

Examples:
  loxl types --search hysteresis
  loxl types PulseAt
"#,
    },
    Command {
        name: "templates",
        usage: &["loxl templates [<file | template>]"],
        summary: "List the template library, or print one file of it",
        details: r#"Without a name, lists each template with its parameters and what it does.
With a name, prints the library file to copy into the module directory.
"#,
    },
    Command {
        name: "roundtrip",
        usage: &["loxl roundtrip <cfg.Loxone>"],
        summary: "Check that a config is written back byte for byte",
        details: "Exit 1 if the output differs from the file.\n",
    },
    Command {
        name: "render",
        usage: &["loxl render <cfg.Loxone> --page <title> [--lock <lock.json>] [--out <page.svg>]"],
        summary: "Show the layout metrics of a page, or draw it as SVG",
        details: r#"Options:
  --page <title>      The page to show (required)
  --out <page.svg>    Also draw the page as SVG
  --lock <lock.json>  Color the lockfile's managed blocks in the drawing

The metrics are overlaps, backward and long wires, crossings and wire length.
"#,
    },
];

const PROJECT_HELP: &str = r#"lox.toml: the project file

A directory with a lox.toml is a project: one Miniserver config to deploy.
The file has `key = "value"` lines and # comments. Paths are relative to the file.

  base    The deployed .Loxone config (required)
  module  Module file or directory (required)
  lock    Lockfile (default: logic.lock.json)
  out     Compiled config (default: out.Loxone)
  serial  Miniserver serial
  page    Default page for new blocks

In a project, `loxl compile` and `loxl test` need no flags.
check, fmt, lint, drift, coverage and rename use the project's files by default.
"#;

type AnyError = Box<dyn std::error::Error>;

/// How help and error hints spell the command: `loxl` for the standalone
/// binary, `lox logic` inside lox.
static PROG: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn prog() -> &'static str {
    PROG.get().map_or("loxl", String::as_str)
}

/// `text` with each `loxl` command spelled as [`prog`]. A `.loxl` file
/// name stays as it is.
fn spell(text: &str) -> String {
    spell_as(text, prog())
}

fn spell_as(text: &str, p: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (i, _) in text.match_indices("loxl ") {
        let starts_word = text[..i]
            .chars()
            .next_back()
            .is_none_or(|c| matches!(c, ' ' | '`' | '\n' | '('));
        if starts_word {
            out.push_str(&text[last..i]);
            out.push_str(p);
            out.push(' ');
            last = i + "loxl ".len();
        }
    }
    out.push_str(&text[last..]);
    out
}

fn command(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// The usage lines, the first after `prefix`, the others aligned under it.
fn usage_lines(c: &Command, prefix: &str) -> String {
    let indent = " ".repeat(prefix.len());
    let lines: Vec<String> = c
        .usage
        .iter()
        .enumerate()
        .map(|(i, u)| format!("{}{u}", if i == 0 { prefix } else { &indent }))
        .collect();
    spell(&lines.join("\n"))
}

/// `loxl help`: one line per command.
fn overview() -> String {
    let p = prog();
    let mut s = format!(
        "{p}: Lox Logic, Loxone config as code\n\nUsage: {p} <command> [<args>]\n\nCommands:\n"
    );
    for c in COMMANDS {
        s.push_str(&format!("  {:<11} {}\n", c.name, c.summary));
    }
    s.push_str(&spell(
        "\nRun `loxl <command> --help` for a command's options.\n\
         Run `loxl help project` for the lox.toml project file.\n",
    ));
    s
}

/// `loxl <command> --help`.
fn command_help(c: &Command) -> String {
    format!(
        "{}\n\n{}.\n\n{}",
        usage_lines(c, "Usage: "),
        c.summary,
        spell(c.details)
    )
}

/// The error for arguments `name` does not accept: its usage and where
/// the help is.
fn usage_error(name: &str) -> AnyError {
    let c = command(name).expect("a known command");
    format!(
        "{}\nRun `{} {name} --help` for the options.",
        usage_lines(c, "usage: "),
        prog()
    )
    .into()
}

fn unknown_flag(name: &str, flag: &str) -> AnyError {
    format!("unknown flag `{flag}`. Run `{} {name} --help`.", prog()).into()
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
        [] => {
            print!("{}", overview());
            return Ok(ExitCode::from(2));
        }
        ["help"] | ["--help"] | ["-h"] => {
            print!("{}", overview());
            return Ok(ExitCode::SUCCESS);
        }
        ["help", "project"] => {
            print!("{}", spell(PROJECT_HELP));
            return Ok(ExitCode::SUCCESS);
        }
        ["help", name] => {
            let Some(c) = command(name) else {
                eprintln!("unknown command `{name}`. Run `{} help`.", prog());
                return Ok(ExitCode::from(2));
            };
            print!("{}", command_help(c));
            return Ok(ExitCode::SUCCESS);
        }
        [name, rest @ ..] if rest.iter().any(|a| matches!(*a, "--help" | "-h")) => {
            if let Some(c) = command(name) {
                print!("{}", command_help(c));
                return Ok(ExitCode::SUCCESS);
            }
        }
        _ => {}
    }
    match args {
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
        ["coverage", rest @ ..] => cmd_coverage(rest),
        ["repair", rest @ ..] => cmd_repair(rest),
        ["scan", rest @ ..] => cmd_scan(rest),
        ["observe", rest @ ..] => cmd_observe(rest),
        ["types", rest @ ..] => cmd_types(rest),
        ["templates"] => cmd_templates(None),
        ["templates", name] => cmd_templates(Some(name)),
        ["templates", ..] => Err(usage_error("templates")),
        ["roundtrip", path] => cmd_roundtrip(path),
        ["roundtrip", ..] => Err(usage_error("roundtrip")),
        ["render", rest @ ..] => cmd_render(rest),
        ["lsp"] | ["lsp", "--stdio"] => {
            loxone_config::lsp::run_stdio()?;
            Ok(ExitCode::SUCCESS)
        }
        ["lsp", ..] => Err(usage_error("lsp")),
        [cmd, ..] => {
            eprintln!("unknown command `{cmd}`. Run `{} help`.", prog());
            Ok(ExitCode::from(2))
        }
        [] => unreachable!("handled above"),
    }
}

/// The `*.loxl` files of a module directory — recursive, so a project can
/// split its sources into subdirectories (`rooms/`, `systems/`) — sorted
/// by path (the merge order — semantics don't depend on it, but
/// determinism does). Dot-entries (`.git`, editor droppings) are skipped.
pub(crate) fn module_dir_files(
    dir: &std::path::Path,
) -> Result<Vec<PathBuf>, (String, loxone_config::Error)> {
    fn collect(
        dir: &std::path::Path,
        depth: usize,
        out: &mut Vec<PathBuf>,
    ) -> loxone_config::Result<()> {
        if depth > 16 {
            return Err(loxone_config::Error::Compile(format!(
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
            } else if path.extension().is_some_and(|x| x == "loxl") {
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
            loxone_config::Error::Compile(
                "no .loxl files in directory (searched recursively)".into(),
            ),
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

/// Load a module from one file or a directory of `*.loxl` fragments.
/// Directory fragments parse individually (errors name the file);
/// name resolution runs once on the merged module. Every fragment is
/// parsed even when one fails, so all syntax errors come back at once.
fn load_module(path: &str) -> Result<Module, Vec<Placed>> {
    let p = std::path::Path::new(path);
    if p.is_dir() {
        let mut fragments = Vec::new();
        let mut errors = Vec::new();
        let files = module_dir_files(p).map_err(|(f, e)| vec![Placed::new(f, None, e)])?;
        for f in files {
            let name = f.display().to_string();
            let loaded = std::fs::read_to_string(&f)
                .map_err(loxone_config::Error::Io)
                .and_then(|src| Module::parse_fragment(&src))
                .and_then(|mut fragment| {
                    fragment.load_files(fragment_dir(&f))?;
                    Ok(fragment)
                });
            match loaded {
                Ok(fragment) => fragments.push(fragment),
                Err(e) => errors.extend(place(path, &name, e)),
            }
        }
        // A fragment that does not parse declares nothing: name
        // resolution would only report its names as undeclared.
        if !errors.is_empty() {
            return Err(errors);
        }
        let module = Module::from_fragments(fragments);
        // Validate the expanded view: module-level statements reference an
        // instance's blocks (`a.b`), which only exist after template
        // expansion — mirroring what compile checks.
        module
            .with_expanded(|x| x.validate())
            .map_err(|e| place(path, path, e))?;
        Ok(module)
    } else {
        let src = std::fs::read_to_string(path)
            .map_err(|e| vec![Placed::new(path.into(), None, loxone_config::Error::Io(e))])?;
        let mut module = Module::parse(&src).map_err(|e| place(path, path, e))?;
        module
            .load_files(fragment_dir(p))
            .map_err(|e| vec![Placed::new(path.into(), None, e)])?;
        Ok(module)
    }
}

/// One error of a module, with the file and 1-based line it is about.
struct Placed {
    file: String,
    line: Option<usize>,
    error: loxone_config::Error,
}

impl Placed {
    fn new(file: String, line: Option<usize>, error: loxone_config::Error) -> Placed {
        Placed { file, line, error }
    }
}

impl std::fmt::Display for Placed {
    /// `file:line: message`. A parse error's message names its line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.error, self.line) {
            (loxone_config::Error::Parse { .. }, _) | (_, None) => {
                write!(f, "{}: {}", self.file, self.error)
            }
            (e, Some(line)) => write!(f, "{}:{line}: {e}", self.file),
        }
    }
}

/// Every error `e` stands for, placed: a parse error in `file` at its
/// line, any other at the place in module `path` its names point to
/// (else in `file`). Sorted by file, then line.
fn place(path: &str, file: &str, e: loxone_config::Error) -> Vec<Placed> {
    let order = module_files(path);
    let mut out: Vec<Placed> = e
        .into_list()
        .into_iter()
        .map(|e| match &e {
            loxone_config::Error::Parse { line, .. } => Placed::new(file.into(), Some(*line), e),
            _ => match error_line(path, &e) {
                Some((f, line)) => Placed::new(f, Some(line), e),
                None => Placed::new(file.into(), None, e),
            },
        })
        .collect();
    out.sort_by_key(|p| {
        let rank = order
            .iter()
            .position(|f| f.display().to_string() == p.file)
            .unwrap_or(usize::MAX);
        (rank, p.line.unwrap_or(usize::MAX))
    });
    out
}

/// The `.loxl` files of module `path`, in order.
fn module_files(path: &str) -> Vec<PathBuf> {
    let p = Path::new(path);
    if p.is_dir() {
        module_dir_files(p).unwrap_or_default()
    } else {
        vec![p.to_path_buf()]
    }
}

/// `errors` as one message, a line each, the first [`MAX_ERRORS`].
fn placed_text(errors: &[Placed]) -> String {
    let mut lines: Vec<String> = errors
        .iter()
        .take(loxone_config::MAX_ERRORS)
        .map(|p| p.to_string())
        .collect();
    if errors.len() > loxone_config::MAX_ERRORS {
        lines.push(format!(
            "{} more errors not shown",
            errors.len() - loxone_config::MAX_ERRORS
        ));
    }
    lines.join("\n")
}

/// The file and 1-based line a module-level error is about: the fragment
/// of a directory module (or the line of a single file) that declares or
/// uses the names the message quotes. `None` for a parse error, whose
/// message carries its line, and where no file names them.
fn error_line(path: &str, e: &loxone_config::Error) -> Option<(String, usize)> {
    if matches!(
        e,
        loxone_config::Error::Parse { .. } | loxone_config::Error::Io(_)
    ) {
        return None;
    }
    let files = module_files(path);
    let (file, line) = loxone_config::lsp::analysis::locate_error(&files, &e.to_string())?;
    Some((file.display().to_string(), line))
}

/// The directory `file("…")` paths in a `.loxl` file resolve against:
/// the file's own directory.
fn fragment_dir(file: &Path) -> &Path {
    file.parent().unwrap_or(Path::new("."))
}

/// Load a module — one `.loxl` file, or a directory of fragments — the way
/// every command here does (also used by lox's `config push`).
pub fn read_module(path: &str) -> Result<Module, AnyError> {
    load_module(path).map_err(|e| placed_text(&e).into())
}

/// [`read_module`] without whole-module name resolution: statement-local
/// checks only. The incremental adopt needs this — the manual fix for a
/// wire into a managed sink references the new block's slug *before* the
/// adoption declares it, so the module only resolves after the append.
fn read_module_lenient(path: &str) -> Result<Module, AnyError> {
    let p = std::path::Path::new(path);
    let mut fragments = Vec::new();
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
        fragments.push(fragment);
    }
    Ok(Module::from_fragments(fragments))
}

/// A `.Loxone` file, or (feature `archive`) a backup ZIP holding one.
fn read_doc(path: &str) -> Result<LoxoneDoc, AnyError> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    #[cfg(feature = "archive")]
    let bytes = loxone_config::archive::program_xml(&bytes).map_err(|e| format!("{path}: {e}"))?;
    Ok(LoxoneDoc::parse(&bytes).map_err(|e| format!("{path}: {e}"))?)
}

fn cmd_check(args: &[&str]) -> Result<ExitCode, AnyError> {
    let (json, path) = match args {
        [] => (false, project_module()?),
        ["--json"] => (true, project_module()?),
        [path] => (false, path.to_string()),
        ["--json", path] | [path, "--json"] => (true, path.to_string()),
        _ => return Err(usage_error("check")),
    };
    let path = path.as_str();
    // Full static validation: parse (syntax + references), then template
    // expansion and expression desugaring, then types, ports, and wire
    // directions against the builtin table. Counts describe the source;
    // the deep checks run on the desugared form (what compile will see).
    let checked = load_module(path).and_then(|m| {
        m.with_expanded(|x| {
            x.validate()?;
            let (x, d) = x.desugar()?;
            x.validate()?;
            loxone_config::logic::validate_ports(&x)?;
            Ok((x, d))
        })
        .map(|(x, d)| (m, x, d))
        .map_err(|e| place(path, path, e))
    });
    let count = |m: &Module, f: fn(&loxone_config::logic::Item) -> bool| {
        m.items.iter().filter(|i| f(i)).count()
    };
    match checked {
        Ok((m, x, d)) => {
            let templates = count(&m, |i| matches!(i, loxone_config::logic::Item::Template(_)));
            let instances = count(&m, |i| matches!(i, loxone_config::logic::Item::Instance(_)));
            if json {
                println!(
                    "{}",
                    serde_json::json!({ "ok": true, "path": path, "counts": {
                        "externs": m.externs().count(), "blocks": m.blocks().count(),
                        "wires": m.wire_pairs().len(), "sets": m.sets().count(),
                        "lets": m.lets().count(), "removed": m.removed().count(),
                        "moved": m.moved().count(),
                        "templates": templates, "instances": instances,
                        "signals": m.signals().count(),
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
        Err(errors) if json => {
            // Structured diagnostics, every error found, by file and line:
            // `path` names the file an error is in (for a module
            // directory, the fragment), `line` is 1-based or null. The
            // top-level `path` is the first error's file.
            let shown = &errors[..errors.len().min(loxone_config::MAX_ERRORS)];
            let list: Vec<serde_json::Value> = shown
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "path": p.file, "line": p.line, "message": p.error.to_string(),
                    })
                })
                .collect();
            let mut out = serde_json::json!({
                "ok": false, "path": errors[0].file, "errors": list,
            });
            if errors.len() > shown.len() {
                out["truncated"] = serde_json::json!(errors.len() - shown.len());
            }
            println!("{out}");
            Ok(ExitCode::FAILURE)
        }
        Err(errors) => {
            for p in errors.iter().take(loxone_config::MAX_ERRORS) {
                eprintln!("error: {p}");
            }
            if errors.len() > loxone_config::MAX_ERRORS {
                eprintln!(
                    "{} more errors not shown",
                    errors.len() - loxone_config::MAX_ERRORS
                );
            }
            Ok(ExitCode::FAILURE)
        }
    }
}

fn cmd_fmt(args: &[&str]) -> Result<ExitCode, AnyError> {
    let (mode, path) = match args {
        ["--write"] => ("write", project_module()?),
        ["--check"] => ("check", project_module()?),
        [path] => ("print", path.to_string()),
        ["--write", path] => ("write", path.to_string()),
        ["--check", path] => ("check", path.to_string()),
        _ => return Err(usage_error("fmt")),
    };
    let path = path.as_str();
    // Formatting is per file and needs no name resolution (a fragment of
    // a module directory may reference slugs from sibling files), so
    // every target parses as a fragment. The one thing the files share is
    // how they spell an operating mode: by the slug any of them gives it.
    let targets: Vec<PathBuf> = if std::path::Path::new(path).is_dir() {
        if mode == "print" {
            return Err("fmt on a module directory requires --write or --check".into());
        }
        module_dir_files(std::path::Path::new(path)).map_err(|(p, e)| format!("{p}: {e}"))?
    } else {
        vec![PathBuf::from(path)]
    };
    let mut parsed = Vec::new();
    for f in &targets {
        let name = f.display();
        let src = std::fs::read_to_string(f).map_err(|e| format!("{name}: {e}"))?;
        let module = Module::parse_fragment(&src).map_err(|e| format!("{name}: {e}"))?;
        parsed.push((src, module));
    }
    let spellings = loxone_config::logic::ModeSpellings::of(parsed.iter().map(|(_, m)| m));
    let mut dirty = false;
    for (f, (src, mut module)) in targets.iter().zip(parsed) {
        let name = f.display();
        spellings.apply(&mut module);
        let canonical = module.to_text();
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
                        "{name}: not canonical (run `{} fmt --write {path}`)",
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
            other => return Err(unknown_flag("compile", other)),
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
                    "compile needs --base, --module, --lock and --out, or a lox.toml project. \
                     Run `{} compile --help`.",
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
    let compiled = compile(&base_doc, &m, &mut lock, &opts).map_err(|e| match e {
        // Several errors: each on its own line, placed in the module.
        loxone_config::Error::Many(_) => placed_text(&place(&module, &module, e)).into(),
        e => AnyError::from(e),
    })?;
    std::fs::write(&out, compiled.to_bytes())?;
    lock.save(&lock_path)?;
    println!(
        "compiled {module} against {base} -> {out} ({} objects, NextObj {}); lock: {}",
        compiled.objects().len(),
        compiled.counters().next_obj,
        lock_path.display()
    );
    for slug in loxone_config::logic::unpaired_created(&compiled, &lock) {
        eprintln!(
            "note: `{slug}` is a Tree device the source created, not paired yet. Pair it with \
             a Tree search in Loxone Config; how the Miniserver runs a device it has not \
             searched for is untested."
        );
    }
    // Placement keeps to the sheet where it can (D50); say what it put
    // past it. An object that keeps its place in the base was put there in
    // Loxone Config, so it is not the compile's to report.
    let slug_of: std::collections::BTreeMap<&str, &str> = lock
        .objects
        .iter()
        .map(|(slug, o)| (o.uuid.as_str(), slug.as_str()))
        .collect();
    let managed = slug_of.keys().copied().collect();
    let managed = loxone_config::layout::placed(&base_doc, &compiled, &managed);
    for (page, uuids) in loxone_config::layout::off_sheet(&compiled, &managed) {
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
    let mut trace = false;

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
            "--trace" => trace = true,
            dir if !dir.starts_with('-') && project_dir.is_none() => {
                project_dir = Some(PathBuf::from(dir));
            }
            other => return Err(unknown_flag("test", other)),
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
                    "test needs --base, --module and --lock, or a lox.toml project. \
                     Run `{} test --help`.",
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
    let mut plan = m.with_expanded(|x| {
        let (flat, _) = x.desugar()?;
        plan_tests(&flat, &lock_copy, &compiled, filter.as_deref())
    })?;
    plan.respell(&m);
    if plan.runs.is_empty() {
        match &filter {
            Some(f) => println!("no tests match --filter {f}"),
            None => println!("no tests in {module_path}"),
        }
        return Ok(ExitCode::SUCCESS);
    }
    if show_spec && lox_flag.is_none() {
        return Err("--show-spec prints the lox-cli scenario JSON; it needs --lox".into());
    }
    if trace && lox_flag.is_some() {
        return Err("--trace traces the built-in simulator; it does not work with --lox".into());
    }
    let lox_used = lox_flag.is_some();
    let results = match lox_flag {
        None if trace => trace_tests(&plan, &compiled, &lock_copy),
        None => run_tests(&plan, &compiled),
        Some(lox) => run_lox_sim(&lox, &plan, &compiled, show_spec)?,
    };
    if show_spec {
        return Ok(ExitCode::SUCCESS);
    }
    let mut failed = 0;
    for r in &results {
        if let Some(e) = &r.error {
            failed += 1;
            println!("test \"{}\" ... ERROR", r.name);
            println!("    {e}");
        } else if r.pass {
            println!("test \"{}\" ... ok", r.name);
        } else {
            failed += 1;
            println!("test \"{}\" ... FAILED", r.name);
            for line in &r.failures {
                println!("    {line}");
            }
        }
        if !r.trace.is_empty() {
            println!("    trace:");
            for line in &r.trace {
                println!("      {line}");
            }
        }
    }
    println!(
        "{} tests: {} passed, {failed} failed",
        results.len(),
        results.len() - failed
    );
    if failed > 0 && !trace && !lox_used {
        println!("(--trace shows each test cycle by cycle)");
    }
    Ok(if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Run the plan in lox-cli's `lox sim run` instead of the built-in
/// simulator (`loxl test --lox <path>`). With `show_spec` it only prints
/// the scenario JSON.
fn run_lox_sim(
    lox: &str,
    plan: &TestPlan,
    compiled: &LoxoneDoc,
    show_spec: bool,
) -> Result<Vec<TestResult>, AnyError> {
    let specs = lox_specs(plan, compiled)?;
    if show_spec {
        println!("{}", specs.json);
        return Ok(Vec::new());
    }
    let probe = std::process::Command::new(lox)
        .args(["sim", "--help"])
        .output();
    if !probe.map(|o| o.status.success()).unwrap_or(false) {
        return Err(format!(
            "`{lox}` is not a sim-capable lox-cli binary (github.com/eisber/lox-cli); \
             without --lox the built-in simulator runs the tests"
        )
        .into());
    }
    let tmp = std::env::temp_dir().join(format!("loxl-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let cfg_path = tmp.join("compiled.Loxone");
    let specs_path = tmp.join("specs.json");
    std::fs::write(&cfg_path, compiled.to_bytes())?;
    std::fs::write(&specs_path, &specs.json)?;
    let output = std::process::Command::new(lox)
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
    Ok(read_results(&stdout, plan, &specs)?)
}

fn cmd_rename(args: &[&str]) -> Result<ExitCode, AnyError> {
    let (old, new, dir) = match args {
        [old, new] => (*old, *new, "."),
        [old, new, dir] if !dir.starts_with('-') => (*old, *new, *dir),
        _ => return Err(usage_error("rename")),
    };
    let project_file = Project::find(Path::new(dir)).ok_or_else(|| {
        format!("no lox.toml in `{dir}` — rename needs a project (module, lock, base)")
    })?;
    let project = Project::load(&project_file)?;
    // Fragments load individually so each file can be rewritten in place.
    let files = if project.module.is_dir() {
        module_dir_files(&project.module).map_err(|(p, e)| format!("{p}: {e}"))?
    } else {
        vec![project.module.clone()]
    };
    let mut sources = Vec::new();
    for f in files {
        let src = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        sources.push((f, src));
    }
    let r = plan_rename(&project, &sources, old, new)?;
    for (path, text) in &r.files {
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    r.lock.save(&project.lock)?;
    std::fs::write(&project.out, &r.out)?;
    println!(
        "renamed `{old}` -> `{new}`: {} file(s) rewritten, \
         {} lock entr{} rekeyed, output {}",
        r.files.len(),
        r.moved,
        if r.moved == 1 { "y" } else { "ies" },
        if r.title_changes == 0 {
            "byte-identical".to_string()
        } else {
            format!(
                "changed in {} Title label(s) the slug feeds",
                r.title_changes
            )
        }
    );
    Ok(ExitCode::SUCCESS)
}

/// A rename `loxl rename` verified, not yet written.
pub struct Renamed {
    /// The fragments whose text changes, with their new text
    pub files: Vec<(PathBuf, String)>,
    /// The lockfile with the moved keys
    pub lock: Lockfile,
    /// The compiled output
    pub out: Vec<u8>,
    /// Lock entries rekeyed
    pub moved: usize,
    /// `Title=` labels the slug feeds
    pub title_changes: usize,
}

/// Rename the module-level name `old` to `new` in the module of `project`,
/// given as its fragment files and their text (`sources`, in module
/// order), with the lockfile entries to match. Verified before anything is
/// returned: the lock must be current, and the renamed module must compile
/// to the same output but for the `Title=` labels the slug feeds. Used by
/// `loxl rename` and the language server.
pub fn plan_rename(
    project: &Project,
    sources: &[(PathBuf, String)],
    old: &str,
    new: &str,
) -> Result<Renamed, AnyError> {
    valid_slug(new)?;
    let mut fragments: Vec<(PathBuf, String, Module)> = Vec::new();
    for (f, src) in sources {
        let m = Module::parse_fragment(src).map_err(|e| format!("{}: {e}", f.display()))?;
        fragments.push((f.clone(), src.clone(), m));
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

    let files = fragments
        .iter()
        .filter_map(|(path, src, m)| {
            let text = m.to_text();
            (*src != text).then(|| (path.clone(), text))
        })
        .collect();
    Ok(Renamed {
        files,
        lock: lock_new,
        out: out_new.to_bytes(),
        moved,
        title_changes,
    })
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
    let (mut fix, mut summary, mut path) = (false, false, None);
    for &a in args {
        match a {
            "--fix" => fix = true,
            "--summary" => summary = true,
            p if !p.starts_with('-') && path.is_none() => path = Some(p.to_string()),
            _ => return Err(usage_error("lint")),
        }
    }
    let path = path.unwrap_or_else(|| ".".to_string());
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
    let files: Vec<PathBuf> = if Path::new(&module_path).is_dir() {
        module_dir_files(Path::new(&module_path)).map_err(|(p, e)| format!("{p}: {e}"))?
    } else {
        vec![PathBuf::from(&module_path)]
    };
    // The project's base and lockfile, read once.
    let base = match &project {
        Some(prj) => Some((
            read_doc(&prj.base.display().to_string())?,
            Lockfile::load(&prj.lock)?,
        )),
        None => None,
    };
    let compile_project = |module: &Module| -> Result<Option<(LoxoneDoc, Lockfile)>, AnyError> {
        let (Some(prj), Some((base_doc, lock))) = (project.as_ref(), base.as_ref()) else {
            return Ok(None);
        };
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
        let mut lock = lock.clone();
        let compiled = compile(base_doc, module, &mut lock, &opts)?;
        Ok(Some((compiled, lock)))
    };

    if fix {
        let mut fixed = Vec::new();
        let (mut count, mut suffix_files) = (0, 0);
        let (mut zeros_written, mut zero_files) = (0, 0);
        let mut sources = Vec::new();
        // Existing blocks with ports at 0 that nothing sets: needs the
        // compiled config, so only in a project.
        let current = read_module(&module_path)?;
        let zeros = match compile_project(&current)
            .map_err(|e| format!("compile before the fix failed: {e}"))?
        {
            Some((compiled, _)) => {
                let lock = &base.as_ref().expect("a project has a lockfile").1;
                implicit_zeros(&current, lock, &compiled)?
            }
            None => Vec::new(),
        };
        let mut parsed = Vec::new();
        for f in &files {
            let src = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
            parsed.push(Module::parse_fragment(&src).map_err(|e| format!("{}: {e}", f.display()))?);
            sources.push(src);
        }
        // Externs and templates may live in another file of the module.
        let whole = Module::from_fragments(parsed);
        for (f, src) in files.iter().zip(&sources) {
            // Only the values change; the rest of the file stays as written.
            let (found, mut text) =
                fix_unit_suffixes_text(src, &whole).map_err(|e| format!("{}: {e}", f.display()))?;
            let n = found.len();
            let mut z = 0;
            if !zeros.is_empty() {
                let mut m =
                    Module::parse_fragment(&text).map_err(|e| format!("{}: {e}", f.display()))?;
                z = fix_implicit_zeros(&mut m, &zeros);
                if z > 0 {
                    text = m.to_text();
                }
            }
            if n + z > 0 {
                count += n;
                zeros_written += z;
                suffix_files += usize::from(n > 0);
                zero_files += usize::from(z > 0);
                fixed.push((f.clone(), text));
            }
        }
        if !fixed.is_empty() && project.is_some() {
            // The fix only respells values and states the 0s a compile
            // writes anyway: prove the compiled config and the lockfile
            // stay byte-identical before writing.
            let module = read_module(&module_path)?;
            let mut fragments = Vec::new();
            for f in &files {
                let src = match fixed.iter().find(|(p, _)| p == f) {
                    Some((_, text)) => text.clone(),
                    None => std::fs::read_to_string(f)?,
                };
                let mut m = Module::parse_fragment(&src)?;
                m.load_files(fragment_dir(f))?;
                fragments.push(m);
            }
            let fixed_module = if Path::new(&module_path).is_dir() {
                Module::from_fragments(fragments)
            } else {
                fragments.pop().unwrap_or_default()
            };
            let before = compile_project(&module)
                .map_err(|e| format!("compile before the fix failed: {e}"))?;
            let after = compile_project(&fixed_module)
                .map_err(|e| format!("compile after the fix failed: {e}"))?;
            let same = match (&before, &after) {
                (Some((d0, l0)), Some((d1, l1))) => {
                    d0.to_bytes() == d1.to_bytes() && l0.to_json() == l1.to_json()
                }
                _ => true,
            };
            if !same {
                return Err(
                    "the fixed module compiles differently, so nothing was written. \
                            Report this as a bug."
                        .into(),
                );
            }
        }
        for (f, text) in &fixed {
            std::fs::write(f, text).map_err(|e| format!("{}: {e}", f.display()))?;
        }
        let files_fixed = |n: usize| format!("{n} file{}", if n == 1 { "" } else { "s" });
        if count > 0 {
            println!(
                "fixed: {count} unit-suffix value{} in {}",
                if count == 1 { "" } else { "s" },
                files_fixed(suffix_files)
            );
        }
        if zeros_written > 0 {
            println!(
                "fixed: wrote {zeros_written} implicit 0{} in {}",
                if zeros_written == 1 { "" } else { "s" },
                files_fixed(zero_files)
            );
        }
    }

    let module = read_module(&module_path)?;
    let mut findings = lint_source(&module)?;
    let project = match compile_project(&module)
        .map_err(|e| format!("compile for dead-output analysis failed: {e}"))?
    {
        Some((compiled, _)) => {
            let lock = &base.as_ref().expect("a project has a lockfile").1;
            findings.extend(lint_dead_outputs(&module, lock, &compiled)?);
            findings.extend(lint_circuits(&module, lock, &compiled)?);
            findings.extend(lint_implicit_zero(&module, lock, &compiled)?);
            findings.extend(lint_tiles(&module, &compiled));
            findings.extend(lint_access(&compiled));
            true
        }
        None => {
            eprintln!("note: no lox.toml project — dead-output analysis skipped");
            false
        }
    };
    let findings = finish_lint(&module, findings, project)?;

    if findings.is_empty() {
        println!("lint: clean");
        return Ok(ExitCode::SUCCESS);
    }
    // Name the declaring file where the module is a directory of fragments.
    let locate: Vec<(PathBuf, Module)> = if Path::new(&module_path).is_dir() {
        files
            .iter()
            .filter_map(|f| {
                let src = std::fs::read_to_string(f).ok()?;
                Some((f.clone(), Module::parse_fragment(&src).ok()?))
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
                Item::Signal(x) => x.slug == finding.slug,
                Item::Defaults(d) => d.block_type == finding.slug,
                _ => false,
            });
            if declares {
                return format!("{}: ", f.display());
            }
        }
        String::new()
    };
    // Grouped by kind, in the order kinds first appear.
    let mut kinds: Vec<(LintKind, Vec<&LintFinding>)> = Vec::new();
    for f in &findings {
        match kinds.iter_mut().find(|(k, _)| *k == f.kind) {
            Some((_, list)) => list.push(f),
            None => kinds.push((f.kind, vec![f])),
        }
    }
    let width = kinds
        .iter()
        .map(|(k, _)| k.label().len())
        .max()
        .unwrap_or(0);
    for (kind, list) in &kinds {
        if summary {
            println!("{:width$}  {}", kind.label(), list.len());
            continue;
        }
        println!("{} ({})", kind.label(), list.len());
        for finding in list {
            println!(
                "  {}`{}` — {}",
                file_of(finding),
                finding.slug,
                finding.detail
            );
        }
    }
    let fixable = findings
        .iter()
        .filter(|f| {
            f.kind == LintKind::UnitSuffix
                || (f.kind == LintKind::ImplicitZero && f.detail.contains("lint --fix"))
        })
        .count();
    println!(
        "lint: {} finding{}{}",
        findings.len(),
        if findings.len() == 1 { "" } else { "s" },
        if fixable > 0 {
            format!(" ({fixable} fixable with `loxl lint --fix`)")
        } else {
            String::new()
        }
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
                return Err(unknown_flag("decompile", flag));
            }
            p if path.is_none() => path = Some(p),
            _ => return Err(usage_error("decompile")),
        }
    }
    let Some(path) = path else {
        return Err(usage_error("decompile"));
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
            let file = dir.join(format!("{}.loxl", p.slug));
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
                return Err(unknown_flag("adopt", flag));
            }
            p if path.is_none() => path = Some(p),
            _ => return Err(usage_error("adopt")),
        }
    }
    if uuid.is_some() || as_slug.is_some() || module_path.is_some() || lock_path.is_some() {
        let (Some(path), Some(uuid), Some(slug), Some(module_path), Some(lock_path)) =
            (path, uuid, as_slug, module_path, lock_path)
        else {
            return Err(usage_error("adopt"));
        };
        if out_module.is_some() || out_dir.is_some() || out_lock.is_some() {
            return Err(usage_error("adopt"));
        }
        return cmd_adopt_one(path, uuid, slug, module_path, lock_path);
    }
    let (Some(path), Some(out_lock)) = (path, out_lock) else {
        return Err(usage_error("adopt"));
    };
    if out_module.is_some() == out_dir.is_some() {
        return Err(usage_error("adopt"));
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
            std::fs::write(dir.join(format!("{stem}.loxl")), fragment.to_text())?;
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

    // Every fragment's new text, verified as it will be written: the block
    // goes into its page's file — under that file's `page` statement — and
    // a promoted extern's statements, which are the block's arguments now,
    // leave every fragment that holds them.
    let is_dir = Path::new(module_path).is_dir();
    let target = if is_dir {
        Path::new(module_path).join(format!("{}.loxl", slugify(&adopted.page_title)))
    } else {
        PathBuf::from(module_path)
    };
    let mut files: Vec<(PathBuf, String, String)> = Vec::new();
    let paths = if is_dir {
        module_dir_files(Path::new(module_path)).map_err(|(p, e)| format!("{p}: {e}"))?
    } else {
        vec![PathBuf::from(module_path)]
    };
    for f in paths {
        let src = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let mut text = src.clone();
        if !adopted.promoted.is_empty() {
            let mut m =
                Module::parse_fragment(&src).map_err(|e| format!("{}: {e}", f.display()))?;
            let mut stripped = false;
            for x in &adopted.promoted {
                stripped |= m.strip_extern(x);
            }
            if stripped {
                text = m.to_text();
            }
        }
        files.push((f, src, text));
    }
    if !files.iter().any(|(f, _, _)| *f == target) {
        // A fresh page file opens with its `page` statement (D28), so the
        // adopted blocks stay pinned to the page they were drawn on; a
        // device's file (D64) needs none — its blocks find their place in
        // the periphery tree by `device:` and type.
        let text = if adopted.on_page {
            Module {
                items: vec![loxone_config::logic::Item::Page(
                    loxone_config::logic::PageDecl {
                        title: adopted.page_title.clone(),
                        room: None,
                        category: None,
                        comment: None,
                    },
                )],
            }
            .to_text()
        } else {
            String::new()
        };
        files.push((target.clone(), String::new(), text));
        files.sort_by(|a, b| a.0.cmp(&b.0));
    }
    let (_, _, text) = files
        .iter_mut()
        .find(|(f, _, _)| *f == target)
        .expect("target file listed above");
    if !text.is_empty() {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
    }
    text.push_str(
        &Module {
            items: adopted.items.clone(),
        }
        .to_text(),
    );
    // In the form `loxl fmt` writes, so the file stays canonical.
    *text = Module::parse_fragment(text)
        .map_err(|e| format!("{}: {e}", target.display()))?
        .to_text();

    let mut fragments = Vec::new();
    for (f, _, text) in &files {
        let mut m = Module::parse_fragment(text).map_err(|e| format!("{}: {e}", f.display()))?;
        m.load_files(fragment_dir(f))
            .map_err(|e| format!("{}: {e}", f.display()))?;
        fragments.push(m);
    }
    let merged = Module::from_fragments(fragments);
    // The checks `loxl check` runs: references resolve in the expanded,
    // desugared module (template instances and expressions included).
    let (desugared, _) = merged.expand()?.desugar()?;
    desugared.validate()?;
    validate_ports(&desugared)?;
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
    let mut vlock = lock.clone();
    let out = compile(&doc, &merged, &mut vlock, &vopts)?;
    // The block's wires onto externs are recorded by a compile; take them
    // from this one so the next compile leaves the lock as written.
    lock.extern_wires = vlock.extern_wires;
    let mut d = loxone_config::diff::diff(&doc, &out);
    // The AutoPilot container's inputs follow the rules (a compile keeps
    // them as Loxone Config saves them), whatever the block adopted.
    let watched = autopilot_input(&doc);
    d.wires_added
        .retain(|w| Some(&w.to_port) != watched.as_ref());
    d.wires_removed
        .retain(|w| Some(&w.to_port) != watched.as_ref());
    if !d.is_empty() {
        return Err(format!(
            "adoption verification failed — rebuilding with the updated module/lock \
             is not a semantic no-op (nothing was written):\n{d:#?}"
        )
        .into());
    }

    let mut rewritten = Vec::new();
    for (f, src, text) in &files {
        if src != text {
            std::fs::write(f, text).map_err(|e| format!("{}: {e}", f.display()))?;
            if *f != target {
                rewritten.push(f.display().to_string());
            }
        }
    }
    lock.save(Path::new(lock_path))?;

    if !adopted.promoted.is_empty() {
        println!(
            "promoted extern{} `{}` to managed: the statements moved into \
             the declaration{}",
            if adopted.promoted.len() == 1 { "" } else { "s" },
            adopted.promoted.join("`, `"),
            if rewritten.is_empty() {
                String::new()
            } else {
                format!(" (also rewritten: {})", rewritten.join(", "))
            }
        );
    }
    println!(
        "adopted uuid {uuid} as `{slug}` {} \"{}\"{}",
        if adopted.on_page {
            "on page"
        } else {
            "in folder"
        },
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
    let (flags, files): (Vec<&str>, Vec<&str>) = args.iter().partition(|a| a.starts_with("--"));
    let (exit_code, json, summary) = (
        flags.contains(&"--exit-code"),
        flags.contains(&"--json"),
        flags.contains(&"--summary"),
    );
    let (old, new) = match (
        files.as_slice(),
        flags
            .iter()
            .all(|f| ["--exit-code", "--json", "--summary"].contains(f))
            && !(json && summary),
    ) {
        ([old, new], true) => (*old, *new),
        _ => return Err(usage_error("diff")),
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
    let d = loxone_config::diff::diff(&a, &b);
    if json || summary {
        if json {
            println!("{}", serde_json::to_string_pretty(&d)?);
        } else {
            print!("{}", loxone_config::diff::summary(&a, &b, &d));
        }
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
            && let (Some(old), Some(new)) = (
                loxone_config::autopilot::parse(from),
                loxone_config::autopilot::parse(to),
            )
        {
            let lines = loxone_config::autopilot::changes(&old, &new);
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
        if loxone_config::connectors::file_params(&p.block_type).contains(&p.port_key.as_str()) {
            println!(
                "* {} {}.{} program changed:",
                p.block_type, p.object_uuid, p.port_key
            );
            let (from, to) = (p.from.as_deref(), p.to.as_deref());
            for line in loxone_config::diff::text_changes(from.unwrap_or(""), to.unwrap_or("")) {
                println!("    {line}");
            }
            continue;
        }
        // A tile function (D41) is already a readable `name="value"` list,
        // a schedule column (D53) a list of intervals.
        if p.port_key.starts_with("tile ") || p.port_key.starts_with("schedule ") {
            let none = if p.port_key.starts_with("tile ") {
                "(empty)"
            } else {
                "(no column)"
            };
            let side = |v: &Option<String>| v.clone().unwrap_or_else(|| none.into());
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
            loxone_config::access::Change::Added | loxone_config::access::Change::Removed => {
                println!("{head}")
            }
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

fn cmd_coverage(args: &[&str]) -> Result<ExitCode, AnyError> {
    let json = args.contains(&"--json");
    let rest: Vec<&str> = args.iter().copied().filter(|a| *a != "--json").collect();
    let (config, lock_path) = match rest.as_slice() {
        [] => (None, None),
        [p] if Path::new(p).is_dir() => (None, Some(*p)),
        [config] if !config.starts_with('-') => (Some(config.to_string()), None),
        [config, "--lock", lock] | ["--lock", lock, config] => {
            (Some(config.to_string()), Some(*lock))
        }
        _ => return Err(usage_error("coverage")),
    };
    let (config, lock, module) = match (config, lock_path) {
        (Some(c), Some(l)) => (c, Lockfile::load(Path::new(l))?, None),
        // A bare config: nothing is source yet — the report is what an
        // adoption could take over.
        (Some(c), None) => (c, Lockfile::new(), None),
        (None, dir) => {
            let dir = dir.unwrap_or(".");
            let f = Project::find(Path::new(dir))
                .ok_or_else(|| format!("no lox.toml in `{dir}`. {}", usage_error("coverage")))?;
            let p = Project::load(&f)?;
            // The module tells a claimed attribute from a carried one.
            let module = read_module(&p.module.display().to_string())?;
            (
                p.base.display().to_string(),
                Lockfile::load(&p.lock)?,
                Some(module),
            )
        }
    };
    let report = loxone_config::coverage::coverage(&read_doc(&config)?, &lock, module.as_ref());
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", report.to_text());
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_drift(args: &[&str]) -> Result<ExitCode, AnyError> {
    let (config, lock_path) = match args {
        [config, "--lock", lock] | ["--lock", lock, config] => (*config, lock.to_string()),
        [config] => {
            let Some(f) = Project::find(Path::new(".")) else {
                return Err(format!(
                    "no --lock given and no lox.toml in the current directory. \
                     Pass --lock <lock.json>, or run `{} drift` in a project.",
                    prog()
                )
                .into());
            };
            (*config, Project::load(&f)?.lock.display().to_string())
        }
        _ => return Err(usage_error("drift")),
    };
    let lock_path = lock_path.as_str();
    let doc = read_doc(config)?;
    let lock = Lockfile::load(Path::new(lock_path))?;
    let Some(recorded) = &lock.target.semantic_fingerprint else {
        return Err(format!(
            "{lock_path} has no record to compare against. Run adopt or compile once to create it."
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
             {} attribute(s) ({}{}). Loxone reads them as they are, so their line \
             breaks are lost. `{p} repair` restores them.",
            refs.len(),
            shown.join(", "),
            if more > 0 {
                format!(", +{more} more")
            } else {
                String::new()
            },
            p = prog()
        );
    }
    let current = loxone_config::diff::semantic_fingerprint(&doc);
    if &current == recorded {
        println!("in sync: {config} matches {lock_path}");
        return Ok(ExitCode::SUCCESS);
    }
    // The config the last compile started from: nothing changed it since,
    // its output is only not pushed yet.
    if lock.target.source_config_sha256.as_deref()
        == Some(loxone_config::lock::sha256_hex(&doc.to_bytes()).as_str())
    {
        println!(
            "in sync: {config} is the config the last compile of {lock_path} started from. \
             Its output is not pushed yet."
        );
        return Ok(ExitCode::SUCCESS);
    }
    // A lock written by an older Lox Logic records an older form, blind
    // to what later joined the comparison. A match in that form is in
    // sync as far as it can tell, and the next compile records the
    // current one.
    use loxone_config::diff as d;
    type Form = fn(&LoxoneDoc) -> String;
    let older: [(Form, &str); 12] = [
        (
            d::semantic_fingerprint_v12,
            "central block members and energy flow units",
        ),
        (d::semantic_fingerprint_v11, "app names and block settings"),
        (
            d::semantic_fingerprint_v10,
            "extension settings and statistics",
        ),
        (d::semantic_fingerprint_v9, "PulseAt fire times"),
        (d::semantic_fingerprint_v8, "Air and Tree device settings"),
        (d::semantic_fingerprint_v7, "device settings"),
        (d::semantic_fingerprint_v6, "light moods"),
        (d::semantic_fingerprint_v5, "DayTimer schedules"),
        (d::semantic_fingerprint_v4, "notification messages"),
        (d::semantic_fingerprint_v3, "Touch Pure Flex tile programs"),
        (d::semantic_fingerprint_v2, "program code"),
        (d::semantic_fingerprint_v1, "room and category bindings"),
    ];
    if let Some((_, blind_to)) = older.iter().find(|(form, _)| form(&doc) == *recorded) {
        println!("{}", in_sync_older(config, lock_path, blind_to));
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
        // after Lox Logic (docs/loxone-format.md names the known producer).
        let dups = doc.duplicate_uuids();
        if !dups.is_empty() {
            println!(
                "note: {config} has {} duplicate UUID(s). Lox Logic never writes \
                 them, so another tool changed the file.",
                dups.len()
            );
        }
        if pending.is_empty() {
            println!(
                "drift: {config} changed since the last adopt or compile ({lock_path}). \
                 Run `{p} diff <last-compiled.Loxone> {config}` to see what changed.",
                p = prog()
            );
        } else {
            println!(
                "drift: {config} still has {}, removed by the last compile ({lock_path}). \
                 Push the compiled config, then download it again. \
                 `{p} diff <last-compiled.Loxone> {config}` shows any other change.",
                pending.join(", "),
                p = prog()
            );
        }
        Ok(ExitCode::FAILURE)
    }
}

/// The `drift` result for a lock whose record has an older form, which
/// does not compare `blind_to`.
fn in_sync_older(config: &str, lock_path: &str, blind_to: &str) -> String {
    format!(
        "in sync: {config} matches {lock_path}, which an older version of Lox Logic wrote. \
         It does not compare {blind_to}. The next compile updates it."
    )
}

/// `loxl types`: the builtin table with its documented port facts — the
/// reference an author (or agent) writes a block against.
fn cmd_templates(name: Option<&str>) -> Result<ExitCode, AnyError> {
    use loxone_config::library;
    let Some(name) = name else {
        let mut file = "";
        for e in library::entries() {
            if e.file != file {
                println!("{}{}.loxl", if file.is_empty() { "" } else { "\n" }, e.file);
                file = e.file;
            }
            println!("  {}({})", e.name, e.params);
            println!("      {}", e.description);
        }
        println!(
            "\n`{} templates <file | template>` prints a file to copy into the module.",
            prog()
        );
        return Ok(ExitCode::SUCCESS);
    };
    let Some((_, source)) = library::source(name) else {
        let names: Vec<String> = library::FILES
            .iter()
            .map(|(f, _)| f.to_string())
            .chain(library::entries().into_iter().map(|e| e.name))
            .collect();
        return Err(format!("no template or file `{name}` (known: {})", names.join(", ")).into());
    };
    print!("{source}");
    Ok(ExitCode::SUCCESS)
}

fn cmd_types(args: &[&str]) -> Result<ExitCode, AnyError> {
    use loxone_config::connectors;
    let json = args.contains(&"--json");
    let mut names: Vec<&str> = args.iter().copied().filter(|a| *a != "--json").collect();
    // `--search <text>`: the types whose name or purpose mentions it.
    let search = match names.iter().position(|a| *a == "--search") {
        Some(i) if i + 1 < names.len() => {
            let q = names.remove(i + 1);
            names.remove(i);
            Some(q)
        }
        Some(_) => return Err(usage_error("types")),
        None => None,
    };
    let dir = |d: connectors::PortDir| match d {
        connectors::PortDir::Input => "input",
        connectors::PortDir::Output => "output",
        connectors::PortDir::Param => "param",
        connectors::PortDir::Api => "api",
    };
    let describe = |t: &str| -> Option<serde_json::Value> {
        let ports: Vec<serde_json::Value> = connectors::builtin(t)?
            .iter()
            .map(|spec| {
                let meta = loxone_config::port_meta::port_meta(t, spec.key);
                serde_json::json!({
                    "key": spec.key,
                    "dir": dir(spec.dir),
                    "unit": meta.and_then(|m| m.unit),
                    "label": meta.and_then(|m| m.label),
                    "min": meta.and_then(|m| m.min),
                    "max": meta.and_then(|m| m.max),
                    "gui_default": loxone_config::gui_defaults::gui_default(t, spec.key),
                    "documented_default": meta.and_then(|m| m.default),
                    "observed_default": loxone_config::observed_defaults::observed_default(t, spec.key),
                    "description": loxone_config::port_doc::describe(t, spec.key),
                })
            })
            .collect();
        Some(serde_json::json!({
            "type": t,
            "summary": loxone_config::blockdoc::summary(t),
            "docs": loxone_config::blockdoc::kb_url(t),
            "ports": ports,
            "attr_params": connectors::attr_params(t),
        }))
    };
    match names.as_slice() {
        [] => {
            let types: Vec<&str> = match search {
                Some(q) => loxone_config::blockdoc::search(q),
                None => connectors::BUILTIN_TYPES.to_vec(),
            };
            if json {
                let all: Vec<_> = types.iter().filter_map(|t| describe(t)).collect();
                println!("{}", serde_json::to_string_pretty(&all)?);
            } else {
                for t in &types {
                    let ports = connectors::builtin(t).unwrap_or_default();
                    let count = |d| ports.iter().filter(|p| p.dir == d).count();
                    println!(
                        "{t:<24} {:>2} in  {:>2} out  {:>2} param",
                        count(connectors::PortDir::Input) + count(connectors::PortDir::Api),
                        count(connectors::PortDir::Output),
                        count(connectors::PortDir::Param),
                    );
                    if let Some(s) = loxone_config::blockdoc::summary(t) {
                        println!("    {s}");
                    }
                }
                if types.is_empty() {
                    eprintln!("no type mentions `{}`", search.unwrap_or_default());
                    return Ok(ExitCode::FAILURE);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        [t] => {
            let Some(desc) = describe(t) else {
                let hint = connectors::BUILTIN_TYPES
                    .iter()
                    .find(|b| b.eq_ignore_ascii_case(t))
                    .map(|b| format!(" — did you mean `{b}`?"))
                    .unwrap_or_default();
                return Err(format!(
                    "`{t}` is not a builtin type{hint}. `{} types` lists them.",
                    prog()
                )
                .into());
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&desc)?);
                return Ok(ExitCode::SUCCESS);
            }
            println!("{t}");
            if let Some(summary) = loxone_config::blockdoc::summary(t) {
                println!("  {summary}");
            }
            if let Some(url) = loxone_config::blockdoc::kb_url(t) {
                println!("  {url}");
            }
            let s = |v: &serde_json::Value| v.as_str().unwrap_or("").to_string();
            for port in desc["ports"].as_array().into_iter().flatten() {
                let range = match (port["min"].as_str(), port["max"].as_str()) {
                    (None, None) => String::new(),
                    (lo, hi) => format!("{} … {}", lo.unwrap_or("−∞"), hi.unwrap_or("∞")),
                };
                // What a new block gets: the GUI default, else the
                // documented one.
                let mut default = port["gui_default"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| s(&port["documented_default"]));
                if let Some(o) = port["observed_default"].as_str()
                    && o != default
                {
                    default = if default.is_empty() {
                        format!("{o} (observed)")
                    } else {
                        format!("{default} (observed {o})")
                    };
                }
                let line = format!(
                    "  {:<18} {:<6} {:<5} {:<7} {:<14} {}",
                    s(&port["key"]),
                    s(&port["dir"]),
                    s(&port["unit"]),
                    s(&port["label"]),
                    range,
                    default,
                );
                println!("{}", line.trim_end());
                if let Some(d) = port["description"].as_str() {
                    println!("      {d}");
                }
            }
            for a in desc["attr_params"].as_array().into_iter().flatten() {
                let a = s(a);
                if connectors::clock_params(t).contains(&a.as_str()) {
                    // `At` on PulseAt: a time of day, stored as seconds
                    // since midnight in `Sec=`.
                    println!(
                        "  {a:<18} attr   time of day (HH:MM[:SS]) → {}=",
                        connectors::attr_name(t, &a)
                    );
                } else if let Some(c) = connectors::choice_values(t, &a) {
                    println!("  {a:<18} attr   {}", connectors::choice_list(c));
                } else if connectors::bool_params(t).contains(&a.as_str()) {
                    println!("  {a:<18} attr   on or off");
                } else if connectors::text_params(t).contains(&a.as_str()) {
                    println!("  {a:<18} attr   text");
                } else {
                    println!("  {a:<18} attr");
                }
            }
            if !loxone_config::gui_defaults::gui_defaults(t).is_empty() {
                println!(
                    "\n  A new block gets the default for each port the source leaves out, as \
                     Loxone Config places it.\n  A block that already exists has 0 there \
                     unless it has a value (`{} lint` reports it as implicit-zero).",
                    prog()
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(usage_error("types")),
    }
}

fn cmd_observe(args: &[&str]) -> Result<ExitCode, AnyError> {
    use loxone_config::connectors::{self, LegacyDb, Observations};

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
        return Err(usage_error("observe"));
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
        let json: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("{lp}: {e}"))?;
        let legacy: LegacyDb =
            connectors::legacy_db_from_json(&json, &obs).map_err(|e| format!("{lp}: {e}"))?;
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

fn cmd_scan(args: &[&str]) -> Result<ExitCode, AnyError> {
    if args.contains(&"--redact") {
        return cmd_scan_redact(args);
    }
    let json = args.contains(&"--json");
    let paths: Vec<&str> = args.iter().copied().filter(|a| *a != "--json").collect();
    if paths.is_empty() || paths.iter().any(|p| p.starts_with("--")) {
        return Err(usage_error("scan"));
    }
    let mut files = Vec::new();
    for p in &paths {
        collect_configs(std::path::Path::new(p), &mut files).map_err(|e| format!("{p}: {e}"))?;
    }
    let mut report = serde_json::Map::new();
    let (mut total, mut unreadable) = (0, 0);
    for f in &files {
        let name = f.display().to_string();
        // An unreadable file cannot be cleared: report it, scan the rest.
        let doc = match std::fs::read(f)
            .map_err(AnyError::from)
            .and_then(|b| LoxoneDoc::parse(&b).map_err(AnyError::from))
        {
            Ok(doc) => doc,
            Err(e) => {
                eprintln!("{name}: not scanned: {e}");
                unreadable += 1;
                continue;
            }
        };
        let findings = loxone_config::scan::scan(&doc);
        total += findings.len();
        if !json {
            for x in &findings {
                println!(
                    "{name}: {}: {} {} ({} chars)",
                    x.rule, x.object, x.attr, x.len
                );
            }
        }
        report.insert(name, serde_json::to_value(&findings)?);
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "{total} finding(s) in {} config(s){}{}",
            files.len() - unreadable,
            if unreadable > 0 {
                format!(", {unreadable} not scanned")
            } else {
                String::new()
            },
            if total > 0 {
                " — personal data: keep these files out of the repository"
            } else {
                ""
            }
        );
    }
    Ok(if total + unreadable > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// `loxl scan --redact <cfg> --out <copy>`: a shareable copy, every
/// finding replaced by a placeholder.
fn cmd_scan_redact(args: &[&str]) -> Result<ExitCode, AnyError> {
    let mut input = None;
    let mut out = None;
    let mut it = args.iter().copied();
    while let Some(a) = it.next() {
        match a {
            "--redact" => {}
            "--out" => out = Some(it.next().ok_or_else(|| usage_error("scan"))?),
            a if a.starts_with("--") => return Err(usage_error("scan")),
            a if input.is_none() => input = Some(a),
            _ => return Err(usage_error("scan")),
        }
    }
    let (Some(input), Some(out)) = (input, out) else {
        return Err(usage_error("scan"));
    };
    if std::path::Path::new(input) == std::path::Path::new(out) {
        return Err("--out must name another file than the config".into());
    }
    let mut doc = LoxoneDoc::parse(&std::fs::read(input).map_err(|e| format!("{input}: {e}"))?)?;
    let n = loxone_config::scan::redact(&mut doc);
    std::fs::write(out, doc.to_bytes()).map_err(|e| format!("{out}: {e}"))?;
    let left = loxone_config::scan::scan(&doc).len();
    println!("{out}: {n} value(s) redacted, {left} finding(s) left");
    Ok(if left > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// `path` itself if it is a file, else every `*.Loxone` below it.
fn collect_configs(
    path: &std::path::Path,
    out: &mut Vec<std::path::PathBuf>,
) -> std::io::Result<()> {
    if !path.is_dir() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(path)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<_>>()?;
    entries.sort();
    for e in entries {
        if e.is_dir() {
            collect_configs(&e, out)?;
        } else if e.extension().is_some_and(|x| x == "Loxone") {
            out.push(e);
        }
    }
    Ok(())
}

fn cmd_repair(args: &[&str]) -> Result<ExitCode, AnyError> {
    let (config, out) = match args {
        [config, "--out", out] | ["--out", out, config] => (*config, *out),
        _ => return Err(usage_error("repair")),
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
            other => return Err(unknown_flag("render", other)),
        }
    }
    let (Some(config), Some(page)) = (config, page) else {
        return Err("render requires <cfg.Loxone> and --page <title>".into());
    };
    let doc = read_doc(&config)?;
    let uuids = loxone_config::layout::PageGeometry::pages_titled(&doc, &page);
    let [uuid] = uuids.as_slice() else {
        return Err(format!("{} pages titled {page:?} in {config}", uuids.len()).into());
    };
    let geo = loxone_config::layout::PageGeometry::of(&doc, uuid).expect("page found by title");
    let managed: std::collections::BTreeSet<String> = match &lock_path {
        Some(p) => Lockfile::load(Path::new(p))?
            .objects
            .values()
            .map(|o| o.uuid.clone())
            .collect(),
        None => Default::default(),
    };
    print!("{}", loxone_config::render::metrics(&geo));
    if let Some(out) = out {
        std::fs::write(&out, loxone_config::render::svg(&geo, &managed))?;
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

/// The `Input` connector of the config's AutoPilot container.
fn autopilot_input(doc: &LoxoneDoc) -> Option<String> {
    doc.objects()
        .iter()
        .filter(|o| o.block_type == "AutoPilot")
        .find_map(|o| {
            doc.element_at(&o.path)?
                .child_elements()
                .find(|c| c.name == "Co" && c.attr("K") == Some("Input"))?
                .attr("U")
                .map(str::to_string)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_has_help_in_both_forms() {
        for c in COMMANDS {
            assert_eq!(run(&[c.name, "--help"]).unwrap(), ExitCode::SUCCESS);
            assert_eq!(run(&["help", c.name]).unwrap(), ExitCode::SUCCESS);
            assert!(
                c.usage[0].starts_with(&format!("loxl {} ", c.name))
                    || c.usage[0] == format!("loxl {}", c.name)
            );
            let help = command_help(c);
            for line in help.lines() {
                assert!(
                    line.chars().count() <= 100,
                    "{}: line too long: {line}",
                    c.name
                );
            }
        }
        assert_eq!(run(&["help", "nope"]).unwrap(), ExitCode::from(2));
    }

    #[test]
    fn help_text_has_no_decision_ids() {
        let all: String = COMMANDS
            .iter()
            .map(|c| format!("{}{}{}", c.usage.join("\n"), c.summary, c.details))
            .collect::<String>()
            + PROJECT_HELP;
        let ids = all
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| {
                w.len() > 1 && w.starts_with('D') && w[1..].chars().all(|c| c.is_ascii_digit())
            })
            .collect::<Vec<_>>();
        assert!(ids.is_empty(), "decision IDs in help: {ids:?}");
    }

    #[test]
    fn spell_keeps_file_names() {
        assert_eq!(
            spell_as(
                "loxl check m.loxl, `loxl fmt` and _periphery.loxl x",
                "lox logic"
            ),
            "lox logic check m.loxl, `lox logic fmt` and _periphery.loxl x"
        );
    }

    #[test]
    fn drift_names_what_an_older_lock_does_not_compare() {
        assert_eq!(
            in_sync_older("live.Loxone", "logic.lock.json", "light moods"),
            "in sync: live.Loxone matches logic.lock.json, which an older version of Lox Logic \
             wrote. It does not compare light moods. The next compile updates it."
        );
    }
}
