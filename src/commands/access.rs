//! `lox user …` and `lox access …`: the Miniserver's users, who may open
//! which NFC Code Touch, and the changes — NFC tags (learn, pair, remove) and
//! access codes. The user lox connects with needs the right to manage users.

use anyhow::{Result, bail};
use serde::Serialize;
use std::io::{IsTerminal, Write};
use std::time::Duration;

use crate::commands::RunContext;
use crate::config::Config;
use crate::nfc_learn::{self, LearnedTag};
use crate::snapshot;
use crate::usermgmt::{self, AccessReport, UserChange, Verdict};
use crate::{AccessCmd, AccessConfigSource, UserCmd, UserCodeCmd, UserNfcCmd};
use loxone_client::client::LoxClient;

fn print_json(v: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

pub fn cmd_user(ctx: &RunContext, action: UserCmd) -> Result<()> {
    let lox = Config::load()?.client()?;
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
            let time = |t: Option<i64>| t.map(loxone_client::time::lox_timestamp_to_string);
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
            // lastEdit counts from 2009-01-01 UTC, unlike the validity times
            if let Some(t) = u
                .last_edit
                .map(loxone_client::time::lox_utc_timestamp_to_string)
            {
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
        UserCmd::Nfc(UserNfcCmd::Add {
            user,
            id,
            name,
            yes,
        }) => {
            let u = usermgmt::resolve_user(&lox, &user)?;
            let change = usermgmt::plan_add_nfc(&u, &id, &name)?;
            run_change(ctx, &lox, change, yes, None)?;
        }
        UserCmd::Nfc(UserNfcCmd::Rm { user, tag, yes }) => {
            let u = usermgmt::resolve_user(&lox, &user)?;
            let change = usermgmt::plan_remove_nfc(&u, &tag)?;
            run_change(ctx, &lox, change, yes, None)?;
        }
        UserCmd::Code(UserCodeCmd::Set { user, yes }) => {
            let u = usermgmt::resolve_user(&lox, &user)?;
            let change = usermgmt::plan_set_code(&u);
            let code = read_code(ctx)?;
            run_change(ctx, &lox, change, yes, Some(&code))?;
        }
    }
    Ok(())
}

/// Whether there is someone to ask: a terminal, and prompts not switched off.
fn can_ask(ctx: &RunContext) -> bool {
    !ctx.json
        && !ctx.non_interactive
        && std::io::stdin().is_terminal()
        && std::io::stderr().is_terminal()
}

/// Ask before a change; `--yes` skips the question.
fn confirm(ctx: &RunContext, yes: bool, question: &str) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !can_ask(ctx) {
        bail!("this change requires --yes: there is no terminal to confirm it on");
    }
    eprint!("{} [y/N] ", question);
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        bail!("Aborted — nothing was changed")
    }
}

/// The new access code: a hidden prompt, asked twice, on a terminal; else
/// the first line of stdin. Never from the command line, never echoed.
fn read_code(ctx: &RunContext) -> Result<String> {
    let code = if std::io::stdin().is_terminal() {
        if ctx.json || ctx.non_interactive {
            bail!(
                "no access code on stdin — pipe it in (prompts are off with -o json and \
                 --non-interactive)"
            );
        }
        let code = rpassword::prompt_password("New access code (2-8 digits): ")?;
        usermgmt::validate_access_code(code.trim())?;
        let again = rpassword::prompt_password("Repeat the code: ")?;
        if code.trim() != again.trim() {
            bail!("the two codes differ — nothing was changed");
        }
        code
    } else {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        line
    };
    let code = code.trim().to_string();
    usermgmt::validate_access_code(&code)?;
    Ok(code)
}

/// Show a planned change (--dry-run), or confirm and carry it out.
fn run_change(
    ctx: &RunContext,
    lox: &LoxClient,
    mut change: UserChange,
    yes: bool,
    code: Option<&str>,
) -> Result<()> {
    if ctx.dry_run {
        change.dry_run = true;
        if ctx.json {
            return print_json(&change);
        }
        println!("[dry-run] {}", change.question());
        for r in &change.requests {
            println!("  would send: {}", r);
        }
        for w in &change.warnings {
            println!("  warning: {}", w);
        }
        return Ok(());
    }
    if !yes && !ctx.quiet {
        for w in &change.warnings {
            eprintln!("warning: {}", w);
        }
    }
    confirm(ctx, yes, &change.question())?;
    usermgmt::apply(lox, &mut change, code)?;
    print_change(ctx, &change)
}

fn print_change(ctx: &RunContext, change: &UserChange) -> Result<()> {
    if ctx.json {
        return print_json(change);
    }
    match (change.action.as_str(), &change.tag) {
        ("add-nfc-tag", Some(t)) => println!(
            "Paired the NFC tag {} as '{}' with {}.",
            t.id, t.name, change.user
        ),
        ("remove-nfc-tag", Some(t)) => println!(
            "Removed the NFC tag '{}' ({}) from {}.",
            t.name, t.id, change.user
        ),
        _ => println!("Set a new access code for {}.", change.user),
    }
    for w in &change.warnings {
        eprintln!("warning: {}", w);
    }
    if !ctx.quiet
        && let Some(note) = &change.note
    {
        eprintln!("note: {}", note);
    }
    Ok(())
}

/// `lox access learn -o json`: the tag, and the pairing if --user asked for one.
#[derive(Serialize)]
struct LearnOutput {
    #[serde(flatten)]
    tag: LearnedTag,
    #[serde(skip_serializing_if = "Option::is_none")]
    paired: Option<UserChange>,
}

fn learn_timeout(s: &str) -> Result<Duration> {
    let t = crate::otel::parse_interval(s)
        .map_err(|_| anyhow::anyhow!("--timeout '{}': expected e.g. 60s or 2m", s))?;
    if t.is_zero() || t > nfc_learn::MAX_TIMEOUT {
        bail!("--timeout must be between 1s and 5m");
    }
    Ok(t)
}

fn cmd_learn(
    ctx: &RunContext,
    code_touch: &str,
    user: Option<&str>,
    name: Option<&str>,
    timeout: &str,
    visu_password_file: Option<&std::path::Path>,
    yes: bool,
) -> Result<()> {
    let timeout = learn_timeout(timeout)?;
    let cfg = Config::load()?;
    let mut lox = cfg.client()?;
    let mut target = nfc_learn::target(&mut lox, code_touch)?;
    let owner = match user {
        Some(q) => Some(usermgmt::resolve_user(&lox, q)?),
        None => None,
    };
    let tag_name = name.map(usermgmt::validate_tag_name).transpose()?;

    if ctx.dry_run {
        let mut tag = LearnedTag::planned(&target, timeout);
        if let (Some(u), Some(n)) = (&owner, tag_name) {
            tag.would_send.push(format!(
                "GET {}",
                usermgmt::add_nfc_path(&u.uuid, "<learned ID>", n)
            ));
        }
        if ctx.json {
            return print_json(&tag);
        }
        println!("[dry-run] learn an NFC tag at {}", target.name);
        for c in &tag.would_send {
            println!("  would send: {}", c);
        }
        return Ok(());
    }
    // fail before the user holds a tag to the reader for nothing
    if owner.is_some() && !yes && !can_ask(ctx) {
        bail!("pairing the tag requires --yes: there is no terminal to confirm it on");
    }
    if target.secured {
        target.visu_password = match crate::visu::from_settings(visu_password_file, &cfg)? {
            Some(pw) => Some(pw),
            None if can_ask(ctx) => Some(crate::visu::prompt(&cfg.user)?),
            None => return Err(target.needs_password().expect("secured, no password")),
        };
    }

    if !ctx.quiet {
        eprintln!(
            "Hold the NFC tag or the unlocked phone to {} now ({} s; Ctrl-C ends learn mode)…",
            target.name,
            timeout.as_secs()
        );
    }
    let rt = tokio::runtime::Runtime::new()?;
    let learned = rt.block_on(async {
        let mut session =
            loxone_client::stream::Session::open(&cfg.connection(), Duration::from_secs(20))
                .await?;
        let ctrl_c = async {
            // without a handler, never fire (Ctrl-C then ends the process)
            if tokio::signal::ctrl_c().await.is_err() {
                std::future::pending::<()>().await;
            }
        };
        nfc_learn::learn(&mut session, &target, timeout, ctrl_c).await
    })?;
    // names for the IDs; the tag is read even when the list is not
    let users = usermgmt::user_list(&lox).unwrap_or_default();
    let tag = LearnedTag::new(&target, learned, &users);
    if !ctx.json {
        println!("{}", tag.describe());
        for w in &tag.warnings {
            eprintln!("warning: {}", w);
        }
    }

    let paired = match (owner, tag_name) {
        (Some(u), Some(n)) => pair_learned(ctx, &lox, &tag, &u, n, yes)?,
        _ => None,
    };
    if ctx.json {
        return print_json(&LearnOutput { tag, paired });
    }
    if let Some(change) = &paired {
        print_change(ctx, change)?;
    }
    Ok(())
}

/// Pair a learned tag with `user`, unless it is theirs already; refuse a tag
/// another user has.
fn pair_learned(
    ctx: &RunContext,
    lox: &LoxClient,
    tag: &LearnedTag,
    user: &usermgmt::User,
    name: &str,
    yes: bool,
) -> Result<Option<UserChange>> {
    match &tag.user_uuid {
        Some(owner) if owner.eq_ignore_ascii_case(&user.uuid) => {
            if !ctx.json {
                println!("{} has this tag already — nothing to pair.", user.name);
            }
            return Ok(None);
        }
        Some(owner) => bail!(
            "the tag {} is paired with {} already — remove it there first \
             (`lox user nfc rm {} \"{}\"`)",
            tag.id,
            tag.user.as_deref().unwrap_or(owner),
            tag.user.as_deref().unwrap_or(owner),
            tag.id
        ),
        None => {}
    }
    let mut change = usermgmt::plan_add_nfc(user, &tag.id, name)?;
    confirm(ctx, yes, &change.question())?;
    usermgmt::apply(lox, &mut change, None)?;
    Ok(Some(change))
}

pub fn cmd_access(ctx: &RunContext, action: AccessCmd) -> Result<()> {
    match action {
        AccessCmd::Log { code_touch, lines } => {
            let mut lox = Config::load()?.client()?;
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
            let mut lox = Config::load()?.client()?;
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
        AccessCmd::Learn {
            code_touch,
            user,
            name,
            timeout,
            yes,
            visu_password_file,
        } => {
            cmd_learn(
                ctx,
                &code_touch,
                user.as_deref(),
                name.as_deref(),
                &timeout,
                visu_password_file.as_deref(),
                yes,
            )?;
        }
        AccessCmd::Check {
            user,
            code_touch,
            src,
        } => {
            let cfg = Config::load()?;
            let mut lox = cfg.client()?;
            let u = usermgmt::resolve_user(&lox, &user)?;
            let snap = load_config(&cfg, &src)?;
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
