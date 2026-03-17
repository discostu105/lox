//! GitOps workflow for version-controlled Loxone config backups.
//!
//! Automates: FTP download → LoxCC decompress → semantic diff → git commit.
//! Repository layout per Miniserver (by serial or hostname):
//!
//! ```text
//! <repo>/
//!   <serial>/
//!     config.Loxone    # decompressed XML
//!     backup.zip       # original backup for safe restore
//!     metadata.yaml    # firmware version, timestamp, counts
//! ```

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;
use crate::ftp;
use crate::loxcc;
use crate::loxone_xml::{self, ConfigDiff};

/// Build a git Command with consistent config for the managed repo.
/// Disables GPG signing and sets a fallback author if not configured,
/// so commits work in CI, cron, and container environments.
fn git_cmd(repo: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo).arg("-c").arg("commit.gpgsign=false");
    cmd
}

/// Run a git command in the given repo directory. Returns stdout on success.
fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let output = git_cmd(repo)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run a git command, returning Ok(stdout) even on non-zero exit (for queries).
fn git_ok(repo: &Path, args: &[&str]) -> Option<String> {
    git_cmd(repo)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Derive a subdirectory name for this Miniserver.
fn ms_dir(cfg: &Config) -> String {
    if !cfg.serial.is_empty() {
        cfg.serial.clone()
    } else {
        // Fallback to hostname (strip scheme/port)
        cfg.host
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split(':')
            .next()
            .unwrap_or("default")
            .replace(['/', '.'], "_")
    }
}

/// Initialize a git repository for config tracking.
pub fn init(path: &Path, cfg: &Config) -> Result<PathBuf> {
    let repo = path.to_path_buf();
    std::fs::create_dir_all(&repo)
        .with_context(|| format!("Cannot create directory {}", repo.display()))?;

    // git init (idempotent — safe if already a repo)
    git(&repo, &["init"])?;

    // Ensure user identity is configured for this repo (needed for commits in
    // headless/cron environments where global git config may not exist).
    if git_ok(&repo, &["config", "user.name"]).is_none() {
        git(&repo, &["config", "user.name", "lox"])?;
    }
    if git_ok(&repo, &["config", "user.email"]).is_none() {
        git(&repo, &["config", "user.email", "lox@localhost"])?;
    }

    // Write .gitignore
    let gitignore = repo.join(".gitignore");
    if !gitignore.exists() {
        std::fs::write(&gitignore, "# lox config gitops\n*.tmp\n")?;
        git(&repo, &["add", ".gitignore"])?;
        // Only commit if there's something to commit
        if git_ok(&repo, &["diff", "--cached", "--quiet"]).is_none() {
            git(
                &repo,
                &["commit", "-m", "Initial commit — lox config gitops"],
            )?;
        }
    }

    // Create the miniserver subdirectory
    let ms = ms_dir(cfg);
    let ms_path = repo.join(&ms);
    std::fs::create_dir_all(&ms_path)?;

    Ok(repo)
}

/// Metadata stored alongside each config snapshot.
#[derive(serde::Serialize, serde::Deserialize)]
struct Metadata {
    miniserver: String,
    serial: String,
    backup_file: String,
    backup_date: String,
    config_version: String,
    config_date: String,
    controls: usize,
    rooms: usize,
    categories: usize,
    users: usize,
}

/// Pull the latest config, decompress, diff, and commit.
/// Returns `Ok(true)` if a new commit was created, `Ok(false)` if no changes.
pub fn pull(repo: &Path, cfg: &Config, quiet: bool) -> Result<bool> {
    let ms = ms_dir(cfg);
    let ms_path = repo.join(&ms);
    std::fs::create_dir_all(&ms_path)?;

    // 1. List and download latest backup via FTP
    if !quiet {
        eprintln!("Fetching backup list from Miniserver...");
    }
    let backups = ftp::list_backups(cfg)?;
    if backups.is_empty() {
        bail!("No configs found on the Miniserver.");
    }
    let newest = &backups[0];
    if !quiet {
        eprintln!(
            "Downloading {} ({} KB)...",
            newest.filename,
            newest.size / 1024
        );
    }
    let zip_data = ftp::download_backup(cfg, &newest.filename)?;

    // 2. Decompress LoxCC → XML
    let xml_data = loxcc::extract_and_decompress(&zip_data)?;
    if !quiet {
        eprintln!(
            "Decompressed {} KB → {} KB",
            zip_data.len() / 1024,
            xml_data.len() / 1024
        );
    }

    // 3. Parse new config summary
    let new_summary = loxone_xml::parse_config_summary(&xml_data)?;

    // 4. Load previous config (if exists) for diffing
    let xml_path = ms_path.join("config.Loxone");
    let diff = if xml_path.exists() {
        let old_xml = std::fs::read(&xml_path)?;
        let old_summary = loxone_xml::parse_config_summary(&old_xml)?;
        Some(loxone_xml::diff_configs(&old_summary, &new_summary))
    } else {
        None
    };

    // 5. Write files
    std::fs::write(&xml_path, &xml_data)?;
    std::fs::write(ms_path.join("backup.zip"), &zip_data)?;

    let metadata = Metadata {
        miniserver: cfg
            .host
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .to_string(),
        serial: cfg.serial.clone(),
        backup_file: newest.filename.clone(),
        backup_date: newest.formatted_date(),
        config_version: new_summary.version.clone(),
        config_date: new_summary.date.clone(),
        controls: new_summary.controls.len(),
        rooms: new_summary.rooms.len(),
        categories: new_summary.categories.len(),
        users: new_summary.users.len(),
    };
    std::fs::write(
        ms_path.join("metadata.yaml"),
        serde_yaml::to_string(&metadata)?,
    )?;

    // 6. Stage all changes
    git(repo, &["add", &format!("{}/", ms)])?;

    // 7. Check if there are staged changes
    if git_ok(repo, &["diff", "--cached", "--quiet"]).is_some() {
        if !quiet {
            println!("No changes detected.");
        }
        return Ok(false);
    }

    // 8. Generate commit message from diff
    let commit_msg = build_commit_message(&ms, &metadata, diff.as_ref());
    if !quiet {
        println!("{}", commit_msg);
    }

    git(repo, &["commit", "-m", &commit_msg])?;

    if !quiet {
        println!("Committed.");
    }
    Ok(true)
}

/// Build a semantic commit message from the config diff.
fn build_commit_message(ms: &str, meta: &Metadata, diff: Option<&ConfigDiff>) -> String {
    let mut msg = format!(
        "[{}] Config backup {} (v{})",
        ms, meta.backup_date, meta.config_version
    );

    match diff {
        None => {
            msg.push_str("\n\nInitial config snapshot.");
            msg.push_str(&format!(
                "\n{} controls, {} rooms, {} categories, {} users",
                meta.controls, meta.rooms, meta.categories, meta.users
            ));
        }
        Some(d) if !d.has_changes() => {
            msg.push_str("\n\nNo structural changes (metadata or internal IDs updated).");
        }
        Some(d) => {
            msg.push('\n');

            for c in &d.controls_added {
                msg.push_str(&format!(
                    "\n+ Added control: \"{}\" ({})",
                    c.name, c.control_type
                ));
            }
            for c in &d.controls_changed {
                msg.push_str(&format!(
                    "\n~ {}: \"{}\" -> \"{}\"",
                    c.name, c.old_value, c.new_value
                ));
            }
            for c in &d.controls_removed {
                msg.push_str(&format!(
                    "\n- Removed control: \"{}\" ({})",
                    c.name, c.control_type
                ));
            }
            for r in &d.rooms_added {
                msg.push_str(&format!("\n+ Added room: \"{}\"", r));
            }
            for r in &d.rooms_renamed {
                msg.push_str(&format!("\n~ Renamed room: \"{}\" -> \"{}\"", r.old, r.new));
            }
            for r in &d.rooms_removed {
                msg.push_str(&format!("\n- Removed room: \"{}\"", r));
            }
            for c in &d.categories_added {
                msg.push_str(&format!("\n+ Added category: \"{}\"", c));
            }
            for c in &d.categories_renamed {
                msg.push_str(&format!(
                    "\n~ Renamed category: \"{}\" -> \"{}\"",
                    c.old, c.new
                ));
            }
            for c in &d.categories_removed {
                msg.push_str(&format!("\n- Removed category: \"{}\"", c));
            }
            for u in &d.users_added {
                msg.push_str(&format!("\n+ Added user: \"{}\"", u));
            }
            for u in &d.users_removed {
                msg.push_str(&format!("\n- Removed user: \"{}\"", u));
            }
        }
    }

    msg
}

/// Show git log for the config repo.
pub fn log(repo: &Path, ms_filter: Option<&str>, count: usize) -> Result<String> {
    let mut args = vec!["log", "--oneline", "--decorate"];
    let count_str = format!("-{}", count);
    args.push(&count_str);

    // If filtering by miniserver, restrict to that path
    let ms_path;
    if let Some(ms) = ms_filter {
        args.push("--");
        ms_path = format!("{}/", ms);
        args.push(&ms_path);
    }

    git(repo, &args)
}

/// Restore a config from a specific commit.
/// Checks out the backup.zip from that commit and uploads it via FTP.
pub fn restore(repo: &Path, cfg: &Config, commit: &str, force: bool) -> Result<()> {
    let ms = ms_dir(cfg);
    let zip_rel = format!("{}/backup.zip", ms);

    // Verify the commit and file exist
    let blob = format!("{}:{}", commit, zip_rel);
    let check = Command::new("git")
        .args(["cat-file", "-t", &blob])
        .current_dir(repo)
        .output()?;
    if !check.status.success() {
        bail!(
            "No backup.zip found for Miniserver '{}' at commit {}",
            ms,
            commit
        );
    }

    // Show what we're about to restore
    let log_line = git(repo, &["log", "--oneline", "-1", commit])?;
    eprintln!("Restoring config from: {}", log_line);

    if !force {
        eprintln!(
            "\nWARNING: This will upload the config to the Miniserver.\n\
             A bad configuration can require physical SD card access to recover.\n\n\
             Use --force to proceed."
        );
        std::process::exit(1);
    }

    // Extract backup.zip from that commit
    let show_output = Command::new("git")
        .args(["show", &blob])
        .current_dir(repo)
        .output()
        .context("Failed to extract backup.zip from git")?;
    if !show_output.status.success() {
        bail!("Failed to read {} from commit {}", zip_rel, commit);
    }
    let zip_data = show_output.stdout;

    // Determine the filename from metadata at that commit
    let meta_blob = format!("{}:{}/metadata.yaml", commit, ms);
    let filename = Command::new("git")
        .args(["show", &meta_blob])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            let meta: Metadata = serde_yaml::from_str(&text).ok()?;
            Some(meta.backup_file)
        })
        .unwrap_or_else(|| "restore.zip".to_string());

    eprintln!("Uploading {} ({} KB)...", filename, zip_data.len() / 1024);
    ftp::upload_backup(cfg, &filename, &zip_data)?;
    println!("Upload complete.");
    println!("Reboot the Miniserver to apply: lox reboot");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ms_dir_with_serial() {
        let cfg = Config {
            serial: "504F94AABBCC".into(),
            host: "https://192.168.1.77".into(),
            ..Default::default()
        };
        assert_eq!(ms_dir(&cfg), "504F94AABBCC");
    }

    #[test]
    fn test_ms_dir_without_serial() {
        let cfg = Config {
            serial: String::new(),
            host: "https://192.168.1.77".into(),
            ..Default::default()
        };
        assert_eq!(ms_dir(&cfg), "192_168_1_77");
    }

    #[test]
    fn test_build_commit_message_initial() {
        let meta = Metadata {
            miniserver: "192.168.1.77".into(),
            serial: "ABC123".into(),
            backup_file: "sps_194_20260308182256.zip".into(),
            backup_date: "2026-03-08 18:22:56".into(),
            config_version: "42".into(),
            config_date: "2026-03-08".into(),
            controls: 150,
            rooms: 12,
            categories: 8,
            users: 3,
        };
        let msg = build_commit_message("ABC123", &meta, None);
        assert!(msg.contains("[ABC123]"));
        assert!(msg.contains("v42"));
        assert!(msg.contains("Initial config snapshot"));
        assert!(msg.contains("150 controls"));
    }

    #[test]
    fn test_build_commit_message_with_changes() {
        let meta = Metadata {
            miniserver: "192.168.1.77".into(),
            serial: "ABC123".into(),
            backup_file: "sps_195_20260309100000.zip".into(),
            backup_date: "2026-03-09 10:00:00".into(),
            config_version: "43".into(),
            config_date: "2026-03-09".into(),
            controls: 151,
            rooms: 12,
            categories: 8,
            users: 3,
        };
        let diff = loxone_xml::ConfigDiff {
            version_old: "42".into(),
            version_new: "43".into(),
            date_old: "2026-03-08".into(),
            date_new: "2026-03-09".into(),
            controls_added: vec![loxone_xml::ControlEntry {
                name: "Garage Light".into(),
                control_type: "Switch".into(),
            }],
            controls_removed: vec![],
            controls_changed: vec![],
            rooms_added: vec![],
            rooms_removed: vec![],
            rooms_renamed: vec![],
            categories_added: vec![],
            categories_removed: vec![],
            categories_renamed: vec![],
            users_added: vec![],
            users_removed: vec![],
        };
        let msg = build_commit_message("ABC123", &meta, Some(&diff));
        assert!(msg.contains("+ Added control: \"Garage Light\" (Switch)"));
    }

    #[test]
    fn test_build_commit_message_no_structural_changes() {
        let meta = Metadata {
            miniserver: "192.168.1.77".into(),
            serial: "ABC123".into(),
            backup_file: "sps_195_20260309100000.zip".into(),
            backup_date: "2026-03-09 10:00:00".into(),
            config_version: "43".into(),
            config_date: "2026-03-09".into(),
            controls: 150,
            rooms: 12,
            categories: 8,
            users: 3,
        };
        let diff = loxone_xml::ConfigDiff {
            version_old: "42".into(),
            version_new: "43".into(),
            date_old: "2026-03-08".into(),
            date_new: "2026-03-09".into(),
            controls_added: vec![],
            controls_removed: vec![],
            controls_changed: vec![],
            rooms_added: vec![],
            rooms_removed: vec![],
            rooms_renamed: vec![],
            categories_added: vec![],
            categories_removed: vec![],
            categories_renamed: vec![],
            users_added: vec![],
            users_removed: vec![],
        };
        let msg = build_commit_message("ABC123", &meta, Some(&diff));
        assert!(msg.contains("No structural changes"));
    }

    #[test]
    fn test_git_init_creates_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = Config {
            serial: "TEST123".into(),
            host: "https://192.168.1.1".into(),
            ..Default::default()
        };
        let repo = init(tmp.path(), &cfg).unwrap();
        assert!(repo.join(".git").exists());
        assert!(repo.join(".gitignore").exists());
        assert!(repo.join("TEST123").is_dir());
    }
}
