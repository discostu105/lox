//! Scene definitions — multi-step command sequences

use anyhow::Context;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::config::Config;

#[derive(Debug, Serialize, Deserialize)]
pub struct SceneStep {
    pub control: String,
    pub cmd: String,
    #[serde(default)]
    pub delay_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Scene {
    pub name: Option<String>,
    pub description: Option<String>,
    pub steps: Vec<SceneStep>,
}

impl Scene {
    pub fn scenes_dir() -> PathBuf {
        Config::dir().join("scenes")
    }

    pub fn load(name: &str) -> Result<Self> {
        let path = Self::scenes_dir().join(format!("{}.yaml", name));
        let content =
            fs::read_to_string(&path).with_context(|| format!("Scene '{}' not found", name))?;
        Ok(serde_yaml::from_str(&content)?)
    }

    pub fn list() -> Result<Vec<String>> {
        let dir = Self::scenes_dir();
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut names = vec![];
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.extension().map(|e| e == "yaml").unwrap_or(false) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort();
        Ok(names)
    }
}
