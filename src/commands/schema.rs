//! `lox schema`: the command tree as JSON, for AI agent discovery.

use anyhow::{Result, bail};
use clap::CommandFactory;
use serde_json::Value;

use crate::Cli;
use crate::commands::RunContext;

pub fn cmd_schema(_ctx: &RunContext, command: Option<String>) -> Result<()> {
    let schema = build_schema(command.as_deref())?;
    println!("{}", serde_json::to_string_pretty(&schema)?);
    Ok(())
}

fn build_schema(filter: Option<&str>) -> Result<Value> {
    let cmd = Cli::command();

    if let Some(name) = filter {
        let sub = cmd.get_subcommands().find(|s| {
            s.get_name().eq_ignore_ascii_case(name)
                || s.get_all_aliases().any(|a| a.eq_ignore_ascii_case(name))
        });
        let Some(sub) = sub else {
            bail!(
                "Unknown command '{}'. Run `lox schema` for all commands.",
                name
            );
        };
        return Ok(describe_command(sub, true));
    }

    let commands: Vec<Value> = cmd
        .get_subcommands()
        .filter(|s| !s.is_hide_set())
        .map(|s| describe_command(s, false))
        .collect();

    Ok(serde_json::json!({
        "name": "lox",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Loxone Miniserver CLI",
        "global_flags": [
            {"name": "--output", "short": "-o", "type": "enum", "values": ["table", "json", "csv"], "default": "table"},
            {"name": "--quiet", "short": "-q", "type": "bool", "description": "Suppress non-essential output"},
            {"name": "--dry-run", "type": "bool", "description": "Validate and resolve inputs without executing commands"},
            {"name": "--non-interactive", "type": "bool", "description": "Fail instead of prompting for confirmation"},
            {"name": "--trace-id", "type": "string", "description": "Trace ID for correlating agent actions"},
            {"name": "--verbose", "short": "-v", "type": "count", "description": "Verbose output (-v requests, -vv bodies)"},
        ],
        "commands": commands,
    }))
}

fn describe_command(cmd: &clap::Command, detailed: bool) -> Value {
    let mut obj = serde_json::json!({
        "name": cmd.get_name(),
        "description": cmd.get_about().map(|s| s.to_string()).unwrap_or_default(),
    });

    // Collect positional and flag arguments
    let args: Vec<Value> = cmd
        .get_arguments()
        .filter(|a| a.get_id() != "help" && a.get_id() != "version")
        .map(|a| {
            let mut arg = serde_json::json!({
                "name": a.get_id().to_string(),
            });
            if let Some(help) = a.get_help() {
                arg["description"] = Value::String(help.to_string());
            }
            if let Some(short) = a.get_short() {
                arg["short"] = Value::String(format!("-{}", short));
            }
            if a.get_long().is_some() {
                arg["long"] = Value::String(format!("--{}", a.get_id()));
            }
            if a.get_num_args().is_some_and(|r| r.max_values() == 0) {
                arg["type"] = Value::String("bool".to_string());
            }
            if let Some(vals) = a.get_default_values().first() {
                arg["default"] = Value::String(vals.to_string_lossy().to_string());
            }
            arg
        })
        .collect();

    if !args.is_empty() {
        obj["args"] = Value::Array(args);
    }

    // Subcommands
    let subs: Vec<Value> = cmd
        .get_subcommands()
        .filter(|s| !s.is_hide_set())
        .map(|s| describe_command(s, detailed))
        .collect();

    if !subs.is_empty() {
        obj["subcommands"] = Value::Array(subs);
    }

    if detailed {
        // Include control type hints for common commands
        let type_hints = match cmd.get_name() {
            "blind" => Some("Jalousie, CentralJalousie"),
            "gate" => Some("Gate, CentralGate"),
            "alarm" => Some("Alarm"),
            "thermostat" => Some("IRoomControllerV2, IRoomController"),
            "door" => Some("DoorLock"),
            "intercom" => Some("Intercom"),
            "charger" => Some("Charger, EV"),
            "light" => Some("LightControllerV2, LightController, ColorPickerV2"),
            _ => None,
        };
        if let Some(types) = type_hints {
            obj["control_types"] = Value::String(types.to_string());
        }

        // Include valid actions
        let actions = match cmd.get_name() {
            "blind" => Some(vec![
                "up",
                "down",
                "full-up",
                "full-down",
                "stop",
                "shade",
                "shade <0-100>",
                "pos <0-100>",
            ]),
            "gate" => Some(vec!["open", "close", "stop"]),
            "alarm" => Some(vec!["arm", "arm-home", "disarm", "quit"]),
            "door" => Some(vec!["lock", "unlock", "open"]),
            "intercom" => Some(vec!["answer", "hangup", "open"]),
            "charger" => Some(vec!["start", "stop", "pause"]),
            _ => None,
        };
        if let Some(acts) = actions {
            obj["valid_actions"] = Value::Array(
                acts.into_iter()
                    .map(|a| Value::String(a.to_string()))
                    .collect(),
            );
        }
    }

    obj
}
