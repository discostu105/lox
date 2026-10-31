//! Program code from source (D40): `Code: file("…")` on a Code16 claims
//! the PicoC program text — loaded relative to the `.loxl` file, written
//! in Loxone's native encoding (raw newlines, never `&#xA;`), carried
//! verbatim while unclaimed, never lifted by decompile — and the diff,
//! fingerprint and drift scan see program text the way Loxone reads it.

use loxone_config::logic::{
    CompileOptions, DecompileOptions, DecompileScope, Module, compile, decompile,
};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

/// CRLF on purpose: an editor's line endings must not reach the config.
const PROGRAM: &str = "// Hue\r\nint a = 1;\r\nif (a < 2 && a > 0) printf(\"it's on\");\r\n";

fn base() -> LoxoneDoc {
    LoxoneDoc::parse(&std::fs::read("examples/configs/pool.Loxone").unwrap()).unwrap()
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: Some("Pool".into()),
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

/// A scratch directory holding `prog.c`; the module is parsed as if read
/// from `<dir>/pool.loxl`.
fn scratch(name: &str, program: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("prog.c"), program).unwrap();
    dir
}

fn module(src: &str, dir: &std::path::Path) -> Module {
    let mut m = Module::parse(src).unwrap();
    m.load_files(dir).unwrap();
    m
}

/// The raw (still escaped) `Code=` of the block titled `Prog`.
fn raw_code(doc: &LoxoneDoc) -> Option<String> {
    let o = doc
        .objects()
        .into_iter()
        .find(|o| o.title.as_deref() == Some("Prog"))?;
    doc.element_at(&o.path)?.attr("Code").map(str::to_string)
}

const CLAIMED: &str = "prog = Code16(\"Prog\", Code: file(\"prog.c\"))\n";
const UNCLAIMED: &str = "prog = Code16(\"Prog\")\n";

#[test]
fn file_binding_is_canonical_and_loads_next_to_its_loxl_file() {
    let dir = scratch("program-canonical", PROGRAM);
    let src = "prog = Code16(\n\t\"Prog\",\n\tCode: file(\"prog.c\"), # PicoC\n)\n";
    let m = Module::parse(src).unwrap();
    assert_eq!(m.to_text(), src, "file(…) prints back as written");

    // Unloaded, the value has no text — compile says how to load it.
    let err = compile(&base(), &m, &mut Lockfile::new(), &opts()).unwrap_err();
    assert!(err.to_string().contains("was not loaded"), "{err}");

    let m = module(src, &dir);
    let out = compile(&base(), &m, &mut Lockfile::new(), &opts()).unwrap();
    let code = raw_code(&out).unwrap();
    // Loxone's own encoding: bare LF line breaks, the five entities.
    assert_eq!(
        code,
        "// Hue\nint a = 1;\nif (a &lt; 2 &amp;&amp; a &gt; 0) \
         printf(&quot;it&apos;s on&quot;);\n"
    );
    assert!(!code.contains("&#"));

    let missing = Module::parse("prog = Code16(\"Prog\", Code: file(\"nope.c\"))\n")
        .unwrap()
        .load_files(&dir)
        .unwrap_err();
    assert!(missing.to_string().contains("nope.c"), "{missing}");
}

#[test]
fn file_binds_only_program_text() {
    let dir = scratch("program-refusals", PROGRAM);
    for src in [
        "f = Formula(\"F\", Formula: file(\"prog.c\"))\n",
        "m = Monoflop(\"M\", On: file(\"prog.c\"))\n",
    ] {
        let err = compile(&base(), &module(src, &dir), &mut Lockfile::new(), &opts()).unwrap_err();
        assert!(
            err.to_string().contains("only program text"),
            "{src}: {err}"
        );
    }
    for (src, want) in [
        ("p = Code16(Code: file())\n", "file(\"<path>\")"),
        ("p = Code16(Code: file(\"\"))\n", "names no file"),
        ("p = Code16(Code: file(prog))\n", "file(\"<path>\")"),
    ] {
        let err = Module::parse(src).unwrap_err();
        assert!(err.to_string().contains(want), "{src}: {err}");
    }
    // Program text is an attribute, not a connector: never a wire.
    let err = compile(
        &base(),
        &Module::parse("a = And(\"A\")\np = Code16(\"Prog\", Code: a.Q)\n").unwrap(),
        &mut Lockfile::new(),
        &opts(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("never a wire"), "{err}");
}

#[test]
fn unclaimed_code_is_carried_and_a_claim_replaces_it() {
    let dir = scratch("program-claims", PROGRAM);
    let mut lock = Lockfile::new();
    let first = compile(&base(), &module(CLAIMED, &dir), &mut lock, &opts()).unwrap();
    let native = raw_code(&first).unwrap();

    // Simulate the foreign writer + GUI round trip the house config went
    // through: every line break became the literal text `&#xA;`.
    let mut damaged = first.clone();
    let o = damaged
        .objects()
        .into_iter()
        .find(|o| o.title.as_deref() == Some("Prog"))
        .unwrap();
    let collapsed = native.replace('\n', "&amp;#xA;");
    damaged
        .element_at_mut(&o.path)
        .unwrap()
        .set_attr_raw("Code", &collapsed);
    assert_eq!(
        damaged.char_ref_attrs(),
        vec![("Prog".to_string(), "Code".to_string())]
    );
    assert!(first.char_ref_attrs().is_empty());

    // Withdrawn claim: the GUI owns the program again — carried verbatim,
    // damage included.
    let carried = compile(&damaged, &module(UNCLAIMED, &dir), &mut lock, &opts()).unwrap();
    assert_eq!(raw_code(&carried).unwrap(), collapsed);

    // Claimed again: the source text replaces whatever the base carries.
    let repaired = compile(&damaged, &module(CLAIMED, &dir), &mut lock, &opts()).unwrap();
    assert_eq!(raw_code(&repaired).unwrap(), native);
    assert_eq!(
        repaired.to_bytes(),
        first.to_bytes(),
        "re-claiming reproduces the original output"
    );

    // The diff reads `&#xA;` as Loxone does — text, not a line break — so
    // the damage is a program change, rendered as a line diff.
    let d = loxone_config::diff::diff(&first, &damaged);
    assert_eq!(d.param_changes.len(), 1);
    assert_eq!(d.param_changes[0].port_key, "Code");
    let lines = loxone_config::diff::text_changes(
        d.param_changes[0].from.as_deref().unwrap(),
        d.param_changes[0].to.as_deref().unwrap(),
    );
    assert_eq!(lines[0], "4 -> 1 lines; line 1 on: 4 removed, 1 added");
    assert!(lines[1].starts_with("- // Hue"), "{lines:?}");
    assert!(
        lines.last().unwrap().starts_with("+ // Hue&#xA;int a"),
        "{lines:?}"
    );
}

#[test]
fn decompile_leaves_the_program_with_the_gui() {
    let dir = scratch("program-decompile", PROGRAM);
    let out = compile(
        &base(),
        &module(CLAIMED, &dir),
        &mut Lockfile::new(),
        &opts(),
    )
    .unwrap();
    for scope in [DecompileScope::Full, DecompileScope::ManagedOnly] {
        let (m, _) = decompile(
            &out,
            &DecompileOptions {
                scope,
                ..Default::default()
            },
        )
        .unwrap();
        let text = m.to_text();
        assert!(text.contains("prog = Code16("), "{text}");
        assert!(!text.contains("Code:"), "{text}");
    }
}

#[test]
fn program_text_joins_the_fingerprint_in_v3_only() {
    let dir = scratch("program-fingerprint", PROGRAM);
    let with = compile(
        &base(),
        &module(CLAIMED, &dir),
        &mut Lockfile::new(),
        &opts(),
    )
    .unwrap();
    let edited_dir = scratch("program-fingerprint-edit", "// Hue\nint a = 2;\n");
    let edited = compile(
        &base(),
        &module(CLAIMED, &edited_dir),
        &mut Lockfile::new(),
        &opts(),
    )
    .unwrap();
    use loxone_config::diff::{semantic_fingerprint, semantic_fingerprint_v2};
    assert_ne!(semantic_fingerprint(&with), semantic_fingerprint(&edited));
    // An older lock's v2 fingerprint is blind to the program, so an
    // upgrade alone never reads as drift.
    assert_eq!(
        semantic_fingerprint_v2(&with),
        semantic_fingerprint_v2(&edited)
    );
}
