---
title: AI Agent Integration
layout: default
parent: Guides
nav_order: 3
---

# AI Agent Integration
{: .no_toc }

`lox` was designed from the ground up for AI agent integration.
{: .fs-5 .fw-300 }

## Table of contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Design principles

Every command follows these conventions for reliable agent interaction:

- **Exit codes**: 0 on success, non-zero on error
- **Structured output**: `-o json` for machine-readable output on every command
- **Fuzzy matching**: agents don't need UUIDs — natural names work
- **Disambiguation**: `--room` flag and `[Room]` bracket syntax resolve ambiguous names
- **Error messages**: readable errors with suggestions for correction
- **Dry-run**: `--dry-run` validates without executing
- **Tracing**: `--trace-id` correlates agent actions across logs
- **Non-interactive**: `--non-interactive` fails instead of prompting (implied by `-o json`)
- **Schema discovery**: `lox schema` lets agents discover commands programmatically

## Tool definition

Give your LLM a shell tool:

```json
{
  "name": "lox",
  "description": "Control Loxone smart home. Use -o json for structured output.",
  "parameters": {
    "command": {
      "type": "string",
      "description": "e.g. 'on Wohnzimmer', 'blind Südseite pos 50', 'sensors --type door-window'"
    }
  }
}
```

The agent calls `lox <command>` as a shell tool and reads stdout.

## Agent workflow

```bash
# 1. Discover available commands
lox schema -o json

# 2. Discover controls in the home
lox ls -o json

# 3. Preview before executing
lox --dry-run on "Licht" -o json

# 4. Execute with tracing
lox --trace-id "run-42" on "Licht"

# 5. Check device health
lox health --problems -o json
```

## Schema discovery

Agents can introspect what commands exist and what parameters they accept:

```bash
lox schema                    # list all commands
lox schema blind              # schema for a specific command
lox schema -o json            # JSON for programmatic use
```

## Error handling

When using `-o json`, errors return structured envelopes:

```json
{
  "ok": false,
  "error": "control_not_found",
  "message": "No control matching 'Nonexistent'"
}
```

Error codes:

| Code | Description |
|:-----|:------------|
| `control_not_found` | No control matches the name |
| `ambiguous_control` | Multiple controls match — use `--room` to disambiguate |
| `config_not_found` | Missing config file |
| `user_changes_pending` | `lox config push --force` refused: users changed live, loading the upload would delete that |
| `confirmation_required` | Command needs `--yes` (reboot, firmware update; user and access changes without a terminal) |
| `declined_by_user` | The confirmation was answered with no |
| `user_not_found` | No user matches the name |
| `tag_not_found` | The user has no NFC tag with that name or ID |
| `code_in_use` | Another user has this access code already |
| `learn_timeout` | No NFC tag was held to the Code Touch in time |
| `unauthorized` | Invalid credentials |
| `forbidden` | Insufficient permissions |
| `not_found` | Resource not found |
| `http_error` | HTTP request failed |
| `connection_error` | Cannot reach Miniserver |
| `error` | Generic error |

## Explaining behavior

To answer *why is this light on?*, an agent can read the config logic instead of guessing:

```bash
lox config wiring "Licht Gang" --trace up -o json    # inputs, sensors, parameters upstream
lox config find bewegung -o json                     # blocks by name, type, device, room
lox config diff -o json                              # the last config change
```

## Writing logic

Agents can also author Miniserver logic as [Lox Logic](config-as-code) text: `lox logic check --json` returns
line-numbered errors, `lox config diff base.Loxone out.Loxone` shows the result by name, and only
`lox config push --force` (a step for the human) touches the Miniserver. Rules for agents:
[crates/loxone-config/docs/agents.md](https://github.com/discostu105/lox/blob/main/crates/loxone-config/docs/agents.md).

## Conditional logic

Use `lox if` for state-based decisions:

```bash
# Returns exit code 0 (true) or 1 (false)
lox if "Temperatur" gt 25 && lox blind "Beschattung" pos 70
lox if "Schalter" eq 1 && lox on "Licht"
```

## Real-time streaming

For continuous monitoring, use WebSocket streaming:

```bash
lox stream -o json --reconnect           # NDJSON stream of all changes, across reboots
lox stream --room "Kitchen" -o json      # filtered by room
lox stream --type LightControllerV2      # filtered by type
```

## MCP server

Clients that speak the [Model Context Protocol](https://modelcontextprotocol.io) but cannot run
shell commands (Claude Desktop, ChatGPT connectors, Cursor, VS Code agents, n8n) can use
`lox mcp serve`. The client launches it and talks to it over stdio. No extra runtime is needed.

The server is built on the official Rust MCP SDK and speaks the newest revision, **MCP 2026-07-28**
(stateless `server/discover` lifecycle), as well as every earlier revision back to 2024-11-05, so
current and older clients both work.

### Connect a client

```bash
lox mcp config            # prints the JSON snippet and the Claude Code command
```

Claude Desktop, Cursor and other JSON-configured clients take the `mcpServers` snippet:

```json
{
  "mcpServers": {
    "loxone": { "command": "/usr/local/bin/lox", "args": ["mcp", "serve"] }
  }
}
```

Claude Code:

```bash
claude mcp add loxone -- /usr/local/bin/lox mcp serve
```

The server uses your normal `lox` configuration. Add `--ctx home` before `mcp` to pin it to one
Miniserver context.

### Tools

| Tool | Kind | What it does |
|:-----|:-----|:-------------|
| `list_rooms` | read | Rooms with control counts |
| `list_controls` | read | Controls filtered by name, room, type, category; each names the tool that operates it |
| `get_control` | read | Live value, state attributes and named outputs of one control |
| `list_sensors` | read | Temperature, door/window, motion, smoke or energy readings |
| `get_energy_flow` | read | PV production, consumption, grid import/export, battery; optionally the top meters by power |
| `list_light_moods` | read | Mood IDs of a lighting controller |
| `list_scenes` | read | Your `lox` scenes |
| `system_status` | read | Firmware, PLC state, memory |
| `get_wiring` | read | The Loxone Config logic around a control: what drives it, what it drives, parameters; `trace` up to the sensors or down to the actuators |
| `list_users` | read | Users with state and admin flag (needs a connection user that may manage users) |
| `get_user` | read | One user: validity, groups, NFC tags, number of access codes |
| `access_log` | read | Access log of an NFC Code Touch: who opened when, denied attempts |
| `access_check` | read | Why a user may or may not open an NFC Code Touch: verdict, rights, warnings |
| `hw_status` | read | Devices on the bus against the program file: missing, offline, replaced, not in the program |
| `logic_check`, `logic_lint`, `logic_coverage`, `logic_test` | read | Check, lint, coverage and tests of a Lox Logic project |
| `logic_types`, `logic_decompile` | read | The block catalogue; a config as Lox Logic |
| `logic_diff`, `logic_drift` | read | What changed between two configs; whether the live config drifted from the lock |
| `logic_compile` | read | Compile a project into a scratch directory and show what changes. The project stays as it is |
| `config_sync`, `config_push_dry_run` | read | Whether project and Miniserver agree; a push staged, never uploaded |
| `switch` | action | `on`, `off`, `pulse` |
| `blind` | action | `up`, `down`, `stop`, `shade`, `position`, `slats` |
| `light` | action | `mood`, `dim`, `color` |
| `thermostat` | action | `temp`, `mode`, `override` |
| `run_scene` | action | Run a `lox` scene, with progress per step; stops when cancelled |
| `gate` | risky | `open`, `close` (confirmed), `stop` |
| `alarm` | risky | `arm`, `arm-home`, `disarm` (confirmed), `quit` |
| `door` | risky | `lock`, `unlock`, `open` (all confirmed) |
| `send_command` | risky | Raw Loxone command, only with `--allow-raw` |
| `learn_nfc_tag` | risky | Read an NFC tag's ID at a Code Touch (confirmed). Tell the user to have the tag or unlocked phone ready. A secured Code Touch needs `LOX_VISU_PASSWORD` or `visu_password_file` in the config of the `lox mcp` process |
| `assign_nfc_tag` | risky | Pair an NFC tag with a user (confirmed) |
| `remove_nfc_tag` | risky | Remove a user's NFC tag (confirmed) |
| `set_access_code` | risky | Set a user's access code (confirmed). The code is never echoed |

Controls are addressed exactly like on the command line: a name substring, `Name [Room]`, an alias
or a UUID, with an optional `room` argument. Every tool publishes input and output schemas. Every
action tool accepts `dry_run: true` and returns the equivalent `lox` command line in `cli`.

### Safety

High-risk actions (doors, opening or closing gates, arming or disarming the alarm) are confirmed by
**you**, not by the model. When the AI asks to open the front door, your MCP client shows a
confirmation such as:

```
Door lock — Haustür (Wohnzimmer): open?
Equivalent command: lox door "Haustür" open -r Wohnzimmer
```

Nothing is sent unless you confirm. A declined confirmation is reported to the model as
`declined_by_user`. The same applies to `switch` or raw commands aimed at a door lock, gate or alarm,
and to push-buttons that open a door (recognized by their door/gate/lock/key icon).

To have other controls confirmed too, list them in your config (`~/.lox/config.yaml`, or the context
entry) by UUID, alias, or name with an optional `[Room]`. Every action on them is then confirmed:

```yaml
confirm:
  - Pool Abdeckung [Pool]   # both cover buttons
```

| Flag | Effect |
|:-----|:-------|
| *(none)* | Read tools and everyday actions. Risky actions need your confirmation. Clients that cannot ask are refused (`action_not_allowed`). |
| `--read-only` | Only read tools are listed and callable. |
| `--allow-risky` | Risky actions (including the `confirm:` list) run without asking. |
| `--allow-raw` | Also `send_command` for raw Loxone commands. |

The policy is fixed when the server starts, and the model cannot change it. Risky tools carry
`destructiveHint`. Credentials never pass through tool arguments, and alarm PINs are never echoed back.
