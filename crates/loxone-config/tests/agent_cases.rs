//! The agent cases (`tools/agent`): the fixture compiles, and each case's
//! hidden tests fail on the bare fixture and pass with its reference
//! solution. A case whose tests a bare fixture passes grades nothing.

use loxone_config::logic::{CompileOptions, Module, compile, plan_tests, run_tests};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

const AGENT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tools/agent");

fn opts(mint_time_unix: i64) -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix,
        page_title: None,
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    }
}

fn fixture() -> LoxoneDoc {
    let base = LoxoneDoc::parse(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/configs/rohbau.Loxone"
        ))
        .unwrap(),
    )
    .unwrap();
    let source = std::fs::read_to_string(format!("{AGENT}/fixture.loxl")).unwrap();
    let module = Module::parse(&source).unwrap();
    // Minted at an earlier time than the solutions: minted UUIDs derive
    // from the serial and the mint time.
    compile(&base, &module, &mut Lockfile::new(), &opts(1_767_000_000)).unwrap()
}

/// Each test's name and whether it passed, with its failures and error.
fn run(fixture: &LoxoneDoc, source: &str) -> Vec<(String, bool, String)> {
    let module = Module::parse(source).unwrap();
    let mut lock = Lockfile::new();
    let compiled = compile(fixture, &module, &mut lock, &opts(1_767_225_600)).unwrap();
    let (flat, _) = module.expand().unwrap().desugar().unwrap();
    let plan = plan_tests(&flat, &lock, &compiled, None).unwrap();
    run_tests(&plan, &compiled)
        .into_iter()
        .map(|r| (r.name, r.pass, format!("{:?} {:?}", r.failures, r.error)))
        .collect()
}

#[test]
fn each_case_fails_bare_and_passes_with_its_reference() {
    // The published crate leaves `tools/` out.
    if !std::path::Path::new(AGENT).exists() {
        return;
    }
    let fixture = fixture();
    let mut cases: Vec<_> = std::fs::read_dir(format!("{AGENT}/cases"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    cases.sort();
    assert!(cases.len() >= 10, "{cases:?}");
    for dir in cases {
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
        let (request, tests, reference) = (
            read("request.md"),
            read("tests.loxl"),
            read("reference.loxl"),
        );
        assert!(!request.trim().is_empty(), "{dir:?}");
        let bare = run(&fixture, &tests);
        assert!(!bare.is_empty(), "{dir:?}: no tests");
        assert!(
            bare.iter().any(|(_, pass, _)| !pass),
            "{dir:?}: the bare fixture passes every test"
        );
        for (name, pass, why) in run(&fixture, &format!("{reference}\n{tests}")) {
            assert!(pass, "{dir:?}: \"{name}\": {why}");
        }
    }
}
