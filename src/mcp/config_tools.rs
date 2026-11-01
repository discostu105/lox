//! The config-as-code tools: `lox logic` and `lox config sync/push`, run as a
//! child process of this binary. Both print their results to stdout, which
//! carries the protocol here, so they cannot run in-process. The tools get
//! exactly what the CLI prints, and the CLI line to run it by hand.

use anyhow::{Context, Result, anyhow};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A compile or test of a large program takes seconds; a push dry run
/// downloads the config.
const TIMEOUT: Duration = Duration::from_secs(300);

/// More than this per stream is cut, so a decompiled house fits a reply.
const MAX_OUTPUT: usize = 512 * 1024;

/// One run of the CLI.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CliRun {
    /// The command line, to run it by hand
    pub cli: String,
    /// The working directory it ran in
    pub dir: String,
    pub exit_code: i32,
    /// Exit code 0. Check, lint, drift, diff and test exit 1 on findings.
    pub ok: bool,
    /// stdout, when it is JSON
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<Value>,
    /// stdout, when it is text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// stderr: reports, warnings and errors
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<String>,
    /// Output was longer than 512 KB and is cut
    pub truncated: bool,
}

/// A compile into a scratch directory, and what it changes.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CompileRun {
    pub compile: CliRun,
    /// The compiled config, for `logic_diff` or `logic_decompile`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out: Option<String>,
    /// `lox logic diff --json` from the project's base to the compiled config
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<CliRun>,
}

/// Runs this binary's CLI.
#[derive(Clone)]
pub struct Runner {
    exe: PathBuf,
}

impl Runner {
    pub fn current() -> Self {
        Runner {
            exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("lox")),
        }
    }

    #[cfg(test)]
    pub fn with_exe(exe: PathBuf) -> Self {
        Runner { exe }
    }

    /// Run `lox <args>` in `dir`. A pinned `--ctx` goes along.
    pub async fn run(&self, dir: &Path, args: &[String]) -> Result<CliRun> {
        let mut full = Vec::new();
        if let Some(ctx) = crate::config::ctx_override() {
            full.push("--ctx".to_string());
            full.push(ctx);
        }
        full.extend(args.iter().cloned());
        let child = tokio::process::Command::new(&self.exe)
            .args(&full)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output();
        let out = tokio::time::timeout(TIMEOUT, child)
            .await
            .map_err(|_| anyhow!("`lox {}` took longer than 5 minutes", full.join(" ")))?
            .with_context(|| format!("cannot run {}", self.exe.display()))?;
        let (stdout, cut_out) = cut(&out.stdout);
        let (stderr, cut_err) = cut(&out.stderr);
        let json = (!cut_out)
            .then(|| serde_json::from_str::<Value>(&stdout).ok())
            .flatten();
        let cli = std::iter::once("lox".to_string())
            .chain(full.iter().map(|a| crate::actions::shell_quote(a)))
            .collect::<Vec<_>>()
            .join(" ");
        Ok(CliRun {
            cli,
            dir: dir.display().to_string(),
            exit_code: out.status.code().unwrap_or(-1),
            ok: out.status.success(),
            output: json.is_none().then_some(stdout).filter(|s| !s.is_empty()),
            json,
            messages: Some(stderr).filter(|s| !s.trim().is_empty()),
            truncated: cut_out || cut_err,
        })
    }

    /// `lox logic <args>`.
    pub async fn logic(&self, dir: &Path, args: &[&str]) -> Result<CliRun> {
        let args: Vec<String> = std::iter::once("logic")
            .chain(args.iter().copied())
            .map(String::from)
            .collect();
        self.run(dir, &args).await
    }

    /// Compile the project into a fresh scratch directory, with a copy of its
    /// lockfile: the project stays as it is.
    pub async fn compile(&self, project: &Path) -> Result<CompileRun> {
        let p = load_project(project)?;
        let scratch = scratch_dir()?;
        let lock = scratch.join("lock.json");
        if p.lock.exists() {
            std::fs::copy(&p.lock, &lock)
                .with_context(|| format!("cannot copy {}", p.lock.display()))?;
        }
        let out = scratch.join("out.Loxone");
        let (out_s, lock_s) = (out.display().to_string(), lock.display().to_string());
        let compile = self
            .logic(
                project,
                &["compile", ".", "--out", &out_s, "--lock", &lock_s],
            )
            .await?;
        if !compile.ok || !out.exists() {
            return Ok(CompileRun {
                compile,
                out: None,
                changes: None,
            });
        }
        let base = p.base.display().to_string();
        let changes = self
            .logic(project, &["diff", "--json", &base, &out_s])
            .await?;
        Ok(CompileRun {
            compile,
            out: Some(out_s),
            changes: Some(changes),
        })
    }
}

/// The project's lox.toml.
pub fn load_project(dir: &Path) -> Result<loxone_config::Project> {
    let file = loxone_config::Project::find(dir).ok_or_else(|| {
        crate::mcp::ops::invalid(format!(
            "No lox.toml in {}. Pass `project`: the directory of a Lox Logic project.",
            dir.display()
        ))
    })?;
    Ok(loxone_config::Project::load(&file)?)
}

/// The project directory a tool works on: `project`, relative to the server's
/// working directory, which is also the default.
pub fn project_dir(project: Option<&str>) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("no working directory")?;
    let dir = match project {
        Some(p) => cwd.join(p),
        None => cwd,
    };
    if !dir.is_dir() {
        return Err(crate::mcp::ops::invalid(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    Ok(dir)
}

fn scratch_dir() -> Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
    let dir = std::env::temp_dir()
        .join("lox-mcp")
        .join(format!("{stamp}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    Ok(dir)
}

/// Text, cut to `MAX_OUTPUT` bytes at a character boundary.
fn cut(bytes: &[u8]) -> (String, bool) {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= MAX_OUTPUT {
        return (text.into_owned(), false);
    }
    let mut end = MAX_OUTPUT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cut_keeps_whole_characters() {
        let long = "ä".repeat(MAX_OUTPUT);
        let (text, cut_off) = cut(long.as_bytes());
        assert!(cut_off);
        assert!(text.len() <= MAX_OUTPUT);
        assert!(text.chars().all(|c| c == 'ä'));
        assert_eq!(cut(b"short"), ("short".to_string(), false));
    }
}
