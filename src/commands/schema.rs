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

    let global_flags: Vec<Value> = cmd
        .get_arguments()
        .filter(|a| a.is_global_set())
        .filter_map(describe_global_flag)
        .collect();

    Ok(serde_json::json!({
        "name": "lox",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Loxone Miniserver CLI",
        "global_flags": global_flags,
        "commands": commands,
    }))
}

/// One global flag, read from the CLI definition so the list cannot fall
/// behind it.
fn describe_global_flag(a: &clap::Arg) -> Option<Value> {
    let mut flag = serde_json::json!({ "name": format!("--{}", a.get_long()?) });
    if let Some(short) = a.get_short() {
        flag["short"] = Value::String(format!("-{}", short));
    }
    let values: Vec<Value> = a
        .get_possible_values()
        .iter()
        .filter(|v| !v.is_hide_set())
        .map(|v| Value::String(v.get_name().to_string()))
        .collect();
    let kind = if matches!(a.get_action(), clap::ArgAction::Count) {
        "count"
    } else if is_switch(a) {
        "bool"
    } else if !values.is_empty() {
        flag["values"] = Value::Array(values);
        "enum"
    } else {
        "string"
    };
    flag["type"] = Value::String(kind.to_string());
    if let Some(default) = a.get_default_values().first() {
        flag["default"] = Value::String(default.to_string_lossy().to_string());
    }
    if let Some(help) = a.get_help() {
        flag["description"] = Value::String(help.to_string());
    }
    Some(flag)
}

/// A flag that takes no value.
fn is_switch(a: &clap::Arg) -> bool {
    matches!(
        a.get_action(),
        clap::ArgAction::SetTrue | clap::ArgAction::SetFalse
    )
}

fn describe_command(cmd: &clap::Command, detailed: bool) -> Value {
    let mut obj = serde_json::json!({
        "name": cmd.get_name(),
        "description": cmd.get_about().map(|s| s.to_string()).unwrap_or_default(),
    });

    // Collect positional and flag arguments
    let args: Vec<Value> = cmd
        .get_arguments()
        .filter(|a| a.get_id() != "help" && a.get_id() != "version" && !a.is_global_set())
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
            if let Some(long) = a.get_long() {
                arg["long"] = Value::String(format!("--{}", long));
            }
            if is_switch(a) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `long` in the schema is a flag clap accepts for that command.
    fn check_flags(cmd: &clap::Command, described: &Value, path: &str) {
        for arg in described["args"].as_array().into_iter().flatten() {
            let Some(long) = arg["long"].as_str() else {
                continue;
            };
            let known = cmd
                .get_arguments()
                .any(|a| a.get_long().is_some_and(|l| format!("--{l}") == long));
            assert!(known, "{path}: {long} is not a flag of the command");
            assert!(!long.contains('_'), "{path}: {long}");
        }
        for sub in described["subcommands"].as_array().into_iter().flatten() {
            let name = sub["name"].as_str().unwrap();
            let sub_cmd = cmd.find_subcommand(name).unwrap();
            check_flags(sub_cmd, sub, &format!("{path} {name}"));
        }
    }

    #[test]
    fn flags_are_named_as_the_command_line_takes_them() {
        let schema = build_schema(None).unwrap();
        let cli = Cli::command();
        let commands = schema["commands"].as_array().unwrap();
        assert!(commands.len() > 30);
        for described in commands {
            let name = described["name"].as_str().unwrap();
            check_flags(cli.find_subcommand(name).unwrap(), described, name);
        }
        // A flag of several words, which the argument id spells with `_`
        let on = commands.iter().find(|c| c["name"] == "on").unwrap();
        let longs: Vec<_> = on["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| a["long"].as_str())
            .collect();
        assert!(longs.contains(&"--all-in-room"), "{longs:?}");
        // A switch is marked as one; a flag with a value is not.
        let reboot = commands.iter().find(|c| c["name"] == "reboot").unwrap();
        assert_eq!(reboot["args"][0]["long"], "--yes");
        assert_eq!(reboot["args"][0]["type"], "bool");
        let room = on["args"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["long"] == "--room")
            .unwrap();
        assert!(room.get("type").is_none(), "{room}");
    }

    #[test]
    fn global_flags_come_from_the_cli_definition() {
        let schema = build_schema(None).unwrap();
        let flags = schema["global_flags"].as_array().unwrap();
        let names: Vec<_> = flags.iter().filter_map(|f| f["name"].as_str()).collect();
        for expected in [
            "--output",
            "--quiet",
            "--no-color",
            "--no-header",
            "--dry-run",
            "--non-interactive",
            "--trace-id",
            "--verbose",
            "--ctx",
        ] {
            assert!(names.contains(&expected), "{expected} missing in {names:?}");
        }
        let output = flags.iter().find(|f| f["name"] == "--output").unwrap();
        assert_eq!(output["type"], "enum");
        assert_eq!(output["short"], "-o");
        assert_eq!(output["default"], "table");
        assert_eq!(
            output["values"],
            serde_json::json!(["table", "json", "csv"])
        );
        let verbose = flags.iter().find(|f| f["name"] == "--verbose").unwrap();
        assert_eq!(verbose["type"], "count");
        let dry_run = flags.iter().find(|f| f["name"] == "--dry-run").unwrap();
        assert_eq!(dry_run["type"], "bool");
    }

    #[test]
    fn an_unknown_command_is_an_error() {
        assert!(build_schema(Some("nonsense")).is_err());
        assert_eq!(build_schema(Some("ON")).unwrap()["name"], "on");
    }
}
