# Design: `lox mcp` — Model Context Protocol server

> **Status: IMPLEMENTED (v2)** — built on rmcp 3.5 (the official Rust SDK), speaks MCP **2026-07-28** and every
> revision back to 2024-11-05 over stdio. 16 tools with input and output schemas, user confirmation for risky
> actions, progress and cancellation for scenes. Tracking issue: #107.
> Out of scope for now: Streamable HTTP transport, resources and `subscriptions/listen`, the tasks extension (§10).

## 1. Why

`lox` already works as a shell tool for agents that can run commands (Claude Code, Codex, Gemini CLI).
Many AI clients cannot run shell commands and only speak MCP: Claude Desktop, ChatGPT connectors,
Cursor, n8n, VS Code agents. Vendors are moving there too; Nanoleaf ships an official MCP server for
its lights. Community Loxone MCP servers exist (Rust, Kotlin, TypeScript, Python), but each
re-implements Miniserver access from scratch.

`lox` has the hard parts already: fuzzy name resolution with room qualifiers, aliases, multi-context
config, token/structure caching, and a shared action layer (`src/actions.rs`) with per-action risk
levels and type checks. The MCP server is a protocol adapter on top.

Goals:

1. `lox mcp serve` turns the installed binary into an MCP server. No extra runtime, no second tool.
2. **Current protocol**: the newest MCP revision, with full backward compatibility for older clients.
3. Agents get **few, well-described, schema-typed tools** that cover discovery and everyday control.
4. **Safe by default**: doors, gates and the alarm need a human's confirmation, not just the model's intent.
5. Behavior matches the CLI and the TUI exactly, because all three go through `actions.rs`.

## 2. Protocol

The server is built on [rmcp](https://crates.io/crates/rmcp) (default features off; `server`,
`macros`, `transport-io`, `elicitation`). rmcp owns JSON-RPC framing, version negotiation, the
lifecycle, cancellation, and the result shapes of every revision; `lox` owns the tools.

| Revision | Lifecycle | What `lox` uses |
|----------|-----------|-----------------|
| **2026-07-28** | stateless: `server/discover`, per-request `_meta` (SEP-2575) | multi-round-trip confirmation (SEP-2322), cache hints on `tools/list` (SEP-2549), deterministic tool order |
| 2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05 | `initialize` handshake | server-to-client elicitation for confirmation |

- Newer clients probe `server/discover`; older ones send `initialize`. rmcp answers both on the same
  stdio stream, so one binary serves Claude Desktop today and 2026-07-28 clients as they ship.
- Transport: stdio. stdout carries protocol messages only; diagnostics go to stderr.
- `tools/list` is fixed for the server's lifetime, so 2026-07-28 clients get `ttlMs` = 1 h and
  `cacheScope: public`. Older clients get the legacy result shape without the hints.
- `serverInfo` carries a title, description, website and an SVG icon (2025-11-25 icons).
- Miniserver access is blocking (`reqwest::blocking`); every tool runs it on tokio's blocking pool,
  so a slow Miniserver never stalls the protocol loop, pings or cancellations.

### Cost of the SDK

rmcp adds 11 crates (rmcp, rmcp-macros, schemars and their helpers) and about 2.8 MB to the stripped
release binary (13.9 → 16.7 MB). In exchange the server tracks each spec revision through the SDK
instead of by hand: 2026-07-28 replaced the handshake, server-to-client requests, sessions, ping and
several error codes at once.

## 3. Commands

```
lox mcp serve  [--read-only | --allow-risky] [--allow-raw]   # run the server on stdio
lox mcp config [--read-only | --allow-risky] [--allow-raw]   # print client config snippets
lox mcp tools  [--read-only | --allow-risky] [--allow-raw]   # list the tools that would be exposed
```

Global flags keep their meaning: `--ctx home` pins the server to one Miniserver context,
`--dry-run` makes every action tool a dry run, `-v` logs HTTP requests to stderr.
`lox mcp config` embeds the absolute path of the running binary and any `--ctx`, so the snippet
can be pasted as is.

## 4. Tools

Tools are curated around user intent instead of mirroring the 52 CLI commands. They are defined with
rmcp's `#[tool]` macros over typed parameter structs; the input schema is generated from the Rust
types (flat, inlined enums, no `$ref`, so every client and model can read it). Each tool also has
a generated **`outputSchema`**, a title, and annotations (`readOnlyHint`, `destructiveHint`,
`idempotentHint`, `openWorldHint`).

### 4.1 Read tools (`readOnlyHint: true`)

| Tool | Arguments | Returns |
|------|-----------|---------|
| `list_rooms` | — | rooms with control counts |
| `list_controls` | `name?`, `room?`, `type?`, `category?`, `favorites_only?` | name, type, room, category, uuid, and the tool that operates it |
| `get_control` | `name`, `room?` | live value, state attributes and named outputs |
| `list_sensors` | `kind?` (all, temperature, door-window, motion, smoke, energy), `room?` | sensor readings |
| `get_energy_flow` | `top?` | production, consumption, grid import/export, battery charge and state of charge, the flow monitor's nodes; with `top`, the meters ranked by current power |
| `list_light_moods` | `name`, `room?` | mood IDs and names of a lighting controller (WebSocket) |
| `list_scenes` | — | local `lox` scenes with description and step count |
| `system_status` | — | firmware, PLC state, heap |
| `get_wiring` | `name`, `room?`, `trace?` (up, down, both), `depth?`, `all_params?`, `download?`, `live?` | the config logic around a control or block: inputs, outputs (hardware refs folded to the device and room), non-default parameters, optional traced paths, and the app control it is. Reads the config repo or the cached `.Loxone`; `download` fetches it via FTP (read-only). `live` adds the current value to every wire (and traced node) whose source connector is a state the app streams, as `live: {value, text?, control, state}` with `text` in the control's own format (`22000lx`), and the control's own main value as `value`: one WebSocket state dump for all of them, the same read as the TUI's wiring overlay. A wire without a streamed source (an unvisualized block, a hardware input) gets no value, never a guessed one; a failed WebSocket leaves the config wiring and says why in `live_error` |
| `list_users` | — | users with state and admin flag (`getuserlist2`) |
| `get_user` | `name` | one user (`getuser`): state, validity, groups, NFC tags, number of access codes |
| `access_log` | `code_touch`, `limit?` | the NFC Code Touch's access log (`/log/<uuid>.txt`), newest entries, decoded |
| `access_check` | `user`, `code_touch?`, `cached?` | per Code Touch: verdict (granted, no-access-right, no-credential, user-disabled, unknown-full-access), the rights found, warnings where the program file and the live state disagree. Downloads the config via FTP (read-only) unless `cached` |

### 4.2 Action tools

All action tools take `name`, optional `room`, and optional `dry_run`. They resolve the control,
check its type, build an `actions::Action`, confirm it if needed (§5), and send its commands.

| Tool | Actions | Confirmation |
|------|---------|--------------|
| `switch` | `on`, `off`, `pulse` | only on door locks, gates, alarms (§5) |
| `blind` | `up`, `down`, `stop`, `shade`, `position` (+ `value` 0–100), `slats` (+ `value`) | — |
| `light` | `mood` (`plus`/`minus`/`off`/ID), `dim` (0–100), `color` (`#RRGGBB`, `hsv(h,s,v)`, `temp(b,k)`) | — |
| `thermostat` | `temp`, `mode`, `override` (+ `minutes`) | — |
| `gate` | `open`, `close`, `stop` | open, close |
| `alarm` | `arm`, `arm-home`, `disarm`, `quit` (+ `no_motion`, `code`) | all but quit |
| `door` | `lock`, `unlock`, `open` | all |
| `run_scene` | scene name | — (user-authored); reports progress, stops on cancel |
| `send_command` | raw Loxone command | only listed with `--allow-raw`; confirmed on risky types |

User-management tools take their own arguments; every one is confirmed (§5) and accepts `dry_run`.
Each change is sent once, without retries, and its result carries the note that the change may live
only in the Miniserver's custom changes until Loxone Config saves the program.

| Tool | Arguments | Does |
|------|-----------|------|
| `learn_nfc_tag` | `code_touch`, `timeout_secs?` (60, at most 300) | starts the Code Touch's learn mode over the WebSocket (`nfc/startlearn`), waits for a new `nfcLearnResult` (the value in the initial state dump is the last learned tag and ignored), ends learn mode (`nfc/stoplearn`) on success, error, timeout and cancel; returns the ID and, for a known tag, name and user. A secured Code Touch gets both as secured commands signed with the visualization password from `LOX_VISU_PASSWORD` or the config's `visu_password_file` (never a tool argument); without one the tool fails before asking. A tag that is already paired opens the door instead of being learned, and the tool says so The confirmation tells the user to hold the tag or unlocked phone to the reader; a progress notification repeats it when the call carries a progress token |
| `assign_nfc_tag` | `user`, `id`, `name` | `addusernfc`; refuses a tag the user has already |
| `remove_nfc_tag` | `user`, `tag` (ID or name) | `removeusernfc` |
| `set_access_code` | `user`, `code` (2–8 digits) | `updateuseraccesscode`; the code is masked (`****`) in results, errors, `dry_run` and the confirmation, and bound to the confirmation by hash only; `code_in_use` when another user has it |

Results are structured content matching the tool's `outputSchema`: the resolved control, the action
as text, the commands sent (PINs masked), the Miniserver response, and the equivalent `lox` command
line (`cli`).

`run_scene` sends a `notifications/progress` per step when the request carries a progress token,
and checks the request's cancellation token between steps and during delays: a cancelled scene
stops sending immediately.

## 5. Safety model

The policy is fixed when the server starts; the model cannot change it.

| Mode | Read tools | Everyday actions | Risky actions | Raw commands |
|------|:---:|:---:|:---:|:---:|
| `--read-only` | ✓ | — (not listed) | — | — |
| default | ✓ | ✓ | **after the user confirms** | — |
| `--allow-risky` | ✓ | ✓ | ✓ (no prompt) | — |
| `--allow-raw` | ✓ | ✓ | per the above | ✓ |

**What is risky.** `Action::risk() == Risk::Confirm` (the set the TUI asks to confirm), plus any
generic action (`on`, `off`, `pulse`, raw, value) aimed at a door lock, gate or alarm, so `switch off`
on a door lock or a raw `open` to a gate cannot bypass the gate. Installations often wire a door opener
as a plain `Pushbutton` or `Switch` (e.g. "Tür öffnen" in an access category), so generic actions are
also gated on controls whose icon or category icon names a door, gate, garage, lock or key
(`IconsFilled/door-open.svg`, `login-key.svg`; category names are user-chosen and localized, icons are
not), and on controls Loxone marks `isSecured`. Finally, the user can list any control under
`confirm:` in the `lox` config (UUID, alias, or case-insensitive name substring with an optional
`[Room]`); every action on a listed control is confirmed, e.g. a pool cover:

```yaml
confirm:
  - Pool Abdeckung [Pool]
```

The user-management tools (`learn_nfc_tag`, `assign_nfc_tag`, `remove_nfc_tag`, `set_access_code`)
are always confirmed; their confirmation is bound to the change's requests (for `set_access_code`, to
a SHA-256 of the code, not the code).

`--allow-risky` skips all of these confirmations. Dry runs are never gated; they report
`needs_confirmation: true`.

**How the user confirms.** The question goes to the human through the client's UI, never to the model:

- **2026-07-28 — multi-round-trip request.** The first `tools/call` returns `resultType: "input_required"`
  with an elicitation (`{ confirm: boolean }`, message naming the control, action and equivalent CLI
  command) and a `requestState`. The client asks the user and retries with `inputResponses`.
  The `requestState` is a random single-use nonce kept server-side for 10 minutes and bound to the
  control UUID and the exact commands, so it cannot be forged, replayed, or reused for another action.
  Every action tool accepts the retry, since any of them can hit a confirmation.
- **Older revisions — elicitation request.** The server sends `elicitation/create` during the call
  and waits up to 5 minutes for the answer.
- **Clients without elicitation** get `action_not_allowed`, naming `--allow-risky` and the CLI command
  the user can run themselves.

A declined confirmation returns `declined_by_user` and tells the model not to retry.

Other rules: risky tools carry `destructiveHint`, so clients that confirm destructive calls will ask
too; credentials never pass through tool arguments; alarm PINs and access codes are passed to the
Miniserver and never echoed back.

## 6. Errors

Tool failures are results with `isError: true` whose text is the CLI error envelope, so the model sees
them and can correct itself:

```json
{ "ok": false, "error": "ambiguous_control", "message": "Ambiguous: 'Licht'. Pass `room`, write the name as 'Name [Room]', or use the UUID. …" }
```

Codes match `lox -o json` (`control_not_found`, `ambiguous_control`, `config_not_found`,
`unauthorized`, `connection_error`, `tag_not_found`, `code_in_use`, `learn_timeout`, …) plus
`invalid_arguments`, `action_not_allowed`,
`declined_by_user`, `confirmation_expired` and `confirmation_mismatch`. Arguments that do not match the
input schema (wrong enum value, missing field) are tool errors too, not protocol errors: rmcp's
plain-text message is wrapped in the envelope as `invalid_arguments`
(`"Invalid arguments: unknown variant `fly`, expected one of `up`, …"`).
Protocol errors are left to rmcp (unknown tool, malformed request, unsupported protocol version).

## 7. State and lifetime

- Config is loaded lazily on the first tool call, so a client can list tools before `lox setup` has
  run; the tool call then reports `config_not_found` with the fix.
- The `LoxClient` (and its in-memory structure) is rebuilt every 15 minutes. That picks up
  `lox cache refresh`, the 24 h structure TTL and changed credentials in long-lived client sessions.
- Blocking work holds a mutex on the client; scene delays do not, so other tools stay usable while a
  scene waits.
- EOF on stdin ends the server with exit code 0.

## 8. Code layout

| File | Role |
|------|------|
| `src/mcp/mod.rs` | `ServerOptions`, `lox mcp serve/config/tools`, the stdio runtime |
| `src/mcp/server.rs` | `LoxMcp`: `#[tool_router]` tools, typed parameters, confirmation (MRTR and elicitation), progress, cancellation, `tools/list` cache hints, server info |
| `src/mcp/ops.rs` | synchronous Miniserver operations and the `JsonSchema` output types |
| `src/mcp/tests.rs` | protocol tests: an rmcp client against the server over an in-memory duplex |

## 9. Testing

- **Protocol tests** drive a real rmcp client against `LoxMcp` over an in-memory duplex and an
  `httpmock` Miniserver. Lifecycle-dependent behavior runs under both 2026-07-28 (discover) and
  2025-11-25 (initialize): identification, cache hints, confirmation accepted, declined and
  unsupported, the raw `input_required` round trip, forged and mismatched `requestState`, the
  `switch`-on-a-door-lock bypass, progress notifications, and cancellation mid-scene.
- **Schema tests**: every tool has a title, annotations, an inline input schema and an output schema
  whose `required` fields are always present.
- **CLI smoke tests** pipe JSON-RPC into the real binary: an `initialize` session, a `server/discover`
  session, and a stateless 2026-07-28 `tools/call`.
- **Interop**: the official Python SDK (`mcp` 2.2) was run against the release binary in `auto`
  (discover), pinned `2026-07-28`, and `legacy` modes, including the door confirmation. It caught the
  one bug the Rust tests had missed: output schemas that required fields the server omits.

## 10. Later

- Streamable HTTP transport (`lox mcp serve --http`) with authorization, for remote connectors.
- Resources (`loxone://rooms`, `loxone://controls/{uuid}`) with `subscriptions/listen` backed by
  `lox stream`, so clients see state changes live.
- The tasks extension (SEP-2663) for long scenes.
- More device tools: intercom, EV charger, music zones, statistics/history.
