//! The visualization password and secured commands. Controls with
//! `isSecured` in the structure file (an NFC Code Touch, for one) refuse
//! plain `jdev/sps/io/{uuid}/{cmd}` with code 500 and take the command only
//! as `jdev/sps/ios/{hash}/{uuid}/{cmd}`, the hash made from the connecting
//! user's visualization password and a single-use key from
//! `jdev/sys/getvisusalt/{user}`.
//!
//! The password never comes from the command line: it is read from a file
//! (`--visu-password-file`, `visu_password_file:` in the config), the
//! `LOX_VISU_PASSWORD` environment variable, or a hidden prompt. It is never
//! printed or logged; `Debug` redacts it.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::config::Config;

/// The environment variable that holds the visualization password.
pub const ENV: &str = "LOX_VISU_PASSWORD";

/// A visualization password.
#[derive(Clone)]
pub struct VisuPassword(String);

impl std::fmt::Debug for VisuPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VisuPassword(<redacted>)")
    }
}

impl VisuPassword {
    fn new(pw: String) -> Option<Self> {
        (!pw.is_empty()).then_some(Self(pw))
    }
}

/// The command that fetches a single-use key and salt for `user`.
pub fn salt_cmd(user: &str) -> String {
    format!("jdev/sys/getvisusalt/{}", user)
}

/// `cmd` at `uuid` as a secured command, signed with `password` and the
/// answer to [`salt_cmd`].
pub fn secured_cmd(password: &VisuPassword, salt: &Value, uuid: &str, cmd: &str) -> Result<String> {
    let hash = crate::token::visu_hash(&password.0, salt)?;
    Ok(format!("jdev/sps/ios/{}/{}/{}", hash, uuid, cmd))
}

/// `~/…` expanded to the home directory.
pub fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(path),
    }
}

/// The password in `path`: its first line, without the line ending.
pub fn read_file(path: &Path) -> Result<VisuPassword> {
    let text = std::fs::read_to_string(path).with_context(|| {
        format!(
            "could not read the visualization password file {} — check the path in \
             --visu-password-file or `visu_password_file:` in the config",
            path.display()
        )
    })?;
    let first = text.lines().next().unwrap_or("").to_string();
    match VisuPassword::new(first) {
        Some(pw) => Ok(pw),
        None => bail!(
            "the visualization password file {} is empty — write the password on its first line",
            path.display()
        ),
    }
}

/// The password from the settings, first match wins: `flag_file`
/// (`--visu-password-file`), `LOX_VISU_PASSWORD`, `visu_password_file:` in
/// the config. `None` when none is set.
pub fn from_settings(flag_file: Option<&Path>, cfg: &Config) -> Result<Option<VisuPassword>> {
    if let Some(path) = flag_file {
        return read_file(path).map(Some);
    }
    if let Some(pw) = std::env::var(ENV).ok().and_then(VisuPassword::new) {
        return Ok(Some(pw));
    }
    match &cfg.visu_password_file {
        Some(path) => read_file(&expand_home(path)).map(Some),
        None => Ok(None),
    }
}

/// Ask for the password without echoing it.
pub fn prompt(user: &str) -> Result<VisuPassword> {
    let pw = rpassword::prompt_password(format!("Visualization password of {}: ", user))?;
    VisuPassword::new(pw).context("no visualization password entered")
}

/// The error for a secured control without a password.
pub fn missing(control: &str, user: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{} is secured: its commands need the visualization password of {} — set {}, put \
         `visu_password_file: ~/.lox/visu-password` in the config (`lox setup set \
         --visu-password-file <path>`), or run it in a terminal to be asked",
        control,
        user,
        ENV
    )
}

/// The key and salt in an answer to [`salt_cmd`]; `None` for an answer to
/// another command. Some firmware sends the value as a JSON string.
pub fn parse_salt_reply(text: &str) -> Option<Result<Value>> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    let ll = v.get("LL")?;
    let control = ll.get("control").and_then(Value::as_str).unwrap_or("");
    if !control.contains("getvisusalt") {
        return None;
    }
    let code = ll
        .get("Code")
        .or_else(|| ll.get("code"))
        .and_then(crate::json_val_str)
        .unwrap_or_default();
    if code.trim() != "200" {
        return Some(Err(anyhow::anyhow!(
            "the Miniserver refused getvisusalt (code {}) — the user lox connects with \
             may not exist or may not be allowed to use secured commands",
            code.trim()
        )));
    }
    let value = ll.get("value").cloned().unwrap_or(Value::Null);
    let value = match value.as_str().map(serde_json::from_str::<Value>) {
        Some(Ok(obj @ Value::Object(_))) => obj,
        _ => value,
    };
    Some(Ok(value))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn password(pw: &str) -> VisuPassword {
        VisuPassword(pw.to_string())
    }

    #[test]
    fn debug_never_shows_the_password() {
        let pw = Some(password("correct horse"));
        assert!(!format!("{:?}", pw).contains("correct horse"));
        assert!(!format!("{:#?}", pw).contains("correct horse"));
    }

    #[test]
    fn the_secured_command_carries_the_hash() {
        // the synthetic vector of token::tests::test_visu_hash_sha256
        let salt = serde_json::json!({
            "key": "0123456789abcdef0123456789abcdef",
            "salt": "73616c7479",
            "hashAlg": "SHA256",
        });
        assert_eq!(
            secured_cmd(
                &password("correct horse"),
                &salt,
                "0f1e2d3c-000d-1111-ffff00000000000d",
                "nfc/startlearn"
            )
            .unwrap(),
            "jdev/sps/ios/14bcf905db0aba629be845ec52001f25b98c2f1a689cca7248f35bbc0d22a6e4/\
             0f1e2d3c-000d-1111-ffff00000000000d/nfc/startlearn"
        );
        assert_eq!(salt_cmd("alice"), "jdev/sys/getvisusalt/alice");
    }

    #[test]
    fn salt_replies_as_object_or_string() {
        let obj = serde_json::json!({"LL": {"control": "jdev/sys/getvisusalt/alice",
            "value": {"key": "00ff", "salt": "ab", "hashAlg": "SHA256"}, "Code": "200"}});
        let v = parse_salt_reply(&obj.to_string()).unwrap().unwrap();
        assert_eq!(v["key"], "00ff");
        let s = serde_json::json!({"LL": {"control": "dev/sys/getvisusalt/alice",
            "value": "{\"key\":\"00ff\",\"salt\":\"ab\"}", "code": 200}});
        let v = parse_salt_reply(&s.to_string()).unwrap().unwrap();
        assert_eq!(v["salt"], "ab");
    }

    #[test]
    fn other_replies_are_not_salt_and_refusals_are_errors() {
        let other = serde_json::json!({"LL": {"control": "jdev/sps/io/x/nfc/stoplearn",
            "value": "", "Code": "200"}});
        assert!(parse_salt_reply(&other.to_string()).is_none());
        let refused = serde_json::json!({"LL": {"control": "jdev/sys/getvisusalt/alice",
            "value": "", "Code": "404"}});
        assert!(parse_salt_reply(&refused.to_string()).unwrap().is_err());
    }

    #[test]
    fn the_file_holds_the_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("visu-password");
        std::fs::write(&p, "s3cret\r\n").unwrap();
        assert_eq!(read_file(&p).unwrap().0, "s3cret");
        std::fs::write(&p, "\n").unwrap();
        let err = format!("{:#}", read_file(&p).unwrap_err());
        assert!(err.contains("is empty"), "{}", err);
        let err = format!("{:#}", read_file(&dir.path().join("nope")).unwrap_err());
        assert!(err.contains("could not read"), "{}", err);
    }

    #[test]
    fn the_flag_file_wins_and_the_config_path_expands_home() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("pw");
        std::fs::write(&p, "from-file\n").unwrap();
        let cfg = Config {
            visu_password_file: Some(dir.path().join("missing").display().to_string()),
            ..Default::default()
        };
        assert_eq!(
            from_settings(Some(&p), &cfg).unwrap().unwrap().0,
            "from-file"
        );
        assert_eq!(
            expand_home("~/.lox/visu-password"),
            dirs::home_dir()
                .unwrap_or_default()
                .join(".lox/visu-password")
        );
        assert_eq!(expand_home("/etc/pw"), PathBuf::from("/etc/pw"));
    }
}
