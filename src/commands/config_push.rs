//! `lox config push` — deploy an lxir project to the Miniserver, verified end
//! to end.
//!
//! Each step stops the push when it fails:
//!
//! 1. Download the config the Miniserver runs now (kept under `backups/`).
//!    Its custom changes (what the app, the web UI or the API changed and
//!    Loxone Config has not saved) are not in that download, and loading an
//!    upload deletes them: with any present, `--force` refuses unless
//!    `--discard-custom-changes` says to delete them.
//! 2. Drift check: the live config must still be the project's base, at least
//!    semantically (`lox ir diff` would show no logic changes). A change saved
//!    from Loxone Config or the app since then is never overwritten. Users,
//!    groups and access rights may differ: the report lists them as kept.
//! 3. Compile the module against the live download, with a scratch copy of the
//!    lock. What the module does not own — users, their passwords and access
//!    rights, layout, visualization — stays as it is live instead of reverting
//!    to the base.
//! 4. Show what the push changes. Nothing to push ends here.
//! 5. Stage the upload: the downloaded ZIP with only `sps0.LoxCC` replaced,
//!    verified to extract to exactly the compiled file. Its app catalogue
//!    (`LoxAPP3.json`) is Loxone Config's from before: controls of the app the
//!    push renames, moves or deletes are named. Without `--force` the push
//!    ends here.
//! 6. Upload it as `sps_new.zip`, which the Miniserver loads on start, reboot,
//!    and wait until it answers again. Loading it drops the Miniserver's
//!    custom changes, users edited in the app among them: when the live users
//!    differ from the program file's, staging reports it and `--force`
//!    refuses unless `--discard-custom-changes` (see `push_users`).
//! 7. Download again: the Miniserver must now run exactly the compiled file.
//!    With app controls changed, compare the catalogue it serves with the
//!    uploaded one: unchanged means the app still shows them as before.
//! 8. Move the project forward: lock, out file and `base` in lox.toml point at
//!    the verified state.
//!
//! With `--patched-from DOWNLOAD` the base is a byte patch of an earlier
//! download (GUI-owned edits lxir cannot express): the live config must be
//! byte-identical to DOWNLOAD, and the compile uses the patched base.

use crate::app_catalogue::{self, StaleEntry};
use crate::client::LoxClient;
use crate::config::Config;
use crate::custom_changes::{self, CustomChange};
use crate::{ftp, logic, loxcc};
use anyhow::{Context, Result, anyhow, bail};
use lxir::ir::{CompileOptions, compile};
use lxir::{Lockfile, LoxoneDoc, Project};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What a push needs from the Miniserver — a trait, so the whole flow also
/// runs against a fake in tests.
pub(crate) trait Miniserver {
    /// The serial from the connection settings, if known.
    fn serial(&self) -> Option<String>;
    /// Every user of the user management, with details.
    fn live_users(&self) -> Result<Vec<crate::usermgmt::User>>;
    /// The newest config ZIP on the Miniserver: its file name and bytes.
    fn download_newest(&self) -> Result<(String, Vec<u8>)>;
    /// The files of `/sys/custom` (name, LoxCC bytes): custom changes an
    /// upload would delete.
    fn custom_changes(&self) -> Result<Vec<(String, Vec<u8>)>>;
    /// Upload a config ZIP as `/prog/sps_new.zip`.
    fn upload(&self, zip: &[u8]) -> Result<()>;
    fn reboot(&self) -> Result<()>;
    /// Return once the Miniserver answers again after a reboot.
    fn wait_until_up(&self) -> Result<()>;
    /// The app catalogue the Miniserver serves (`/data/LoxApp3.json`).
    fn app_catalogue(&self) -> Result<Vec<u8>>;
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

    fn live_users(&self) -> Result<Vec<crate::usermgmt::User>> {
        use crate::usermgmt::{user, user_list};
        let lox = LoxClient::new(self.cfg.clone())?;
        user_list(&lox)?
            .iter()
            .map(|u| user(&lox, &u.uuid))
            .collect()
    }

    fn download_newest(&self) -> Result<(String, Vec<u8>)> {
        let newest = ftp::list_backups(&self.cfg)?
            .into_iter()
            .next()
            .context("No configs found on the Miniserver")?;
        let data = ftp::download_backup(&self.cfg, &newest.filename)?;
        Ok((newest.filename, data))
    }

    fn custom_changes(&self) -> Result<Vec<(String, Vec<u8>)>> {
        ftp::download_custom_changes(&self.cfg)
    }

    fn upload(&self, zip: &[u8]) -> Result<()> {
        ftp::upload_config(&self.cfg, zip)
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

    fn app_catalogue(&self) -> Result<Vec<u8>> {
        LoxClient::new(self.cfg.clone())?.get_bytes("/data/LoxApp3.json")
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
    /// Upload even though the Miniserver holds custom changes, deleting them.
    pub discard_custom_changes: bool,
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
    /// Users, groups and access rights that differ between the base and the
    /// live config; the push compiles against live, so they stay as they are
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kept_as_live: Vec<String>,
    /// Logic changes from live to the compiled config
    pub changes: Vec<String>,
    /// Custom changes on the Miniserver that loading the upload deletes
    pub custom_changes_pending: Vec<CustomChange>,
    /// Controls of the app the push renames, moves or deletes; the uploaded
    /// app catalogue still describes them as before
    pub app_catalogue_stale: Vec<StaleEntry>,
    /// After the push: whether the Miniserver serves a rebuilt catalogue
    /// (`false`: the uploaded one, so the app still shows the old state)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_catalogue_rebuilt: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compiled: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staged: Option<PathBuf>,
    /// The config downloaded after the push — the project's new base
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<PathBuf>,
    /// Users changed live that the program file does not hold: loading the
    /// upload deletes these changes
    pub user_changes_pending: Vec<crate::commands::push_users::PendingUserChange>,
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
    let pending = check_custom_changes(ms, opts, &live_xml, &say)?;

    // 2. Drift: nobody else may have changed the logic since the base.
    let mut kept_as_live = Vec::new();
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
                if !drift.logic_is_empty() {
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
                // Users and access rights belong to Loxone Config and the app:
                // whatever changed there since the base is kept, and named.
                kept_as_live = drift.access.iter().map(|c| c.to_string()).collect();
                if !quiet && !kept_as_live.is_empty() {
                    eprintln!("Kept as live (users and access, base → live):");
                    for line in &kept_as_live {
                        eprintln!("  {line}");
                    }
                }
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
        kept_as_live,
        changes: Vec::new(),
        custom_changes_pending: pending,
        app_catalogue_stale: Vec::new(),
        app_catalogue_rebuilt: None,
        compiled: None,
        staged: None,
        verified: None,
        user_changes_pending: Vec::new(),
    };
    if compiled == live_xml {
        say("Nothing to push: the Miniserver already runs the compiled module.");
        return Ok(report);
    }
    report.changes = change_lines(&live_xml, &compiled)?;
    if !quiet {
        eprintln!("Changes (live → compiled):");
        for line in &report.changes {
            eprintln!("  {line}");
        }
    }
    report.compiled = Some(compiled_path.clone());
    let catalogue = match app_catalogue::from_zip(&live_zip) {
        Ok(catalogue) => catalogue,
        Err(e) => {
            say(&format!(
                "Warning: cannot read the app catalogue of {name}: {e:#}"
            ));
            None
        }
    };
    if let Some(catalogue) = &catalogue {
        let d = lxir::diff::diff(&parse(&live_xml)?, &parse(&compiled)?);
        report.app_catalogue_stale = app_catalogue::stale_entries(catalogue, &d);
    }
    if !report.app_catalogue_stale.is_empty() {
        let list: String = report
            .app_catalogue_stale
            .iter()
            .map(|e| format!("\n  \"{}\": {}", e.app_name, e.change))
            .collect();
        say(&format!(
            "Note: the push changes controls of the app:{list}\n\
             The upload carries Loxone Config's app catalogue ({}) from before, which still \
             shows them as they were. A save from Loxone Config rewrites it.",
            app_catalogue::MEMBER
        ));
    }

    // 5. Stage exactly the compiled bytes.
    let staged = loxcc::stage_upload_zip(&live_zip, &compiled)?;
    let staged_path = file("push.zip");
    fs::write(&staged_path, &staged)?;
    report.staged = Some(staged_path.clone());
    report.user_changes_pending = user_changes(ms, &live_xml, opts, &say)?;
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
    say(&format!("Uploading as {}...", ftp::CONFIG_UPLOAD_NAME));
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

    if let (false, Some(uploaded)) = (report.app_catalogue_stale.is_empty(), &catalogue) {
        report.app_catalogue_rebuilt = check_served_catalogue(ms, uploaded, &say);
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

/// The custom changes an upload would delete. With `--force` any present
/// stop the push, unless `--discard-custom-changes`; a staging run only warns.
/// A failed check counts as changes present: it stops `--force` too.
fn check_custom_changes(
    ms: &dyn Miniserver,
    opts: &PushOptions,
    live_xml: &[u8],
    say: &dyn Fn(&str),
) -> Result<Vec<CustomChange>> {
    let refuse = opts.force && !opts.discard_custom_changes;
    let files = match ms.custom_changes() {
        Ok(files) => files,
        Err(e) if refuse => {
            return Err(e.context(format!(
                "Cannot check {} for custom changes, which loading the upload would delete. \
                 --discard-custom-changes pushes without the check.",
                ftp::CUSTOM_CHANGES_DIR
            )));
        }
        Err(e) => {
            say(&format!(
                "Warning: cannot check {} for custom changes: {e:#}",
                ftp::CUSTOM_CHANGES_DIR
            ));
            return Ok(Vec::new());
        }
    };
    let pending = custom_changes::describe(&files, Some(live_xml));
    if pending.is_empty() {
        return Ok(pending);
    }
    let list: String = pending
        .iter()
        .map(|c| format!("\n  {}", c.label()))
        .collect();
    let head = format!(
        "The Miniserver holds {} custom change(s) that are not in the program file \
         (made in the app, the web UI or through the API):{list}\n\
         Loading the upload deletes them.",
        pending.len()
    );
    if refuse {
        bail!(
            "{head} To keep them, load the project from the Miniserver in Loxone Config and \
             save it, then bring the project up to date with that download. To delete them \
             on purpose, push with --discard-custom-changes."
        );
    }
    if opts.force {
        say(&format!("{head} Deleting them (--discard-custom-changes)."));
    } else {
        say(&format!(
            "{head} `--force` will refuse while they are there (or delete them with \
             --discard-custom-changes)."
        ));
    }
    Ok(pending)
}

/// After a push that changed controls of the app: does the Miniserver serve
/// the uploaded catalogue (`Some(false)`) or one it rebuilt (`Some(true)`)?
/// Compares `lastModified`, the time Loxone Config wrote it: the served
/// catalogue may be filtered for the user, so the rest can differ anyway.
/// Only says what it finds; `None` when it cannot tell.
fn check_served_catalogue(
    ms: &dyn Miniserver,
    uploaded: &serde_json::Value,
    say: &dyn Fn(&str),
) -> Option<bool> {
    let served = match ms.app_catalogue().and_then(|b| app_catalogue::parse(&b)) {
        Ok(served) => served,
        Err(e) => {
            say(&format!(
                "Warning: cannot read the app catalogue the Miniserver serves: {e:#}"
            ));
            return None;
        }
    };
    let (Some(was), Some(now)) = (
        app_catalogue::last_modified(uploaded),
        app_catalogue::last_modified(&served),
    ) else {
        say("Warning: an app catalogue without lastModified; cannot tell whether it was rebuilt.");
        return None;
    };
    if was != now {
        say(&format!(
            "The Miniserver serves a rebuilt app catalogue (lastModified {now}, uploaded {was})."
        ));
        return Some(true);
    }
    say(&format!(
        "The Miniserver serves the uploaded app catalogue (lastModified {was}): the app still \
         shows the changed controls as they were until Loxone Config saves the project."
    ));
    Some(false)
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

/// The user changes loading an upload of a program file based on `live_xml`
/// would delete. With `--force` they stop the push unless
/// `--discard-custom-changes`; a staging run only reports them. Users that
/// cannot be read stop `--force` too.
fn user_changes(
    ms: &dyn Miniserver,
    live_xml: &[u8],
    opts: &PushOptions,
    say: &dyn Fn(&str),
) -> Result<Vec<crate::commands::push_users::PendingUserChange>> {
    use crate::commands::push_users::{UNREADABLE, UserChangesPending, pending_in};
    let refuse = opts.force && !opts.discard_custom_changes;
    let pending = match ms.live_users() {
        Ok(users) => pending_in(live_xml, &users)?,
        Err(e) if refuse => return Err(e.context(UNREADABLE)),
        Err(e) => {
            say(&format!(
                "Warning: cannot read the users to check for user changes: {e:#}"
            ));
            return Ok(Vec::new());
        }
    };
    if pending.is_empty() {
        return Ok(pending);
    }
    if refuse {
        return Err(UserChangesPending(pending).into());
    }
    say(if opts.force {
        "Deleting these user changes (--discard-custom-changes):"
    } else {
        "Users changed on the Miniserver since its program file was saved; loading the upload \
         would delete that (--force refuses unless --discard-custom-changes):"
    });
    for p in &pending {
        say(&format!("  {}: {}", p.user, p.changes.join("; ")));
    }
    Ok(pending)
}

/// What the push changes, live → compiled. The logic view leaves out refs,
/// pages and the document; when only those differ (a title claim renaming
/// the refs that copy a title, say), the semantic diff's totals say so
/// instead of an empty list.
fn change_lines(live: &[u8], compiled: &[u8]) -> Result<Vec<String>> {
    let lines = logic::diff_lines(live, compiled)?;
    if !lines.is_empty() {
        return Ok(lines);
    }
    let d = lxir::diff::diff(&parse(live)?, &parse(compiled)?);
    Ok(vec![if d.is_empty() {
        "no semantic changes: only bytes outside the logic differ".to_string()
    } else {
        format!("refs, pages or document only: {}", summary(&d))
    }])
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
        (d.access.len(), "user/access changes"),
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
    use crate::commands::push_users::{self, UserChangesPending};
    use std::cell::RefCell;

    /// A Miniserver in memory: serves its current config ZIP; an upload
    /// becomes the current config on reboot, as `sps_new.zip` does.
    struct FakeMiniserver {
        serial: Option<String>,
        current: RefCell<Vec<u8>>,
        /// The user management; `None` refuses to show it.
        users: Option<Vec<crate::usermgmt::User>>,
        uploaded: RefCell<Option<Vec<u8>>>,
        reboots: RefCell<usize>,
        /// The files of /sys/custom; `None`: listing it fails
        custom: Option<Vec<(String, Vec<u8>)>>,
    }

    impl FakeMiniserver {
        fn new(zip: Vec<u8>) -> Self {
            FakeMiniserver {
                serial: Some("504F94112233".into()),
                current: RefCell::new(zip),
                users: Some(Vec::new()),
                uploaded: RefCell::new(None),
                reboots: RefCell::new(0),
                custom: Some(Vec::new()),
            }
        }
    }

    impl Miniserver for FakeMiniserver {
        fn serial(&self) -> Option<String> {
            self.serial.clone()
        }
        fn live_users(&self) -> Result<Vec<crate::usermgmt::User>> {
            self.users.clone().ok_or_else(|| {
                anyhow!(
                    "the Miniserver refused to show its user management — the user lox \
                     connects with needs the right to manage users (an admin); change it \
                     with `lox setup set --user <admin>`"
                )
            })
        }
        fn download_newest(&self) -> Result<(String, Vec<u8>)> {
            Ok((
                "sps_100_20261029120000.zip".into(),
                self.current.borrow().clone(),
            ))
        }
        fn custom_changes(&self) -> Result<Vec<(String, Vec<u8>)>> {
            self.custom
                .clone()
                .ok_or_else(|| anyhow!("550 Permission denied"))
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
        /// Serves the catalogue of the config it runs, as is.
        fn app_catalogue(&self) -> Result<Vec<u8>> {
            use std::io::Read;
            let zip = self.current.borrow().clone();
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip))?;
            let mut file = archive.by_name(app_catalogue::MEMBER)?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(bytes)
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
            discard_custom_changes: false,
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

    /// The project with users in its base, and a Miniserver that runs the
    /// base and whose user management matches it.
    fn project_with_users() -> (tempfile::TempDir, FakeMiniserver) {
        let dir = project();
        let xml = String::from_utf8(base(dir.path())).unwrap().replacen(
            "<C Type=\"PlaceCaption\"",
            &format!(
                "{}\n<C Type=\"PlaceCaption\"",
                push_users::tests::users_xml()
            ),
            1,
        );
        fs::write(dir.path().join("pool.Loxone"), &xml).unwrap();
        let mut ms = FakeMiniserver::new(config_zip(xml.as_bytes()));
        ms.users = Some(push_users::tests::live_users());
        (dir, ms)
    }

    /// alice learned a second tag in the app since the program was saved.
    fn learn_a_tag_live(ms: &mut FakeMiniserver) {
        let alice = &mut ms.users.as_mut().unwrap()[0];
        alice.last_edit = Some(562000500);
        alice.nfc_tags.push(crate::usermgmt::NfcTag {
            name: "alice card".into(),
            id: "04 D4 E5 F6".into(),
        });
    }

    #[test]
    fn push_goes_ahead_when_the_users_match_the_program_file() {
        let (dir, ms) = project_with_users();

        let report = push(&ms, &options(dir.path(), true), true).unwrap();

        assert_eq!(report.outcome, "pushed");
        assert!(report.user_changes_pending.is_empty());
    }

    #[test]
    fn push_refuses_to_discard_user_changes() {
        let (dir, mut ms) = project_with_users();
        learn_a_tag_live(&mut ms);

        let err = push(&ms, &options(dir.path(), true), true).unwrap_err();

        let pending = err.downcast_ref::<UserChangesPending>().expect("typed");
        assert_eq!(pending.0.len(), 1);
        assert_eq!(crate::categorize_error(&err), "user_changes_pending");
        let msg = err.to_string();
        assert!(msg.contains("alice (last edit: "), "{msg}");
        assert!(
            msg.contains("NFC tags: 2 (alice phone, alice card) live"),
            "{msg}"
        );
        assert!(ms.uploaded.borrow().is_none());
        assert_eq!(*ms.reboots.borrow(), 0);
    }

    #[test]
    fn staging_reports_the_user_changes_an_upload_would_discard() {
        let (dir, mut ms) = project_with_users();
        learn_a_tag_live(&mut ms);

        let report = push(&ms, &options(dir.path(), false), true).unwrap();

        assert_eq!(report.outcome, "staged");
        let pending = &report.user_changes_pending;
        assert_eq!(pending[0].user, "alice");
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["user_changes_pending"][0]["user"], "alice");
        assert_eq!(
            json["user_changes_pending"][0]["changes"][1],
            pending[0].changes[1]
        );
    }

    #[test]
    fn discard_custom_changes_deletes_user_changes_too() {
        let (dir, mut ms) = project_with_users();
        learn_a_tag_live(&mut ms);
        let mut opts = options(dir.path(), true);
        opts.discard_custom_changes = true;

        let report = push(&ms, &opts, true).unwrap();

        assert_eq!(report.outcome, "pushed");
        assert_eq!(report.user_changes_pending[0].user, "alice");
    }

    #[test]
    fn users_that_cannot_be_read_stop_force_only() {
        let (dir, mut ms) = project_with_users();
        ms.users = None;

        let err = push(&ms, &options(dir.path(), true), true).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("Cannot read the Miniserver's users"), "{msg}");
        assert!(msg.contains("lox setup set --user"), "{msg}");
        assert!(msg.contains("--discard-custom-changes"), "{msg}");
        assert!(ms.uploaded.borrow().is_none());

        let staged = push(&ms, &options(dir.path(), false), true).unwrap();
        assert_eq!(staged.outcome, "staged");
        assert!(staged.user_changes_pending.is_empty());

        let mut opts = options(dir.path(), true);
        opts.discard_custom_changes = true;
        let report = push(&ms, &opts, true).unwrap();
        assert_eq!(report.outcome, "pushed");
    }

    #[test]
    fn changes_name_ref_renames_the_logic_view_leaves_out() {
        let dir = project();
        let live = base(dir.path());
        let renamed = String::from_utf8(live.clone())
            .unwrap()
            .replacen(
                r#"Title="Netzbezug" Nio="4""#,
                r#"Title="Netzbezug neu" Nio="4""#,
                1,
            )
            .into_bytes();
        assert_ne!(renamed, live);
        assert!(logic::diff_lines(&live, &renamed).unwrap().is_empty());

        let lines = change_lines(&live, &renamed).unwrap();

        assert_eq!(lines, ["refs, pages or document only: 1 renamed"]);
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

    /// A Miniserver whose app changed the base's first titled block since the
    /// last save: one /sys/custom file. Returns it and the block's title.
    fn with_custom_change(dir: &Path) -> (FakeMiniserver, String) {
        let xml = base(dir);
        let doc = LoxoneDoc::parse(&xml).unwrap();
        let obj = doc
            .objects()
            .into_iter()
            .find(|o| o.title.is_some() && o.block_type != "Page")
            .unwrap();
        let fragment = format!(r#"<C Type="{}" U="{}"/>"#, obj.block_type, obj.uuid);
        let mut ms = FakeMiniserver::new(config_zip(&xml));
        ms.custom = Some(vec![(
            format!("{}.LoxCC", obj.uuid),
            loxcc::compress_loxcc(fragment.as_bytes()).unwrap(),
        )]);
        (ms, obj.title.unwrap())
    }

    #[test]
    fn push_refuses_while_custom_changes_are_pending() {
        let dir = project();
        let (ms, title) = with_custom_change(dir.path());

        let err = push(&ms, &options(dir.path(), true), true).unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains("1 custom change(s)"), "{msg}");
        assert!(msg.contains(&format!("\"{title}\"")), "{msg}");
        assert!(msg.contains("--discard-custom-changes"), "{msg}");
        assert!(ms.uploaded.borrow().is_none());
        assert_eq!(*ms.reboots.borrow(), 0);
    }

    #[test]
    fn push_discards_custom_changes_on_purpose() {
        let dir = project();
        let (ms, title) = with_custom_change(dir.path());
        let mut opts = options(dir.path(), true);
        opts.discard_custom_changes = true;

        let report = push(&ms, &opts, true).unwrap();

        assert_eq!(report.outcome, "pushed");
        assert_eq!(report.custom_changes_pending.len(), 1);
        assert_eq!(
            report.custom_changes_pending[0].title.as_deref(),
            Some(title.as_str())
        );
        assert!(!report.custom_changes_pending[0].new);
    }

    #[test]
    fn staging_reports_custom_changes_without_refusing() {
        let dir = project();
        let (ms, _) = with_custom_change(dir.path());

        let report = push(&ms, &options(dir.path(), false), true).unwrap();

        assert_eq!(report.outcome, "staged");
        assert_eq!(report.custom_changes_pending.len(), 1);
        let json = serde_json::to_value(&report).unwrap();
        assert!(json["custom_changes_pending"][0]["type"].is_string());
    }

    #[test]
    fn a_failed_custom_changes_check_stops_force_but_not_staging() {
        let dir = project();
        let mut ms = FakeMiniserver::new(config_zip(&base(dir.path())));
        ms.custom = None;

        let err = push(&ms, &options(dir.path(), true), true).unwrap_err();
        assert!(format!("{err:#}").contains("550"), "{err:#}");
        assert!(ms.uploaded.borrow().is_none());

        let staged = push(&ms, &options(dir.path(), false), true).unwrap();
        assert_eq!(staged.outcome, "staged");

        let mut opts = options(dir.path(), true);
        opts.discard_custom_changes = true;
        assert_eq!(push(&ms, &opts, true).unwrap().outcome, "pushed");
    }

    #[test]
    fn the_served_catalogue_tells_whether_the_miniserver_rebuilt_it() {
        use std::io::Write;
        let uploaded = r#"{"lastModified":"2026-10-29 17:15:28","controls":{}}"#;
        let zip_with = |catalogue: &str| {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file(app_catalogue::MEMBER, opts).unwrap();
            w.write_all(catalogue.as_bytes()).unwrap();
            w.finish().unwrap().into_inner()
        };
        let uploaded_json = app_catalogue::parse(uploaded.as_bytes()).unwrap();
        let said = RefCell::new(Vec::<String>::new());
        let say = |m: &str| said.borrow_mut().push(m.to_string());

        let same = FakeMiniserver::new(zip_with(uploaded));
        assert_eq!(
            check_served_catalogue(&same, &uploaded_json, &say),
            Some(false)
        );
        assert!(said.borrow()[0].contains("2026-10-29 17:15:28"));

        let rebuilt = FakeMiniserver::new(zip_with(
            r#"{"lastModified":"2026-10-29 18:02:11","controls":{}}"#,
        ));
        assert_eq!(
            check_served_catalogue(&rebuilt, &uploaded_json, &say),
            Some(true)
        );

        let none = FakeMiniserver::new(config_zip(b"<x/>"));
        assert_eq!(check_served_catalogue(&none, &uploaded_json, &say), None);
    }

    #[test]
    fn push_keeps_users_and_access_as_live() {
        let dir = project();
        // Base and live both have a user; since the base, the app gave her
        // a second tag and access to a Code Touch (synthetic values).
        let user = "\t\t<C Type=\"LoxCaption\" V=\"175\" U=\"50000000-0000-0001-ffff504f94112233\" \
                    Title=\"Users and Permissions\" CaptionType=\"20\" SubType=\"20\">\r\n\
                    \t\t\t<C Type=\"User\" V=\"175\" U=\"50000001-0000-0010-ffff504f94112233\" \
                    Title=\"alice\" LastEdit=\"562500000\" NFCArr=\"0A1B\" NFCNArr=\"Key fob\"/>\r\n\
                    \t\t</C>\r\n";
        let anchor = "\t\t<C Type=\"VirtualInCaption\"";
        let base_xml = String::from_utf8(base(dir.path())).unwrap().replacen(
            anchor,
            &format!("{user}{anchor}"),
            1,
        );
        fs::write(dir.path().join("pool.Loxone"), &base_xml).unwrap();
        let live = base_xml.replacen(
            r#"LastEdit="562500000" NFCArr="0A1B" NFCNArr="Key fob"/>"#,
            r#"LastEdit="562522922" NFCArr="0A1B;2C3D" NFCNArr="Key fob;Phone"/>"#,
            1,
        );
        assert_ne!(live, base_xml);
        let ms = FakeMiniserver::new(config_zip(live.as_bytes()));

        let report = push(&ms, &options(dir.path(), false), true).unwrap();

        assert_eq!(report.outcome, "staged");
        assert!(report.rebased);
        assert_eq!(report.kept_as_live.len(), 2, "{:?}", report.kept_as_live);
        assert!(report.kept_as_live[0].starts_with(r#"user "alice": NFC tag "Phone" added"#));
        assert!(report.kept_as_live[1].starts_with(r#"user "alice": last edit"#));
        let compiled = fs::read(report.compiled.as_ref().unwrap()).unwrap();
        let compiled = String::from_utf8(compiled).unwrap();
        assert!(
            compiled.contains(r#"NFCNArr="Key fob;Phone""#),
            "live users stay"
        );
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["kept_as_live"].as_array().unwrap().len(), 2);
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
