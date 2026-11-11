//! `lox setup` and `lox ctx` never replace a config they could not read, and
//! the config file stays private. Unix only: the tests place the global
//! config through `HOME`.
#![cfg(unix)]

use assert_cmd::Command;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const FLAT: &str = "host: https://house.example\nuser: admin\npass: s3cret\naliases:\n  lamp: 1a2b3c4d-0001-0001-ffff000000000001\n";

struct Home(tempfile::TempDir);

impl Home {
    fn new() -> Self {
        Home(tempfile::tempdir().unwrap())
    }

    fn with_config(content: &str) -> Self {
        let home = Self::new();
        fs::create_dir_all(home.config().parent().unwrap()).unwrap();
        fs::write(home.config(), content).unwrap();
        home
    }

    fn config(&self) -> PathBuf {
        self.0.path().join(".lox").join("config.yaml")
    }

    fn lox(&self) -> Command {
        let mut cmd = Command::cargo_bin("lox").unwrap();
        cmd.env_remove("LOX_CONFIG")
            .env("HOME", self.0.path())
            .current_dir(self.0.path());
        cmd
    }

    fn read(&self) -> String {
        fs::read_to_string(self.config()).unwrap()
    }
}

fn stderr(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap();
    assert!(!out.status.success(), "expected a failure");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn ctx_add_does_not_replace_a_single_miniserver_config() {
    let home = Home::with_config(FLAT);
    let err = stderr(home.lox().args([
        "ctx",
        "add",
        "office",
        "--host",
        "office.example",
        "--user",
        "u",
        "--pass",
        "p",
    ]));
    assert!(err.contains("lox ctx migrate"), "{err}");
    assert_eq!(home.read(), FLAT);

    // After the migration both are there.
    home.lox().args(["ctx", "migrate"]).assert().success();
    home.lox()
        .args([
            "ctx",
            "add",
            "office",
            "--host",
            "office.example",
            "--user",
            "u",
            "--pass",
            "p",
        ])
        .assert()
        .success();
    let saved = home.read();
    assert!(
        saved.contains("house.example") && saved.contains("office.example"),
        "{saved}"
    );
    assert!(
        saved.contains("s3cret") && saved.contains("lamp"),
        "{saved}"
    );
}

#[test]
fn ctx_commands_do_not_replace_a_config_they_cannot_read() {
    // The second context lacks its password.
    let broken = "active_context: home\ncontexts:\n  home:\n    host: https://h\n    user: u\n    pass: p\n  cabin:\n    host: https://c\n    user: u\n";
    for args in [
        vec![
            "ctx", "add", "office", "--host", "o", "--user", "u", "--pass", "p",
        ],
        vec!["ctx", "use", "home"],
        vec!["ctx", "remove", "home"],
        vec!["ctx", "rename", "home", "house"],
    ] {
        let home = Home::with_config(broken);
        let err = stderr(home.lox().args(&args));
        assert!(err.contains("config.yaml"), "{args:?}: {err}");
        assert_eq!(home.read(), broken, "{args:?}");
    }
}

#[test]
fn setup_set_does_not_replace_contexts_when_none_is_active() {
    let contexts = "contexts:\n  home:\n    host: https://h\n    user: u\n    pass: p\n  cabin:\n    host: https://c\n    user: u\n    pass: p\n";
    let home = Home::with_config(contexts);
    let err = stderr(home.lox().args(["setup", "set", "--host", "other.example"]));
    assert!(err.contains("lox ctx use"), "{err}");
    assert_eq!(home.read(), contexts);

    // The same with a context that does not exist.
    let err = stderr(
        home.lox()
            .args(["--ctx", "typo", "setup", "set", "--pass", "x"]),
    );
    assert!(err.contains("typo"), "{err}");
    assert_eq!(home.read(), contexts);
}

#[test]
fn setup_set_creates_a_private_config_and_keeps_what_it_does_not_change() {
    let home = Home::new();
    home.lox()
        .args([
            "setup",
            "set",
            "--host",
            "house.example",
            "--user",
            "admin",
            "--pass",
            "s3cret",
        ])
        .assert()
        .success();
    let mode = |p: PathBuf| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(home.config()), 0o600);
    assert_eq!(mode(home.config().parent().unwrap().to_path_buf()), 0o700);

    home.lox()
        .args(["setup", "set", "--serial", "504F94A00000"])
        .assert()
        .success();
    let saved = home.read();
    assert!(
        saved.contains("https://house.example") && saved.contains("s3cret"),
        "{saved}"
    );
    assert!(saved.contains("504F94A00000"), "{saved}");
    assert_eq!(mode(home.config()), 0o600);
    // No temporary file is left next to it.
    let names: Vec<_> = fs::read_dir(home.config().parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["config.yaml"]);
}

#[test]
fn setup_set_tightens_a_config_that_was_readable_by_others() {
    let home = Home::with_config(FLAT);
    fs::set_permissions(home.config(), fs::Permissions::from_mode(0o644)).unwrap();
    home.lox()
        .args(["setup", "set", "--user", "other"])
        .assert()
        .success();
    assert_eq!(
        fs::metadata(home.config()).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn a_new_host_drops_the_cache_and_the_token_of_the_old_one() {
    let home = Home::with_config(FLAT);
    let dir = home.config().parent().unwrap().to_path_buf();
    let cache = dir.join("cache").join("structure.json");
    let token = dir.join("token.json");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();

    fs::write(&cache, "{}").unwrap();
    fs::write(&token, "{}").unwrap();
    home.lox()
        .args(["setup", "set", "--serial", "1"])
        .assert()
        .success();
    assert!(
        cache.exists() && token.exists(),
        "the same Miniserver keeps both"
    );

    home.lox()
        .args(["setup", "set", "--user", "guest"])
        .assert()
        .success();
    assert!(
        cache.exists() && !token.exists(),
        "another user needs a new token"
    );

    fs::write(&token, "{}").unwrap();
    home.lox()
        .args(["setup", "set", "--host", "cabin.example"])
        .assert()
        .success();
    assert!(!cache.exists() && !token.exists());
}

#[test]
fn a_scene_is_named_not_given_as_a_path() {
    let home = Home::with_config(FLAT);
    let outside = home.0.path().join("evil.yaml");
    fs::write(&outside, "steps:\n  - control: Door\n    cmd: open\n").unwrap();
    for name in [outside.with_extension("").to_str().unwrap(), "../evil"] {
        let err = stderr(home.lox().args(["run", name, "--dry-run"]));
        assert!(err.contains("is not a scene name"), "{name}: {err}");
        let err = stderr(home.lox().args(["scene", "show", name]));
        assert!(err.contains("is not a scene name"), "{name}: {err}");
    }
    let err = stderr(home.lox().args(["scene", "new", "../planted"]));
    assert!(err.contains("is not a scene name"), "{err}");
    assert!(!home.0.path().join(".lox").join("planted.yaml").exists());
}

#[test]
fn lox_variables_work_without_a_config_file_and_are_never_saved() {
    let home = Home::new();
    let out = home
        .lox()
        .env("LOX_HOST", "house.example")
        .env("LOX_USER", "admin")
        .env("LOX_PASS", "s3cret")
        .args(["setup", "show"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let shown = String::from_utf8_lossy(&out.stdout);
    assert!(
        shown.contains("https://house.example") && shown.contains("admin"),
        "{shown}"
    );
    assert!(!shown.contains("s3cret"), "{shown}");
    assert!(!home.config().exists());

    // A stored config: the variable wins for this run, the file keeps its own.
    let home = Home::with_config(FLAT);
    let out = home
        .lox()
        .env("LOX_USER", "guest")
        .args([
            "alias",
            "add",
            "pump",
            "1a2b3c4d-0001-0002-ffff000000000002",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let saved = home.read();
    assert!(
        saved.contains("user: admin") && saved.contains("pump"),
        "{saved}"
    );
    let shown = home
        .lox()
        .env("LOX_USER", "guest")
        .args(["setup", "show"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&shown.stdout).contains("guest"));
}
