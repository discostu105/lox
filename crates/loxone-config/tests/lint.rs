//! End-to-end test of `loxl lint` through the real binary: source lints
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

fn loxl(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_loxl"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run loxl")
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
    std::fs::write(dir.join("src/_periphery.loxl"), PERIPHERY).unwrap();
    std::fs::write(dir.join("src/sued.loxl"), SUED).unwrap();
    dir
}

#[test]
fn lint_reports_and_clears() {
    let dir = make_project("lint-e2e");
    let compile = loxl(&dir, &["compile"]);
    assert!(compile.status.success());

    // Everything referenced, every block consumed: clean, exit 0.
    let out = loxl(&dir, &["lint"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "expected clean: {stdout}");
    assert!(stdout.contains("lint: clean"), "{stdout}");

    // A block nothing consumes: flagged against the compiled config.
    std::fs::write(
        dir.join("src/sued.loxl"),
        format!("{SUED}\nverwaist = And(\n\tI1: sonne.Q,\n\tI2: wind_alarm.Q,\n)\n"),
    )
    .unwrap();
    assert!(loxl(&dir, &["compile"]).status.success());

    let out = loxl(&dir, &["lint"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(!out.status.success(), "expected findings: {stdout}");
    assert!(stdout.contains("dead-outputs (1)\n"), "stdout: {stdout}");
    assert!(stdout.contains("`verwaist` — "), "stdout: {stdout}");
    // Findings name their declaring fragment.
    assert!(stdout.contains("sued.loxl"), "stdout: {stdout}");

    // Outside a project (module path): source lints only, with a note —
    // an unresolvable extern is fine here because nothing compiles.
    std::fs::write(
        dir.join("src/_periphery.loxl"),
        format!("{PERIPHERY}extern lonely = VirtualIn(iname: \"VI4\")\n"),
    )
    .unwrap();
    let out = loxl(&dir, &["lint", "src"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "expected findings: {stdout}");
    assert!(stdout.contains("unused-extern (1)\n"), "{stdout}");
    assert!(stdout.contains("_periphery.loxl: `lonely` — "), "{stdout}");
    assert!(stdout.contains("_periphery.loxl"), "{stdout}");
    assert!(!stdout.contains("dead-outputs"), "{stdout}");
    assert!(stderr.contains("dead-output analysis skipped"), "{stderr}");
}

#[test]
fn values_outside_the_documented_range_are_flagged() {
    // D52: the range check reads the value in the port's own unit —
    // `2h` on a minutes port is 120 min, past the documented 60.
    let src = loxone_config::logic::Module::parse(
        "extern vi = VirtualIn(iname: \"VI1\")\n\
         extern jal = AutoJalousie(title: \"J\")\n\
         puls = Monoflop(InputTrigger: vi.Q, Time: 30s)\n\
         lc = LightController2(I1: puls.Q, AlarmClockPeriod: 2h)\n\
         jal.Safety <- puls.Q\n\
         jal.TargetPos = 120%\n",
    )
    .unwrap();
    let findings = loxone_config::logic::lint_source(&src).unwrap();
    let range: Vec<String> = findings
        .iter()
        .filter(|f| f.kind == loxone_config::logic::LintKind::OutOfRange)
        .map(|f| format!("{}: {}", f.slug, f.detail))
        .collect();
    assert_eq!(
        range,
        [
            "lc: AlarmClockPeriod = 120 min is outside the documented range 0 … 60 min",
            "jal: TargetPos = 120 % is outside the documented range 0 … 100 %",
        ]
    );
}

#[test]
fn a_file_without_its_page_in_a_paged_module_is_flagged() {
    // `sued.loxl` has no `page`; the file before it names one. Its
    // blocks go to the default page, not to that file's page.
    let dir = make_project("lint-default-page");
    std::fs::write(
        dir.join("src/a_nord.loxl"),
        "page \"Beschattung\"\n\nnord = Not(\n\tI: sonne.Q,\n)\n",
    )
    .unwrap();
    let out = loxl(&dir, &["lint", "src"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(!out.status.success(), "expected findings: {stdout}");
    assert!(stdout.contains("default-page (1)\n"), "{stdout}");
    assert!(
        stdout.contains("sued.loxl: `beschatten` — 1 block before any `page`"),
        "{stdout}"
    );
    // The fix names the page the file before it ends on.
    assert!(
        stdout.contains("Start the file with `page \"Beschattung\"`"),
        "{stdout}"
    );
    // With its own `page` line the file is clean.
    std::fs::write(
        dir.join("src/sued.loxl"),
        format!("page \"Beschattung\"\n\n{SUED}"),
    )
    .unwrap();
    let out = loxl(&dir, &["lint", "src"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(!stdout.contains("default-page"), "{stdout}");
}

#[test]
fn lint_groups_by_kind_and_summarizes() {
    let dir = make_project("lint-summary");
    std::fs::write(
        dir.join("src/_periphery.loxl"),
        format!(
            "{PERIPHERY}extern lonely = VirtualIn(iname: \"VI4\")\n\
             extern solo = VirtualIn(iname: \"VI5\")\nlet nie = 3\n"
        ),
    )
    .unwrap();
    let out = loxl(&dir, &["lint", "src"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(!out.status.success(), "{stdout}");
    let extern_at = stdout.find("unused-extern (2)\n").expect(&stdout);
    let let_at = stdout.find("unused-let (1)\n").expect(&stdout);
    assert!(extern_at < let_at, "{stdout}");
    assert!(stdout.ends_with("lint: 3 findings\n"), "{stdout}");

    let out = loxl(&dir, &["lint", "--summary", "src"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(!out.status.success(), "{stdout}");
    assert_eq!(
        stdout, "unused-extern  2\nunused-let     1\nlint: 3 findings\n",
        "{stdout}"
    );
}

#[test]
fn lint_fix_writes_unit_suffixes_and_keeps_the_compiled_config() {
    let dir = make_project("lint-fix");
    std::fs::write(
        dir.join("src/sued.loxl"),
        format!(
            "page \"Beschattung\"\n\n{SUED}\n\
             puls = Monoflop(InputTrigger: sonne.Q, Time: 300)\n\
             kurz = Monoflop(InputTrigger: puls.Q, Time: 0.02)\n\
             krumm = Monoflop(InputTrigger: kurz.Q, Time: 0.0005)\n\
             jal_sued.InputTrigger <- krumm.Q\n"
        ),
    )
    .unwrap();
    assert!(
        loxl(&dir, &["compile", "--out", "out.Loxone"])
            .status
            .success()
    );
    let before = std::fs::read(dir.join("out.Loxone")).unwrap();
    let lock = std::fs::read_to_string(dir.join("haus.lock.json")).unwrap();

    let out = loxl(&dir, &["lint"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.contains("unit-suffix (2)\n"), "{stdout}");
    assert!(
        stdout.contains("Write `Time: 5min` instead of `Time: 300`."),
        "{stdout}"
    );
    assert!(
        stdout.contains("(2 fixable with `loxl lint --fix`)"),
        "{stdout}"
    );

    let out = loxl(&dir, &["lint", "--fix"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stdout.starts_with("fixed: 2 unit-suffix values in 1 file\n"),
        "{stdout}{stderr}"
    );
    assert!(!stdout.contains("unit-suffix ("), "{stdout}");
    let src = std::fs::read_to_string(dir.join("src/sued.loxl")).unwrap();
    assert!(src.contains("Time: 5min)"), "{src}");
    assert!(src.contains("Time: 20ms)"), "{src}");
    // Half a millisecond has no exact suffix spelling: it stays a number.
    assert!(src.contains("Time: 0.0005)"), "{src}");

    // Same config, same lockfile.
    assert!(
        loxl(&dir, &["compile", "--out", "out.Loxone"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(dir.join("out.Loxone")).unwrap(), before);
    assert_eq!(
        std::fs::read_to_string(dir.join("haus.lock.json")).unwrap(),
        lock
    );
}

#[test]
fn lint_fix_covers_extern_sets_templates_and_device_settings() {
    let dir = make_project("lint-fix-wide");
    std::fs::write(
        dir.join("src/_vorlagen.loxl"),
        "template verz(zeit = 300, titel = \"Verz\")\n\
         \tm = Monoflop(\"{titel}\", Time: zeit)\n\
         end\n\n\
         # `zeit` also feeds a port without a unit, where a suffix reads differently.\n\
         template beschriftet(zeit = 300) -> m\n\
         \tm = Monoflop(Time: zeit)\n\
         \tn = Equal(Input1: m.Q, Input2: zeit)\n\
         end\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/sued.loxl"),
        format!(
            "page \"Beschattung\"\n\n{SUED}\n\
             a = verz(zeit: 120, InputTrigger: sonne.Q)\n\
             b = beschriftet(zeit: 120, InputTrigger: a.Q)\n\
             jal_sued.InputTrigger <- b.Q\n\
             zaehler = VirtualHttpIn(\"Zähler\", Address: \"http://192.0.2.10/\", Poll: 60, \
             Timeout: 4000)\n"
        ),
    )
    .unwrap();
    let out = loxl(&dir, &["compile", "--out", "out.Loxone"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let before = std::fs::read(dir.join("out.Loxone")).unwrap();
    let lock = std::fs::read_to_string(dir.join("haus.lock.json")).unwrap();

    let out = loxl(&dir, &["lint", "--fix"]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stdout.starts_with("fixed: 4 unit-suffix values in 2 files\n"),
        "{stdout}{stderr}"
    );
    let vorlagen = std::fs::read_to_string(dir.join("src/_vorlagen.loxl")).unwrap();
    assert!(
        vorlagen.contains("template verz(zeit = 5min,"),
        "{vorlagen}"
    );
    assert!(
        vorlagen.contains("template beschriftet(zeit = 300) -> m"),
        "{vorlagen}"
    );
    let sued = std::fs::read_to_string(dir.join("src/sued.loxl")).unwrap();
    assert!(sued.contains("verz(zeit: 2min,"), "{sued}");
    assert!(sued.contains("beschriftet(zeit: 120,"), "{sued}");
    // Documented in seconds and in milliseconds.
    assert!(sued.contains("Poll: 1min"), "{sued}");
    assert!(sued.contains("Timeout: 4s"), "{sued}");

    let out = loxl(&dir, &["compile", "--out", "out.Loxone"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(dir.join("out.Loxone")).unwrap(), before);
    assert_eq!(
        std::fs::read_to_string(dir.join("haus.lock.json")).unwrap(),
        lock
    );
}

#[test]
fn a_module_error_names_the_fragment_and_line() {
    let dir = make_project("check-located");
    std::fs::write(
        dir.join("src/sued.loxl"),
        format!("{SUED}\nnord = Not(I: nirgends.Q)\n"),
    )
    .unwrap();
    let out = loxl(&dir, &["check", "src"]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success());
    let line = SUED.lines().count() + 2;
    assert!(
        stderr.contains(&format!("sued.loxl:{line}: compile error:")),
        "{stderr}"
    );
    let out = loxl(&dir, &["check", "--json", "src"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["path"].as_str().unwrap().ends_with("sued.loxl"), "{v}");
    assert_eq!(v["errors"][0]["line"], line, "{v}");
}
