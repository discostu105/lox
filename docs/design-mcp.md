# Design: `lox mcp` — Model Context Protocol server

> **Status: IMPLEMENTED (v1)** — stdio transport, 15 tools, safety tiers. Tracking issue: #107.
> Out of scope for v1: HTTP transport, MCP resources/prompts, change notifications (§9).

## 1. Why

`lox` already works as a shell tool for agents that can run commands (Claude Code, Codex, Gemini CLI).
Many AI clients cannot run shell commands and only speak MCP: Claude Desktop, ChatGPT connectors,
Cursor, n8n, VS Code agents. Vendors are moving there too; Nanoleaf ships an official MCP server for
its lights. Community Loxone MCP servers exist (Rust, Kotlin, TypeScript, Python), but each
re-implements Miniserver access from scratch.

`lox` has the hard parts already: fuzzy name resolution with room qualifiers, aliases, multi-context
config, token/structure caching, and a shared action layer (`src/actions.rs`) with per-action risk
levels and type checks. An MCP server is a thin protocol adapter on top.

Goals:

1. `lox mcp serve` turns the installed binary into an MCP server. No extra runtime, no second tool.
2. Agents get **few, well-described tools** that cover discovery and everyday control.
3. **Safe by default**: doors, gates and the alarm need an explicit opt-in when the server starts.
4. Behavior matches the CLI and the TUI exactly, because all three go through `actions.rs`.

## 2. Transport and protocol

- **stdio**, newline-delimited JSON-RPC 2.0 (the MCP stdio transport). stdout carries protocol
  messages only; every diagnostic goes to stderr.
- Hand-rolled, not the `rmcp` crate. The server needs `initialize`, `ping`, `tools/list` and
  `tools/call`; that is ~200 lines over `serde_json` and adds no dependencies, keeping the single
  binary small and the MSRV unchanged.
- Protocol versions: the server answers with the client's requested version when it knows it
  (`2025-11-25`, `2025-06-18`, `2025-03-26`, `2024-11-05`), otherwise with its newest.
- Capabilities: `tools` only (`listChanged: false`). The `initialize` result carries
  `instructions` that tell the model how to discover and address controls.
- Requests are handled one at a time. Notifications (`notifications/initialized`,
  `notifications/cancelled`, …) are accepted and ignored. JSON-RPC batches are accepted for older
  clients. Unknown methods return `-32601`, malformed JSON `-32700`, unknown tools `-32602`.

## 3. Commands

```
lox mcp serve  [--read-only] [--allow-risky] [--allow-raw]   # run the server on stdio
lox mcp config [--read-only] [--allow-risky] [--allow-raw]   # print client config snippets
lox mcp tools  [--read-only] [--allow-risky] [--allow-raw]   # list the tools that would be exposed
```

Global flags keep their meaning: `--ctx home` pins the server to one Miniserver context,
`--dry-run` makes every action tool a dry run, `-v` logs HTTP requests to stderr.
`lox mcp config` embeds the absolute path of the running binary and any `--ctx`, so the snippet
can be pasted as is.

## 4. Tools

Tools are curated around user intent instead of mirroring the 52 CLI commands. Tool names are
`snake_case` verbs or device nouns; arguments are always named.

### 4.1 Read tools (`readOnlyHint: true`)

| Tool | Arguments | Returns |
|------|-----------|---------|
| `list_rooms` | — | rooms with control counts |
| `list_controls` | `name?`, `room?`, `type?`, `category?`, `favorites_only?` | name, type, room, category, uuid, and the tool that operates it |
| `get_control` | `name`, `room?` | live value, state attributes and named outputs, plus metadata |
| `list_sensors` | `kind?` (all, temperature, door-window, motion, smoke, energy), `room?` | sensor readings |
| `list_light_moods` | `name`, `room?` | mood IDs and names of a lighting controller (WebSocket) |
| `list_scenes` | — | local `lox` scenes with description and step count |
| `system_status` | — | firmware, PLC state, heap, Miniserver clock |

### 4.2 Action tools

All action tools take `name`, optional `room`, and optional `dry_run`. They resolve the control,
check its type, build an `actions::Action`, and send its commands.

| Tool | Actions | Risk |
|------|---------|------|
| `switch` | `on`, `off`, `pulse` | none |
| `blind` | `up`, `down`, `stop`, `shade`, `position` (+ `value` 0–100), `slats` (+ `value`) | none |
| `light` | `mood` (`plus`/`minus`/`off`/ID), `dim` (0–100), `color` (`#RRGGBB`, `hsv(h,s,v)`, `temp(b,k)`) | none |
| `thermostat` | `temp`, `mode`, `override` (+ `minutes`) | none |
| `gate` | `open`, `close`, `stop` | open/close: **risky** |
| `alarm` | `arm`, `arm-home`, `disarm`, `quit` (+ `no_motion`, `code`) | all but quit: **risky** |
| `door` | `lock`, `unlock`, `open` | **risky** |
| `run_scene` | scene name | none (user-authored) |
| `send_command` | raw Loxone command | only listed with `--allow-raw` |

Results are JSON objects: the resolved control, the action as text, the commands sent, the
Miniserver response code and value, and the equivalent `lox` command line (`cli`). The same object
is returned as `structuredContent` and as pretty-printed text content.

## 5. Safety model

Three tiers, fixed when the server starts. The model cannot change them.

| Mode | Read tools | Everyday actions | Risky actions | Raw commands |
|------|:---:|:---:|:---:|:---:|
| `--read-only` | ✓ | — (not listed) | — | — |
| default | ✓ | ✓ | refused | — |
| `--allow-risky` | ✓ | ✓ | ✓ | — |
| `--allow-raw` | ✓ | ✓ | per `--allow-risky` | ✓ |

- "Risky" is exactly `Action::risk() == Risk::Confirm`, the same list the TUI asks to confirm.
  Refused calls return a tool error with code `action_not_allowed`, naming the flag that enables them,
  so the model can explain it to the user instead of retrying.
- Tool annotations mark `gate`, `alarm`, `door` and `send_command` as `destructiveHint: true`, so
  clients that ask before destructive calls will ask.
- Every tool accepts `dry_run: true`; the global `--dry-run` forces it on.
- Credentials never leave the server: they come from the `lox` config, never from tool arguments.
  Alarm PINs are passed through to the Miniserver and never echoed back.

## 6. Errors

Tool failures are returned as results with `isError: true`, not as JSON-RPC errors, so the model sees
them and can correct itself. The payload reuses the CLI error envelope:

```json
{ "ok": false, "error": "ambiguous_control", "message": "Ambiguous: 'Licht'. Use [Room] qualifier or --room flag. …" }
```

Error codes match `lox -o json` (`control_not_found`, `ambiguous_control`, `config_not_found`,
`unauthorized`, `connection_error`, …) plus `invalid_arguments` and `action_not_allowed`.

## 7. State and lifetime

- Config is loaded lazily on the first tool call, so a client can list tools even before `lox setup`
  has run; the tool call then reports `config_not_found` with the fix.
- The `LoxClient` (and its in-memory structure) is rebuilt every 15 minutes. That picks up
  `lox cache refresh`, the 24 h structure TTL and changed credentials in long-lived client sessions.
- EOF on stdin ends the server with exit code 0.

## 8. Testing

- Unit tests drive `McpServer::handle_line` against an `httpmock` Miniserver: handshake and version
  negotiation, tool listing per mode, each read tool, action dry runs and real sends, risk gating,
  argument validation, error envelopes, notifications and batches.
- A CLI smoke test pipes `initialize` + `tools/list` into the real binary.

## 9. Later

- Streamable HTTP transport (`lox mcp serve --http :8765` with a bearer token) for remote connectors.
- MCP resources (`loxone://rooms`, `loxone://controls/{uuid}`) and `resources/subscribe` backed by
  `lox stream`.
- More device tools as they come up: intercom, EV charger, music zones, statistics/history.
