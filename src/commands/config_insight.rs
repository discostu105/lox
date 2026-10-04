//! `lox config diff/show/wiring/find/lint/history`: read-only views of the
//! logic in a `.Loxone` config, from a file, the config repo or the cache.

use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::commands::RunContext;
use crate::config::Config;
use crate::logic::{self, BlockRef, Logic, TraceView, WireView};
use crate::snapshot::{self, Snapshot};
use crate::{ConfigSource, TraceDir};

fn print_json(v: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

/// A config file needs no context; everything else does.
fn load_cfg(file: bool) -> Result<Config> {
    if file {
        Ok(Config::load().unwrap_or_default())
    } else {
        Config::load()
    }
}

fn load(src: &ConfigSource) -> Result<(Snapshot, Logic)> {
    let cfg = load_cfg(src.file.is_some())?;
    let snap = snapshot::load(&cfg, src.at.as_deref(), src.file.as_deref(), src.download)?;
    let l = Logic::parse(&snap.bytes).context("cannot read the config's logic")?;
    Ok((snap, l))
}

// ── diff ─────────────────────────────────────────────────────────────────────

/// A diff side: an existing file, else a snapshot of the config repo.
fn side(cfg: &Config, spec: &str) -> Result<Snapshot> {
    if Path::new(spec).is_file() {
        snapshot::file(spec)
    } else {
        snapshot::at_commit(cfg, &snapshot::resolve(cfg, spec)?)
    }
}

pub fn cmd_diff(ctx: &RunContext, old: Option<String>, new: Option<String>) -> Result<()> {
    let (old, new) = match (old, new) {
        (Some(o), None) if o.contains("..") && !Path::new(&o).exists() => {
            let (a, b) = o.split_once("..").unwrap_or_default();
            let b = b.trim_start_matches('.');
            (Some(a.to_string()), (!b.is_empty()).then(|| b.to_string()))
        }
        x => x,
    };
    let files = [&old, &new]
        .iter()
        .all(|s| s.as_deref().is_some_and(|p| Path::new(p).is_file()));
    let cfg = load_cfg(files)?;
    let (a, b) = match (old, new) {
        (Some(o), Some(n)) => (side(&cfg, &o)?, side(&cfg, &n)?),
        (Some(o), None) => (side(&cfg, &o)?, snapshot::current(&cfg)?),
        (None, _) => {
            let c = snapshot::commits(&cfg, 2)?;
            match c.as_slice() {
                [n, o] => (snapshot::at_commit(&cfg, o)?, snapshot::at_commit(&cfg, n)?),
                [_] => {
                    bail!("the config repository has only one snapshot — nothing to compare yet")
                }
                _ => bail!("the config repository has no snapshots yet — run `lox config pull`"),
            }
        }
    };
    let lines = logic::diff(&a.bytes, &b.bytes)?;
    if ctx.json {
        return print_json(&serde_json::json!({
            "old": a.label,
            "new": b.label,
            "changes": lines,
        }));
    }
    if !ctx.quiet {
        println!("{} → {}\n", a.label, b.label);
    }
    print_diff(&lines);
    Ok(())
}

fn print_diff(lines: &[logic::DiffLine]) {
    if lines.is_empty() {
        println!("No logic changes (layout or metadata only).");
        return;
    }
    for l in lines {
        if let Some(p) = l.text.strip_prefix("# ") {
            println!("\n{}", p);
        } else if l.text.starts_with("= ") {
            println!("{}", l.text);
        } else {
            println!("  {}", l.text);
        }
    }
}

// ── show ─────────────────────────────────────────────────────────────────────

fn find_page(l: &Logic, q: &str) -> Result<String> {
    let pages = l.pages();
    if let Some(p) = pages.iter().find(|p| p.title.eq_ignore_ascii_case(q)) {
        return Ok(p.title.clone());
    }
    let lq = q.to_lowercase();
    let hits: Vec<&str> = pages
        .iter()
        .filter(|p| p.title.to_lowercase().contains(&lq))
        .map(|p| p.title.as_str())
        .collect();
    match hits.as_slice() {
        [one] => Ok(one.to_string()),
        [] => bail!(
            "no page matching '{}' — `lox config show` lists the pages",
            q
        ),
        many => bail!(
            "'{}' matches {} pages: {} — use more of the title",
            q,
            many.len(),
            many.join(", ")
        ),
    }
}

pub fn cmd_show(
    ctx: &RunContext,
    page: Option<String>,
    all: bool,
    src: ConfigSource,
) -> Result<()> {
    let (snap, l) = load(&src)?;
    if all {
        let sources = l.all_sources()?;
        if ctx.json {
            let v: Vec<_> = sources
                .iter()
                .map(|(t, s)| serde_json::json!({"page": t, "source": s}))
                .collect();
            return print_json(&serde_json::json!({"config": snap.label, "pages": v}));
        }
        for (i, (t, s)) in sources.iter().enumerate() {
            if i > 0 {
                println!();
            }
            println!("// ── {} ──", t);
            print!("{}", s);
        }
        return Ok(());
    }
    if let Some(q) = page {
        let title = find_page(&l, &q)?;
        let source = l.page_source(&title)?;
        if ctx.json {
            let blocks: Vec<BlockRef> = l
                .page_blocks(&title)
                .iter()
                .map(|b| l.block_ref(&b.uuid, None))
                .collect();
            return print_json(&serde_json::json!({
                "config": snap.label,
                "page": title,
                "blocks": blocks,
                "source": source,
            }));
        }
        print!("{}", source);
        return Ok(());
    }
    let pages = l.pages();
    if ctx.json {
        return print_json(&serde_json::json!({"config": snap.label, "pages": pages}));
    }
    let w = pages
        .iter()
        .map(|p| p.title.chars().count())
        .max()
        .unwrap_or(4)
        .max(4);
    if !ctx.no_header {
        println!("{:<w$}  {:>6}  {:>6}", "PAGE", "BLOCKS", "WIRES", w = w);
        println!("{}", "─".repeat(w + 16));
    }
    for p in &pages {
        println!("{:<w$}  {:>6}  {:>6}", p.title, p.blocks, p.wires, w = w);
    }
    println!(
        "\n{} pages · {} blocks · {} wires ({})",
        pages.len(),
        l.browsable().count(),
        l.folded_wires().len(),
        snap.label
    );
    Ok(())
}

// ── wiring ───────────────────────────────────────────────────────────────────

/// Where a block lives, leaving out the page it shares with the center.
fn place(b: &BlockRef, center_page: Option<&str>) -> String {
    let page = b
        .page
        .as_deref()
        .filter(|p| b.device.is_none() && Some(*p) != center_page);
    let mut parts: Vec<&str> = Vec::new();
    for s in [b.device.as_deref(), page, b.room.as_deref()]
        .into_iter()
        .flatten()
    {
        if !s.is_empty() && !parts.contains(&s) {
            parts.push(s);
        }
    }
    parts.join(" · ")
}

fn endpoint(b: &BlockRef) -> String {
    match &b.key {
        Some(k) => format!("{}.{}", b.title, k),
        None => b.title.clone(),
    }
}

fn print_wires(title: &str, arrow: &str, wires: &[WireView], center_page: Option<&str>) {
    if wires.is_empty() {
        return;
    }
    println!("\n{}", title);
    let kw = wires.iter().map(|w| w.key.len()).max().unwrap_or(0);
    let ew = wires
        .iter()
        .map(|w| endpoint(&w.block).chars().count())
        .max()
        .unwrap_or(0)
        .min(48);
    for w in wires {
        let at = place(&w.block, center_page);
        let e = endpoint(&w.block);
        if at.is_empty() {
            println!("  {:<kw$} {} {}", w.key, arrow, e, kw = kw);
        } else {
            println!(
                "  {:<kw$} {} {:<ew$}  {}",
                w.key,
                arrow,
                e,
                at,
                kw = kw,
                ew = ew
            );
        }
        if !w.also.is_empty() {
            let names: Vec<&str> = w.also.iter().map(|a| a.title.as_str()).collect();
            let more = names.len().saturating_sub(6);
            println!(
                "  {:<kw$}   also → {}{}",
                "",
                names[..names.len().min(6)].join(", "),
                if more > 0 {
                    format!(" (+{})", more)
                } else {
                    String::new()
                },
                kw = kw
            );
        }
    }
}

fn print_trace(title: &str, arrow: &str, nodes: &[TraceView], center_page: Option<&str>) {
    println!("\n{}", title);
    if nodes.is_empty() {
        println!("  (nothing wired)");
    }
    for t in nodes {
        let at = place(&t.block, center_page);
        println!(
            "  {}{} {} {}{}{}",
            "  ".repeat(t.depth.saturating_sub(1)),
            t.parent_key,
            arrow,
            endpoint(&t.block),
            if at.is_empty() {
                String::new()
            } else {
                format!("  · {}", at)
            },
            if t.cycle { "  ↺ (loop)" } else { "" }
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_wiring(
    ctx: &RunContext,
    name: String,
    room: Option<String>,
    trace: Option<TraceDir>,
    depth: usize,
    all_params: bool,
    src: ConfigSource,
) -> Result<()> {
    let (snap, l) = load(&src)?;
    let b = l.resolve(&name, room.as_deref())?;
    let tr = trace.map(|t| (t != TraceDir::Down, t != TraceDir::Up, depth.max(1)));
    let v = l
        .wiring_view(&b.uuid, all_params, tr)
        .context("block has no wiring")?;
    if ctx.json {
        let mut j = serde_json::to_value(&v)?;
        j["config"] = serde_json::Value::String(snap.label);
        return print_json(&j);
    }
    let c = &v.block;
    let cp = c.page.as_deref();
    let mut ctxs = vec![c.typ.clone()];
    if let Some(d) = &c.device {
        ctxs.push(d.clone());
    } else if let Some(p) = &c.page {
        ctxs.push(format!("page {}", p));
    }
    // pages are often named after their room: say it once
    if let Some(r) = c.room.as_ref().filter(|r| c.page.as_ref() != Some(*r)) {
        ctxs.push(format!("room {}", r));
    }
    println!("{}  ({})", c.title, ctxs.join(" · "));
    if !ctx.quiet {
        println!("{} · {}", c.uuid, snap.label);
    }
    if !v.params.is_empty() {
        println!(
            "\n{}",
            if all_params {
                "PARAMETERS"
            } else {
                "PARAMETERS (non-default)"
            }
        );
        let kw = v.params.iter().map(|p| p.key.len()).max().unwrap_or(0);
        for p in &v.params {
            println!(
                "  {:<kw$}  {}{}",
                p.key,
                p.text,
                if p.default { "  (default)" } else { "" },
                kw = kw
            );
        }
    }
    match (&v.upstream, &v.downstream) {
        (None, None) => {
            print_wires("INPUTS", "←", &v.inputs, cp);
            print_wires("OUTPUTS", "→", &v.outputs, cp);
            if v.inputs.is_empty() && v.outputs.is_empty() {
                println!("\nNothing is wired to this block.");
            }
        }
        (up, down) => {
            if let Some(u) = up {
                print_trace("UPSTREAM (to the sensors)", "←", u, cp);
            }
            if let Some(d) = down {
                print_trace("DOWNSTREAM (to the actuators)", "→", d, cp);
            }
        }
    }
    Ok(())
}

// ── find ─────────────────────────────────────────────────────────────────────

pub fn cmd_find(ctx: &RunContext, query: Vec<String>, src: ConfigSource) -> Result<()> {
    let (snap, l) = load(&src)?;
    let q = query.join(" ");
    let hits: Vec<BlockRef> = l
        .search(&q)
        .iter()
        .map(|b| l.block_ref(&b.uuid, None))
        .collect();
    if ctx.json {
        return print_json(&serde_json::json!({"config": snap.label, "blocks": hits}));
    }
    let nw = hits
        .iter()
        .map(|b| b.title.chars().count())
        .max()
        .unwrap_or(4)
        .clamp(4, 40);
    let tw = hits
        .iter()
        .map(|b| b.typ.len())
        .max()
        .unwrap_or(4)
        .clamp(4, 24);
    if !ctx.no_header && !hits.is_empty() {
        println!(
            "{:<nw$}  {:<tw$}  {:<36}  UUID",
            "NAME",
            "TYPE",
            "WHERE",
            nw = nw,
            tw = tw
        );
        println!("{}", "─".repeat(nw + tw + 36 + 42));
    }
    for b in &hits {
        println!(
            "{:<nw$}  {:<tw$}  {:<36}  {}",
            b.title,
            b.typ,
            trunc(&place(b, None), 36),
            b.uuid,
            nw = nw,
            tw = tw
        );
    }
    println!(
        "\n{} {} ({})",
        hits.len(),
        if hits.len() == 1 { "block" } else { "blocks" },
        snap.label
    );
    Ok(())
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
    t.push('…');
    t
}

// ── lint ─────────────────────────────────────────────────────────────────────

pub fn cmd_lint(ctx: &RunContext, src: ConfigSource) -> Result<()> {
    let (snap, l) = load(&src)?;
    let findings = l.lint();
    if ctx.json {
        let v: Vec<_> = findings
            .iter()
            .map(|f| {
                serde_json::json!({
                    "kind": f.kind,
                    "severity": f.severity,
                    "block": l.block_ref(&f.block, None),
                    "message": f.message,
                })
            })
            .collect();
        return print_json(&serde_json::json!({"config": snap.label, "findings": v}));
    }
    // warnings first, then kinds in order of appearance
    let mut kinds: Vec<(&str, &str)> = Vec::new();
    for f in &findings {
        if !kinds.iter().any(|(k, _)| *k == f.kind) {
            kinds.push((f.kind, f.severity));
        }
    }
    kinds.sort_by_key(|(_, sev)| *sev != "warn");
    let kinds: Vec<&str> = kinds.into_iter().map(|(k, _)| k).collect();
    for k in &kinds {
        let fs: Vec<_> = findings.iter().filter(|f| f.kind == *k).collect();
        println!("{} ({}, {})", k, fs[0].severity, fs.len());
        for f in fs {
            let b = l.block_ref(&f.block, None);
            let at = place(&b, None);
            println!(
                "  {}{} — {}",
                b.title,
                if at.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", at)
                },
                f.message
            );
        }
        println!();
    }
    let warn = findings.iter().filter(|f| f.severity == "warn").count();
    println!(
        "{} findings · {} warnings ({})",
        findings.len(),
        warn,
        snap.label
    );
    Ok(())
}

// ── history ──────────────────────────────────────────────────────────────────

pub fn cmd_history(
    ctx: &RunContext,
    name: String,
    room: Option<String>,
    count: usize,
) -> Result<()> {
    let cfg = Config::load()?;
    let h = snapshot::block_history(&cfg, &name, room.as_deref(), count)?;
    if ctx.json {
        let v: Vec<_> = h
            .entries
            .iter()
            .map(|(c, lines)| {
                serde_json::json!({
                    "commit": c.hash,
                    "date": c.date,
                    "saved": c.saved(),
                    "subject": c.subject,
                    "changes": lines,
                })
            })
            .collect();
        return print_json(&serde_json::json!({"block": h.uuid, "title": h.title, "history": v}));
    }
    println!("{}  ({})\n", h.title, h.uuid);
    print!("{}", h.text());
    Ok(())
}
