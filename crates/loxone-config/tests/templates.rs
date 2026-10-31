//! The template library (`templates/`): each file is canonical, and its
//! tests (`templates/tests/<file>.loxl`) pass in the simulator against
//! the empty house `examples/configs/rohbau.Loxone`.

use loxone_config::library::FILES;
use loxone_config::logic::{CompileOptions, Module, compile, plan_tests, run_tests};
use loxone_config::uuid::parse_serial;
use loxone_config::{Lockfile, LoxoneDoc};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/templates");

#[test]
fn each_library_file_passes_its_tests() {
    let base = LoxoneDoc::parse(include_bytes!("../examples/configs/rohbau.Loxone")).unwrap();
    let opts = CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600,
        page_title: None,
        allow_removals: false,
        accept_version: None,
        relayout: Vec::new(),
    };
    for (file, library) in FILES {
        assert_eq!(
            Module::parse_fragment(library).unwrap().to_text(),
            *library,
            "templates/{file}.loxl is not canonical (loxl fmt --write)"
        );
        let tests = std::fs::read_to_string(format!("{DIR}/tests/{file}.loxl")).unwrap();
        let module = Module::parse(&format!("{library}\n{tests}")).unwrap();
        let mut lock = Lockfile::new();
        let compiled = compile(&base, &module, &mut lock, &opts).unwrap();
        let (flat, _) = module.expand().unwrap().desugar().unwrap();
        let plan = plan_tests(&flat, &lock, &compiled, None).unwrap();
        let results = run_tests(&plan, &compiled);
        assert!(!results.is_empty(), "{file}: no tests");
        for r in results {
            assert!(
                r.pass,
                "{file}: \"{}\": {:?} {:?}",
                r.name, r.failures, r.error
            );
        }
    }
}
