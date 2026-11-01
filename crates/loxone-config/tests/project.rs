//! End-to-end tests of `lox.toml` projects through the real binary:
//! zero-flag compile, recursive fragment discovery, flag override, and
//! the project defaults of `check`, `fmt`, and `drift`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Fragments of the shipped example module, split the way a project
/// splits sources: periphery externs in one file, logic in a nested one.
const PERIPHERY: &str = "\
# Externe Objekte gehören Loxone Config.

extern aussentemp = VirtualIn(iname: \"VI1\")
extern wind_alarm = VirtualIn(iname: \"VI2\")
extern sonne = VirtualIn(iname: \"VI3\")
extern jal_sued = AutoJalousie(title: \"Beschattung Süd\")
";

const SUED: &str = "\
# Beschattung Süd — referenziert Externs aus _periphery.loxl.

let temp_schwelle = 28

temp_hoch = GreaterEqual(\"Temp über 28\", Input1: aussentemp.Q, Input2: temp_schwelle)
beschatten = And(I1: temp_hoch.Q, I2: sonne.Q)

jal_sued.AutoShade <- beschatten.Q
jal_sued.Safety <- wind_alarm.Q

jal_sued.TargetPos = 70
";

fn loxl(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_loxl"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run loxl")
}

fn assert_ok(out: &Output, what: &str) -> String {
    assert!(
        out.status.success(),
        "{what} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A fresh project directory under the cargo-managed test tmpdir.
fn make_project(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(dir.join("src/rooms")).unwrap();
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/configs/haus.Loxone"),
        dir.join("haus.Loxone"),
    )
    .unwrap();
    std::fs::write(
        dir.join("lox.toml"),
        "# Beispielprojekt\n\
         base = \"haus.Loxone\"\n\
         module = \"src\"\n\
         lock = \"haus.lock.json\"\n\
         serial = \"504F94112233\"\n\
         page = \"Beschattung\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/_periphery.loxl"), PERIPHERY).unwrap();
    std::fs::write(dir.join("src/rooms/sued.loxl"), SUED).unwrap();
    dir
}

#[test]
fn zero_flag_compile_inside_a_project() {
    let dir = make_project("zero-flag");
    // A dot-directory with garbage must be skipped by discovery.
    std::fs::create_dir_all(dir.join("src/.cache")).unwrap();
    std::fs::write(dir.join("src/.cache/junk.loxl"), "not Lox Logic at all").unwrap();

    let stdout = assert_ok(&loxl(&dir, &["compile"]), "zero-flag compile");
    assert!(stdout.contains("-> out.Loxone"), "stdout: {stdout}");
    assert!(dir.join("out.Loxone").exists());
    assert!(dir.join("haus.lock.json").exists());

    // Recompile: lock pins every identity, output is byte-stable.
    let first = std::fs::read(dir.join("out.Loxone")).unwrap();
    assert_ok(&loxl(&dir, &["compile"]), "recompile");
    assert_eq!(first, std::fs::read(dir.join("out.Loxone")).unwrap());

    // A flag overrides the project file; the rest still comes from it.
    assert_ok(&loxl(&dir, &["compile", "--out", "other.Loxone"]), "--out");
    assert_eq!(first, std::fs::read(dir.join("other.Loxone")).unwrap());

    // The compiled output matches the lock's fingerprint (drift, lock
    // path from the project). The base is the config the compile started
    // from: in sync, its output only not pushed yet. A base changed since
    // drifts.
    assert_ok(&loxl(&dir, &["drift", "out.Loxone"]), "drift out");
    let base = assert_ok(&loxl(&dir, &["drift", "haus.Loxone"]), "drift base");
    assert!(base.contains("not pushed yet"), "stdout: {base}");
    let changed = std::fs::read_to_string(dir.join("haus.Loxone"))
        .unwrap()
        .replace("Beschattung Süd", "Beschattung Nord");
    std::fs::write(dir.join("changed.Loxone"), changed).unwrap();
    assert!(!loxl(&dir, &["drift", "changed.Loxone"]).status.success());
}

#[test]
fn drift_accepts_a_fingerprint_recorded_by_an_older_loxl() {
    // Locks written before D39 carry the v1 fingerprint, which is blind to
    // room/category bindings. Upgrading Lox Logic must not turn every project
    // red: a v1 match is in sync, with a note; the next compile records v2.
    // The same holds for the v5 form, blind to DayTimer schedules (D53).
    let dir = make_project("drift-v1");
    assert_ok(&loxl(&dir, &["compile"]), "compile");
    let out =
        loxone_config::LoxoneDoc::parse(&std::fs::read(dir.join("out.Loxone")).unwrap()).unwrap();
    let current = loxone_config::diff::semantic_fingerprint(&out);
    let lock_path = dir.join("haus.lock.json");
    for (older, label) in [
        (
            loxone_config::diff::semantic_fingerprint_v1(&out),
            "drift v1",
        ),
        (
            loxone_config::diff::semantic_fingerprint_v5(&out),
            "drift v5",
        ),
        (
            loxone_config::diff::semantic_fingerprint_v6(&out),
            "drift v6",
        ),
    ] {
        let lock = std::fs::read_to_string(&lock_path).unwrap();
        assert!(lock.contains(&current));
        std::fs::write(&lock_path, lock.replace(&current, &older)).unwrap();

        let stdout = assert_ok(&loxl(&dir, &["drift", "out.Loxone"]), label);
        assert!(
            stdout.contains("an older version of Lox Logic"),
            "stdout: {stdout}"
        );
        assert_ok(&loxl(&dir, &["drift", "haus.Loxone"]), "drift base");

        assert_ok(&loxl(&dir, &["compile"]), "recompile");
        assert!(
            std::fs::read_to_string(&lock_path)
                .unwrap()
                .contains(&current)
        );
    }
}

#[test]
fn explicit_project_dir_and_missing_toml() {
    let dir = make_project("explicit-dir");
    let parent = dir.parent().unwrap().to_path_buf();
    let stdout = assert_ok(
        &loxl(
            &parent,
            &["compile", dir.file_name().unwrap().to_str().unwrap()],
        ),
        "compile <project-dir>",
    );
    assert!(stdout.contains("objects"), "stdout: {stdout}");
    assert!(dir.join("out.Loxone").exists());

    // An explicit directory without a lox.toml is a hard error…
    let bare = parent.join("no-project-here");
    std::fs::create_dir_all(&bare).unwrap();
    let out = loxl(&parent, &["compile", "no-project-here"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(stderr.contains("no lox.toml"), "stderr: {stderr}");

    // …and so is a flagless compile outside any project.
    let out = loxl(&bare, &["compile"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(stderr.contains("lox.toml"), "stderr: {stderr}");
}

#[test]
fn check_and_fmt_default_to_the_project_module() {
    let dir = make_project("check-fmt");
    // check with no path sees both fragments — 4 externs from
    // _periphery.loxl, 2 blocks from the nested rooms/sued.loxl.
    let stdout = assert_ok(&loxl(&dir, &["check"]), "zero-arg check");
    assert!(stdout.contains("4 externs, 2 blocks"), "stdout: {stdout}");

    let stdout = assert_ok(&loxl(&dir, &["fmt", "--check"]), "zero-arg fmt");
    assert!(stdout.contains("sued.loxl: canonical"), "stdout: {stdout}");

    // Outside a project, zero-arg check says what is missing.
    let bare = dir.parent().unwrap().join("no-project-bare");
    std::fs::create_dir_all(&bare).unwrap();
    let out = loxl(&bare, &["check"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(stderr.contains("no lox.toml"), "stderr: {stderr}");
}
