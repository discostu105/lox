//! End-to-end tests of `lxir rename` through the real binary: source and
//! lockfile move together, identities survive, and the verification gate
//! distinguishes byte-identical renames from Title-label changes.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PERIPHERY: &str = "\
# Externe Objekte gehören Loxone Config.

extern aussentemp = VirtualIn(iname: \"VI1\")
extern wind_alarm = VirtualIn(iname: \"VI2\")
extern sonne = VirtualIn(iname: \"VI3\")
extern jal_sued = AutoJalousie(title: \"Beschattung Süd\")
";

const SUED: &str = "\
# sonne und aussentemp speisen das Gatter.

let temp_schwelle = 28

temp_hoch = GreaterEqual(
\t\"Temp über 28\",
\tInput1: aussentemp.Q,
\tInput2: temp_schwelle,
)

beschatten = And(
\tI1: temp_hoch.Q,
\tI2: sonne.Q,
)

jal_sued.AutoShade <- beschatten.Q
jal_sued.Safety <- wind_alarm.Q

jal_sued.TargetPos = 70
";

fn lxir(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lxir"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run lxir")
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

fn assert_err(out: &Output, needle: &str, what: &str) {
    assert!(!out.status.success(), "{what}: expected failure");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(stderr.contains(needle), "{what}: stderr: {stderr}");
}

fn make_project(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/configs/haus.Loxone"),
        dir.join("haus.Loxone"),
    )
    .unwrap();
    std::fs::write(
        dir.join("lox.toml"),
        "base = \"haus.Loxone\"\n\
         module = \"src\"\n\
         lock = \"haus.lock.json\"\n\
         serial = \"504F94112233\"\n\
         page = \"Beschattung\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/_periphery.lxir"), PERIPHERY).unwrap();
    std::fs::write(dir.join("src/sued.lxir"), SUED).unwrap();
    dir
}

#[test]
fn rename_moves_source_lock_and_output_together() {
    let dir = make_project("rename-e2e");
    assert_ok(&lxir(&dir, &["compile"]), "baseline compile");
    let out_before = std::fs::read(dir.join("out.Loxone")).unwrap();
    let lock_before = std::fs::read_to_string(dir.join("haus.lock.json")).unwrap();
    assert!(lock_before.contains("\"sonne\""));

    // Extern rename: nothing in the output derives from the slug.
    let stdout = assert_ok(
        &lxir(&dir, &["rename", "sonne", "sonnenschein"]),
        "rename extern",
    );
    assert!(stdout.contains("byte-identical"), "stdout: {stdout}");
    assert_eq!(out_before, std::fs::read(dir.join("out.Loxone")).unwrap());
    let periphery = std::fs::read_to_string(dir.join("src/_periphery.lxir")).unwrap();
    assert!(periphery.contains("extern sonnenschein = VirtualIn"));
    let sued = std::fs::read_to_string(dir.join("src/sued.lxir")).unwrap();
    assert!(sued.contains("I2: sonnenschein.Q"));
    assert!(
        sued.contains("# sonnenschein und aussentemp speisen das Gatter."),
        "comment not renamed: {sued}"
    );
    let lock = std::fs::read_to_string(dir.join("haus.lock.json")).unwrap();
    assert!(lock.contains("\"sonnenschein\""));
    assert!(!lock.contains("\"sonne\""), "stale lock key: {lock}");

    // Labeled block rename: the explicit Title stays, bytes stay.
    let stdout = assert_ok(
        &lxir(&dir, &["rename", "temp_hoch", "temp_ueber_schwelle"]),
        "rename labeled block",
    );
    assert!(stdout.contains("byte-identical"), "stdout: {stdout}");
    assert_eq!(out_before, std::fs::read(dir.join("out.Loxone")).unwrap());
    let lock = std::fs::read_to_string(dir.join("haus.lock.json")).unwrap();
    assert!(lock.contains("\"temp_ueber_schwelle\""));

    // Auto-labeled block rename: Title = slug, so exactly that changes.
    let stdout = assert_ok(
        &lxir(&dir, &["rename", "beschatten", "beschattung_an"]),
        "rename auto-labeled block",
    );
    assert!(stdout.contains("1 Title label"), "stdout: {stdout}");
    let out_after = std::fs::read(dir.join("out.Loxone")).unwrap();
    assert_ne!(out_before, out_after);
    let changed = String::from_utf8_lossy(&out_after);
    assert!(changed.contains("Title=\"beschattung_an\""));

    // After every rename the pair is current: a recompile is a no-op on
    // lock and output alike.
    let lock_after = std::fs::read(dir.join("haus.lock.json")).unwrap();
    assert_ok(&lxir(&dir, &["compile"]), "recompile after renames");
    assert_eq!(
        lock_after,
        std::fs::read(dir.join("haus.lock.json")).unwrap()
    );
    assert_eq!(out_after, std::fs::read(dir.join("out.Loxone")).unwrap());
    assert_ok(&lxir(&dir, &["check"]), "check after renames");
}

#[test]
fn rename_refusals_leave_everything_untouched() {
    let dir = make_project("rename-refusals");
    assert_ok(&lxir(&dir, &["compile"]), "baseline compile");
    let snapshot = |d: &Path| {
        (
            std::fs::read_to_string(d.join("src/_periphery.lxir")).unwrap(),
            std::fs::read_to_string(d.join("src/sued.lxir")).unwrap(),
            std::fs::read_to_string(d.join("haus.lock.json")).unwrap(),
        )
    };
    let before = snapshot(&dir);

    assert_err(
        &lxir(&dir, &["rename", "nixda", "doch"]),
        "not declared",
        "unknown old slug",
    );
    assert_err(
        &lxir(&dir, &["rename", "sonne", "wind_alarm"]),
        "already declared",
        "collision",
    );
    assert_err(
        &lxir(&dir, &["rename", "sonne", "and"]),
        "reserved",
        "reserved word",
    );
    assert_err(
        &lxir(&dir, &["rename", "sonne", "Sonne"]),
        "invalid slug",
        "uppercase",
    );
    assert_eq!(before, snapshot(&dir), "a refused rename must not write");

    // An out-of-sync pair is refused too: touch the module, don't compile.
    let sued = std::fs::read_to_string(dir.join("src/sued.lxir")).unwrap();
    std::fs::write(
        dir.join("src/sued.lxir"),
        sued.replace("TargetPos = 70", "TargetPos = 55"),
    )
    .unwrap();
    assert_err(
        &lxir(&dir, &["rename", "sonne", "sonnenschein"]),
        "not current",
        "stale lock",
    );
}
