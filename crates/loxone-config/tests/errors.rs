//! One run reports every independent error: syntax errors recover at
//! the next statement, name and port errors are collected per
//! statement, and one missing declaration is reported once.

use loxone_config::{Error, Module};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The messages of `e`, one per error.
fn messages(e: Error) -> Vec<String> {
    e.into_list().into_iter().map(|e| e.to_string()).collect()
}

/// The lines of the parse errors in `e`.
fn parse_lines(e: Error) -> Vec<usize> {
    e.into_list()
        .into_iter()
        .map(|e| match e {
            Error::Parse { line, .. } => line,
            other => panic!("not a parse error: {other}"),
        })
        .collect()
}

#[test]
fn every_syntax_error_is_reported_with_its_line() {
    let src = "\
extern sw = Switch(title: \"Licht\")
a = Or(I1 sw.Q)
b = Or(
  In1: sw.Q,
  I2 = 3,
)
rule r1 bad words
  when sw.Q on
  then b on
end
c = Not(In: sw.Q)
d.Q <- <- sw.Q
";
    let e = Module::parse(src).unwrap_err();
    assert!(matches!(e, Error::Many(_)), "{e}");
    // The rest of a failed argument list and of a failed rule body is
    // skipped with the statement, not reported line by line.
    assert_eq!(parse_lines(e), vec![2, 5, 7, 12]);
}

#[test]
fn a_failed_call_head_skips_its_argument_lines() {
    let src = "\
a = Or(I1 sw.Q,
  In2: sw.Q,
  oops oops,
)
b = Or(I1 3)
";
    assert_eq!(parse_lines(Module::parse(src).unwrap_err()), vec![1, 5]);
}

#[test]
fn one_syntax_error_is_still_a_plain_parse_error() {
    let e = Module::parse("a = Or(I1 3)\n").unwrap_err();
    assert!(matches!(e, Error::Parse { line: 1, .. }), "{e}");
}

#[test]
fn independent_name_errors_are_all_reported() {
    let src = "\
extern sw = Switch(title: \"Licht\")
a = And(In1: sw.Q, In2: nope.Q)
b = Or(In1: sw.Q)
b.In2 <- sw.Q
c = Not(In: sw.Q)
c.Q = 3
a = Not()
";
    let m = messages(Module::parse(src).unwrap_err());
    assert_eq!(m.len(), 2, "{m:#?}");
    assert!(m[0].contains("duplicate name `a`"), "{m:#?}");
    assert!(m[1].contains("undeclared slug `nope`"), "{m:#?}");
}

#[test]
fn a_missing_declaration_is_reported_once() {
    let src = "\
extern sw = Switch(title: \"Licht\")
a = And(In1: sw.Q, In2: nope.Q)
b = Or(In1: nope.Q)
c = Or(In1: nope.Q, In2: sw.Q)
d = Or(In1: other.Q)
";
    let m = messages(Module::parse(src).unwrap_err());
    assert_eq!(m.len(), 2, "{m:#?}");
    assert!(m[0].contains("`nope`"), "{m:#?}");
    assert!(m[1].contains("`other`"), "{m:#?}");
}

#[test]
fn a_block_of_an_unknown_type_is_reported_once() {
    let src = "\
extern sw = Switch(title: \"Licht\")
a = And(In1: sw.Q, In2: sw.Q)
b = Foo(I1: sw.Q)
c = Or(In1: b.Q, In2: b.Q)
d = Or(In1: a.Qx)
g = Not(In: sw.Q, Gg: 3)
";
    let m = Module::parse(src).unwrap();
    let m = messages(loxone_config::logic::validate_ports(&m).unwrap_err());
    // `c`'s wires from `b` are not reported: `b`'s type is.
    assert_eq!(m.len(), 3, "{m:#?}");
    for want in ["type `Foo`", "unknown port `Qx`", "unknown port `Gg`"] {
        assert!(m.iter().any(|x| x.contains(want)), "{want}: {m:#?}");
    }
}

#[test]
fn errors_in_template_instances_are_all_reported_in_source_spelling() {
    let src = "\
extern sw = Switch(title: \"Licht\")
template Licht(taster: Switch) -> (Q: lc.Q)
  lc = Or(In1: taster.Q)
  aus = Not(In: taster.Q)
end
k = Licht(taster: sw)
w = Licht(taster: sw)
x = Or(In1: k.aus.Qx, In2: w.Qy)
y = nix(taster: sw)
";
    let m = Module::parse_fragment(src).unwrap();
    let e = m
        .with_expanded(|x| {
            x.validate()?;
            loxone_config::logic::validate_ports(&x)
        })
        .unwrap_err();
    // The expansion fails first: its errors gate the later checks.
    let m = messages(e);
    assert_eq!(m.len(), 2, "{m:#?}");
    assert!(
        m.iter().any(|x| x.contains("unknown template `nix`")),
        "{m:#?}"
    );
    assert!(m.iter().any(|x| x.contains("has no output `Qy`")), "{m:#?}");

    // An unknown output is an expansion error too.
    let src = src.replace("y = nix(taster: sw)\n", "");
    let m = Module::parse_fragment(&src).unwrap();
    let e = m
        .with_expanded(|x| {
            x.validate()?;
            loxone_config::logic::validate_ports(&x)
        })
        .unwrap_err();
    let m = messages(e);
    assert_eq!(m.len(), 1, "{m:#?}");
    assert!(m[0].contains("has no output `Qy`"), "{m:#?}");

    // The port check names the block as the source spells it.
    let src = src.replace("In2: w.Qy", "In2: w.Q");
    let m = Module::parse_fragment(&src).unwrap();
    let e = m
        .with_expanded(|x| {
            x.validate()?;
            loxone_config::logic::validate_ports(&x)
        })
        .unwrap_err();
    let m = messages(e);
    assert_eq!(m.len(), 1, "{m:#?}");
    assert!(m[0].contains("on block `k.aus`"), "{m:#?}");
}

fn loxl(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_loxl"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run loxl")
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

#[test]
fn check_lists_every_error_by_file_and_line() {
    let dir = scratch("errors-check");
    std::fs::write(
        dir.join("src/a.loxl"),
        "extern sw = Switch(title: \"Licht\")\nx = Or(In1: nope.Q)\ny = Or(In1: sw.Q)\ny.In2 <- sw.Q\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/b.loxl"),
        "z = Not(In: sw.Q)\nz2 = Not(In: other.Q)\n",
    )
    .unwrap();

    let out = loxl(&dir, &["check", "--json", "src"]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false);
    let errors = v["errors"].as_array().unwrap();
    let places: Vec<(String, u64)> = errors
        .iter()
        .map(|e| {
            let file = Path::new(e["path"].as_str().unwrap());
            (
                file.file_name().unwrap().to_string_lossy().into_owned(),
                e["line"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        places,
        vec![("a.loxl".into(), 2), ("b.loxl".into(), 2)],
        "{v}"
    );
    assert_eq!(v["path"], errors[0]["path"]);

    // Text: one `error:` line each.
    let out = loxl(&dir, &["check", "src"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        stderr.lines().filter(|l| l.starts_with("error: ")).count(),
        2,
        "{stderr}"
    );

    // Syntax errors in two fragments: both, and no name errors.
    std::fs::write(dir.join("src/a.loxl"), "x = Or(I1 3)\n").unwrap();
    std::fs::write(dir.join("src/b.loxl"), "z = Not(In: sw.Q)\nz2 = = 3\n").unwrap();
    let out = loxl(&dir, &["check", "--json", "src"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let lines: Vec<u64> = v["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["line"].as_u64().unwrap())
        .collect();
    assert_eq!(lines, vec![1, 2], "{v}");
}
