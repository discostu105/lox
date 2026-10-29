//! `lox user ls/show` and `lox access log/check/perms`: the Miniserver's users
//! and who may open which NFC Code Touch. Read-only; the user lox connects
//! with needs the right to manage users.

use anyhow::Result;

use crate::client::LoxClient;
use crate::commands::RunContext;
use crate::config::Config;
use crate::snapshot;
use crate::usermgmt::{self, AccessReport, Verdict};
use crate::{AccessCmd, AccessConfigSource, UserCmd};

fn print_json(v: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

pub fn cmd_user(ctx: &RunContext, action: UserCmd) -> Result<()> {
    let lox = LoxClient::new(Config::load()?)?;
    match action {
        UserCmd::Ls => {
            let mut users = usermgmt::user_list(&lox)?;
            users.sort_by_key(|u| u.name.to_lowercase());
            if ctx.json {
                return print_json(&users);
            }
            if !ctx.no_header {
                println!("{:<24} {:<20} {:<6} UUID", "NAME", "STATE", "ADMIN");
            }
            for u in &users {
                println!(
                    "{:<24} {:<20} {:<6} {}",
                    u.name,
                    u.state_text,
                    yes_no(u.is_admin),
                    u.uuid
                );
            }
            if !ctx.quiet {
                println!("\n{} users", users.len());
            }
        }
        UserCmd::Groups => {
            let mut groups = usermgmt::group_list(&lox)?;
            groups.sort_by_key(|g| g.name.to_lowercase());
            if ctx.json {
                return print_json(&groups);
            }
            if !ctx.no_header {
                println!("{:<24} {:<12} UUID", "NAME", "TYPE");
            }
            for g in &groups {
                println!(
                    "{:<24} {:<12} {}",
                    g.name,
                    usermgmt::group_type_text(g.typ),
                    g.uuid
                );
            }
            if !ctx.quiet {
                println!("\n{} groups", groups.len());
            }
        }
        UserCmd::Show { user } => {
            let u = usermgmt::resolve_user(&lox, &user)?;
            if ctx.json {
                return print_json(&u);
            }
            let time = |t: Option<i64>| t.map(crate::lox_timestamp_to_string);
            println!("{}", u.name);
            println!("  {:<13} {}", "UUID", u.uuid);
            if !u.description.is_empty() {
                println!("  {:<13} {}", "Description", u.description);
            }
            println!("  {:<13} {}", "State", u.state_text);
            if let Some(t) = time(u.valid_from) {
                println!("  {:<13} {}", "Valid from", t);
            }
            if let Some(t) = time(u.valid_until) {
                println!("  {:<13} {}", "Valid until", t);
            }
            println!("  {:<13} {}", "Admin", yes_no(u.is_admin));
            if let Some(t) = time(u.last_edit) {
                println!("  {:<13} {}", "Last edit", t);
            }
            if let Some(t) = &u.trust_member {
                println!("  {:<13} {}", "Trust member", t);
            }
            let groups: Vec<&str> = u.groups.iter().map(|g| g.name.as_str()).collect();
            println!(
                "  {:<13} {}",
                "Groups",
                if groups.is_empty() {
                    "-".to_string()
                } else {
                    groups.join(", ")
                }
            );
            println!("  {:<13} {}", "NFC tags", u.nfc_tags.len());
            for t in &u.nfc_tags {
                println!("    {:<28} {}", t.name, t.id);
            }
            println!("  {:<13} {}", "Access codes", u.access_codes);
        }
    }
    Ok(())
}

pub fn cmd_access(ctx: &RunContext, action: AccessCmd) -> Result<()> {
    match action {
        AccessCmd::Log { code_touch, lines } => {
            let mut lox = LoxClient::new(Config::load()?)?;
            let log = usermgmt::read_access_log(&mut lox, &code_touch, lines)?;
            if ctx.json {
                return print_json(&log);
            }
            if log.total == 0 && log.skipped == 0 {
                println!("{}: no access log yet", log.code_touch);
                return Ok(());
            }
            if !ctx.no_header {
                println!(
                    "{:<19} {:<12} {:<6} {:<24} {:<16} DESCRIPTION",
                    "TIME", "RESULT", "OUTPUT", "METHOD", "USER"
                );
            }
            for e in &log.entries {
                let time = e.time.get(..19).unwrap_or(&e.time).replace('T', " ");
                let output = if e.output > 0 {
                    e.output.to_string()
                } else {
                    "-".into()
                };
                let mut desc = e.description.clone().unwrap_or_default();
                if let Some(m) = &e.ext_medium {
                    desc = format!("{} {}", m, e.ext_authority.as_deref().unwrap_or(""));
                }
                println!(
                    "{:<19} {:<12} {:<6} {:<24} {:<16} {}",
                    time,
                    e.result,
                    output,
                    e.method,
                    e.user.as_deref().unwrap_or("-"),
                    desc.trim()
                );
            }
            if !ctx.quiet {
                let denied = log.entries.iter().filter(|e| e.result == "denied").count();
                println!(
                    "\n{} of {} entries ({} denied) — {}",
                    log.count, log.total, denied, log.code_touch
                );
                if log.skipped > 0 {
                    eprintln!("{} unreadable entries skipped", log.skipped);
                }
            }
        }
        AccessCmd::Perms { control } => {
            let mut lox = LoxClient::new(Config::load()?)?;
            let p = usermgmt::read_control_permissions(&mut lox, &control)?;
            if ctx.json {
                return print_json(&p);
            }
            if !ctx.no_header {
                println!("{:<10} {:<28} UUID", "TYPE", "NAME");
            }
            for pr in &p.principals {
                println!("{:<10} {:<28} {}", pr.typ, pr.name, pr.uuid);
            }
            if !ctx.quiet {
                println!(
                    "\n{} assigned to {} ({}) — UI and access rights alike; members of \
                     groups are not listed",
                    p.count, p.control, p.typ
                );
            }
        }
        AccessCmd::Check {
            user,
            code_touch,
            src,
        } => {
            let mut lox = LoxClient::new(Config::load()?)?;
            let u = usermgmt::resolve_user(&lox, &user)?;
            let snap = load_config(&lox.cfg, &src)?;
            let report = usermgmt::access_report(
                &mut lox,
                &u,
                code_touch.as_deref(),
                &snap.bytes,
                &snap.label,
            )?;
            if ctx.json {
                return print_json(&report);
            }
            print_report(&report);
        }
    }
    Ok(())
}

/// The program file the access rights come from: a fresh download unless
/// told otherwise, since rights checked against an old file mislead.
fn load_config(cfg: &Config, src: &AccessConfigSource) -> Result<snapshot::Snapshot> {
    if src.cached {
        snapshot::current(cfg)
    } else {
        snapshot::load(cfg, src.at.as_deref(), src.file.as_deref(), true)
    }
}

fn print_report(r: &AccessReport) {
    println!(
        "{} ({}) — {} NFC tag{}, {} access code{}",
        r.user,
        r.user_state,
        r.nfc_tags,
        if r.nfc_tags == 1 { "" } else { "s" },
        r.access_codes,
        if r.access_codes == 1 { "" } else { "s" },
    );
    println!("Access rights from {}\n", r.config);
    for c in &r.checks {
        let verdict = match c.verdict {
            Verdict::Granted => "GRANTED",
            Verdict::NoAccessRight => "NO ACCESS RIGHT",
            Verdict::NoCredential => "NO CREDENTIAL",
            Verdict::UserDisabled => "USER DISABLED",
            Verdict::UnknownFullAccess => "UNKNOWN (FULL ACCESS)",
        };
        println!("{}: {}", c.code_touch, verdict);
        println!("  {}", c.reason);
        for w in &c.warnings {
            println!("  warning: {}", w);
        }
        for h in &c.hints {
            println!("  note: {}", h);
        }
    }
}
