use anyhow::{Context, Result, bail};
use clap::Subcommand;
use reqwest::blocking::Client;
use std::collections::HashMap;
use std::fmt::Write;
use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::commands::config_insight::{ConfigSource, TraceDir};
use crate::commands::{RunContext, config_insight, config_push};
use crate::config::Config;
use crate::scene::Scene;
use crate::util::json_val_str;
use crate::{custom_changes, ftp, gitops, loxone_xml};
use loxone_client::client::{LOXONE_EPOCH_SECS, LoxClient, USER_AGENT};
use loxone_client::system::{check_ll, code_of};
use loxone_client::token;
use loxone_config::archive;

#[derive(Subcommand)]
pub(crate) enum SetupCmd {
    /// Set one or more config fields (omitted fields are preserved)
    Set {
        #[arg(long, env = "LOX_HOST")]
        host: Option<String>,
        #[arg(long, env = "LOX_USER")]
        user: Option<String>,
        /// Password (or set LOX_PASS env var to avoid it appearing in the process table)
        #[arg(long, env = "LOX_PASS")]
        pass: Option<String>,
        #[arg(long, env = "LOX_SERIAL")]
        serial: Option<String>,
        /// Enable SSL certificate verification (for valid certs)
        #[arg(long)]
        verify_ssl: bool,
        /// Disable SSL certificate verification (default, for self-signed Miniserver certs)
        #[arg(long, conflicts_with = "verify_ssl")]
        no_verify_ssl: bool,
        /// File holding the visualization password for secured controls
        /// (e.g. ~/.lox/visu-password; stores the path, not the password; "" removes it)
        #[arg(long, value_name = "PATH")]
        visu_password_file: Option<String>,
    },
    /// Show current config (password redacted)
    Show,
}

pub fn cmd_setup(ctx: &RunContext, action: SetupCmd) -> Result<()> {
    match action {
        SetupCmd::Set {
            host,
            user,
            pass,
            serial,
            verify_ssl,
            no_verify_ssl,
            visu_password_file,
        } => {
            let mut cfg = Config::load().unwrap_or_default();
            if let Some(h) = host {
                cfg.host = if h.starts_with("http://") || h.starts_with("https://") {
                    h
                } else {
                    format!("https://{}", h)
                };
            }
            if let Some(u) = user {
                cfg.user = u;
            }
            if let Some(p) = pass {
                cfg.pass = p;
            }
            if let Some(s) = serial {
                cfg.serial = s;
            }
            if verify_ssl {
                cfg.verify_ssl = Some(true);
            } else if no_verify_ssl {
                cfg.verify_ssl = Some(false);
            }
            if let Some(f) = visu_password_file {
                // the path only; the file is read when a secured command needs it
                cfg.visu_password_file = (!f.is_empty()).then_some(f);
            }
            let path = cfg.save()?;
            if !ctx.quiet {
                println!("✓  Config saved to {:?}", path);
            }
        }
        SetupCmd::Show => print!("{}", setup_text(&Config::load()?)),
    }
    Ok(())
}

/// `lox setup show`: the connection settings, the password redacted.
fn setup_text(cfg: &Config) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "host:   {}", cfg.host);
    let _ = writeln!(out, "user:   {}", cfg.user);
    let _ = writeln!(out, "pass:   {}", "*".repeat(cfg.pass.len()));
    if !cfg.serial.is_empty() {
        let _ = writeln!(out, "serial: {}", cfg.serial);
    }
    if !cfg.aliases.is_empty() {
        let _ = writeln!(out, "aliases:");
        for (name, uuid) in sorted_aliases(&cfg.aliases) {
            let _ = writeln!(out, "  {}: {}", name, uuid);
        }
    }
    if let Some(f) = &cfg.visu_password_file {
        let _ = writeln!(out, "visu_password_file: {}", f);
    }
    if !cfg.confirm.is_empty() {
        let _ = writeln!(out, "confirm:");
        for entry in &cfg.confirm {
            let _ = writeln!(out, "  - {}", entry);
        }
    }
    out
}

fn sorted_aliases(aliases: &HashMap<String, String>) -> Vec<(&String, &String)> {
    let mut sorted: Vec<_> = aliases.iter().collect();
    sorted.sort_by_key(|(k, _)| k.as_str());
    sorted
}

#[derive(Subcommand)]
pub(crate) enum AliasCmd {
    /// Add or update an alias
    Add { name: String, uuid: String },
    /// Remove an alias
    Remove { name: String },
    /// List all aliases
    #[command(alias = "list")]
    Ls,
}

pub fn cmd_alias(ctx: &RunContext, action: AliasCmd) -> Result<()> {
    let mut cfg = Config::load()?;
    match action {
        AliasCmd::Add { name, uuid } => {
            cfg.aliases.insert(name.clone(), uuid.clone());
            cfg.save()?;
            if !ctx.quiet {
                println!("✓  alias '{}' → {}", name, uuid);
            }
        }
        AliasCmd::Remove { name } => {
            if cfg.aliases.remove(&name).is_some() {
                cfg.save()?;
                if !ctx.quiet {
                    println!("✓  removed alias '{}'", name);
                }
            } else {
                println!("No alias named '{}'", name);
            }
        }
        AliasCmd::Ls => print!("{}", aliases_table(&cfg.aliases, !ctx.no_header)),
    }
    Ok(())
}

fn aliases_table(aliases: &HashMap<String, String>, header: bool) -> String {
    if aliases.is_empty() {
        return "No aliases. Add with: lox alias add <name> <uuid>\n".to_string();
    }
    let mut out = String::new();
    if header {
        let _ = writeln!(out, "{:<24} UUID", "ALIAS");
        let _ = writeln!(out, "{}", "─".repeat(60));
    }
    for (name, uuid) in sorted_aliases(aliases) {
        let _ = writeln!(out, "{:<24} {}", name, uuid);
    }
    out
}

#[derive(Subcommand)]
pub(crate) enum SceneCmd {
    /// List all saved scenes
    #[command(alias = "list")]
    Ls,
    /// Print a scene's YAML definition
    Show { name: String },
    /// Create a new empty scene file
    New { name: String },
}

pub fn cmd_scene(_ctx: &RunContext, action: SceneCmd) -> Result<()> {
    let cfg = Config::load().unwrap_or_default();
    match action {
        SceneCmd::Ls => {
            let scenes: Vec<(String, String)> = Scene::list_with_config(&cfg)?
                .into_iter()
                .map(|name| {
                    let desc = Scene::load_with_config(&name, &cfg)
                        .ok()
                        .and_then(|s| s.description)
                        .unwrap_or_default();
                    (name, desc)
                })
                .collect();
            print!("{}", scenes_text(&scenes));
        }
        SceneCmd::Show { name } => {
            println!(
                "{}",
                fs::read_to_string(Scene::scenes_dir_for(&cfg).join(format!("{}.yaml", name)))
                    .with_context(|| format!("Scene '{}' not found", name))?
            );
        }
        SceneCmd::New { name } => {
            let dir = Scene::scenes_dir_for(&cfg);
            fs::create_dir_all(&dir)?;
            let path = dir.join(format!("{}.yaml", name));
            if path.exists() {
                bail!("Already exists");
            }
            fs::write(
                &path,
                format!(
                    "name: \"{name}\"\ndescription: \"\"\nsteps:\n  - control: \"\"\n    cmd: \"on\"\n"
                ),
            )?;
            println!("✓  {:?}", path);
        }
    }
    Ok(())
}

/// `lox scene ls`: the scenes as (name, description).
fn scenes_text(scenes: &[(String, String)]) -> String {
    if scenes.is_empty() {
        return "No scenes. Create: lox scene new <name>\n".to_string();
    }
    let mut out = String::new();
    for (name, desc) in scenes {
        let _ = writeln!(out, "  {:<20}  {}", name, desc);
    }
    out
}

#[derive(Subcommand)]
pub(crate) enum CacheCmd {
    /// Show cache info
    Info,
    /// Clear structure cache (forces fresh fetch)
    Clear,
    /// Refresh structure cache now
    Refresh,
    /// Check if cache is current (without full download)
    Check,
}

pub fn cmd_cache(ctx: &RunContext, action: CacheCmd) -> Result<()> {
    let cfg = Config::load()?;
    let cache = cfg.structure_cache_path();
    match action {
        CacheCmd::Info => {
            if cache.exists() {
                let meta = cache.metadata()?;
                let age = std::time::SystemTime::now()
                    .duration_since(meta.modified()?)
                    .unwrap_or_default();
                print!("{}", cache_info_text(&cache, meta.len(), age));
            } else {
                println!("No cache. Will be created on first command.");
            }
        }
        CacheCmd::Clear => {
            if cache.exists() {
                fs::remove_file(&cache)?;
                println!("✓ Cache cleared");
            } else {
                println!("No cache to clear");
            }
        }
        CacheCmd::Check => {
            let lox = cfg.client()?;
            let resp = lox.get_json("/jdev/sps/LoxAPPversion3")?;
            let remote_ver = resp
                .pointer("/LL/value")
                .and_then(json_val_str)
                .unwrap_or_else(|| "?".to_string());
            if ctx.json {
                println!("{}", cache_check_json(&remote_ver, cache.exists()));
            } else {
                print!("{}", cache_check_text(&remote_ver, &cache));
            }
        }
        CacheCmd::Refresh => {
            let client = Client::builder()
                .user_agent(USER_AGENT)
                .danger_accept_invalid_certs(true)
                .redirect(LoxClient::same_origin_redirect_policy(&cfg.host))
                .timeout(Duration::from_secs(15))
                .build()?;
            if cache.exists() {
                let _ = fs::remove_file(&cache);
            }
            LoxClient::load_or_fetch_structure(&cfg.connection(), &client)?;
            println!("✓ Structure cache refreshed ({:?})", cache);
        }
    }
    Ok(())
}

/// `lox cache info` for a cache file of `size` bytes and `age`; valid for 24 h.
fn cache_info_text(cache: &Path, size: u64, age: Duration) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Cache: {:?}", cache);
    let _ = writeln!(out, "Size:  {:.1} KB", size as f64 / 1024.0);
    let _ = writeln!(
        out,
        "Age:   {}m {}s",
        age.as_secs() / 60,
        age.as_secs() % 60
    );
    if age.as_secs() < 86400 {
        let remaining = 86400u64.saturating_sub(age.as_secs());
        let _ = writeln!(
            out,
            "Status: ✓ valid ({}h {}m until refresh)",
            remaining / 3600,
            (remaining % 3600) / 60
        );
    } else {
        let _ = writeln!(out, "Status: ⚠ stale (will refresh on next command)");
    }
    out
}

fn cache_check_json(remote_ver: &str, cache_exists: bool) -> serde_json::Value {
    serde_json::json!({
        "remote_version": remote_ver,
        "cache_exists": cache_exists,
    })
}

fn cache_check_text(remote_ver: &str, cache: &Path) -> String {
    let mut out = format!("Remote structure version: {}\n", remote_ver);
    if cache.exists() {
        let _ = writeln!(out, "Cache: exists at {:?}", cache);
    } else {
        out.push_str("Cache: not present\n");
    }
    out
}

#[derive(Subcommand)]
pub(crate) enum TokenCmd {
    /// Fetch and save a new token (valid 20 days)
    Fetch,
    /// Show current token status
    Info,
    /// Delete saved token (revert to Basic Auth)
    Clear,
    /// Check if token is still valid on the Miniserver
    Check,
    /// Refresh token to extend validity
    Refresh,
    /// Revoke token on the Miniserver
    Revoke,
}

pub fn cmd_token(ctx: &RunContext, action: TokenCmd) -> Result<()> {
    let cfg = Config::load()?;
    match action {
        TokenCmd::Fetch => {
            println!("Fetching token from Miniserver...");
            let rt = tokio::runtime::Runtime::new()?;
            match rt.block_on(token::acquire_token(&cfg.connection())) {
                Ok(ts) => {
                    let days = ts.valid_until.saturating_sub(unix_now()) / 86400;
                    println!("✓ Token saved to {:?}", cfg.token_path());
                    println!("  Valid for: {} days", days);
                }
                Err(e) => bail!("Token fetch failed: {}", e),
            }
        }
        TokenCmd::Info => match token::TokenStore::load_for(&cfg.connection()) {
            Some(ts) => print!("{}", token_info_text(&ts, unix_now())),
            None => println!("No token saved. Using Basic Auth. Run: lox token fetch"),
        },
        TokenCmd::Clear => {
            let path = cfg.token_path();
            if path.exists() {
                fs::remove_file(&path)?;
                println!("✓ Token cleared (reverting to Basic Auth)");
            } else {
                println!("No token to clear");
            }
        }
        TokenCmd::Check => {
            let ts = token::TokenStore::load_for(&cfg.connection())
                .ok_or_else(|| anyhow::anyhow!("No token saved. Run: lox token fetch"))?;
            let lox = cfg.client()?;
            // Hash the token with the key for the check endpoint
            let hash = token::hash_token(&ts.token, &ts.key);
            let resp = lox.get_json(&format!("/jdev/sys/checktoken/{}/{}", hash, cfg.user))?;
            // a Gen 1 string or a Gen 2 number; shown as text either way
            let code = code_of(&resp).map_or_else(|| "?".to_string(), |c| c.to_string());
            if ctx.json {
                println!(
                    "{}",
                    serde_json::json!({"valid": code == "200", "code": code})
                );
            } else if code == "200" {
                println!("✓ Token is valid on the Miniserver");
            } else {
                println!("✗ Token is not valid (code {})", code);
            }
        }
        TokenCmd::Refresh => {
            let ts = token::TokenStore::load_for(&cfg.connection())
                .ok_or_else(|| anyhow::anyhow!("No token saved. Run: lox token fetch"))?;
            let lox = cfg.client()?;
            let hash = token::hash_token(&ts.token, &ts.key);
            let resp = lox.get_json(&format!("/jdev/sys/refreshtoken/{}/{}", hash, cfg.user))?;
            check_ll("refreshtoken", &resp).context("Token refresh failed")?;
            // Update the valid_until in our local store
            let valid_until = resp
                .pointer("/LL/value")
                .and_then(|v| v.get("validUntil"))
                .and_then(|v| v.as_u64());
            if let Some(vu) = valid_until {
                let unix_until = if vu > 1_500_000_000 {
                    vu
                } else {
                    LOXONE_EPOCH_SECS.saturating_add(vu)
                };
                let new_ts = token::TokenStore {
                    token: ts.token,
                    key: ts.key,
                    valid_until: unix_until,
                };
                new_ts.save_for(&cfg.connection())?;
                let days = unix_until.saturating_sub(unix_now()) / 86400;
                println!("✓ Token refreshed ({} days remaining)", days);
            } else {
                println!("✓ Token refreshed");
            }
        }
        TokenCmd::Revoke => {
            let ts = token::TokenStore::load_for(&cfg.connection())
                .ok_or_else(|| anyhow::anyhow!("No token saved. Run: lox token fetch"))?;
            let lox = cfg.client()?;
            let hash = token::hash_token(&ts.token, &ts.key);
            let resp = lox.get_json(&format!("/jdev/sys/killtoken/{}/{}", hash, cfg.user))?;
            check_ll("killtoken", &resp).context("Token revoke failed")?;
            // Remove local token
            let path = cfg.token_path();
            if path.exists() {
                fs::remove_file(&path)?;
            }
            println!("✓ Token revoked and cleared");
        }
    }
    Ok(())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// `lox token info` at unix time `now`: the token shortened, and how long it
/// stays valid.
fn token_info_text(ts: &token::TokenStore, now: u64) -> String {
    let mut out = String::new();
    let days_left = ts.valid_until.saturating_sub(now) / 86400;
    if ts.token.len() >= 12 {
        let _ = writeln!(
            out,
            "Token: {}...{}",
            &ts.token[..8],
            &ts.token[ts.token.len() - 4..]
        );
    } else {
        let _ = writeln!(out, "Token: {}", ts.token);
    }
    if ts.is_valid() {
        let _ = writeln!(out, "Status: ✓ valid ({} days remaining)", days_left);
    } else {
        let _ = writeln!(out, "Status: ⚠ expired — run: lox token fetch");
    }
    out
}

#[derive(Subcommand)]
pub(crate) enum ConfigCmd {
    /// Download the latest Loxone Config from the Miniserver via FTP
    Download {
        /// Custom output filename
        #[arg(long, value_name = "PATH")]
        save_as: Option<String>,
        /// Also decompress LoxCC to XML
        #[arg(long)]
        extract: bool,
    },
    /// List available configs on the Miniserver
    #[command(alias = "list")]
    Ls,
    /// Decompress a local config ZIP to .Loxone XML
    Extract {
        /// Path to a config ZIP file
        file: String,
        /// Custom output filename
        #[arg(long, value_name = "PATH")]
        save_as: Option<String>,
    },
    /// Compress a .Loxone XML file to sps0.LoxCC (the config inside a config ZIP)
    Compress {
        /// Path to a .Loxone XML file
        file: String,
        /// Write an upload ZIP instead: this downloaded config ZIP with only
        /// sps0.LoxCC replaced, verified to extract byte-identically to FILE
        #[arg(long, value_name = "ZIP")]
        template: Option<String>,
        /// Custom output filename (default: FILE with .LoxCC, or .zip with --template)
        #[arg(long, value_name = "PATH")]
        save_as: Option<String>,
    },
    /// Deploy a Lox Logic project: drift check, compile, stage, upload, program
    /// restart, verify (requires --force to upload)
    Push {
        /// Project directory (holds lox.toml)
        #[arg(default_value = ".")]
        project: String,
        /// Upload the staged config and restart the program, which loads it
        /// within seconds (as Loxone Config's save does)
        #[arg(long)]
        force: bool,
        /// Load the upload by rebooting the Miniserver instead (about a
        /// minute) — for changes the running system only takes on start
        #[arg(long)]
        reboot: bool,
        /// The base is a byte patch of this download (.Loxone); the live
        /// config must still be exactly it
        #[arg(long, value_name = "FILE")]
        patched_from: Option<String>,
        /// Delete managed blocks missing from the module (see `lox logic help`)
        #[arg(long)]
        allow_removals: bool,
        /// Accept a base saved by a new Loxone Config release (its exact version)
        #[arg(long, value_name = "VERSION")]
        accept_version: Option<String>,
        /// Where downloads and push artifacts go (default: PROJECT/backups)
        #[arg(long, value_name = "DIR")]
        backup_dir: Option<String>,
        /// Push even though the Miniserver holds custom changes (made in the
        /// app, the web UI or through the API, not saved by Loxone Config);
        /// loading the upload deletes them
        #[arg(long)]
        discard_custom_changes: bool,
    },
    /// Upload a config to the Miniserver via FTP (dangerous — requires --force)
    ///
    /// The ZIP goes to /prog/sps_new.zip, which the Miniserver loads on its
    /// next start (`lox reboot`). It then deletes its custom changes: what the
    /// app, the web UI or the API changed and Loxone Config has not saved (light
    /// moods, timers, rules). The upload lists them first.
    Upload {
        /// Path to a config ZIP file
        file: String,
        /// Confirm the upload
        #[arg(long)]
        force: bool,
    },
    /// List the users of a config file: NFC tags, access codes, groups and
    /// access rights (live users: `lox user ls`)
    Users {
        /// Path to a .Loxone XML file (from `lox config extract`) or a backup ZIP
        file: String,
    },
    /// List hardware devices from a .Loxone config file
    Devices {
        /// Path to a .Loxone XML file (from `lox config extract`)
        file: String,
    },
    /// What changed in the logic: blocks, parameters and wires, page by page
    ///
    /// OLD and NEW are config files (ZIP or .Loxone) or snapshots of the config
    /// repo: a commit (`abc1234`, `HEAD~1`), a version (`v273`) or a save date
    /// (`2026-08-25`). No arguments: the last change. One: from OLD to the
    /// newest config. `OLD..NEW` works too.
    #[command(verbatim_doc_comment)]
    Diff {
        /// Older config (file or snapshot)
        old: Option<String>,
        /// Newer config (file or snapshot)
        new: Option<String>,
    },
    /// List the logic pages, or show a page as Lox Logic source
    Show {
        /// Page to show as source (exact title or a unique part of it)
        #[arg(long, short)]
        page: Option<String>,
        /// Show every page as source
        #[arg(long, conflicts_with = "page")]
        all: bool,
        #[command(flatten)]
        src: ConfigSource,
    },
    /// What is wired to a block: inputs, outputs and parameters, with
    /// hardware shown as the device and room it lives in
    #[command(verbatim_doc_comment)]
    Wiring {
        /// Block name, `Name [room]` or UUID
        name: String,
        /// Room, page or device to disambiguate the name
        #[arg(short, long)]
        room: Option<String>,
        /// Follow the wires up to the sensors, down to the actuators, or both
        #[arg(long, value_enum)]
        trace: Option<TraceDir>,
        /// How many hops to follow with --trace
        #[arg(long, default_value = "4")]
        depth: usize,
        /// Show parameters that still have their default value too
        #[arg(long)]
        all_params: bool,
        #[command(flatten)]
        src: ConfigSource,
    },
    /// Search the config's blocks by name, type, device, room or page
    Find {
        /// Words to search for (all must match)
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        #[command(flatten)]
        src: ConfigSource,
    },
    /// Check the logic for dead blocks, unused inputs, broken references and
    /// blinds or gates whose motor outputs are wired to nothing
    Lint {
        #[command(flatten)]
        src: ConfigSource,
    },
    /// How a block changed across the snapshots of the config repo
    History {
        /// Block name, `Name [room]` or UUID
        name: String,
        /// Room, page or device to disambiguate the name
        #[arg(short, long)]
        room: Option<String>,
        /// Number of snapshots to look back
        #[arg(short = 'n', long, default_value = "50")]
        count: usize,
    },
    /// Initialize a git repository for config version tracking
    Init {
        /// Path for the config git repository
        path: String,
    },
    /// Download the latest config from the Miniserver and commit to git
    Pull {
        /// Suppress output (for cron usage)
        #[arg(long)]
        quiet: bool,
    },
    /// Show config change history from the git repository
    Log {
        /// Number of entries to show
        #[arg(short, long, default_value = "20")]
        count: usize,
    },
    /// Restore a previous config version from git and upload to Miniserver
    ///
    /// Uploaded as /prog/sps_new.zip, which the Miniserver loads on its next
    /// start (`lox reboot`).
    Restore {
        /// Git commit hash to restore from
        commit: String,
        /// Confirm the upload (required — dangerous operation)
        #[arg(long)]
        force: bool,
    },
}

pub fn cmd_config(ctx: &RunContext, action: ConfigCmd) -> Result<()> {
    match action {
        ConfigCmd::Ls => config_ls(ctx)?,
        ConfigCmd::Download { save_as, extract } => config_download(save_as, extract)?,
        ConfigCmd::Extract { file, save_as } => config_extract(&file, save_as)?,
        ConfigCmd::Compress {
            file,
            template,
            save_as,
        } => {
            let report = config_compress(&file, template, save_as)?;
            if ctx.json {
                println!("{}", serde_json::to_string_pretty(&report.json())?);
            } else if !ctx.quiet {
                print!("{}", report.text());
            }
        }
        ConfigCmd::Push {
            project,
            force,
            patched_from,
            allow_removals,
            accept_version,
            backup_dir,
            discard_custom_changes,
            reboot,
        } => {
            let ms = config_push::LiveMiniserver::new(Config::load()?);
            let opts = config_push::PushOptions {
                project: project.into(),
                force: force && !ctx.dry_run,
                patched_from: patched_from.map(Into::into),
                allow_removals,
                accept_version,
                backup_dir: backup_dir.map(Into::into),
                discard_custom_changes,
                reboot,
            };
            let report = config_push::push(&ms, &opts, ctx.quiet)?;
            if ctx.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
        }
        ConfigCmd::Upload { file, force } => config_upload(&file, force)?,
        ConfigCmd::Users { file } => {
            let users = loxone_xml::parse_users(&crate::load_config_xml(&file)?)?;
            if ctx.json {
                println!("{}", serde_json::to_string_pretty(&users)?);
            } else {
                print!("{}", users_table(&users, !ctx.no_header));
            }
        }
        ConfigCmd::Devices { file } => {
            if file.ends_with(".zip") {
                bail!(
                    "Expected a .Loxone XML file. Run `lox config extract {}` first.",
                    file
                );
            }
            let xml = fs::read(&file).with_context(|| format!("Cannot read {}", file))?;
            let devices = loxone_xml::parse_devices(&xml)?;
            if ctx.json {
                println!("{}", serde_json::to_string_pretty(&devices)?);
            } else {
                print!("{}", devices_text(&devices));
            }
        }
        ConfigCmd::Diff { old, new } => config_insight::cmd_diff(ctx, old, new)?,
        ConfigCmd::Show { page, all, src } => config_insight::cmd_show(ctx, page, all, src)?,
        ConfigCmd::Wiring {
            name,
            room,
            trace,
            depth,
            all_params,
            src,
        } => config_insight::cmd_wiring(ctx, name, room, trace, depth, all_params, src)?,
        ConfigCmd::Find { query, src } => config_insight::cmd_find(ctx, query, src)?,
        ConfigCmd::Lint { src } => config_insight::cmd_lint(ctx, src)?,
        ConfigCmd::History { name, room, count } => {
            config_insight::cmd_history(ctx, name, room, count)?
        }
        ConfigCmd::Init { path } => config_init(&path)?,
        ConfigCmd::Pull { quiet: pull_quiet } => {
            let cfg = Config::load()?;
            let q = pull_quiet || ctx.quiet;
            let committed = gitops::pull(config_repo(&cfg)?, &cfg, q)?;
            if q && committed {
                // In quiet/cron mode, just indicate a commit was made
                println!("committed");
            }
        }
        ConfigCmd::Log { count } => {
            let cfg = Config::load()?;
            let ms = (!cfg.serial.is_empty()).then_some(cfg.serial.as_str());
            let output = gitops::log(config_repo(&cfg)?, ms, count)?;
            if output.is_empty() {
                println!("No config history yet. Run `lox config pull` first.");
            } else {
                println!("{}", output);
            }
        }
        ConfigCmd::Restore { commit, force } => {
            let cfg = Config::load()?;
            gitops::restore(config_repo(&cfg)?, &cfg, &commit, force)?;
        }
    }
    Ok(())
}

/// The config repo of `lox config init`.
fn config_repo(cfg: &Config) -> Result<&Path> {
    cfg.config_repo
        .as_deref()
        .map(Path::new)
        .context("No config repo configured. Run `lox config init <path>` first.")
}

/// `lox config ls`: the program files in /prog, newest first.
fn config_ls(ctx: &RunContext) -> Result<()> {
    let cfg = Config::load()?;
    let backups = ftp::list_backups(&cfg.connection())?;
    if backups.is_empty() {
        println!("No configs found on the Miniserver.");
    } else if ctx.json {
        println!("{}", serde_json::to_string_pretty(&backups_json(&backups))?);
    } else {
        print!("{}", backups_table(&backups));
    }
    Ok(())
}

fn backups_json(backups: &[ftp::BackupInfo]) -> Vec<serde_json::Value> {
    backups
        .iter()
        .map(|b| {
            serde_json::json!({
                "filename": b.filename,
                "version": b.version,
                "date": b.formatted_date(),
                "size": b.size,
            })
        })
        .collect()
}

fn backups_table(backups: &[ftp::BackupInfo]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "  {:<4} {:<8} {:<22} Size", "#", "Version", "Date");
    for (i, b) in backups.iter().enumerate() {
        let _ = writeln!(
            out,
            "  {:<4} {:<8} {:<22} {} KB{}",
            i + 1,
            b.version,
            b.formatted_date(),
            b.size / 1024,
            if i == 0 { "  (latest)" } else { "" }
        );
    }
    out
}

/// `lox config download`: the newest program file, with `extract` also as
/// `.Loxone` XML.
fn config_download(save_as: Option<String>, extract: bool) -> Result<()> {
    let cfg = Config::load()?;
    let backups = ftp::list_backups(&cfg.connection())?;
    if backups.is_empty() {
        bail!("No configs found on the Miniserver.");
    }
    let newest = &backups[0];
    eprintln!(
        "Downloading {} ({} KB)...",
        newest.filename,
        newest.size / 1024
    );
    let data = ftp::download_backup(&cfg.connection(), &newest.filename)?;
    let out_path = save_as.unwrap_or_else(|| newest.filename.clone());
    fs::write(&out_path, &data)?;
    println!("Saved to {}", out_path);

    if extract {
        eprintln!("Extracting sps0.LoxCC...");
        let xml = archive::extract_and_decompress(&data)?;
        let xml_path = out_path
            .strip_suffix(".zip")
            .unwrap_or(&out_path)
            .to_string()
            + ".Loxone";
        fs::write(&xml_path, &xml)?;
        println!("{}", decompressed_line(data.len(), xml.len(), &xml_path));
    }
    Ok(())
}

fn decompressed_line(zip_len: usize, xml_len: usize, xml_path: &str) -> String {
    format!(
        "Decompressed {} KB → {} KB → {}",
        zip_len / 1024,
        xml_len / 1024,
        xml_path
    )
}

/// `lox config extract`: a config ZIP's `sps0.LoxCC` as `.Loxone` XML.
fn config_extract(file: &str, save_as: Option<String>) -> Result<()> {
    let zip_data = fs::read(file).with_context(|| format!("Cannot read {}", file))?;
    eprintln!("Extracting sps0.LoxCC...");
    let xml = archive::extract_and_decompress(&zip_data)?;
    let xml_path = save_as
        .unwrap_or_else(|| file.strip_suffix(".zip").unwrap_or(file).to_string() + ".Loxone");
    fs::write(&xml_path, &xml)?;
    println!(
        "{}",
        decompressed_line(zip_data.len(), xml.len(), &xml_path)
    );
    Ok(())
}

/// What `lox config compress` wrote.
struct CompressReport {
    input: String,
    template: Option<String>,
    output: String,
    xml_bytes: usize,
    output_bytes: usize,
    crc32: u32,
}

impl CompressReport {
    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "input": self.input,
            "template": self.template,
            "output": self.output,
            "xml_bytes": self.xml_bytes,
            "output_bytes": self.output_bytes,
            "crc32": format!("{:08x}", self.crc32),
        })
    }

    fn text(&self) -> String {
        format!(
            "Compressed {} KB → {} KB → {}{}\n",
            self.xml_bytes / 1024,
            self.output_bytes / 1024,
            self.output,
            if self.template.is_some() {
                " (extracts byte-identically)"
            } else {
                ""
            }
        )
    }
}

/// Compress a `.Loxone` file to `sps0.LoxCC`, or with `template` stage it
/// into that config ZIP (verified to extract to exactly the file).
fn config_compress(
    file: &str,
    template: Option<String>,
    save_as: Option<String>,
) -> Result<CompressReport> {
    let xml = fs::read(file).with_context(|| format!("Cannot read {}", file))?;
    if xml.starts_with(b"PK\x03\x04") || xml.starts_with(&archive::LOXCC_MAGIC.to_le_bytes()) {
        bail!(
            "{} is already compressed. Expected a .Loxone XML file (`lox config extract` makes one).",
            file
        );
    }
    let stem = file.strip_suffix(".Loxone").unwrap_or(file);
    let (out, out_path) = match &template {
        // An upload ZIP: the template with only sps0.LoxCC replaced,
        // verified to extract to exactly `xml`
        Some(template) => {
            let shell = fs::read(template).with_context(|| format!("Cannot read {}", template))?;
            let staged = archive::stage_upload_zip(&shell, &xml, crate::util::zip_now())
                .with_context(|| format!("Cannot stage {} into {}", file, template))?;
            (staged, save_as.unwrap_or_else(|| format!("{stem}.zip")))
        }
        None => (
            archive::compress_loxcc(&xml)?,
            save_as.unwrap_or_else(|| format!("{stem}.LoxCC")),
        ),
    };
    fs::write(&out_path, &out).with_context(|| format!("Cannot write {}", out_path))?;
    Ok(CompressReport {
        input: file.to_string(),
        template,
        output: out_path,
        xml_bytes: xml.len(),
        output_bytes: out.len(),
        crc32: crc32fast::hash(&xml),
    })
}

/// `lox config upload`: a config ZIP as /prog/sps_new.zip (only with `force`).
fn config_upload(file: &str, force: bool) -> Result<()> {
    let data = fs::read(file).with_context(|| format!("Cannot read {}", file))?;
    // The Miniserver starts from whatever sps_new.zip holds.
    let xml = match archive::extract_and_decompress(&data) {
        Ok(xml) => xml,
        Err(e) => bail!(
            "{} is not a config ZIP ({:#}). A .Loxone file becomes one with \
             `lox config compress FILE --template DOWNLOAD.zip`.",
            file,
            e
        ),
    };
    let cfg = Config::load();
    if let Ok(cfg) = &cfg {
        custom_changes::warn_before_upload(cfg, Some(&xml));
    }
    if !force {
        eprintln!(
            "⚠  WARNING: Uploading a config will replace the current Miniserver\n\
             \x20  programming. A bad configuration can require physical SD card\n\
             \x20  access to recover.\n\
             \n\
             \x20  Config file: {}\n\
             \n\
             \x20  Use --force to proceed.",
            file
        );
        std::process::exit(1);
    }
    let cfg = cfg?;
    eprintln!(
        "Uploading {} as /prog/{} ({} KB)...",
        file,
        ftp::CONFIG_UPLOAD_NAME,
        data.len() / 1024
    );
    ftp::upload_config(&cfg.connection(), &data)?;
    println!("Upload complete.");
    println!("Reboot the Miniserver to load it: lox reboot");
    Ok(())
}

/// The `lox config users` table, with a count line.
fn users_table(users: &[loxone_xml::LoxUser], header: bool) -> String {
    let mut out = String::new();
    let nfc_count = users.iter().filter(|u| u.nfc).count();
    if header {
        let _ = writeln!(
            out,
            "{:<24} {:<4} {:<6} {:<28} {:<28} DESCRIPTION",
            "NAME", "NFC", "CODES", "GROUPS", "ACCESS"
        );
    }
    let count = |n: usize| if n == 0 { "-".into() } else { n.to_string() };
    for u in users {
        let access: Vec<String> = u
            .access
            .iter()
            .map(|a| format!("{} ({})", a.block, a.schedule))
            .collect();
        let _ = writeln!(
            out,
            "{:<24} {:<4} {:<6} {:<28} {:<28} {}",
            u.name,
            count(u.nfc_tags),
            count(u.access_codes),
            or_dash(&u.groups.join(", ")),
            or_dash(&access.join(", ")),
            u.description,
        );
    }
    let _ = writeln!(out, "\n{} users ({} with NFC)", users.len(), nfc_count);
    out
}

/// `lox config devices`: Tree, LoxAIR and network devices, a section each.
fn devices_text(devices: &[loxone_xml::LoxDevice]) -> String {
    let on = |bus: loxone_xml::DeviceBus| -> Vec<&loxone_xml::LoxDevice> {
        devices.iter().filter(|d| d.bus == bus).collect()
    };
    let tree = on(loxone_xml::DeviceBus::Tree);
    let air = on(loxone_xml::DeviceBus::Air);
    let net = on(loxone_xml::DeviceBus::Network);
    let mut out = String::new();
    if !tree.is_empty() {
        let _ = writeln!(out, "  Tree devices ({})", tree.len());
        let _ = writeln!(out, "  {:<30} {:<12} Type", "Name", "Serial");
        for d in &tree {
            let _ = writeln!(
                out,
                "  {:<30} {:<12} {}",
                d.name,
                d.serial.as_deref().unwrap_or("-"),
                d.type_label
            );
        }
    }
    if !air.is_empty() {
        if !tree.is_empty() {
            out.push('\n');
        }
        let _ = writeln!(out, "  LoxAIR devices ({})", air.len());
        let _ = writeln!(out, "  {:<30} Type", "Name");
        for d in &air {
            let _ = writeln!(out, "  {:<30} {}", d.name, d.type_label);
        }
    }
    if !net.is_empty() {
        if !tree.is_empty() || !air.is_empty() {
            out.push('\n');
        }
        let _ = writeln!(out, "  Network devices ({})", net.len());
        let _ = writeln!(out, "  {:<30} {:<18} MAC", "Name", "Address");
        for d in &net {
            let _ = writeln!(
                out,
                "  {:<30} {:<18} {}",
                d.name,
                d.address.as_deref().unwrap_or("-"),
                d.mac.as_deref().unwrap_or("-")
            );
        }
    }
    let _ = writeln!(out, "\n{} devices total", devices.len());
    out
}

/// `lox config init`: a git repo for the config history, its path saved in
/// the config.
fn config_init(path: &str) -> Result<()> {
    let cfg = Config::load()?;
    let abs_path = if path.starts_with('/') || path.starts_with('~') {
        let expanded = if path.starts_with('~') {
            path.replacen(
                '~',
                &dirs::home_dir().unwrap_or_default().to_string_lossy(),
                1,
            )
        } else {
            path.to_string()
        };
        std::path::PathBuf::from(expanded)
    } else {
        std::env::current_dir()?.join(path)
    };
    let repo = gitops::init(&abs_path, &cfg)?;
    // Save the repo path in config
    let mut cfg = Config::load().unwrap_or_default();
    cfg.config_repo = Some(repo.to_string_lossy().to_string());
    let saved = cfg.save()?;
    println!("Config repo initialized at {}", repo.display());
    println!("Saved repo path to {}", saved.display());
    println!("\nNext: run `lox config pull` to download and commit the current config.");
    Ok(())
}

fn or_dash(s: &str) -> &str {
    if s.is_empty() { "-" } else { s }
}

#[cfg(test)]
mod tests {
    use super::{CompressReport, aliases_table, backups_table, cache_info_text, token_info_text};
    use loxone_client::{ftp::BackupInfo, token::TokenStore};
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn backups_table_marks_the_latest() {
        let b = |v: u32, ts: &str| BackupInfo {
            filename: format!("sps_{v}_{ts}.zip"),
            version: v,
            timestamp: ts.into(),
            size: 4096,
        };
        assert_eq!(
            backups_table(&[b(274, "20261030120000"), b(273, "20261001080000")]),
            "  #    Version  Date                   Size\n\
             \x20 1    274      2026-10-30 12:00:00    4 KB  (latest)\n\
             \x20 2    273      2026-10-01 08:00:00    4 KB\n"
        );
    }

    #[test]
    fn aliases_sorted_or_hint() {
        assert!(aliases_table(&HashMap::new(), true).starts_with("No aliases."));
        let a = HashMap::from([
            ("z".to_string(), "u2".to_string()),
            ("a".into(), "u1".into()),
        ]);
        assert_eq!(
            aliases_table(&a, false),
            format!("{:<24} u1\n{:<24} u2\n", "a", "z")
        );
    }

    #[test]
    fn cache_info_valid_and_stale() {
        let t = cache_info_text(Path::new("/c.json"), 2048, Duration::from_secs(3_725));
        assert_eq!(
            t,
            "Cache: \"/c.json\"\nSize:  2.0 KB\nAge:   62m 5s\nStatus: ✓ valid (22h 57m until refresh)\n"
        );
        let t = cache_info_text(Path::new("/c.json"), 0, Duration::from_secs(90_000));
        assert!(t.ends_with("Status: ⚠ stale (will refresh on next command)\n"));
    }

    #[test]
    fn token_info_shortens_the_token() {
        let ts = TokenStore {
            token: "0123456789abcdef".into(),
            key: String::new(),
            valid_until: u64::MAX / 2,
        };
        let t = token_info_text(&ts, u64::MAX / 2 - 3 * 86400);
        assert_eq!(
            t,
            "Token: 01234567...cdef\nStatus: ✓ valid (3 days remaining)\n"
        );
    }

    #[test]
    fn compress_report_text_and_json() {
        let r = CompressReport {
            input: "a.Loxone".into(),
            template: Some("dl.zip".into()),
            output: "a.zip".into(),
            xml_bytes: 10_240,
            output_bytes: 2_048,
            crc32: 0xab,
        };
        assert_eq!(
            r.text(),
            "Compressed 10 KB → 2 KB → a.zip (extracts byte-identically)\n"
        );
        assert_eq!(r.json()["crc32"], "000000ab");
    }
}
