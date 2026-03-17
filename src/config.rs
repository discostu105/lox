use anyhow::{Context, Result};
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Config {
    pub host: String,
    pub user: String,
    pub pass: String,
    #[serde(default)]
    pub serial: String,
    #[serde(default)]
    pub aliases: HashMap<String, String>,
    /// Enable SSL certificate verification (default: false for Miniserver self-signed certs)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_ssl: Option<bool>,
    /// Path to a git repository for config version tracking (`lox config init`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_repo: Option<String>,
}

impl Config {
    pub fn dir() -> PathBuf {
        home_dir().unwrap_or_default().join(".lox")
    }
    pub fn path() -> PathBuf {
        Self::dir().join("config.yaml")
    }

    pub fn load() -> Result<Self> {
        let path = std::env::var("LOX_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| Self::path());
        let content = fs::read_to_string(&path).with_context(
            || "Config not found. Run: lox config set --host ... --user ... --pass ...",
        )?;
        let mut cfg: Self = serde_yaml::from_str(&content)?;
        if !cfg.host.is_empty()
            && !cfg.host.starts_with("http://")
            && !cfg.host.starts_with("https://")
        {
            cfg.host = format!("https://{}", cfg.host);
        }
        Ok(cfg)
    }

    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, serde_yaml::to_string(self)?)?;
        #[cfg(unix)]
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn dir_ends_with_dot_lox() {
        let dir = Config::dir();
        assert!(
            dir.ends_with(".lox"),
            "Config::dir() should end with .lox, got {:?}",
            dir
        );
    }

    #[test]
    fn path_ends_with_config_yaml() {
        let path = Config::path();
        assert!(
            path.ends_with("config.yaml"),
            "Config::path() should end with config.yaml, got {:?}",
            path
        );
    }

    #[test]
    fn load_from_lox_config_env_var() {
        let dir = tempdir().unwrap();
        let cfg_path = dir.path().join("test_config.yaml");
        let yaml = "host: myhost.local\nuser: admin\npass: secret\n";
        fs::write(&cfg_path, yaml).unwrap();

        // SAFETY: Test runs serially; no other threads access this env var.
        unsafe { std::env::set_var("LOX_CONFIG", cfg_path.to_str().unwrap()) };
        let cfg = Config::load().unwrap();
        unsafe { std::env::remove_var("LOX_CONFIG") };

        assert_eq!(cfg.user, "admin");
        assert_eq!(cfg.pass, "secret");
    }

    #[test]
    fn load_prepends_https_to_bare_hostname() {
        let dir = tempdir().unwrap();
        let cfg_path = dir.path().join("cfg.yaml");
        fs::write(&cfg_path, "host: miniserver.local\nuser: u\npass: p\n").unwrap();

        unsafe { std::env::set_var("LOX_CONFIG", cfg_path.to_str().unwrap()) };
        let cfg = Config::load().unwrap();
        unsafe { std::env::remove_var("LOX_CONFIG") };

        assert_eq!(cfg.host, "https://miniserver.local");
    }

    #[test]
    fn load_preserves_explicit_http_scheme() {
        let dir = tempdir().unwrap();
        let cfg_path = dir.path().join("cfg.yaml");
        fs::write(&cfg_path, "host: http://ms.local\nuser: u\npass: p\n").unwrap();

        unsafe { std::env::set_var("LOX_CONFIG", cfg_path.to_str().unwrap()) };
        let cfg = Config::load().unwrap();
        unsafe { std::env::remove_var("LOX_CONFIG") };

        assert_eq!(cfg.host, "http://ms.local");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempdir().unwrap();
        // Config::save() uses the hardcoded path, so we test roundtrip via
        // manual serialization + LOX_CONFIG-based load.
        let mut cfg = Config {
            host: "https://10.0.0.1".to_string(),
            user: "testuser".to_string(),
            pass: "testpass".to_string(),
            serial: "00:11:22:33:44:55".to_string(),
            ..Default::default()
        };
        cfg.aliases
            .insert("light".to_string(), "some-uuid".to_string());

        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let cfg_path = dir.path().join("roundtrip.yaml");
        fs::write(&cfg_path, &yaml).unwrap();

        unsafe { std::env::set_var("LOX_CONFIG", cfg_path.to_str().unwrap()) };
        let loaded = Config::load().unwrap();
        unsafe { std::env::remove_var("LOX_CONFIG") };

        assert_eq!(loaded.host, "https://10.0.0.1");
        assert_eq!(loaded.user, "testuser");
        assert_eq!(loaded.pass, "testpass");
        assert_eq!(loaded.serial, "00:11:22:33:44:55");
        assert_eq!(loaded.aliases.get("light"), Some(&"some-uuid".to_string()));
    }

    #[test]
    fn default_config_has_empty_fields() {
        let cfg = Config::default();
        assert!(cfg.host.is_empty());
        assert!(cfg.user.is_empty());
        assert!(cfg.pass.is_empty());
        assert!(cfg.serial.is_empty());
        assert!(cfg.aliases.is_empty());
        assert!(cfg.verify_ssl.is_none());
        assert!(cfg.config_repo.is_none());
    }
}
