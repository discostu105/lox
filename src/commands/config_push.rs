//! `lox config push` — deploy an lxir project to the Miniserver, verified end
//! to end.
//!
//! Each step stops the push when it fails:
//!
//! 1. Download the config the Miniserver runs now (kept under `backups/`).
//! 2. Drift check: the live config must still be the project's base, at least
//!    semantically (`lox ir diff` would be empty). A change saved from Loxone
//!    Config or the app since then is never overwritten.
//! 3. Compile the module against the live download, with a scratch copy of the
//!    lock. What lxir does not diff — users and their passwords, layout,
//!    visualization — stays as it is live instead of reverting to the base.
//! 4. Show what the push changes. Nothing to push ends here.
//! 5. Stage the upload: the downloaded ZIP with only `sps0.LoxCC` replaced,
//!    verified to extract to exactly the compiled file. Without `--force` the
//!    push ends here.
//! 6. Upload it as `sps_new.zip`, which the Miniserver loads on start, reboot,
//!    and wait until it answers again.
//! 7. Download again: the Miniserver must now run exactly the compiled file.
//! 8. Move the project forward: lock, out file and `base` in lox.toml point at
//!    the verified state.
//!
//! With `--patched-from DOWNLOAD` the base is a byte patch of an earlier
//! download (GUI-owned edits lxir cannot express): the live config must be
//! byte-identical to DOWNLOAD, and the compile uses the patched base.

use crate::client::LoxClient;
use crate::config::Config;
use crate::{ftp, logic, loxcc};
use anyhow::{Context, Result, anyhow, bail};
use lxir::ir::{CompileOptions, compile};
use lxir::{Lockfile, LoxoneDoc, Project};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The file name the Miniserver loads as its new config on start.
const UPLOAD_NAME: &str = "sps_new.zip";

/// What a push needs from the Miniserver — a trait, so the whole flow also
/// runs against a fake in tests.
pub(crate) trait Miniserver {
    /// The serial from the connection settings, if known.
    fn serial(&self) -> Option<String>;
    /// The newest config ZIP on the Miniserver: its file name and bytes.
    fn download_newest(&self) -> Result<(String, Vec<u8>)>;
    /// Upload a config ZIP as `/prog/sps_new.zip`.
    fn upload(&self, zip: &[u8]) -> Result<()>;
    fn reboot(&self) -> Result<()>;
    /// Return once the Miniserver answers again after a reboot.
    fn wait_until_up(&self) -> Result<()>;
}

/// The Miniserver of the connection settings: FTP for configs, HTTP for the
/// reboot.
pub(crate) struct LiveMiniserver {
    cfg: Config,
}

impl LiveMiniserver {
    pub(crate) fn new(cfg: Config) -> Self {
        LiveMiniserver { cfg }
    }
}

impl Miniserver for LiveMiniserver {
    fn serial(&self) -> Option<String> {
        Some(self.cfg.serial.clone()).filter(|s| !s.is_empty())
    }

    fn download_newest(&self) -> Result<(String, Vec<u8>)> {
        let newest = ftp::list_backups(&self.cfg)?
            .into_iter()
            .next()
            .context("No configs found on the Miniserver")?;
        let data = ftp::download_backup(&self.cfg, &newest.filename)?;
        Ok((newest.filename, data))
    }

    fn upload(&self, zip: &[u8]) -> Result<()> {
        ftp::upload_backup(&self.cfg, UPLOAD_NAME, zip)
    }

    fn reboot(&self) -> Result<()> {
        LoxClient::new(self.cfg.clone())?.get_text("/jdev/sys/reboot")?;
        Ok(())
    }

    fn wait_until_up(&self) -> Result<()> {
        // The old program keeps answering for a moment after the reboot
        // request; that must not count as back.
        std::thread::sleep(Duration::from_secs(20));
        let deadline = Instant::now() + Duration::from_secs(300);
        let client = LoxClient::new(self.cfg.clone())?;
        while client.get_text("/jdev/cfg/version").is_err() {
            if Instant::now() > deadline {
                bail!(
                    "The Miniserver did not answer within 5 minutes after the reboot. \
                     Check it, then compare what it runs with the compiled file \
                     (`lox config download --extract`, `lox ir diff`)."
                );
            }
            std::thread::sleep(Duration::from_secs(5));
        }
        // Let the program and FTP settle before downloading again.
        std::thread::sleep(Duration::from_secs(10));
        Ok(())
    }
}

pub(crate) struct PushOptions {
    /// The project directory (holds lox.toml).
    pub project: PathBuf,
    /// Upload and reboot; without it the push ends after staging.
    pub force: bool,
    /// The download the (byte-patched) base was built from.
    pub patched_from: Option<PathBuf>,
    pub allow_removals: bool,
    pub accept_version: Option<String>,
    /// Where downloads and push artifacts go (default: `backups/` in the
    /// project).
    pub backup_dir: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PushReport {
    /// `nothing-to-push`, `staged` or `pushed`
    pub outcome: &'static str,
    /// The live config before the push (extracted)
    pub live: PathBuf,
    /// Live differed from the base outside the logic, so the module was
    /// compiled against the live download
    pub rebased: bool,
    /// Logic changes from live to the compiled config
    pub changes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compiled: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staged: Option<PathBuf>,
    /// The config downloaded after the push — the project's new base
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<PathBuf>,
}

pub(crate) fn push(ms: &dyn Miniserver, opts: &PushOptions, quiet: bool) -> Result<PushReport> {
    let say = |msg: &str| {
        if !quiet {
            eprintln!("{msg}");
        }
    };
    let project_file = Project::find(&opts.project).ok_or_else(|| {
        anyhow!(
            "No lox.toml in {} — `lox config push` deploys an lxir project \
             (a directory with lox.toml naming base, module and lock; see `lox ir help`)",
            opts.project.display()
        )
    })?;
    let project = Project::load(&project_file)?;
    if let (Some(live), Some(want)) = (ms.serial(), &project.serial)
        && !live.eq_ignore_ascii_case(want)
    {
        bail!(
            "The project targets Miniserver {want}, but the connection settings point at {live}. \
             Pick the right one with `--ctx` / `lox ctx use`, or fix `serial` in {}.",
            project_file.display()
        );
    }
    let backups = opts
        .backup_dir
        .clone()
        .unwrap_or_else(|| project.root.join("backups"));
    prepare_backup_dir(&backups)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let file = |suffix: &str| backups.join(format!("{stamp}-{suffix}"));

    // 1. What the Miniserver runs now.
    say("Downloading the live config...");
    let (name, live_zip) = ms.download_newest()?;
    let live_xml = loxcc::extract_and_decompress(&live_zip)
        .with_context(|| format!("Cannot extract the live config {name}"))?;
    let live_path = file("pre-push.Loxone");
    fs::write(file("pre-push.zip"), &live_zip)?;
    fs::write(&live_path, &live_xml)?;

    // 2. Drift: nobody else may have changed the logic since the base.
    let (compile_base, rebased) = match &opts.patched_from {
        Some(source) => {
            let source_xml =
                fs::read(source).with_context(|| format!("Cannot read {}", source.display()))?;
            if source_xml != live_xml {
                bail!(
                    "The live config ({}) is no longer the download the patched base was \
                     built from ({}). Build the patch again from a fresh download.",
                    live_path.display(),
                    source.display()
                );
            }
            (fs::read(&project.base)?, false)
        }
        None => {
            let base_xml = fs::read(&project.base)
                .with_context(|| format!("Cannot read the base {}", project.base.display()))?;
            if base_xml != live_xml {
                let drift = lxir::diff::diff(&parse(&base_xml)?, &parse(&live_xml)?);
                if !drift.is_empty() {
                    bail!(
                        "The live config changed since the base: {}. Someone saved from \
                         Loxone Config or the app. See what with `lox ir diff {} {}`, \
                         bring the module up to date, and make the download the new base \
                         (`base` in lox.toml) before pushing.",
                        summary(&drift),
                        project.base.display(),
                        live_path.display()
                    );
                }
                say(
                    "The live config differs from the base outside the logic (users, layout, \
                     visualization or save noise); compiling against the live download keeps it.",
                );
            }
            let rebased = base_xml != live_xml;
            (live_xml.clone(), rebased)
        }
    };

    // 3. Compile, with a scratch lock: the project stays untouched until the
    // push is verified.
    let scratch_lock = file("push.lock.json");
    let mut lock = if project.lock.exists() {
        Lockfile::load(&project.lock)?
    } else {
        Lockfile::new()
    };
    let compiled = compile_project(&project, &compile_base, &mut lock, opts)?.to_bytes();
    lock.save(&scratch_lock)?;
    let compiled_path = file("push.Loxone");
    fs::write(&compiled_path, &compiled)?;

    // 4. What the push changes.
    let mut report = PushReport {
        outcome: "nothing-to-push",
        live: live_path,
        rebased,
        changes: Vec::new(),
        compiled: None,
        staged: None,
        verified: None,
    };
    if compiled == live_xml {
        say("Nothing to push: the Miniserver already runs the compiled module.");
        return Ok(report);
    }
    report.changes = logic::diff_lines(&live_xml, &compiled)?;
    if !quiet {
        eprintln!("Changes (live → compiled):");
        for line in &report.changes {
            eprintln!("  {line}");
        }
    }
    report.compiled = Some(compiled_path.clone());

    // 5. Stage exactly the compiled bytes.
    let staged = loxcc::stage_upload_zip(&live_zip, &compiled)?;
    let staged_path = file("push.zip");
    fs::write(&staged_path, &staged)?;
    report.staged = Some(staged_path.clone());
    if !opts.force {
        report.outcome = "staged";
        say(&format!(
            "Staged {} (extracts to exactly {}). Run again with --force to upload it and \
             reboot the Miniserver.",
            staged_path.display(),
            compiled_path.display()
        ));
        return Ok(report);
    }

    // 6. Upload, reboot, wait.
    say(&format!("Uploading as {UPLOAD_NAME}..."));
    ms.upload(&staged)?;
    say("Rebooting the Miniserver...");
    ms.reboot()?;
    say("Waiting for the Miniserver to come back...");
    ms.wait_until_up()?;

    // 7. Verify what it runs now.
    say("Downloading the config again to verify...");
    let (post_name, post_zip) = ms.download_newest()?;
    let post_xml = loxcc::extract_and_decompress(&post_zip)
        .with_context(|| format!("Cannot extract the config after the push ({post_name})"))?;
    let post_path = file("post-push.Loxone");
    fs::write(file("post-push.zip"), &post_zip)?;
    fs::write(&post_path, &post_xml)?;
    if post_xml != compiled {
        let d = lxir::diff::diff(&parse(&compiled)?, &parse(&post_xml)?);
        if !d.is_empty() {
            bail!(
                "The Miniserver does not run what was pushed: {} between {} and {}. \
                 The project was left unchanged.",
                summary(&d),
                compiled_path.display(),
                post_path.display()
            );
        }
        say(&format!(
            "Warning: {} matches the compiled config semantically, but not byte for byte.",
            post_path.display()
        ));
    }

    // 8. The verified state is the project's new base.
    fs::copy(&scratch_lock, &project.lock)
        .with_context(|| format!("Cannot update {}", project.lock.display()))?;
    fs::write(&project.out, &compiled)?;
    let src = fs::read_to_string(&project_file)?;
    fs::write(
        &project_file,
        set_base(&src, &project_path(&post_path, &project.root))?,
    )?;
    say(&format!(
        "Pushed and verified. {} now points at {}; commit the lock and lox.toml.",
        project_file.display(),
        post_path.display()
    ));
    report.outcome = "pushed";
    report.verified = Some(post_path);
    Ok(report)
}

/// `lox ir compile` for the project, against `base` instead of the project's
/// own base.
fn compile_project(
    project: &Project,
    base: &[u8],
    lock: &mut Lockfile,
    opts: &PushOptions,
) -> Result<LoxoneDoc> {
    let module = lxir::cli::read_module(&project.module.display().to_string())
        .map_err(|e| anyhow!("{e}"))?;
    let serial = project
        .serial
        .clone()
        .or_else(|| lock.target.miniserver_serial.clone())
        .context("No serial in lox.toml and none recorded in the lockfile")?;
    let options = CompileOptions {
        machine: lxir::uuid::parse_serial(&serial)?,
        accept_version: opts.accept_version.clone(),
        mint_time_unix: chrono::Utc::now().timestamp(),
        page_title: project.page.clone(),
        allow_removals: opts.allow_removals,
        // Re-laying out a page is a deliberate `lox ir compile --relayout`;
        // a push deploys the placement the project already has.
        relayout: Vec::new(),
    };
    compile(&parse(base)?, &module, lock, &options).context("Compiling the module failed")
}

fn parse(xml: &[u8]) -> Result<LoxoneDoc> {
    Ok(LoxoneDoc::parse(xml)?)
}

/// One-line totals of a semantic diff.
fn summary(d: &lxir::diff::DocDiff) -> String {
    let parts = [
        (d.added.len(), "added"),
        (d.removed.len(), "removed"),
        (d.renamed.len(), "renamed"),
        (d.param_changes.len(), "parameter changes"),
        (d.placements.len(), "room/category changes"),
        (d.wires_added.len(), "wires added"),
        (d.wires_removed.len(), "wires removed"),
    ];
    parts
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Create the backup directory; a new one gets a `.gitignore`, because real
/// configs carry personal data and the project is usually a git repo.
fn prepare_backup_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        fs::create_dir_all(dir).with_context(|| format!("Cannot create {}", dir.display()))?;
        fs::write(
            dir.join(".gitignore"),
            "# Miniserver configs carry personal data — keep them out of git.\n*\n",
        )?;
    }
    Ok(())
}

/// `path` as lox.toml writes it: relative to the project when it is inside,
/// with forward slashes.
fn project_path(path: &Path, root: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

/// lox.toml with its `base = "…"` line pointing at `base`; every other line
/// (comments included) stays as it is.
fn set_base(src: &str, base: &str) -> Result<String> {
    if base.contains('"') {
        bail!("Cannot write {base:?} into lox.toml: paths with quotes are not supported");
    }
    let mut found = false;
    let lines: Vec<String> = src
        .lines()
        .map(|line| {
            let key = line.trim_start().split('=').next().unwrap_or("").trim();
            if !found && key == "base" && line.contains('=') {
                found = true;
                format!("base = \"{base}\"")
            } else {
                line.to_string()
            }
        })
        .collect();
    if !found {
        bail!("lox.toml has no `base = \"…\"` line to update");
    }
    let mut out = lines.join("\n");
    if src.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A Miniserver in memory: serves its current config ZIP; an upload
    /// becomes the current config on reboot, as `sps_new.zip` does.
    struct FakeMiniserver {
        serial: Option<String>,
        current: RefCell<Vec<u8>>,
        uploaded: RefCell<Option<Vec<u8>>>,
        reboots: RefCell<usize>,
    }

    impl FakeMiniserver {
        fn new(zip: Vec<u8>) -> Self {
            FakeMiniserver {
                serial: Some("504F94112233".into()),
                current: RefCell::new(zip),
                uploaded: RefCell::new(None),
                reboots: RefCell::new(0),
            }
        }
    }

    impl Miniserver for FakeMiniserver {
        fn serial(&self) -> Option<String> {
            self.serial.clone()
        }
        fn download_newest(&self) -> Result<(String, Vec<u8>)> {
            Ok((
                "sps_100_20261029120000.zip".into(),
                self.current.borrow().clone(),
            ))
        }
        fn upload(&self, zip: &[u8]) -> Result<()> {
            *self.uploaded.borrow_mut() = Some(zip.to_vec());
            Ok(())
        }
        fn reboot(&self) -> Result<()> {
            *self.reboots.borrow_mut() += 1;
            if let Some(zip) = self.uploaded.borrow_mut().take() {
                *self.current.borrow_mut() = zip;
            }
            Ok(())
        }
        fn wait_until_up(&self) -> Result<()> {
            Ok(())
        }
    }

    /// A config ZIP as the Miniserver serves it: sps0.LoxCC plus a side file.
    fn config_zip(xml: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("sps0.LoxCC", opts).unwrap();
        w.write_all(&loxcc::compress_loxcc(xml).unwrap()).unwrap();
        w.start_file("sps0.LoxPLAN", opts).unwrap();
        w.write_all(b"plan").unwrap();
        w.finish().unwrap().into_inner()
    }

    const EXAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/crates/lxir/examples");

    /// lxir's example project in a temp dir, with the base next to it.
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let src = Path::new(EXAMPLES).join("project");
        fs::copy(
            src.join("pool.lock.json"),
            dir.path().join("pool.lock.json"),
        )
        .unwrap();
        fs::create_dir(dir.path().join("pages")).unwrap();
        for entry in fs::read_dir(src.join("pages")).unwrap() {
            let entry = entry.unwrap();
            fs::copy(
                entry.path(),
                dir.path().join("pages").join(entry.file_name()),
            )
            .unwrap();
        }
        fs::copy(
            Path::new(EXAMPLES).join("configs/pool.Loxone"),
            dir.path().join("pool.Loxone"),
        )
        .unwrap();
        let toml = fs::read_to_string(src.join("lox.toml"))
            .unwrap()
            .replace("../configs/pool.Loxone", "pool.Loxone");
        fs::write(dir.path().join("lox.toml"), toml).unwrap();
        dir
    }

    fn options(dir: &Path, force: bool) -> PushOptions {
        PushOptions {
            project: dir.to_path_buf(),
            force,
            patched_from: None,
            allow_removals: false,
            accept_version: None,
            backup_dir: None,
        }
    }

    fn base(dir: &Path) -> Vec<u8> {
        fs::read(dir.join("pool.Loxone")).unwrap()
    }

    #[test]
    fn push_uploads_reboots_verifies_and_moves_the_base() {
        let dir = project();
        let ms = FakeMiniserver::new(config_zip(&base(dir.path())));

        let report = push(&ms, &options(dir.path(), true), true).unwrap();

        assert_eq!(report.outcome, "pushed");
        assert!(!report.rebased);
        assert!(report.changes.iter().any(|l| l.starts_with('=')));
        assert_eq!(*ms.reboots.borrow(), 1);
        // The Miniserver runs exactly the compiled file, side files untouched.
        let compiled = fs::read(report.compiled.unwrap()).unwrap();
        let live = ms.current.borrow().clone();
        assert_eq!(loxcc::extract_and_decompress(&live).unwrap(), compiled);
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(live)).unwrap();
        assert!(archive.by_name("sps0.LoxPLAN").is_ok());
        // The project moved forward: out, lock, base.
        assert_eq!(fs::read(dir.path().join("out.Loxone")).unwrap(), compiled);
        let verified = report.verified.unwrap();
        let toml = fs::read_to_string(dir.path().join("lox.toml")).unwrap();
        let rel = project_path(&verified, dir.path());
        assert!(toml.contains(&format!("base = \"{rel}\"")), "{toml}");
        assert!(rel.starts_with("backups/"));
        assert!(toml.starts_with('#'), "comments survive");
        let scratch = fs::read_dir(dir.path().join("backups"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.to_string_lossy().ends_with("-push.lock.json"))
            .unwrap();
        assert_eq!(
            fs::read(dir.path().join("pool.lock.json")).unwrap(),
            fs::read(scratch).unwrap(),
            "the lock of the verified compile"
        );
        // The backups dir keeps real configs out of git.
        assert!(dir.path().join("backups/.gitignore").exists());
        // Pushing again: nothing left to do.
        let again = push(&ms, &options(dir.path(), true), true).unwrap();
        assert_eq!(again.outcome, "nothing-to-push");
        assert_eq!(*ms.reboots.borrow(), 1);
    }

    #[test]
    fn push_without_force_stages_and_leaves_everything_alone() {
        let dir = project();
        let ms = FakeMiniserver::new(config_zip(&base(dir.path())));
        let toml = fs::read_to_string(dir.path().join("lox.toml")).unwrap();
        let lock = fs::read(dir.path().join("pool.lock.json")).unwrap();

        let report = push(&ms, &options(dir.path(), false), true).unwrap();

        assert_eq!(report.outcome, "staged");
        assert!(ms.uploaded.borrow().is_none());
        assert_eq!(*ms.reboots.borrow(), 0);
        let staged = fs::read(report.staged.unwrap()).unwrap();
        let compiled = fs::read(report.compiled.unwrap()).unwrap();
        assert_eq!(loxcc::extract_and_decompress(&staged).unwrap(), compiled);
        assert_eq!(
            fs::read_to_string(dir.path().join("lox.toml")).unwrap(),
            toml
        );
        assert_eq!(fs::read(dir.path().join("pool.lock.json")).unwrap(), lock);
        assert!(!dir.path().join("out.Loxone").exists());
    }

    #[test]
    fn push_refuses_when_the_logic_changed_live() {
        let dir = project();
        let base_xml = String::from_utf8(base(dir.path())).unwrap();
        // Someone renamed a block in Loxone Config since the base.
        let title = base_xml
            .split("Title=\"")
            .nth(3)
            .and_then(|s| s.split('"').next())
            .unwrap()
            .to_string();
        let live = base_xml.replacen(
            &format!("Title=\"{title}\""),
            "Title=\"Renamed in Config\"",
            1,
        );
        assert_ne!(live, base_xml);
        let ms = FakeMiniserver::new(config_zip(live.as_bytes()));

        let err = push(&ms, &options(dir.path(), true), true).unwrap_err();

        assert!(err.to_string().contains("changed since the base"), "{err}");
        assert!(ms.uploaded.borrow().is_none());
    }

    #[test]
    fn push_compiles_against_live_when_only_non_logic_bytes_differ() {
        let dir = project();
        // A change outside the logic: the sensor was taken out of the
        // visualization in the app.
        let base_xml = String::from_utf8(base(dir.path())).unwrap();
        let live = base_xml.replacen("<IoData Visu=\"true\" ", "<IoData ", 1);
        assert_ne!(live, base_xml);
        let ms = FakeMiniserver::new(config_zip(live.as_bytes()));

        let report = push(&ms, &options(dir.path(), true), true).unwrap();

        assert_eq!(report.outcome, "pushed");
        assert!(report.rebased);
        let compiled = String::from_utf8(fs::read(report.compiled.unwrap()).unwrap()).unwrap();
        assert_eq!(
            compiled.matches("Visu=\"true\"").count(),
            live.matches("Visu=\"true\"").count(),
            "the live visualization setting is kept"
        );
    }

    #[test]
    fn push_refuses_another_miniserver() {
        let dir = project();
        let mut ms = FakeMiniserver::new(config_zip(&base(dir.path())));
        ms.serial = Some("504F94AABBCC".into());
        let err = push(&ms, &options(dir.path(), true), true).unwrap_err();
        assert!(err.to_string().contains("504F94AABBCC"), "{err}");
    }

    #[test]
    fn push_with_patched_from_needs_the_exact_download() {
        let dir = project();
        let source = dir.path().join("download.Loxone");
        fs::write(&source, b"<C Type=\"Document\"/>").unwrap();
        let ms = FakeMiniserver::new(config_zip(&base(dir.path())));
        let mut opts = options(dir.path(), true);
        opts.patched_from = Some(source);
        let err = push(&ms, &opts, true).unwrap_err();
        assert!(err.to_string().contains("patched base"), "{err}");
    }

    #[test]
    fn push_without_a_project_explains_what_it_needs() {
        let dir = tempfile::tempdir().unwrap();
        let ms = FakeMiniserver::new(Vec::new());
        let err = push(&ms, &options(dir.path(), true), true).unwrap_err();
        assert!(err.to_string().contains("No lox.toml"), "{err}");
    }

    /// A real project against a real download, Miniserver faked:
    /// `LOX_PUSH_PROJECT=<copy of a project> LOX_PUSH_LIVE=<download.zip>
    /// cargo test real_project -- --nocapture`. It pushes with --force, so
    /// the project directory changes — point it at a copy.
    #[test]
    fn push_real_project() {
        let (Some(project), Some(live)) = (
            std::env::var_os("LOX_PUSH_PROJECT"),
            std::env::var_os("LOX_PUSH_LIVE"),
        ) else {
            return;
        };
        let mut ms = FakeMiniserver::new(fs::read(live).unwrap());
        ms.serial = None;
        let report = push(&ms, &options(Path::new(&project), true), false).unwrap();
        eprintln!("{}", serde_json::to_string_pretty(&report).unwrap());
    }

    #[test]
    fn set_base_rewrites_only_the_base_line() {
        let src = "# comment with base = \"x\"\nbase = \"old.Loxone\"\nmodule = \"pages\"\n";
        assert_eq!(
            set_base(src, "backups/new.Loxone").unwrap(),
            "# comment with base = \"x\"\nbase = \"backups/new.Loxone\"\nmodule = \"pages\"\n"
        );
        assert!(set_base("module = \"pages\"\n", "x").is_err());
        assert!(set_base("base = \"a\"\n", "we\"ird").is_err());
    }

    #[test]
    fn project_path_is_relative_inside_the_project() {
        let root = Path::new("/p");
        assert_eq!(
            project_path(Path::new("/p/backups/a.Loxone"), root),
            "backups/a.Loxone"
        );
        assert_eq!(
            project_path(Path::new("/elsewhere/a.Loxone"), root),
            "/elsewhere/a.Loxone"
        );
    }
}
