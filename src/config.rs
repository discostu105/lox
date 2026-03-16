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
        let content = fs::read_to_string(&path).with_context(|| {
            "Config not found. Run: lox config set --host ... --user ... --pass ..."
        })?;
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
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, serde_yaml::to_string(self)?)?;
        #[cfg(unix)]
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        Ok(path)
    }
}
