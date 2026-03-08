use anyhow::{Context, Result};
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
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
    /// IANA timezone name (e.g. "Europe/Vienna"). Uses system local time if unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

impl Config {
    pub fn dir() -> PathBuf { home_dir().unwrap_or_default().join(".lox") }
    pub fn path() -> PathBuf { Self::dir().join("config.yaml") }

    pub fn load() -> Result<Self> {
        let path = Self::path();
        let content = fs::read_to_string(&path)
            .with_context(|| "Config not found. Run: lox config set --host ... --user ... --pass ...")?;
        Ok(serde_yaml::from_str(&content)?)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, serde_yaml::to_string(self)?)?;
        println!("✓  Config saved to {:?}", path);
        Ok(())
    }
}
