//! Corpus-wide adoption fidelity: every real config is a test case.
//!
//! For each `.Loxone` file in `LOXL_CORPUS`, whatever `adopt` accepts must
//! rebuild as a semantic no-op, deterministically. Refusals are fine (old
//! generations of admitted types, unverified attributes — refusing is the
//! designed behavior); silent unfaithfulness is not.
//!
//! The corpus is local-only (`corpus/web/`, unclear licenses — never
//! committed), so the tests skip without `LOXL_CORPUS`. Debug-profile XML
//! parsing is slow on 100+ configs; run this in release:
//!
//! ```sh
//! LOXL_CORPUS=corpus/web cargo test --release --test corpus
//! ```

use loxone_config::Lockfile;
use loxone_config::LoxoneDoc;
use loxone_config::connectors::{BUILTIN_TYPES, PortDir, builtin, merge, observe};
use loxone_config::logic::{
    AccessDecl, CompileOptions, DecompileOptions, Item, Module, MoodsClaim, adopt, adopt_pages,
    compile, decompile,
};
use loxone_config::uuid::parse_serial;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn corpus() -> Option<Vec<PathBuf>> {
    let dir = std::env::var("LOXL_CORPUS").ok()?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("LOXL_CORPUS={dir}: {e}"))
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("loxone"))
        })
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
        relayout: Vec::new(),
    }
}

#[test]
fn corpus_adoptions_rebuild_as_semantic_noops() {
    let Some(files) = corpus() else {
        eprintln!("LOXL_CORPUS not set — skipping corpus adoption fidelity");
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
        let d = loxone_config::diff::diff(&base, &out);
        assert!(
            d.is_empty(),
            "{}: adopted rebuild is not a semantic no-op:\n{d:#?}",
            path.display()
        );
        // The drift baseline recorded at adoption must survive the
        // rebuild: fingerprint stability under our own writes is the
        // whole point of `loxl drift`.
        assert_eq!(
            lock.target.semantic_fingerprint.as_deref(),
            Some(&*loxone_config::diff::semantic_fingerprint(&out)),
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
        // The source as written: the text of the module and of its page
        // fragments (with the `defaults` factored into them) parses back
        // and compiles to the same bytes.
        let text = Module::parse(&module.to_text())
            .unwrap_or_else(|e| panic!("{}: adopted text does not parse: {e}", path.display()));
        let mut lock2 = lock.clone();
        let out3 = compile(&base, &text, &mut lock2, &opts())
            .unwrap_or_else(|e| panic!("{}: text compile failed: {e}", path.display()));
        assert_eq!(
            out.to_bytes(),
            out3.to_bytes(),
            "{}: the adopted text compiles differently",
            path.display()
        );
        let (fragments, _, _) = adopt_pages(&base)
            .unwrap_or_else(|e| panic!("{}: adopt_pages failed: {e}", path.display()));
        let merged = Module::from_fragments(fragments.iter().map(|(stem, m)| {
            Module::parse_fragment(&m.to_text())
                .unwrap_or_else(|e| panic!("{}: {stem}.loxl does not parse: {e}", path.display()))
        }));
        let mut lock3 = lock.clone();
        let out4 = compile(&base, &merged, &mut lock3, &opts())
            .unwrap_or_else(|e| panic!("{}: fragments compile failed: {e}", path.display()));
        assert_eq!(
            out.to_bytes(),
            out4.to_bytes(),
            "{}: the adopted fragments compile differently",
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

/// Access sets and claims are byte fixpoints: what decompile lifts from
/// a config compiles back to exactly its bytes. Revoking every declared
/// right and granting them again restores the same rights.
#[test]
fn corpus_access_declarations_are_byte_fixpoints() {
    let Some(files) = corpus() else {
        eprintln!("LOXL_CORPUS not set — skipping corpus access fixpoints");
        return;
    };
    let (mut configs, mut decls) = (0usize, 0usize);
    for path in &files {
        let name = path.display();
        let bytes = std::fs::read(path).unwrap();
        let base = LoxoneDoc::parse(&bytes).unwrap();
        // Every block as an extern: the declarations alone are under test.
        // (`NfcCodeTouch` is a managed type; with the default options a
        // keypad with wires lifts as a block, which this module of externs
        // and access sets would not carry.)
        let view_opts = DecompileOptions {
            managed_types: Default::default(),
            ..DecompileOptions::default()
        };
        // Some views do not validate (unrelated to access); those configs
        // cannot show a declaration.
        let Ok((view, _)) = decompile(&base, &view_opts) else {
            continue;
        };
        let access: Vec<&AccessDecl> = view.access_decls().collect();
        if access.is_empty() {
            continue;
        }
        let claims: Vec<Item> = view
            .access_claims()
            .map(|c| Item::AccessClaim(c.clone()))
            .collect();
        let externs: Vec<Item> = view
            .externs()
            .filter(|e| view.access_claims().any(|c| c.slug == e.slug))
            .map(|e| Item::Extern(e.clone()))
            .collect();
        let module = |body: bool| Module {
            items: externs
                .iter()
                .cloned()
                .chain(access.iter().map(|a| {
                    let mut a = (*a).clone();
                    if !body {
                        a.body.clear();
                    }
                    Item::Access(a)
                }))
                .chain(claims.iter().cloned())
                .collect(),
        };
        let compiled = |doc: &LoxoneDoc, body: bool| {
            compile(doc, &module(body), &mut Lockfile::new(), &opts())
                .unwrap_or_else(|e| panic!("{name}: compile failed: {e}"))
        };
        let out = compiled(&base, true);
        assert!(
            out.to_bytes() == bytes,
            "{name}: the lifted access sets do not reproduce the bytes"
        );
        let revoked = compiled(&base, false);
        let d = loxone_config::diff::diff(&base, &revoked);
        assert!(d.logic_is_empty() && !d.access.is_empty(), "{name}");
        let granted = compiled(&revoked, true);
        assert!(
            loxone_config::diff::diff(&base, &granted).is_empty(),
            "{name}: revoke and grant again is not a no-op"
        );
        configs += 1;
        decls += access.len();
    }
    eprintln!("corpus: {decls} access sets in {configs} configs are byte fixpoints");
}

#[test]
fn corpus_rule_declarations_are_byte_fixpoints() {
    // Every Automatic Designer rule decompile lifts (D57, D73), declared
    // back onto its config with the blocks it names as externs, must
    // reproduce the bytes: what reads as source writes as the payload it
    // came from.
    let Some(files) = corpus() else {
        eprintln!("LOXL_CORPUS not set — skipping corpus rule fixpoints");
        return;
    };
    let (mut configs, mut decls, mut total) = (0usize, 0usize, 0usize);
    for path in &files {
        let name = path.display();
        let bytes = std::fs::read(path).unwrap();
        let base = LoxoneDoc::parse(&bytes).unwrap();
        total += base
            .objects()
            .iter()
            .filter(|o| o.block_type == "AutopilotRule")
            .count();
        let view_opts = DecompileOptions {
            managed_types: Default::default(),
            ..DecompileOptions::default()
        };
        let Ok((view, _)) = decompile(&base, &view_opts) else {
            continue;
        };
        let rules: Vec<_> = view.rule_decls().cloned().collect();
        if rules.is_empty() {
            continue;
        }
        let module = Module {
            items: view
                .externs()
                .map(|e| Item::Extern(e.clone()))
                .chain(rules.iter().cloned().map(Item::Rule))
                .collect(),
        };
        let out = compile(&base, &module, &mut Lockfile::new(), &opts())
            .unwrap_or_else(|e| panic!("{name}: compile failed: {e}"));
        assert!(
            out.to_bytes() == bytes,
            "{name}: the lifted rules do not reproduce the bytes"
        );
        configs += 1;
        decls += rules.len();
    }
    eprintln!("corpus: {decls} of {total} rules in {configs} configs are byte fixpoints");
}

#[test]
fn corpus_mood_sets_are_byte_fixpoints() {
    // Every controller's lifted moods (D58), claimed back onto it, must
    // reproduce the bytes; reversed and restored, the semantic diff must
    // be empty again (moods keep their identity by name).
    let Some(files) = corpus() else {
        eprintln!("LOXL_CORPUS not set — skipping corpus mood fixpoints");
        return;
    };
    let (mut configs, mut claims, mut not_lifted) = (0usize, 0usize, 0usize);
    for path in &files {
        let name = path.display();
        let bytes = std::fs::read(path).unwrap();
        let base = LoxoneDoc::parse(&bytes).unwrap();
        // Controllers as externs: the claims alone are under test.
        let view_opts = DecompileOptions {
            managed_types: Default::default(),
            ..DecompileOptions::default()
        };
        let Ok((view, _)) = decompile(&base, &view_opts) else {
            continue;
        };
        not_lifted += view
            .items
            .iter()
            .filter(|i| matches!(i, Item::Comment(c) if c.contains(": not lifted") && c.starts_with(" moods ")))
            .count();
        let lifted: Vec<&MoodsClaim> = view.moods_claims().collect();
        if lifted.is_empty() {
            continue;
        }
        let module = |reverse: bool| Module {
            items: view
                .externs()
                .filter(|e| lifted.iter().any(|c| c.slug == e.slug))
                .map(|e| Item::Extern(e.clone()))
                .chain(view.mood_sets().map(|s| {
                    let mut s = s.clone();
                    if reverse {
                        s.body.reverse();
                    }
                    Item::Moods(s)
                }))
                .chain(lifted.iter().map(|c| Item::MoodsClaim((*c).clone())))
                .collect(),
        };
        let compiled = |doc: &LoxoneDoc, reverse: bool| {
            compile(doc, &module(reverse), &mut Lockfile::new(), &opts())
                .unwrap_or_else(|e| panic!("{name}: compile failed: {e}"))
        };
        assert!(
            compiled(&base, false).to_bytes() == bytes,
            "{name}: the lifted mood sets do not reproduce the bytes"
        );
        let reversed = compiled(&base, true);
        let restored = compiled(&reversed, false);
        assert!(
            loxone_config::diff::diff(&base, &restored).is_empty(),
            "{name}: reversing and restoring the moods is not a no-op"
        );
        configs += 1;
        claims += lifted.len();
    }
    eprintln!(
        "corpus: {claims} mood claims in {configs} configs are byte fixpoints; \
         {not_lifted} controllers not lifted"
    );
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
        eprintln!("LOXL_CORPUS not set — skipping builtin-table counterexample hunt");
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
