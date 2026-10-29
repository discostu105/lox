//! End-to-end test of `lxir lint` through the real binary: source lints
//! plus the project-level dead-output analysis against the compiled
//! config.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PERIPHERY: &str = "\
extern aussentemp = VirtualIn(iname: \"VI1\")
extern wind_alarm = VirtualIn(iname: \"VI2\")
extern sonne = VirtualIn(iname: \"VI3\")
extern jal_sued = AutoJalousie(title: \"Beschattung Süd\")
";

const SUED: &str = "\
beschatten = And(
\tI1: sonne.Q,
\tI2: aussentemp.Q,
)

jal_sued.AutoShade <- beschatten.Q
jal_sued.Safety <- wind_alarm.Q
";

fn lxir(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lxir"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run lxir")
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
fn lint_reports_and_clears() {
    let dir = make_project("lint-e2e");
    let compile = lxir(&dir, &["compile"]);
    assert!(compile.status.success());

    // Everything referenced, every block consumed: clean, exit 0.
    let out = lxir(&dir, &["lint"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "expected clean: {stdout}");
    assert!(stdout.contains("lint: clean"), "{stdout}");

    // A block nothing consumes: flagged against the compiled config.
    std::fs::write(
        dir.join("src/sued.lxir"),
        format!("{SUED}\nverwaist = And(\n\tI1: sonne.Q,\n\tI2: wind_alarm.Q,\n)\n"),
    )
    .unwrap();
    assert!(lxir(&dir, &["compile"]).status.success());

    let out = lxir(&dir, &["lint"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(!out.status.success(), "expected findings: {stdout}");
    assert!(
        stdout.contains("dead-outputs: `verwaist`"),
        "stdout: {stdout}"
    );
    // Findings name their declaring fragment.
    assert!(stdout.contains("sued.lxir"), "stdout: {stdout}");

    // Outside a project (module path): source lints only, with a note —
    // an unresolvable extern is fine here because nothing compiles.
    std::fs::write(
        dir.join("src/_periphery.lxir"),
        format!("{PERIPHERY}extern lonely = VirtualIn(iname: \"VI4\")\n"),
    )
    .unwrap();
    let out = lxir(&dir, &["lint", "src"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "expected findings: {stdout}");
    assert!(stdout.contains("unused-extern: `lonely`"), "{stdout}");
    assert!(stdout.contains("_periphery.lxir"), "{stdout}");
    assert!(!stdout.contains("dead-outputs"), "{stdout}");
    assert!(stderr.contains("dead-output analysis skipped"), "{stderr}");
}
