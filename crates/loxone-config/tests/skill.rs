//! The agent skill (skills/lox-logic/SKILL.md) teaches by example: its module
//! snippet must stay valid as the language moves.

use std::process::Command;

#[test]
fn skill_example_passes_check() {
    let skill = std::fs::read_to_string("skills/lox-logic/SKILL.md").unwrap();
    let (_, rest) = skill.split_once("```text\n").expect("a module snippet");
    let (snippet, _) = rest.split_once("```").unwrap();
    let path = std::env::temp_dir().join(format!("loxl-skill-{}.loxl", std::process::id()));
    std::fs::write(&path, snippet).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_loxl"))
        .arg("check")
        .arg(&path)
        .output()
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
