//! `lox mcp` — Model Context Protocol server (issue #107).
//!
//! Built on rmcp, the official Rust SDK. Speaks MCP 2026-07-28 (stateless
//! `server/discover` lifecycle, multi-round-trip requests, cache hints) and
//! every earlier revision back to 2024-11-05 over stdio. stdout carries protocol
//! messages only; diagnostics go to stderr. Design: `docs/design-mcp.md`.

mod ops;
mod server;

use anyhow::Result;
use rmcp::ServiceExt;
use serde_json::{Value, json};

use crate::commands::RunContext;

pub use server::LoxMcp;

/// Server policy, fixed at startup. The model cannot change it.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServerOptions {
    /// Only read tools are listed and callable.
    pub read_only: bool,
    /// Skip the user confirmation for high-risk actions (doors, gates, alarm).
    pub allow_risky: bool,
    /// Expose `send_command` for raw Loxone commands.
    pub allow_raw: bool,
    /// Force every action to be a dry run (global `--dry-run`).
    pub dry_run: bool,
}

/// Run the server on stdio until the client disconnects.
pub fn serve_stdio(opts: ServerOptions) -> Result<()> {
    eprintln!(
        "lox mcp {}: serving on stdio (mode: {}{}{})",
        env!("CARGO_PKG_VERSION"),
        if opts.read_only {
            "read-only"
        } else if opts.allow_risky {
            "allow-risky"
        } else {
            "confirm-risky"
        },
        if opts.allow_raw { ", allow-raw" } else { "" },
        if opts.dry_run { ", dry-run" } else { "" },
    );
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let service = LoxMcp::new(opts)
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| anyhow::anyhow!("MCP session failed to start: {}", e))?;
        service.waiting().await?;
        anyhow::Ok(())
    })
}

// ── CLI ───────────────────────────────────────────────────────────────────────

/// Command line for an MCP client to launch this server.
fn launch_args(opts: &ServerOptions) -> (String, Vec<String>) {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "lox".to_string());
    let mut args = Vec::new();
    if let Some(ctx) = crate::config::ctx_override() {
        args.push("--ctx".to_string());
        args.push(ctx);
    }
    args.push("mcp".to_string());
    args.push("serve".to_string());
    if opts.read_only {
        args.push("--read-only".to_string());
    }
    if opts.allow_risky {
        args.push("--allow-risky".to_string());
    }
    if opts.allow_raw {
        args.push("--allow-raw".to_string());
    }
    (exe, args)
}

pub fn cmd_mcp(ctx: &RunContext, action: crate::McpCmd) -> Result<()> {
    use crate::McpCmd;
    match action {
        McpCmd::Serve { policy } => serve_stdio(policy.options(ctx.dry_run)),
        McpCmd::Config { policy } => {
            let (exe, args) = launch_args(&policy.options(false));
            let snippet = json!({
                "mcpServers": { "loxone": { "command": exe, "args": args } }
            });
            if ctx.json {
                println!("{}", serde_json::to_string_pretty(&snippet)?);
            } else {
                let shell_args: Vec<String> = args
                    .iter()
                    .map(|a| crate::actions::shell_quote(a))
                    .collect();
                println!(
                    "Claude Desktop, Cursor & other JSON-configured clients\n\
                     (add to the client's MCP config, e.g. claude_desktop_config.json):\n"
                );
                println!("{}\n", serde_json::to_string_pretty(&snippet)?);
                println!("Claude Code:\n");
                println!(
                    "  claude mcp add loxone -- {} {}",
                    crate::actions::shell_quote(&exe),
                    shell_args.join(" ")
                );
            }
            Ok(())
        }
        McpCmd::Tools { policy } => {
            let mut defs: Vec<Value> = LoxMcp::new(policy.options(false))
                .tools()
                .into_iter()
                .map(|t| serde_json::to_value(t).unwrap_or_default())
                .collect();
            let kind = |t: &Value| {
                let ann = &t["annotations"];
                if ann["readOnlyHint"].as_bool() == Some(true) {
                    "read"
                } else if ann["destructiveHint"].as_bool() == Some(true) {
                    "risky"
                } else {
                    "action"
                }
            };
            let rank = |t: &Value| match kind(t) {
                "read" => 0,
                "action" => 1,
                _ => 2,
            };
            defs.sort_by_key(|t| (rank(t), t["name"].as_str().unwrap_or("").to_string()));
            if ctx.json {
                println!("{}", serde_json::to_string_pretty(&defs)?);
            } else {
                if !ctx.no_header {
                    println!("{:<18} {:<8} DESCRIPTION", "TOOL", "KIND");
                    println!("{}", "─".repeat(90));
                }
                for t in &defs {
                    let desc = t["description"].as_str().unwrap_or("");
                    let first = desc.split(". ").next().unwrap_or(desc);
                    println!(
                        "{:<18} {:<8} {}",
                        t["name"].as_str().unwrap_or("?"),
                        kind(t),
                        first.trim_end_matches('.')
                    );
                }
                if !ctx.quiet {
                    println!("\n{} tools", defs.len());
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
