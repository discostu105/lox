//! Where a `.Loxone` config comes from: a file, a snapshot in the config repo
//! (`lox config pull`), the per-context cache, or a fresh FTP download.
//!
//! Nothing here writes to the Miniserver: a download only reads its newest
//! backup.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::Config;

/// A config's bytes and where they came from (for headers and footers).
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub bytes: Vec<u8>,
    pub label: String,
}

/// One commit of `<ms>/config.Loxone` in the config repo.
#[derive(Debug, Clone, PartialEq)]
pub struct CommitInfo {
    pub hash: String,
    /// Commit date, `%ci`
    pub date: String,
    /// Subject without the `[ms] ` prefix `lox config pull` writes
    pub subject: String,
    pub body: String,
}

impl CommitInfo {
    pub fn short(&self) -> &str {
        self.hash.get(..7).unwrap_or(&self.hash)
    }

    /// When the config was saved in Loxone Config (from the subject).
    pub fn saved(&self) -> Option<&str> {
        self.subject
            .strip_prefix("Config backup ")
            .and_then(|r| r.get(..19).or(r.get(..16)))
    }
}

/// The config repo of this context, if it is set up.
pub fn repo_dir(cfg: &Config) -> Option<PathBuf> {
    let p = PathBuf::from(cfg.config_repo.as_ref()?);
    p.join(".git").exists().then_some(p)
}

fn need_repo(cfg: &Config) -> Result<PathBuf> {
    repo_dir(cfg).context(
        "no config repository for this context — set it up with `lox config init <dir>` and `lox config pull`",
    )
}

/// `<ms>/config.Loxone`, the file each commit snapshots.
pub fn config_path(cfg: &Config) -> String {
    format!("{}/config.Loxone", crate::gitops::ms_dir(cfg))
}

pub fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = std::process::Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .context("git not found")?;
    if !out.status.success() {
        bail!(
            "git {}: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}

/// The config's commits, newest first.
pub fn commits(cfg: &Config, n: usize) -> Result<Vec<CommitInfo>> {
    let repo = need_repo(cfg)?;
    let prefix = format!("[{}] ", crate::gitops::ms_dir(cfg));
    let out = git(
        &repo,
        &[
            "log",
            "-n",
            &n.to_string(),
            "--format=%H%x09%ci%x09%s%x09%b%x1e",
            "--",
            &config_path(cfg),
        ],
    )?;
    Ok(String::from_utf8_lossy(&out)
        .split('\x1e')
        .filter_map(|rec| {
            let mut p = rec.trim_start_matches('\n').splitn(4, '\t');
            let hash = p.next().filter(|h| !h.is_empty())?;
            let date = p.next()?;
            let subject = p.next().unwrap_or("");
            Some(CommitInfo {
                hash: hash.to_string(),
                date: date.to_string(),
                subject: subject.trim_start_matches(prefix.as_str()).to_string(),
                body: p.next().unwrap_or("").trim().to_string(),
            })
        })
        .collect())
}

/// A snapshot by git revision (`abc1234`, `HEAD~2`), config version (`v273`:
/// the newest backup of that version) or save date (`2026-08-25`: the newest
/// backup saved on or before that day).
pub fn resolve(cfg: &Config, spec: &str) -> Result<CommitInfo> {
    let repo = need_repo(cfg)?;
    let all = commits(cfg, 10_000)?;
    if all.is_empty() {
        bail!("the config repository has no snapshots yet — run `lox config pull`");
    }
    let spec = spec.trim();
    let version = spec
        .strip_prefix('v')
        .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()));
    if let Some(v) = version {
        let tag = format!("(v{})", v);
        return all
            .into_iter()
            .find(|c| c.subject.contains(&tag))
            .with_context(|| format!("no snapshot of config version v{}", v));
    }
    let is_date = spec.len() >= 10
        && spec.as_bytes()[4] == b'-'
        && spec.as_bytes()[7] == b'-'
        && spec[..4].chars().all(|c| c.is_ascii_digit());
    if is_date {
        let when = |c: &CommitInfo| c.saved().unwrap_or(&c.date).to_string();
        // commits are newest first: the first one not after the given day
        return all
            .into_iter()
            .find(|c| {
                let w = when(c);
                w.get(..spec.len()).is_some_and(|p| p <= spec)
            })
            .with_context(|| format!("no snapshot saved on or before {}", spec));
    }
    let hash = git(
        &repo,
        &["rev-parse", "--verify", "--quiet", &format!("{}^{{commit}}", spec)],
    )
    .ok()
    .map(|o| String::from_utf8_lossy(&o).trim().to_string())
    .filter(|h| !h.is_empty())
    .with_context(|| {
        format!(
            "unknown snapshot '{}' — use a commit (see `lox config log`), a version like v273 or a date like 2026-08-25",
            spec
        )
    })?;
    // a commit that did not touch the config: its snapshot is the newest one before
    let ancestors = git(&repo, &["rev-list", &hash, "--", &config_path(cfg)])
        .map(|o| String::from_utf8_lossy(&o).to_string())
        .unwrap_or_default();
    let first = ancestors.lines().next().unwrap_or("").to_string();
    all.into_iter()
        .find(|c| c.hash == first)
        .with_context(|| format!("no config snapshot at or before '{}'", spec))
}

fn label(c: &CommitInfo) -> String {
    match c.saved() {
        Some(s) => format!("{} · saved {}", c.short(), s),
        None => c.short().to_string(),
    }
}

/// The config at a commit.
pub fn at_commit(cfg: &Config, c: &CommitInfo) -> Result<Snapshot> {
    let repo = need_repo(cfg)?;
    let bytes = git(
        &repo,
        &["show", &format!("{}:{}", c.hash, config_path(cfg))],
    )?;
    Ok(Snapshot {
        bytes,
        label: label(c),
    })
}

/// The snapshot before a commit, if there is one.
#[cfg_attr(not(feature = "tui"), allow(dead_code))]
pub fn before(cfg: &Config, c: &CommitInfo) -> Result<Option<(CommitInfo, Snapshot)>> {
    let all = commits(cfg, 10_000)?;
    let Some(i) = all.iter().position(|x| x.hash == c.hash) else {
        return Ok(None);
    };
    match all.get(i + 1) {
        Some(p) => Ok(Some((p.clone(), at_commit(cfg, p)?))),
        None => Ok(None),
    }
}

/// A config file: `.Loxone` XML, or a backup ZIP (LoxCC is decompressed).
pub fn file(path: &str) -> Result<Snapshot> {
    Ok(Snapshot {
        bytes: crate::load_config_xml(path)?,
        label: Path::new(path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string()),
    })
}

/// Downloads are cached per context, next to the structure cache.
pub fn cache_path(cfg: &Config) -> PathBuf {
    cfg.cache_dir().join("config.Loxone")
}

/// `lastModified` of the cached structure: the version a download is stored with.
fn structure_version(cfg: &Config) -> Option<String> {
    let s = std::fs::read(cfg.cache_dir().join("structure.json")).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&s).ok()?;
    v["lastModified"].as_str().map(str::to_string)
}

/// The newest config available without the network: the config repo's
/// working copy, else the cached download.
pub fn current(cfg: &Config) -> Result<Snapshot> {
    current_with_version(cfg, structure_version(cfg).as_deref())
}

/// Like [`current`], marking a cached copy stale when the running structure
/// version (`lastModified`) differs from the one it was downloaded with.
pub fn current_with_version(cfg: &Config, version: Option<&str>) -> Result<Snapshot> {
    if let Some(repo) = repo_dir(cfg)
        && let Ok(bytes) = std::fs::read(repo.join(config_path(cfg)))
    {
        return Ok(Snapshot {
            bytes,
            label: "gitops".into(),
        });
    }
    let cache = cache_path(cfg);
    if let Ok(bytes) = std::fs::read(&cache) {
        let ver = std::fs::read_to_string(cache.with_extension("version")).unwrap_or_default();
        let fresh = version.is_none_or(|v| v.trim() == ver.trim());
        return Ok(Snapshot {
            bytes,
            label: if fresh {
                "cached".into()
            } else {
                "cached · config may be stale".into()
            },
        });
    }
    bail!(
        "no config available offline — run `lox config pull`, pass --download (reads the newest backup via FTP), or --file <path>"
    )
}

/// Download the newest backup via FTP (read-only) and cache it.
pub fn download(cfg: &Config, version: Option<&str>) -> Result<Snapshot> {
    let backups = crate::ftp::list_backups(cfg)?;
    let newest = backups
        .first()
        .context("no config backups on the Miniserver")?;
    let zip = crate::ftp::download_backup(cfg, &newest.filename)?;
    let xml = crate::loxcc::extract_and_decompress(&zip)?;
    let cache = cache_path(cfg);
    if let Some(p) = cache.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(&cache, &xml);
    let ver = version
        .map(str::to_string)
        .or_else(|| structure_version(cfg))
        .unwrap_or_default();
    let _ = std::fs::write(cache.with_extension("version"), ver);
    Ok(Snapshot {
        bytes: xml,
        label: newest.filename.clone(),
    })
}

/// The config a command looks at: `--file`, `--at <snapshot>`, `--download`,
/// or the newest one available offline.
pub fn load(cfg: &Config, at: Option<&str>, file_path: Option<&str>, dl: bool) -> Result<Snapshot> {
    match (file_path, at) {
        (Some(_), Some(_)) => bail!("use either --file or --at, not both"),
        (Some(f), None) => file(f),
        (None, Some(spec)) => at_commit(cfg, &resolve(cfg, spec)?),
        (None, None) if dl => download(cfg, None),
        (None, None) => current(cfg),
    }
}

/// A snapshot by the TUI's spec: a commit, or `<commit>^` for the snapshot
/// before it.
#[cfg_attr(not(feature = "tui"), allow(dead_code))]
pub fn by_spec(cfg: &Config, spec: &str) -> Result<Snapshot> {
    if let Some(h) = spec.strip_suffix('^') {
        let c = resolve(cfg, h)?;
        return before(cfg, &c)?
            .map(|(_, s)| s)
            .context("this is the first snapshot — there is none before it");
    }
    at_commit(cfg, &resolve(cfg, spec)?)
}

/// Parse snapshots on all cores: ~100 ms each for a large config.
pub fn parse_all(snaps: &[Snapshot]) -> Vec<Result<crate::logic::Logic>> {
    use crate::logic::Logic;
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1);
    let chunk = snaps.len().div_ceil(n).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = snaps
            .chunks(chunk)
            .map(|c| s.spawn(move || c.iter().map(|x| Logic::parse(&x.bytes)).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| {
                h.join()
                    .unwrap_or_else(|_| vec![Err(anyhow::anyhow!("parser panicked"))])
            })
            .collect()
    })
}

/// How one block changed, snapshot by snapshot (newest first).
#[derive(Debug, Clone)]
pub struct BlockHistory {
    pub uuid: String,
    pub title: String,
    pub entries: Vec<(CommitInfo, Vec<String>)>,
    /// Snapshots looked at
    pub scanned: usize,
}

/// The history of a block (name, `Name [room]` or UUID) over the newest
/// `count` snapshots.
pub fn block_history(
    cfg: &Config,
    name: &str,
    room: Option<&str>,
    count: usize,
) -> Result<BlockHistory> {
    use crate::logic::{self, Logic};
    // one more than asked: the oldest shown change needs its predecessor
    let commits = commits(cfg, count.max(1) + 1)?;
    if commits.is_empty() {
        bail!("the config repository has no snapshots yet — run `lox config pull`");
    }
    let snaps = commits
        .iter()
        .map(|c| at_commit(cfg, c))
        .collect::<Result<Vec<_>>>()?;
    let logics = parse_all(&snaps);
    let readable: Vec<&Logic> = logics.iter().flatten().collect();
    // the newest snapshot that knows the block names it
    let found = readable.iter().find_map(|l| {
        l.resolve(name, room)
            .ok()
            .map(|b| (b.uuid.clone(), b.title.clone()))
    });
    let (uuid, title) = match (found, readable.first()) {
        (Some(f), _) => f,
        // none knows it: the newest snapshot explains why
        (None, Some(l)) => {
            l.resolve(name, room)?;
            bail!("no block matching '{}' in the config", name)
        }
        (None, None) => bail!("none of the snapshots could be read"),
    };
    let complete = commits.len() <= count;
    let scanned = commits.len().min(count);
    let mut entries = Vec::new();
    for i in 0..scanned {
        let Ok(new) = &logics[i] else { continue };
        let lines = match logics.get(i + 1) {
            Some(Ok(old)) => logic::block_changes(old, new, &uuid),
            Some(Err(_)) => vec!["? the snapshot before could not be read".into()],
            None if complete && new.blocks.contains_key(&uuid) => {
                vec!["= in the first snapshot".into()]
            }
            None => Vec::new(),
        };
        if !lines.is_empty() {
            entries.push((commits[i].clone(), lines));
        }
    }
    Ok(BlockHistory {
        uuid,
        title,
        entries,
        scanned,
    })
}

impl BlockHistory {
    /// One paragraph per snapshot with changes.
    pub fn text(&self) -> String {
        if self.entries.is_empty() {
            return format!("Unchanged in the last {} snapshots.", self.scanned);
        }
        let mut out = String::new();
        for (i, (c, lines)) in self.entries.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&format!(
                "{}  {}\n",
                c.short(),
                c.saved()
                    .map(str::to_string)
                    .unwrap_or_else(|| c.date.chars().take(16).collect())
            ));
            for l in lines {
                out.push_str(&format!("  {}\n", l));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_with_snapshots() -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let run = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .current_dir(repo)
                .args([
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(ok.status.success(), "{:?}", ok);
        };
        run(&["init", "-q"]);
        let cfg = Config {
            host: "http://192.168.1.2".into(),
            config_repo: Some(repo.display().to_string()),
            ..Default::default()
        };
        let ms = crate::gitops::ms_dir(&cfg);
        std::fs::create_dir_all(repo.join(&ms)).unwrap();
        for (i, (date, v)) in [("2026-03-18 19:40:59", 259), ("2026-08-25 20:48:59", 273)]
            .iter()
            .enumerate()
        {
            std::fs::write(
                repo.join(&ms).join("config.Loxone"),
                format!("<ControlList n=\"{}\"/>", i),
            )
            .unwrap();
            run(&["add", "."]);
            run(&[
                "commit",
                "-q",
                "-m",
                &format!("[{}] Config backup {} (v{})", ms, date, v),
            ]);
        }
        (dir, cfg)
    }

    #[test]
    fn resolve_by_rev_version_and_date() {
        let (_d, cfg) = repo_with_snapshots();
        let all = commits(&cfg, 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].saved(), Some("2026-08-25 20:48:59"));
        assert!(!all[0].subject.starts_with('['));
        assert_eq!(resolve(&cfg, "HEAD").unwrap(), all[0]);
        assert_eq!(resolve(&cfg, "HEAD~1").unwrap(), all[1]);
        assert_eq!(resolve(&cfg, all[1].short()).unwrap(), all[1]);
        assert_eq!(resolve(&cfg, "v259").unwrap(), all[1]);
        assert_eq!(resolve(&cfg, "2026-08-24").unwrap(), all[1]);
        assert_eq!(resolve(&cfg, "2026-08-25").unwrap(), all[0]);
        assert!(resolve(&cfg, "v999").is_err());
        assert!(resolve(&cfg, "2020-01-01").is_err());
        assert!(resolve(&cfg, "nonsense").is_err());
        let s = at_commit(&cfg, &all[1]).unwrap();
        assert_eq!(s.bytes, b"<ControlList n=\"0\"/>");
        assert!(s.label.contains("saved 2026-03-18"), "{}", s.label);
        let (p, _) = before(&cfg, &all[0]).unwrap().unwrap();
        assert_eq!(p, all[1]);
        assert!(before(&cfg, &all[1]).unwrap().is_none());
        // the working copy is the current config
        assert_eq!(current(&cfg).unwrap().label, "gitops");
        assert!(load(&cfg, Some("HEAD"), Some("x"), false).is_err());
    }
}
