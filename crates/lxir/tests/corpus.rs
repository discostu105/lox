//! Corpus-wide adoption fidelity: every real config is a test case.
//!
//! For each `.Loxone` file in `LXIR_CORPUS`, whatever `adopt` accepts must
//! rebuild as a semantic no-op, deterministically. Refusals are fine (old
//! generations of admitted types, unverified attributes — refusing is the
//! designed behavior); silent unfaithfulness is not.
//!
//! The corpus is local-only (`corpus/web/`, unclear licenses — never
//! committed), so the tests skip without `LXIR_CORPUS`. Debug-profile XML
//! parsing is slow on 100+ configs; run this in release:
//!
//! ```sh
//! LXIR_CORPUS=corpus/web cargo test --release --test corpus
//! ```

use lxir::LoxoneDoc;
use lxir::connectors::{BUILTIN_TYPES, PortDir, builtin, merge, observe};
use lxir::ir::{CompileOptions, adopt, compile};
use lxir::uuid::parse_serial;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn corpus() -> Option<Vec<PathBuf>> {
    let dir = std::env::var("LXIR_CORPUS").ok()?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("LXIR_CORPUS={dir}: {e}"))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "Loxone"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no .Loxone files in {dir}");
    Some(files)
}

fn opts() -> CompileOptions {
    CompileOptions {
        machine: parse_serial("504F94112233").unwrap(),
        mint_time_unix: 1_767_225_600, // 2026-01-01T00:00:00Z
        page_title: None,
        allow_removals: false,
        accept_version: None,
    }
}

#[test]
fn corpus_adoptions_rebuild_as_semantic_noops() {
    let Some(files) = corpus() else {
        eprintln!("LXIR_CORPUS not set — skipping corpus adoption fidelity");
        return;
    };
    let mut adopted = 0usize;
    let mut refused: BTreeMap<String, usize> = BTreeMap::new();
    for path in &files {
        let base = LoxoneDoc::parse(&std::fs::read(path).unwrap())
            .unwrap_or_else(|e| panic!("{}: parse failed: {e}", path.display()));
        let (module, lock, report) =
            adopt(&base).unwrap_or_else(|e| panic!("{}: adopt failed: {e}", path.display()));
        adopted += report.blocks;
        for r in &report.refused {
            // Aggregate by cause, not instance: the reason text after the
            // identity prefix.
            let cause = r.split("): ").nth(1).unwrap_or(r).to_string();
            *refused.entry(cause).or_default() += 1;
        }
        if report.blocks == 0 {
            continue;
        }
        let mut lock1 = lock.clone();
        let out = compile(&base, &module, &mut lock1, &opts())
            .unwrap_or_else(|e| panic!("{}: compile failed: {e}", path.display()));
        let d = lxir::diff::diff(&base, &out);
        assert!(
            d.is_empty(),
            "{}: adopted rebuild is not a semantic no-op:\n{d:#?}",
            path.display()
        );
        // The drift baseline recorded at adoption must survive the
        // rebuild: fingerprint stability under our own writes is the
        // whole point of `lxir drift`.
        assert_eq!(
            lock.target.semantic_fingerprint.as_deref(),
            Some(&*lxir::diff::semantic_fingerprint(&out)),
            "{}: fingerprint moved across a semantic no-op rebuild",
            path.display()
        );
        // Determinism: compiling again with the updated lock reproduces
        // the bytes exactly.
        let out2 = compile(&base, &module, &mut lock1, &opts())
            .unwrap_or_else(|e| panic!("{}: recompile failed: {e}", path.display()));
        assert_eq!(
            out.to_bytes(),
            out2.to_bytes(),
            "{}: recompile is not byte-identical",
            path.display()
        );
    }
    eprintln!(
        "corpus: {} configs, {adopted} blocks adopted, {} refused",
        files.len(),
        refused.values().sum::<usize>()
    );
    let mut causes: Vec<(usize, &String)> = refused.iter().map(|(c, n)| (*n, c)).collect();
    causes.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (n, cause) in causes {
        eprintln!("  {n:4} × {cause}");
    }
}

#[test]
fn corpus_holds_no_counterexample_to_the_builtin_table() {
    // The admission rules are falsifiable: one observed counterexample
    // evicts a classification (docs/connector-db.md). This test *hunts* —
    // every config added to the corpus re-checks every admitted direction:
    //  - a declared Output must never be observed as a wire sink
    //    (that evidence flips it to Api, the PushButton.OutputAPI story);
    //  - a declared Input/Param must never be observed as a wire source;
    //  - Output and Api never carry `Def=`;
    //  - no instance materializes a key the spec does not list (older
    //    generations are strict subsets of the modern shape).
    let Some(files) = corpus() else {
        eprintln!("LXIR_CORPUS not set — skipping builtin-table counterexample hunt");
        return;
    };
    let mut obs = BTreeMap::new();
    for path in &files {
        let doc = LoxoneDoc::parse(&std::fs::read(path).unwrap())
            .unwrap_or_else(|e| panic!("{}: parse failed: {e}", path.display()));
        merge(&mut obs, observe(&doc));
    }
    let mut checked = 0usize;
    for t in BUILTIN_TYPES {
        let specs = builtin(t).unwrap();
        let Some(seen) = obs.get(*t) else { continue };
        let spec_keys: Vec<&str> = specs.iter().map(|s| s.key).collect();
        for key in seen.keys() {
            assert!(
                spec_keys.contains(&key.as_str()),
                "{t}: corpus materializes connector `{key}` that the builtin table does not list"
            );
        }
        for spec in specs {
            let Some(e) = seen.get(spec.key) else {
                continue;
            };
            checked += 1;
            match spec.dir {
                PortDir::Output => {
                    assert_eq!(
                        e.wired_as_sink, 0,
                        "{t}.{}: declared Output observed as a wire sink ×{} — evidence for Api",
                        spec.key, e.wired_as_sink
                    );
                    assert_eq!(
                        e.has_def, 0,
                        "{t}.{}: declared Output carries Def= ×{}",
                        spec.key, e.has_def
                    );
                }
                PortDir::Api => {
                    assert_eq!(
                        e.has_def, 0,
                        "{t}.{}: Api connector carries Def= ×{} — never a Def target",
                        spec.key, e.has_def
                    );
                }
                PortDir::Input | PortDir::Param => {
                    assert_eq!(
                        e.wired_as_source, 0,
                        "{t}.{}: declared {:?} observed as a wire source ×{}",
                        spec.key, spec.dir, e.wired_as_source
                    );
                }
            }
        }
    }
    assert!(
        checked > 100,
        "suspiciously little evidence ({checked} ports checked)"
    );
    eprintln!("corpus: {checked} admitted port classifications re-checked, no counterexample");
}
