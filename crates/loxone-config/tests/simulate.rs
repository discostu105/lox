//! `loxl test` end to end in the built-in simulator (D68): the shipped
//! example modules compile against the example base and their `test`
//! blocks pass — and a wrong expectation fails.

use loxone_config::logic::{
    CompileOptions, Module, TestResult, compile, plan_tests, run_tests, trace_tests,
};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

const EXAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples");

fn run(source: &str) -> Vec<(String, bool, Vec<String>, Option<String>)> {
    results(source, false)
        .into_iter()
        .map(|r| (r.name, r.pass, r.failures, r.error))
        .collect()
}

fn results(source: &str, trace: bool) -> Vec<TestResult> {
    let base_bytes = std::fs::read(format!("{EXAMPLES}/configs/pool.Loxone")).unwrap();
    let base = LoxoneDoc::parse(&base_bytes).unwrap();
    let module = Module::parse(source).unwrap();
    let opts = CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: None,
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    };
    let mut lock = Lockfile::new();
    let compiled = compile(&base, &module, &mut lock, &opts).unwrap();
    let (flat, _) = module.expand().unwrap().desugar().unwrap();
    let mut plan = plan_tests(&flat, &lock, &compiled, None).unwrap();
    plan.respell(&module);
    if trace {
        trace_tests(&plan, &compiled, &lock)
    } else {
        run_tests(&plan, &compiled)
    }
}

fn example(name: &str) -> String {
    std::fs::read_to_string(format!("{EXAMPLES}/logic/{name}.loxl")).unwrap()
}

#[test]
fn example_tests_pass() {
    for name in ["pool", "sprachtour"] {
        let results = run(&example(name));
        assert_eq!(results.len(), 2, "{name}");
        for (test, pass, failures, error) in results {
            assert!(pass, "{name}: \"{test}\": {failures:?} {error:?}");
        }
    }
}

#[test]
fn a_wrong_expectation_fails_with_its_line() {
    let source = example("sprachtour");
    let marker = "expect hitze.Q == 1";
    assert!(source.contains(marker), "the example changed");
    let broken = source.replacen(marker, "expect hitze.Q == 0", 1);
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
    let source = example("sprachtour");
    let marker = "expect hitze.Q == 1";
    let broken = source.replacen(marker, "expect hitze.Q == 0", 1);
    assert!(results(&source, false)[0].trace.is_empty());
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
