//! `lox mcp` — Model Context Protocol server over stdio (issue #107).
//!
//! Newline-delimited JSON-RPC 2.0 on stdin/stdout. stdout carries protocol
//! messages only; all diagnostics go to stderr. Design: `docs/design-mcp.md`.

mod tools;

use anyhow::Result;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use crate::client::LoxClient;
use crate::commands::RunContext;
use crate::config::Config;

pub use tools::tool_defs;

/// Protocol versions this server understands, newest first.
const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// How long a `LoxClient` (and its in-memory structure) is reused before it is
/// rebuilt from config — picks up `lox cache refresh` and changed credentials.
const CLIENT_TTL: Duration = Duration::from_secs(15 * 60);

const INSTRUCTIONS: &str = "\
Controls a Loxone smart home through its Miniserver.
Discover first: list_rooms, then list_controls (filter by room, type or name). \
Each control in list_controls names the tool that operates it.
Address controls by name: a case-insensitive substring match. If a name is ambiguous, \
pass `room` or write it as 'Name [Room]'. UUIDs work too.
Read live values with get_control or list_sensors. Every action tool accepts dry_run=true \
to preview the exact commands without sending them.
Doors, gates and the alarm are refused unless the user started the server with --allow-risky; \
tell the user when that happens instead of retrying.";

/// Server policy, fixed at startup. The model cannot change it.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServerOptions {
    /// Only read tools are listed and callable.
    pub read_only: bool,
    /// Allow actions with `Risk::Confirm` (doors, gates, alarm).
    pub allow_risky: bool,
    /// Expose `send_command` for raw Loxone commands.
    pub allow_raw: bool,
    /// Force every action to be a dry run (global `--dry-run`).
    pub dry_run: bool,
}

pub struct McpServer {
    opts: ServerOptions,
    /// Fixed config (tests); `None` means `Config::load()` on demand.
    cfg: Option<Config>,
    lox: Option<(LoxClient, Instant)>,
}

// JSON-RPC error codes
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

impl McpServer {
    pub fn new(opts: ServerOptions) -> Self {
        Self {
            opts,
            cfg: None,
            lox: None,
        }
    }

    #[cfg(test)]
    pub fn with_config(opts: ServerOptions, cfg: Config) -> Self {
        Self {
            opts,
            cfg: Some(cfg),
            lox: None,
        }
    }

    /// The Miniserver client, created lazily and rebuilt after `CLIENT_TTL`.
    fn client(&mut self) -> Result<&mut LoxClient> {
        let stale = self
            .lox
            .as_ref()
            .is_none_or(|(_, at)| at.elapsed() > CLIENT_TTL);
        if stale {
            let cfg = match &self.cfg {
                Some(c) => c.clone(),
                None => Config::load()?,
            };
            self.lox = Some((LoxClient::new(cfg)?, Instant::now()));
        }
        Ok(&mut self.lox.as_mut().expect("client just created").0)
    }

    /// Handle one line of input. Returns the line to write back, if any.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(
                    error_response(&Value::Null, PARSE_ERROR, &format!("Parse error: {}", e))
                        .to_string(),
                );
            }
        };
        let out = match msg {
            // JSON-RPC batch (allowed by protocol 2025-03-26)
            Value::Array(items) => {
                if items.is_empty() {
                    Some(error_response(&Value::Null, INVALID_REQUEST, "Empty batch"))
                } else {
                    let replies: Vec<Value> = items
                        .iter()
                        .filter_map(|m| self.handle_message(m))
                        .collect();
                    (!replies.is_empty()).then_some(Value::Array(replies))
                }
            }
            other => self.handle_message(&other),
        };
        out.map(|v| v.to_string())
    }

    /// Handle one JSON-RPC message. Notifications and responses get no reply.
    fn handle_message(&mut self, msg: &Value) -> Option<Value> {
        let Some(obj) = msg.as_object() else {
            return Some(error_response(
                &Value::Null,
                INVALID_REQUEST,
                "Request must be an object",
            ));
        };
        let Some(method) = obj.get("method").and_then(|m| m.as_str()) else {
            // A response from the client (we never send requests) or garbage.
            return if obj.contains_key("id")
                && !obj.contains_key("result")
                && !obj.contains_key("error")
            {
                Some(error_response(
                    obj.get("id").unwrap_or(&Value::Null),
                    INVALID_REQUEST,
                    "Missing method",
                ))
            } else {
                None
            };
        };
        // No id → notification: act on it (nothing to do) and stay silent.
        let id = obj.get("id")?;
        let params = obj.get("params").cloned().unwrap_or(Value::Null);

        let result = match method {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_defs(&self.opts) })),
            "tools/call" => self.tools_call(&params),
            _ => Err((METHOD_NOT_FOUND, format!("Method not found: {}", method))),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, message)) => error_response(id, code, &message),
        })
    }

    fn initialize(&self, params: &Value) -> Value {
        let requested = params
            .get("protocolVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let version = PROTOCOL_VERSIONS
            .iter()
            .find(|v| **v == requested)
            .unwrap_or(&PROTOCOL_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": {
                "name": "lox",
                "title": "Loxone (lox)",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": INSTRUCTIONS,
        })
    }

    fn tools_call(&mut self, params: &Value) -> std::result::Result<Value, (i64, String)> {
        let name = params.get("name").and_then(|n| n.as_str()).ok_or((
            INVALID_PARAMS,
            "tools/call requires a tool name".to_string(),
        ))?;
        if !tool_defs(&self.opts)
            .iter()
            .any(|t| t["name"].as_str() == Some(name))
        {
            return Err((INVALID_PARAMS, format!("Unknown tool: {}", name)));
        }
        let args = match params.get("arguments") {
            None | Some(Value::Null) => json!({}),
            Some(a @ Value::Object(_)) => a.clone(),
            Some(_) => {
                return Err((INVALID_PARAMS, "arguments must be an object".to_string()));
            }
        };
        let opts = self.opts;
        Ok(match tools::call(self, &opts, name, &args) {
            Ok(v) => tool_result(v, false),
            Err(e) => tool_result(tools::error_envelope(&e), true),
        })
    }
}

fn error_response(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

/// Wrap a JSON object as an MCP tool result (text + structured content).
fn tool_result(v: Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string());
    let mut r = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    });
    if v.is_object() {
        r["structuredContent"] = v;
    }
    r
}

/// Run the server until stdin closes.
pub fn serve_stdio(opts: ServerOptions) -> Result<()> {
    eprintln!(
        "lox mcp {}: serving on stdio (mode: {}{}{})",
        env!("CARGO_PKG_VERSION"),
        if opts.read_only {
            "read-only"
        } else if opts.allow_risky {
            "allow-risky"
        } else {
            "default"
        },
        if opts.allow_raw { ", allow-raw" } else { "" },
        if opts.dry_run { ", dry-run" } else { "" },
    );
    let mut server = McpServer::new(opts);
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle_line(&line) {
            writeln!(stdout, "{}", reply)?;
            stdout.flush()?;
        }
    }
    Ok(())
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
            let defs = tool_defs(&policy.options(false));
            if ctx.json {
                println!("{}", serde_json::to_string_pretty(&defs)?);
            } else {
                if !ctx.no_header {
                    println!("{:<18} {:<8} DESCRIPTION", "TOOL", "KIND");
                    println!("{}", "─".repeat(90));
                }
                for t in &defs {
                    let ann = &t["annotations"];
                    let kind = if ann["readOnlyHint"].as_bool() == Some(true) {
                        "read"
                    } else if ann["destructiveHint"].as_bool() == Some(true) {
                        "risky"
                    } else {
                        "action"
                    };
                    let desc = t["description"].as_str().unwrap_or("");
                    let first = desc.split(". ").next().unwrap_or(desc);
                    println!(
                        "{:<18} {:<8} {}",
                        t["name"].as_str().unwrap_or("?"),
                        kind,
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
