//! `loxl scan`: the shipped configs carry no personal data, and a config
//! that does is refused with the place of each value, never the value.

use std::process::Command;

fn scan(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_loxl"))
        .arg("scan")
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

#[test]
fn shipped_configs_are_clean() {
    let (code, out) = scan(&["examples"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.ends_with("0 finding(s) in 7 config(s)\n"), "{out}");
}

#[test]
fn personal_data_is_refused_without_quoting_it() {
    let xml = String::from_utf8(std::fs::read("examples/configs/pool.Loxone").unwrap())
        .unwrap()
        .replacen(
            r#"Title="Pool" "#,
            r#"Title="Pool" Street="Seeweg 7" Latitude="47.5" "#,
            1,
        )
        .replacen(
            r#"Title="Miniserver""#,
            r#"Title="Miniserver" Serial="504F94A1B2C3""#,
            1,
        );
    let dir = std::env::temp_dir().join(format!("loxl-scan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("real.Loxone");
    std::fs::write(&path, xml).unwrap();
    let (code, out) = scan(&[path.to_str().unwrap()]);
    let (json_code, json) = scan(&["--json", path.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!((code, json_code), (1, 1));
    let p = path.display();
    assert_eq!(
        out,
        format!(
            "{p}: location: Document \"Pool\" Street (8 chars)\n\
             {p}: location: Document \"Pool\" Latitude (4 chars)\n\
             {p}: hardware-serial: LoxLIVE \"Miniserver\" Serial (12 chars)\n\
             3 finding(s) in 1 config(s) — personal data: keep these files out of the repository\n"
        )
    );
    for text in [&out, &json] {
        assert!(!text.contains("Seeweg") && !text.contains("47.5") && !text.contains("A1B2C3"));
    }
}
