//! Byte-faithfulness: parse → serialize must reproduce the input exactly.
//!
//! The shipped example configs are always checked. Real Miniserver configs
//! (which must not be committed — they contain personal data) can be checked
//! by pointing `LXIR_CORPUS` at a directory of `.Loxone` files:
//!
//! ```sh
//! LXIR_CORPUS=~/loxone-backups cargo test --test roundtrip
//! ```

use lxir::LoxoneDoc;
use std::path::Path;

fn assert_roundtrip(path: &Path) {
    let input = std::fs::read(path).unwrap();
    let doc = LoxoneDoc::parse(&input)
        .unwrap_or_else(|e| panic!("{}: parse failed: {e}", path.display()));
    let output = doc.to_bytes();
    if output != input {
        let pos = input
            .iter()
            .zip(output.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(input.len().min(output.len()));
        panic!(
            "{}: roundtrip diverges at byte {pos} (in {} bytes, out {} bytes)",
            path.display(),
            input.len(),
            output.len()
        );
    }
}

#[test]
fn example_configs_roundtrip_byte_identical() {
    for entry in std::fs::read_dir("examples/configs").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "Loxone") {
            assert_roundtrip(&path);
        }
    }
}

#[test]
fn compiled_example_output_roundtrips() {
    let path = Path::new("examples/out/haus-compiled.Loxone");
    if path.exists() {
        assert_roundtrip(path);
    }
}

#[test]
fn corpus_roundtrips_byte_identical() {
    let Ok(dir) = std::env::var("LXIR_CORPUS") else {
        eprintln!("LXIR_CORPUS not set — skipping real-config corpus");
        return;
    };
    let mut n = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "Loxone") {
            assert_roundtrip(&path);
            n += 1;
        }
    }
    assert!(n > 0, "no .Loxone files in {dir}");
    eprintln!("corpus: {n} configs byte-identical");
}
