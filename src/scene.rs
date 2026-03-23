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
    /// Scenes directory for a specific config (context-aware).
    pub fn scenes_dir_for(cfg: &Config) -> PathBuf {
        cfg.scenes_dir()
    }

    /// Load a scene by name using context-aware path.
    pub fn load_with_config(name: &str, cfg: &Config) -> Result<Self> {
        Self::load_from(name, &Self::scenes_dir_for(cfg))
    }

    /// List all scene names using context-aware path.
    pub fn list_with_config(cfg: &Config) -> Result<Vec<String>> {
        Self::list_from(&Self::scenes_dir_for(cfg))
    }

    /// Load a scene by name from the given directory.
    pub(crate) fn load_from(name: &str, dir: &std::path::Path) -> Result<Self> {
        let path = dir.join(format!("{}.yaml", name));
        let content =
            fs::read_to_string(&path).with_context(|| format!("Scene '{}' not found", name))?;
        Ok(serde_yaml::from_str(&content)?)
    }

    /// List all scene names (without extension) from the given directory.
    pub(crate) fn list_from(dir: &std::path::Path) -> Result<Vec<String>> {
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut names = vec![];
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().map(|e| e == "yaml").unwrap_or(false)
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                names.push(stem.to_string());
            }
        }
        names.sort();
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scenes_dir_ends_with_scenes() {
        let cfg = Config::default();
        let dir = Scene::scenes_dir_for(&cfg);
        // Default config has empty data_dir, so this just checks the join
        assert!(
            dir.ends_with("scenes"),
            "scenes_dir_for() should end with 'scenes', got {:?}",
            dir
        );
    }

    #[test]
    fn load_valid_scene_from_temp_dir() {
        let dir = tempdir().unwrap();
        let yaml = r#"
name: test_scene
description: A test scene
steps:
  - control: "Light [Kitchen]"
    cmd: "on"
    delay_ms: 500
  - control: "Blind [Bedroom]"
    cmd: "FullDown"
"#;
        fs::write(dir.path().join("myscene.yaml"), yaml).unwrap();

        let scene = Scene::load_from("myscene", dir.path()).unwrap();
        assert_eq!(scene.name, Some("test_scene".to_string()));
        assert_eq!(scene.description, Some("A test scene".to_string()));
        assert_eq!(scene.steps.len(), 2);
        assert_eq!(scene.steps[0].control, "Light [Kitchen]");
        assert_eq!(scene.steps[0].cmd, "on");
        assert_eq!(scene.steps[0].delay_ms, 500);
        assert_eq!(scene.steps[1].control, "Blind [Bedroom]");
        assert_eq!(scene.steps[1].cmd, "FullDown");
        assert_eq!(scene.steps[1].delay_ms, 0); // default
    }

    #[test]
    fn load_returns_error_for_missing_scene() {
        let dir = tempdir().unwrap();
        let result = Scene::load_from("nonexistent", dir.path());
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(
            err_msg.contains("nonexistent"),
            "Error should mention scene name, got: {}",
            err_msg
        );
    }

    #[test]
    fn list_returns_empty_when_dir_does_not_exist() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("no_such_subdir");
        let names = Scene::list_from(&missing).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn list_finds_yaml_files_in_dir() {
        let dir = tempdir().unwrap();
        let scene_yaml = "steps:\n  - control: x\n    cmd: \"on\"\n";
        fs::write(dir.path().join("alpha.yaml"), scene_yaml).unwrap();
        fs::write(dir.path().join("beta.yaml"), scene_yaml).unwrap();
        // Non-yaml file should be ignored
        fs::write(dir.path().join("readme.txt"), "not a scene").unwrap();

        let names = Scene::list_from(dir.path()).unwrap();
        assert_eq!(names, vec!["alpha", "beta"]);
    }
}
