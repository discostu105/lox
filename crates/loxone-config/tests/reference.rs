//! What an agent reads before writing a module: `loxl types` with each
//! type's purpose and `--search`, and the template library.

use std::process::Command;

fn loxl(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_loxl"))
        .args(args)
        .output()
        .unwrap();
    (out.status.success(), String::from_utf8(out.stdout).unwrap())
}

#[test]
fn types_say_what_a_block_is_for_and_search_by_purpose() {
    let (ok, out) = loxl(&["types", "OffDelay"]);
    assert!(ok);
    assert!(out.contains("Switch-off delay"), "{out}");
    assert!(
        out.contains("https://www.loxone.com/enen/kb/switch-off-delay/"),
        "{out}"
    );

    let (ok, out) = loxl(&["types", "--search", "hysteresis"]);
    assert!(ok);
    assert!(out.starts_with("AnalogThresholdTrigger"), "{out}");
    let (ok, _) = loxl(&["types", "--search", "nothing-like-this"]);
    assert!(!ok);

    let (ok, out) = loxl(&["types", "--search", "monoflop", "--json"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["type"], "Monoflop");
    assert!(v[0]["summary"].as_str().unwrap().starts_with("Monoflop"));
    assert_eq!(
        v[0]["ports"][0]["description"],
        "Trigger: a pulse switches Q on for Time."
    );
}

#[test]
fn types_describe_each_documented_port() {
    let (ok, out) = loxl(&["types", "NfcCodeTouch"]);
    assert!(ok);
    assert!(
        out.contains("  Lr                 input        Lr\n      Turns the status LEDs red.\n"),
        "{out}"
    );
    // An undocumented port has no description line.
    assert!(out.contains("  Disable            input\n  Lr"), "{out}");
}

#[test]
fn templates_list_and_print_the_library() {
    let (ok, out) = loxl(&["templates"]);
    assert!(ok);
    assert!(
        out.contains("licht.loxl") && out.contains("  bewegungslicht(bewegung: any"),
        "{out}"
    );
    let (ok, out) = loxl(&["templates", "markise_schutz"]);
    assert!(ok);
    assert!(out.starts_with("# Beschattung"), "{out}");
    let (ok, _) = loxl(&["templates", "nichts"]);
    assert!(!ok);
}
