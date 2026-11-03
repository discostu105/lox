//! `loxl test` end to end in the built-in simulator (D68): the shipped
//! example modules compile against the example base and their `test`
//! blocks pass — and a wrong expectation fails.

use loxone_config::logic::{
    CompileOptions, Module, TestResult, compile, plan_tests, run_tests, trace_tests,
};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

const EXAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples");

fn run(module: &Module) -> Vec<(String, bool, Vec<String>, Option<String>)> {
    results(module, false)
        .into_iter()
        .map(|r| (r.name, r.pass, r.failures, r.error))
        .collect()
}

fn results(module: &Module, trace: bool) -> Vec<TestResult> {
    let base_bytes = std::fs::read(format!("{EXAMPLES}/configs/pool.Loxone")).unwrap();
    let base = LoxoneDoc::parse(&base_bytes).unwrap();
    let opts = CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: None,
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    };
    let mut lock = Lockfile::new();
    let compiled = compile(&base, module, &mut lock, &opts).unwrap();
    let (flat, _) = module.expand().unwrap().desugar().unwrap();
    let mut plan = plan_tests(&flat, &lock, &compiled, None).unwrap();
    plan.respell(module);
    if trace {
        trace_tests(&plan, &compiled, &lock)
    } else {
        run_tests(&plan, &compiled)
    }
}

/// The source files of an example module: one file, or the files of
/// its directory (one per page) in name order.
fn example(name: &str) -> Vec<String> {
    let dir = std::path::Path::new(EXAMPLES).join("logic").join(name);
    if dir.is_dir() {
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "loxl"))
            .collect();
        files.sort();
        return files
            .iter()
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect();
    }
    vec![std::fs::read_to_string(format!("{EXAMPLES}/logic/{name}.loxl")).unwrap()]
}

fn module(files: &[String]) -> Module {
    Module::from_fragments(files.iter().map(|f| Module::parse_fragment(f).unwrap()))
}

/// `files` with `from` replaced by `to` in the one file that has it.
fn edited(files: &[String], from: &str, to: &str) -> Module {
    let hits = files.iter().filter(|f| f.contains(from)).count();
    assert_eq!(hits, 1, "the example changed");
    module(
        &files
            .iter()
            .map(|f| f.replacen(from, to, 1))
            .collect::<Vec<_>>(),
    )
}

#[test]
fn example_tests_pass() {
    for name in ["pool", "sprachtour"] {
        let results = run(&module(&example(name)));
        assert_eq!(results.len(), 2, "{name}");
        for (test, pass, failures, error) in results {
            assert!(pass, "{name}: \"{test}\": {failures:?} {error:?}");
        }
    }
}

#[test]
fn a_wrong_expectation_fails_with_its_line() {
    let files = example("sprachtour");
    let broken = edited(&files, "expect hitze.Q == 1", "expect hitze.Q == 0");
    let results = run(&broken);
    let (_, pass, failures, error) = &results[0];
    assert!(!pass && error.is_none());
    assert!(
        failures[0].contains("expect hitze.Q == 0 — actual 1"),
        "{failures:?}"
    );
}

#[test]
fn a_trace_lists_sets_changes_and_outcomes_by_slug() {
    let files = example("sprachtour");
    let broken = edited(&files, "expect hitze.Q == 1", "expect hitze.Q == 0");
    assert!(results(&module(&files), false)[0].trace.is_empty());
    let r = &results(&broken, true)[0];
    let trace = r.trace.join("\n");
    assert!(trace.contains(": set "), "{trace}");
    // The first cycle lists every watched port, later ones the changes.
    assert!(trace.contains("hitze.Q = "), "{trace}");
    assert!(trace.contains(" → "), "{trace}");
    assert!(
        trace.contains("expect hitze.Q == 0 — actual 1 FAILED"),
        "{trace}"
    );
    let first_cycle = r.trace.iter().position(|l| l.starts_with("t=")).unwrap();
    let first_expect = r.trace.iter().position(|l| l.contains("expect")).unwrap();
    assert!(first_cycle < first_expect, "{trace}");
}
