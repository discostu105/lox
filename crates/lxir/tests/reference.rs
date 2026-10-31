//! What an agent reads before writing a module: `lxir types` with each
//! type's purpose and `--search`, and the template library.

use std::process::Command;

fn lxir(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_lxir"))
        .args(args)
        .output()
        .unwrap();
    (out.status.success(), String::from_utf8(out.stdout).unwrap())
}

#[test]
fn types_say_what_a_block_is_for_and_search_by_purpose() {
    let (ok, out) = lxir(&["types", "OffDelay"]);
    assert!(ok);
    assert!(out.contains("Switch-off delay"), "{out}");
    assert!(
        out.contains("https://www.loxone.com/enen/kb/switch-off-delay/"),
        "{out}"
    );

    let (ok, out) = lxir(&["types", "--search", "hysteresis"]);
    assert!(ok);
    assert!(out.starts_with("AnalogThresholdTrigger"), "{out}");
    let (ok, _) = lxir(&["types", "--search", "nothing-like-this"]);
    assert!(!ok);

    let (ok, out) = lxir(&["types", "--search", "monoflop", "--json"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["type"], "Monoflop");
    assert!(v[0]["summary"].as_str().unwrap().starts_with("Monoflop"));
}

#[test]
fn templates_list_and_print_the_library() {
    let (ok, out) = lxir(&["templates"]);
    assert!(ok);
    assert!(
        out.contains("licht.lxir") && out.contains("  bewegungslicht(bewegung: any"),
        "{out}"
    );
    let (ok, out) = lxir(&["templates", "markise_schutz"]);
    assert!(ok);
    assert!(out.starts_with("# Beschattung"), "{out}");
    let (ok, _) = lxir(&["templates", "nichts"]);
    assert!(!ok);
}
