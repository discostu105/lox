use anyhow::{Context, Result, bail};
use clap::CommandFactory;
use clap_complete::generate;
use reqwest::blocking::Client;
use std::fs;
use std::time::Duration;

use crate::client::{LOXONE_EPOCH_SECS, LoxClient, USER_AGENT};
use crate::commands::RunContext;
use crate::config::Config;
use crate::scene::Scene;
use crate::token;
use crate::{
    AliasCmd, CacheCmd, Cli, ConfigCmd, SceneCmd, SetupCmd, TokenCmd, build_schema, detect_shell,
    ftp, gitops, install_completions, json_val_str, load_config_xml, loxcc, loxone_xml,
};

pub fn cmd_setup(ctx: &RunContext, action: SetupCmd) -> Result<()> {
    match action {
        SetupCmd::Set {
            host,
            user,
            pass,
            serial,
            verify_ssl,
            no_verify_ssl,
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
            let path = cfg.save()?;
            if !ctx.quiet {
                println!("✓  Config saved to {:?}", path);
            }
        }
        SetupCmd::Show => {
            let cfg = Config::load()?;
            println!("host:   {}", cfg.host);
            println!("user:   {}", cfg.user);
            println!("pass:   {}", "*".repeat(cfg.pass.len()));
            if !cfg.serial.is_empty() {
                println!("serial: {}", cfg.serial);
            }
            if !cfg.aliases.is_empty() {
                println!("aliases:");
                let mut aliases: Vec<_> = cfg.aliases.iter().collect();
                aliases.sort_by_key(|(k, _)| k.as_str());
                for (name, uuid) in aliases {
                    println!("  {}: {}", name, uuid);
                }
            }
        }
    }
    Ok(())
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
        AliasCmd::Ls => {
            if cfg.aliases.is_empty() {
                println!("No aliases. Add with: lox alias add <name> <uuid>");
            } else {
                let mut aliases: Vec<_> = cfg.aliases.iter().collect();
                aliases.sort_by_key(|(k, _)| k.as_str());
                if !ctx.no_header {
                    println!("{:<24} UUID", "ALIAS");
                    println!("{}", "─".repeat(60));
                }
                for (name, uuid) in aliases {
                    println!("{:<24} {}", name, uuid);
                }
            }
        }
    }
    Ok(())
}

pub fn cmd_scene(_ctx: &RunContext, action: SceneCmd) -> Result<()> {
    let cfg = Config::load().unwrap_or_default();
    match action {
        SceneCmd::Ls => {
            let names = Scene::list_with_config(&cfg)?;
            if names.is_empty() {
                println!("No scenes. Create: lox scene new <name>");
            } else {
                for name in &names {
                    let desc = Scene::load_with_config(name, &cfg)
                        .ok()
                        .and_then(|s| s.description)
                        .unwrap_or_default();
                    println!("  {:<20}  {}", name, desc);
                }
            }
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

pub fn cmd_cache(ctx: &RunContext, action: CacheCmd) -> Result<()> {
    let cfg = Config::load()?;
    let cache = LoxClient::cache_path(&cfg);
    match action {
        CacheCmd::Info => {
            if cache.exists() {
                let meta = cache.metadata()?;
                let age = std::time::SystemTime::now()
                    .duration_since(meta.modified()?)
                    .unwrap_or_default();
                let size = meta.len();
                println!("Cache: {:?}", cache);
                println!("Size:  {:.1} KB", size as f64 / 1024.0);
                println!("Age:   {}m {}s", age.as_secs() / 60, age.as_secs() % 60);
                if age.as_secs() < 86400 {
                    println!("Status: ✓ valid ({} until refresh)", {
                        let remaining = 86400u64.saturating_sub(age.as_secs());
                        format!("{}h {}m", remaining / 3600, (remaining % 3600) / 60)
                    });
                } else {
                    println!("Status: ⚠ stale (will refresh on next command)");
                }
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
            let lox = LoxClient::new(cfg)?;
            let resp = lox.get_json("/jdev/sps/LoxAPPversion3")?;
            let remote_ver = resp
                .pointer("/LL/value")
                .and_then(json_val_str)
                .unwrap_or_else(|| "?".to_string());
            if ctx.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "remote_version": remote_ver,
                        "cache_exists": cache.exists(),
                    })
                );
            } else {
                println!("Remote structure version: {}", remote_ver);
                if cache.exists() {
                    println!("Cache: exists at {:?}", cache);
                } else {
                    println!("Cache: not present");
                }
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
            LoxClient::load_or_fetch_structure(&cfg, &client)?;
            println!("✓ Structure cache refreshed ({:?})", cache);
        }
    }
    Ok(())
}

pub fn cmd_token(ctx: &RunContext, action: TokenCmd) -> Result<()> {
    let cfg = Config::load()?;
    match action {
        TokenCmd::Fetch => {
            println!("Fetching token from Miniserver...");
            let rt = tokio::runtime::Runtime::new()?;
            match rt.block_on(token::acquire_token(&cfg)) {
                Ok(ts) => {
                    let _exp =
                        std::time::UNIX_EPOCH + std::time::Duration::from_secs(ts.valid_until);
                    let days = ts.valid_until.saturating_sub(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    ) / 86400;
                    println!("✓ Token saved to {:?}", token::TokenStore::path_for(&cfg));
                    println!("  Valid for: {} days", days);
                }
                Err(e) => bail!("Token fetch failed: {}", e),
            }
        }
        TokenCmd::Info => match token::TokenStore::load_for(&cfg) {
            Some(ts) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let days_left = ts.valid_until.saturating_sub(now) / 86400;
                if ts.token.len() >= 12 {
                    println!(
                        "Token: {}...{}",
                        &ts.token[..8],
                        &ts.token[ts.token.len() - 4..]
                    );
                } else {
                    println!("Token: {}", ts.token);
                }
                if ts.is_valid() {
                    println!("Status: ✓ valid ({} days remaining)", days_left);
                } else {
                    println!("Status: ⚠ expired — run: lox token fetch");
                }
            }
            None => println!("No token saved. Using Basic Auth. Run: lox token fetch"),
        },
        TokenCmd::Clear => {
            let path = token::TokenStore::path_for(&cfg);
            if path.exists() {
                fs::remove_file(&path)?;
                println!("✓ Token cleared (reverting to Basic Auth)");
            } else {
                println!("No token to clear");
            }
        }
        TokenCmd::Check => {
            let ts = token::TokenStore::load_for(&cfg)
                .ok_or_else(|| anyhow::anyhow!("No token saved. Run: lox token fetch"))?;
            let lox = LoxClient::new(cfg.clone())?;
            // Hash the token with the key for the check endpoint
            let hash = token::hash_token(&ts.token, &ts.key);
            let resp = lox.get_json(&format!("/jdev/sys/checktoken/{}/{}", hash, cfg.user))?;
            let code = resp
                .pointer("/LL/Code")
                .and_then(json_val_str)
                .unwrap_or_else(|| "?".to_string());
            if ctx.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "valid": code == "200",
                        "code": code,
                    })
                );
            } else if code == "200" {
                println!("✓ Token is valid on the Miniserver");
            } else {
                println!("✗ Token is not valid (code {})", code);
            }
        }
        TokenCmd::Refresh => {
            let ts = token::TokenStore::load_for(&cfg)
                .ok_or_else(|| anyhow::anyhow!("No token saved. Run: lox token fetch"))?;
            let lox = LoxClient::new(cfg.clone())?;
            let hash = token::hash_token(&ts.token, &ts.key);
            let resp = lox.get_json(&format!("/jdev/sys/refreshtoken/{}/{}", hash, cfg.user))?;
            let code = resp
                .pointer("/LL/Code")
                .and_then(json_val_str)
                .unwrap_or_else(|| "?".to_string());
            if code == "200" {
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
                    new_ts.save_for(&cfg)?;
                    let days = unix_until.saturating_sub(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    ) / 86400;
                    println!("✓ Token refreshed ({} days remaining)", days);
                } else {
                    println!("✓ Token refreshed");
                }
            } else {
                bail!("Token refresh failed (code {})", code);
            }
        }
        TokenCmd::Revoke => {
            let ts = token::TokenStore::load_for(&cfg)
                .ok_or_else(|| anyhow::anyhow!("No token saved. Run: lox token fetch"))?;
            let lox = LoxClient::new(cfg.clone())?;
            let hash = token::hash_token(&ts.token, &ts.key);
            let resp = lox.get_json(&format!("/jdev/sys/killtoken/{}/{}", hash, cfg.user))?;
            let code = resp
                .pointer("/LL/Code")
                .and_then(json_val_str)
                .unwrap_or_else(|| "?".to_string());
            if code == "200" {
                // Remove local token
                let path = token::TokenStore::path_for(&cfg);
                if path.exists() {
                    fs::remove_file(&path)?;
                }
                println!("✓ Token revoked and cleared");
            } else {
                bail!("Token revoke failed (code {})", code);
            }
        }
    }
    Ok(())
}

pub fn cmd_config(ctx: &RunContext, action: ConfigCmd) -> Result<()> {
    match action {
        ConfigCmd::Ls => {
            let cfg = Config::load()?;
            let backups = ftp::list_backups(&cfg)?;
            if backups.is_empty() {
                println!("No configs found on the Miniserver.");
            } else if ctx.json {
                let arr: Vec<serde_json::Value> = backups
                    .iter()
                    .map(|b| {
                        serde_json::json!({
                            "filename": b.filename,
                            "version": b.version,
                            "date": b.formatted_date(),
                            "size": b.size,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else {
                println!("  {:<4} {:<8} {:<22} Size", "#", "Version", "Date");
                for (i, b) in backups.iter().enumerate() {
                    println!(
                        "  {:<4} {:<8} {:<22} {} KB{}",
                        i + 1,
                        b.version,
                        b.formatted_date(),
                        b.size / 1024,
                        if i == 0 { "  (latest)" } else { "" }
                    );
                }
            }
        }
        ConfigCmd::Download { save_as, extract } => {
            let cfg = Config::load()?;
            let backups = ftp::list_backups(&cfg)?;
            if backups.is_empty() {
                bail!("No configs found on the Miniserver.");
            }
            let newest = &backups[0];
            eprintln!(
                "Downloading {} ({} KB)...",
                newest.filename,
                newest.size / 1024
            );
            let data = ftp::download_backup(&cfg, &newest.filename)?;
            let out_path = save_as.unwrap_or_else(|| newest.filename.clone());
            fs::write(&out_path, &data)?;
            println!("Saved to {}", out_path);

            if extract {
                eprintln!("Extracting sps0.LoxCC...");
                let xml = loxcc::extract_and_decompress(&data)?;
                let xml_path = out_path
                    .strip_suffix(".zip")
                    .unwrap_or(&out_path)
                    .to_string()
                    + ".Loxone";
                fs::write(&xml_path, &xml)?;
                println!(
                    "Decompressed {} KB → {} KB → {}",
                    data.len() / 1024,
                    xml.len() / 1024,
                    xml_path
                );
            }
        }
        ConfigCmd::Extract { file, save_as } => {
            let zip_data = fs::read(&file).with_context(|| format!("Cannot read {}", file))?;
            eprintln!("Extracting sps0.LoxCC...");
            let xml = loxcc::extract_and_decompress(&zip_data)?;
            let xml_path = save_as.unwrap_or_else(|| {
                file.strip_suffix(".zip").unwrap_or(&file).to_string() + ".Loxone"
            });
            fs::write(&xml_path, &xml)?;
            println!(
                "Decompressed {} KB → {} KB → {}",
                zip_data.len() / 1024,
                xml.len() / 1024,
                xml_path
            );
        }
        ConfigCmd::Upload { file, force } => {
            let cfg = Config::load()?;
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
            let data = fs::read(&file).with_context(|| format!("Cannot read {}", file))?;
            let filename = std::path::Path::new(&file)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&file);
            eprintln!("Uploading {} ({} KB)...", filename, data.len() / 1024);
            ftp::upload_backup(&cfg, filename, &data)?;
            println!("Upload complete.");
            println!("Reboot the Miniserver to apply: lox reboot");
        }
        ConfigCmd::Users { file } => {
            if file.ends_with(".zip") {
                bail!(
                    "Expected a .Loxone XML file. Run `lox config extract {}` first.",
                    file
                );
            }
            let xml = fs::read(&file).with_context(|| format!("Cannot read {}", file))?;
            let users = loxone_xml::parse_users(&xml)?;
            if ctx.json {
                println!("{}", serde_json::to_string_pretty(&users)?);
            } else {
                let nfc_count = users.iter().filter(|u| u.nfc).count();
                println!("  {:<26} {:<6} Description", "Name", "NFC");
                for u in &users {
                    println!(
                        "  {:<26} {:<6} {}",
                        u.name,
                        if u.nfc { "yes" } else { "-" },
                        u.description,
                    );
                }
                println!("\n{} users ({} with NFC)", users.len(), nfc_count);
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
                let tree: Vec<_> = devices
                    .iter()
                    .filter(|d| d.bus == loxone_xml::DeviceBus::Tree)
                    .collect();
                let air: Vec<_> = devices
                    .iter()
                    .filter(|d| d.bus == loxone_xml::DeviceBus::Air)
                    .collect();
                let net: Vec<_> = devices
                    .iter()
                    .filter(|d| d.bus == loxone_xml::DeviceBus::Network)
                    .collect();

                if !tree.is_empty() {
                    println!("  Tree devices ({})", tree.len());
                    println!("  {:<30} {:<12} Type", "Name", "Serial");
                    for d in &tree {
                        println!(
                            "  {:<30} {:<12} {}",
                            d.name,
                            d.serial.as_deref().unwrap_or("-"),
                            d.type_label
                        );
                    }
                }
                if !air.is_empty() {
                    if !tree.is_empty() {
                        println!();
                    }
                    println!("  LoxAIR devices ({})", air.len());
                    println!("  {:<30} Type", "Name");
                    for d in &air {
                        println!("  {:<30} {}", d.name, d.type_label);
                    }
                }
                if !net.is_empty() {
                    if !tree.is_empty() || !air.is_empty() {
                        println!();
                    }
                    println!("  Network devices ({})", net.len());
                    println!("  {:<30} {:<18} MAC", "Name", "Address");
                    for d in &net {
                        println!(
                            "  {:<30} {:<18} {}",
                            d.name,
                            d.address.as_deref().unwrap_or("-"),
                            d.mac.as_deref().unwrap_or("-")
                        );
                    }
                }
                println!("\n{} devices total", devices.len());
            }
        }
        ConfigCmd::Diff { file1, file2 } => {
            let xml1 = load_config_xml(&file1)?;
            let xml2 = load_config_xml(&file2)?;
            let s1 = loxone_xml::parse_config_summary(&xml1)?;
            let s2 = loxone_xml::parse_config_summary(&xml2)?;
            let diff = loxone_xml::diff_configs(&s1, &s2);

            if ctx.json {
                println!("{}", serde_json::to_string_pretty(&diff)?);
            } else {
                println!(
                    "Config version: {} → {}",
                    diff.version_old, diff.version_new
                );
                println!("Modified: {} → {}", diff.date_old, diff.date_new);

                if !diff.controls_added.is_empty()
                    || !diff.controls_removed.is_empty()
                    || !diff.controls_changed.is_empty()
                {
                    println!("\nControls:");
                    for c in &diff.controls_added {
                        println!("  + Added: \"{}\" ({})", c.name, c.control_type);
                    }
                    for c in &diff.controls_changed {
                        println!(
                            "  ~ Changed: \"{}\" — {} \"{}\" → \"{}\"",
                            c.name, c.field, c.old_value, c.new_value
                        );
                    }
                    for c in &diff.controls_removed {
                        println!("  - Removed: \"{}\" ({})", c.name, c.control_type);
                    }
                }

                if !diff.rooms_added.is_empty()
                    || !diff.rooms_removed.is_empty()
                    || !diff.rooms_renamed.is_empty()
                {
                    println!("\nRooms:");
                    for r in &diff.rooms_added {
                        println!("  + Added: \"{}\"", r);
                    }
                    for r in &diff.rooms_renamed {
                        println!("  ~ Renamed: \"{}\" → \"{}\"", r.old, r.new);
                    }
                    for r in &diff.rooms_removed {
                        println!("  - Removed: \"{}\"", r);
                    }
                }

                if !diff.categories_added.is_empty()
                    || !diff.categories_removed.is_empty()
                    || !diff.categories_renamed.is_empty()
                {
                    println!("\nCategories:");
                    for c in &diff.categories_added {
                        println!("  + Added: \"{}\"", c);
                    }
                    for c in &diff.categories_renamed {
                        println!("  ~ Renamed: \"{}\" → \"{}\"", c.old, c.new);
                    }
                    for c in &diff.categories_removed {
                        println!("  - Removed: \"{}\"", c);
                    }
                }

                if !diff.users_added.is_empty() || !diff.users_removed.is_empty() {
                    println!("\nUsers:");
                    for u in &diff.users_added {
                        println!("  + Added: \"{}\"", u);
                    }
                    for u in &diff.users_removed {
                        println!("  - Removed: \"{}\"", u);
                    }
                }

                let total = diff.controls_added.len()
                    + diff.controls_removed.len()
                    + diff.controls_changed.len()
                    + diff.rooms_added.len()
                    + diff.rooms_removed.len()
                    + diff.rooms_renamed.len()
                    + diff.categories_added.len()
                    + diff.categories_removed.len()
                    + diff.categories_renamed.len()
                    + diff.users_added.len()
                    + diff.users_removed.len();

                if !diff.has_changes() {
                    println!("\nNo structural changes.");
                } else {
                    println!("\n{} changes total", total);
                }
            }
        }
        ConfigCmd::Init { path } => {
            let cfg = Config::load()?;
            let abs_path = if path.starts_with('/') || path.starts_with('~') {
                let expanded = if path.starts_with('~') {
                    path.replacen(
                        '~',
                        &dirs::home_dir().unwrap_or_default().to_string_lossy(),
                        1,
                    )
                } else {
                    path.clone()
                };
                std::path::PathBuf::from(expanded)
            } else {
                std::env::current_dir()?.join(&path)
            };
            let repo = gitops::init(&abs_path, &cfg)?;
            // Save the repo path in config
            let mut cfg = Config::load().unwrap_or_default();
            cfg.config_repo = Some(repo.to_string_lossy().to_string());
            let saved = cfg.save()?;
            println!("Config repo initialized at {}", repo.display());
            println!("Saved repo path to {}", saved.display());
            println!("\nNext: run `lox config pull` to download and commit the current config.");
        }
        ConfigCmd::Pull { quiet: pull_quiet } => {
            let cfg = Config::load()?;
            let repo_path = cfg
                .config_repo
                .as_deref()
                .context("No config repo configured. Run `lox config init <path>` first.")?;
            let q = pull_quiet || ctx.quiet;
            let committed = gitops::pull(std::path::Path::new(repo_path), &cfg, q)?;
            if q && committed {
                // In quiet/cron mode, just indicate a commit was made
                println!("committed");
            }
        }
        ConfigCmd::Log { count } => {
            let cfg = Config::load()?;
            let repo_path = cfg
                .config_repo
                .as_deref()
                .context("No config repo configured. Run `lox config init <path>` first.")?;
            let ms = if !cfg.serial.is_empty() {
                Some(cfg.serial.as_str())
            } else {
                None
            };
            let output = gitops::log(std::path::Path::new(repo_path), ms, count)?;
            if output.is_empty() {
                println!("No config history yet. Run `lox config pull` first.");
            } else {
                println!("{}", output);
            }
        }
        ConfigCmd::Restore { commit, force } => {
            let cfg = Config::load()?;
            let repo_path = cfg
                .config_repo
                .as_deref()
                .context("No config repo configured. Run `lox config init <path>` first.")?;
            gitops::restore(std::path::Path::new(repo_path), &cfg, &commit, force)?;
        }
    }
    Ok(())
}

pub fn cmd_completions(
    _ctx: &RunContext,
    shell: Option<clap_complete::Shell>,
    install: bool,
) -> Result<()> {
    let detected = shell.or_else(detect_shell);
    let Some(sh) = detected else {
        bail!("Could not detect shell. Specify one: lox completions bash|zsh|fish");
    };
    let mut cmd = Cli::command();
    if install {
        install_completions(sh, &mut cmd)?;
    } else {
        generate(sh, &mut cmd, "lox", &mut std::io::stdout());
    }
    Ok(())
}

pub fn cmd_schema(_ctx: &RunContext, command: Option<String>) -> Result<()> {
    let schema = build_schema(command.as_deref())?;
    println!("{}", serde_json::to_string_pretty(&schema)?);
    Ok(())
}
