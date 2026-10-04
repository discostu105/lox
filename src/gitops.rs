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
use crate::loxone_xml;

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
pub(crate) fn ms_dir(cfg: &Config) -> String {
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

    // 4. Semantic diff against the previous snapshot (lxir, refs folded)
    let xml_path = ms_path.join("config.Loxone");
    let change = if xml_path.exists() {
        let old_xml = std::fs::read(&xml_path)?;
        match crate::logic::diff_lines(&old_xml, &xml_data) {
            Ok(lines) => Change::Logic(lines),
            Err(e) => Change::Unavailable(format!("{:#}", e)),
        }
    } else {
        Change::Initial(crate::logic::Logic::parse(&xml_data).ok().map(|l| {
            (
                l.pages().len(),
                l.browsable().count(),
                l.folded_wires().len(),
            )
        }))
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
    let commit_msg = build_commit_message(&ms, &metadata, &change);
    if !quiet {
        println!("{}", commit_msg);
    }

    // page headers start with `#`: keep them even with commit.cleanup=strip
    git(repo, &["commit", "--cleanup=whitespace", "-m", &commit_msg])?;

    if !quiet {
        println!("Committed.");
    }
    Ok(true)
}

/// What changed since the previous snapshot, for the commit message.
enum Change {
    /// The first snapshot: page, block and wire counts (if lxir reads it)
    Initial(Option<(usize, usize, usize)>),
    /// Semantic diff lines (`logic::diff_lines`)
    Logic(Vec<String>),
    /// lxir could not read one of the configs
    Unavailable(String),
}

/// Commit bodies stay readable in `git log`; the rest is `lox config diff`.
const MAX_BODY_LINES: usize = 80;

/// Build a semantic commit message from the config diff.
fn build_commit_message(ms: &str, meta: &Metadata, change: &Change) -> String {
    let mut msg = format!(
        "[{}] Config backup {} (v{})",
        ms, meta.backup_date, meta.config_version
    );

    match change {
        Change::Initial(logic) => {
            msg.push_str("\n\nInitial config snapshot.");
            msg.push_str(&format!(
                "\n{} controls, {} rooms, {} categories, {} users",
                meta.controls, meta.rooms, meta.categories, meta.users
            ));
            if let Some((pages, blocks, wires)) = logic {
                msg.push_str(&format!(
                    "\n{} pages, {} blocks, {} wires",
                    pages, blocks, wires
                ));
            }
        }
        Change::Logic(lines) if lines.is_empty() => {
            msg.push_str("\n\nNo logic changes (layout or metadata only).");
        }
        Change::Logic(lines) => {
            msg.push('\n');
            for (i, l) in lines.iter().enumerate() {
                if i == MAX_BODY_LINES {
                    msg.push_str(&format!(
                        "\n… {} more lines — see `lox config diff`",
                        lines.len() - i
                    ));
                    break;
                }
                // a blank line before each page
                if l.starts_with("# ") {
                    msg.push('\n');
                }
                msg.push('\n');
                msg.push_str(l);
            }
        }
        Change::Unavailable(e) => {
            msg.push_str(&format!("\n\nLogic diff unavailable: {}", e));
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

    fn meta() -> Metadata {
        Metadata {
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
        }
    }

    #[test]
    fn test_build_commit_message_initial() {
        let msg = build_commit_message("ABC123", &meta(), &Change::Initial(Some((19, 1229, 402))));
        assert!(msg.contains("[ABC123]"));
        assert!(msg.contains("v42"));
        assert!(msg.contains("Initial config snapshot"));
        assert!(msg.contains("150 controls"));
        assert!(msg.contains("19 pages, 1229 blocks, 402 wires"));
    }

    #[test]
    fn test_build_commit_message_with_changes() {
        let lines = vec![
            "= 1 added · wires +1 −0".to_string(),
            "# Garage".into(),
            "+ block  Garage Light (Switch)".into(),
            "+ wire   Taster.Q → Garage Light.On".into(),
        ];
        let msg = build_commit_message("ABC123", &meta(), &Change::Logic(lines));
        assert!(
            msg.ends_with(
                "(v42)\n\n= 1 added · wires +1 −0\n\n# Garage\n+ block  Garage Light (Switch)\n+ wire   Taster.Q → Garage Light.On"
            ),
            "{}",
            msg
        );
    }

    #[test]
    fn test_build_commit_message_caps_long_diffs() {
        let mut lines = vec!["= 200 added".to_string(), "# Page".into()];
        lines.extend((0..200).map(|i| format!("+ block  B{} (And)", i)));
        let msg = build_commit_message("ABC123", &meta(), &Change::Logic(lines));
        assert!(msg.contains("… 122 more lines"), "{}", msg);
        assert!(msg.lines().count() < 90);
    }

    #[test]
    fn test_build_commit_message_no_logic_changes() {
        let msg = build_commit_message("ABC123", &meta(), &Change::Logic(Vec::new()));
        assert!(msg.contains("No logic changes"));
        let msg = build_commit_message("ABC123", &meta(), &Change::Unavailable("bad xml".into()));
        assert!(msg.contains("Logic diff unavailable: bad xml"));
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
