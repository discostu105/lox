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
        out.contains(
            "  LedRed             input        Lr\n      Turns the status LEDs red.\n      Stored as `Lr`.\n"
        ),
        "{out}"
    );
    // An undocumented port has no description line.
    assert!(
        out.contains("  Disable            input\n  LedRed"),
        "{out}"
    );
}

#[test]
fn types_describe_a_library_template() {
    let (ok, out) = loxl(&["types", "Treppenlicht"]);
    assert!(ok, "{out}");
    assert!(
        out.starts_with("Treppenlicht (template, from the library (licht.loxl))\n"),
        "{out}"
    );
    assert!(
        out.contains("    taster             object    an extern, block or instance of any type\n"),
        "{out}"
    );
    assert!(
        out.contains("    dauer              duration  a duration like `3min`, default 3min\n"),
        "{out}"
    );
    assert!(
        out.contains("  Outputs:\n    Q                  ausgang.Q\n"),
        "{out}"
    );
    let (ok, out) = loxl(&["types", "--json", "Treppenlicht"]);
    assert!(ok, "{out}");
    assert!(out.contains("\"template\": \"Treppenlicht\""), "{out}");
    let (ok, _) = loxl(&["types", "Nirgends"]);
    assert!(!ok);
}

#[test]
fn templates_list_and_print_the_library() {
    let (ok, out) = loxl(&["templates"]);
    assert!(ok);
    assert!(
        out.contains("licht.loxl") && out.contains("  Bewegungslicht(bewegung: any"),
        "{out}"
    );
    let (ok, out) = loxl(&["templates", "MarkiseSchutz"]);
    assert!(ok);
    assert!(out.starts_with("# Beschattung"), "{out}");
    let (ok, _) = loxl(&["templates", "nichts"]);
    assert!(!ok);
}

#[test]
fn types_describe_a_project_template() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("types-template");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("lox.toml"),
        "base = \"base.Loxone\"\nmodule = \"src\"\nlock = \"lock.json\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/a.loxl"),
        "template Takt(quelle: input, plan: schedule, label: string, dauer: duration = 1min, an: bool = on, zahl: number)\n\
         \t# Ein Takt.\n\
         \tm = Monoflop(\"{label}\", Trigger: quelle, Time: dauer)\n\
         end\n",
    )
    .unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_loxl"))
        .current_dir(&dir)
        .args(["types", "Takt"])
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.starts_with("Takt (template, from src)\n  Ein Takt.\n"),
        "{text}"
    );
    assert!(
        text.contains("    plan               schedule  a `schedule` set by name, required\n"),
        "{text}"
    );
    assert!(
        text.contains("    label              string    a quoted string, required\n"),
        "{text}"
    );
    assert!(
        text.contains("    an                 bool      `on` or `off`, default on\n"),
        "{text}"
    );
    assert!(
        text.contains("    zahl               number    a number like `28`, required\n"),
        "{text}"
    );
    assert!(text.contains("  Outputs: none declared\n"), "{text}");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_loxl"))
        .current_dir(&dir)
        .args(["types", "--json", "Takt"])
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("\"from\": \"src\""), "{text}");
    assert!(text.contains("\"kind\": \"input\""), "{text}");
}
