//! The example library (`examples/library/`): third-party configs whose
//! license allows redistribution, each with its decompiled Lox Logic view
//! and a rendering of each logic page. The committed views must be what
//! the current decompiler and renderer produce, so the library always
//! shows today's output. Regenerate them as `examples/library/README.md`
//! describes.

use std::path::{Path, PathBuf};
use std::process::Command;

fn loxl(dir: &Path, args: &[&str]) {
    let out = Command::new(env!("CARGO_BIN_EXE_loxl"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "loxl {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn entries() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/library");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    assert!(!dirs.is_empty());
    dirs
}

fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    v.sort();
    v
}

fn config(entry: &Path) -> String {
    let cfgs = files(entry, "Loxone");
    assert_eq!(cfgs.len(), 1, "{}: exactly one .Loxone", entry.display());
    cfgs[0].file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn every_entry_names_its_source_and_license() {
    for entry in entries() {
        let source = std::fs::read_to_string(entry.join("SOURCE.md"))
            .unwrap_or_else(|_| panic!("{}: SOURCE.md missing", entry.display()));
        // Entries made for this repository have no upstream commit and
        // fall under the crate's own license.
        let own = source.contains("Source: made for this repository");
        let fields: &[&str] = if own {
            &["Source:", "License: Apache-2.0", "Changes:"]
        } else {
            &["Source:", "Commit:", "License:", "Changes:"]
        };
        for field in fields {
            assert!(
                source.contains(field),
                "{}: SOURCE.md lacks {field}",
                entry.display()
            );
        }
        assert!(
            own || entry.join("LICENSE").exists(),
            "{}: LICENSE missing",
            entry.display()
        );
    }
}

#[test]
fn decompiled_views_are_current() {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("library");
    for entry in entries() {
        let name = entry.file_name().unwrap().to_string_lossy().into_owned();
        let out = tmp.join(&name);
        let _ = std::fs::remove_dir_all(&out);
        let out_s = out.to_string_lossy().into_owned();
        loxl(&entry, &["decompile", "--out-dir", &out_s, &config(&entry)]);

        let committed = files(&entry.join("logic"), "loxl");
        let fresh = files(&out, "loxl");
        let names = |v: &[PathBuf]| -> Vec<_> {
            v.iter()
                .map(|p| p.file_name().unwrap().to_owned())
                .collect()
        };
        assert_eq!(
            names(&committed),
            names(&fresh),
            "{name}: the set of pages changed"
        );
        for (c, f) in committed.iter().zip(&fresh) {
            assert!(
                std::fs::read(c).unwrap() == std::fs::read(f).unwrap(),
                "{}: decompiled view is stale — regenerate (examples/library/README.md)",
                c.display()
            );
        }
    }
}

#[test]
fn page_renderings_are_current() {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("library-svg");
    std::fs::create_dir_all(&tmp).unwrap();
    for entry in entries() {
        for module in files(&entry.join("logic"), "loxl") {
            let text = std::fs::read_to_string(&module).unwrap();
            let page = text
                .lines()
                .find_map(|l| l.strip_prefix("page \"")?.strip_suffix('"'))
                .unwrap_or_else(|| panic!("{}: no page statement", module.display()));
            let stem = module.file_stem().unwrap().to_string_lossy().into_owned();
            let fresh = tmp.join(format!(
                "{}-{stem}.svg",
                entry.file_name().unwrap().to_string_lossy()
            ));
            let fresh_s = fresh.to_string_lossy().into_owned();
            loxl(
                &entry,
                &["render", &config(&entry), "--page", page, "--out", &fresh_s],
            );
            let committed = entry.join(format!("{stem}.svg"));
            assert!(
                std::fs::read(&committed).ok() == Some(std::fs::read(&fresh).unwrap()),
                "{}: rendering is stale or missing — regenerate (examples/library/README.md)",
                committed.display()
            );
        }
    }
}

#[test]
fn views_check_and_adoptions_rebuild_byte_identical() {
    // The views are what people copy from: each must pass `loxl check`
    // (#226: a stale constant under a wire once printed as a duplicate
    // argument). Adopting the config and compiling the pair must give the
    // config back byte for byte — the stale constant included (D77).
    use loxone_config::logic::{CompileOptions, adopt, compile};
    use loxone_config::uuid::parse_serial;
    for entry in entries() {
        for module in files(&entry.join("logic"), "loxl") {
            let m = module.to_string_lossy().into_owned();
            loxl(&entry, &["check", &m]);
        }
        let path = entry.join(config(&entry));
        let bytes = std::fs::read(&path).unwrap();
        let base = loxone_config::LoxoneDoc::parse(&bytes).unwrap();
        let (module, mut lock, _) = adopt(&base).unwrap();
        let opts = CompileOptions {
            machine: parse_serial("504F94112233").unwrap(),
            mint_time_unix: 1_767_225_600,
            page_title: None,
            allow_removals: false,
            accept_version: None,
            relayout: Vec::new(),
        };
        let out = compile(&base, &module, &mut lock, &opts).unwrap();
        assert!(
            out.to_bytes() == base.to_bytes(),
            "{}: the adopted rebuild is not byte-identical",
            path.display()
        );
    }
}
