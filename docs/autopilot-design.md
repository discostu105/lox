# Design: `lox autopilot` — Automatik-Regel CLI Support

> **Status: IMPLEMENTED**, revised after testing against a real Miniserver (see
> *Findings* below): `lox autopilot ls` reads the rules from the program file,
> `state` reads the `history` state, `changed` the `changed` state.

## Findings (verified on a Miniserver, firmware 17.2)

- The structure's `autopilot` key holds **one container** for all rules (named
  after the locale, e.g. "Automatic rules"), not one entry per rule. The rules
  exist only in the program file, as `<C Type="AutopilotRule" Title=… APC="{json}">`;
  the JSON carries `name`, `active`, `invalid`, `eventGroups`, `actions`, …
  `lox autopilot ls` reads them through the `lox config` sources.
- States can't be read over HTTP: `/jdev/sps/io/<state uuid>/state` answers 404.
  Both states arrive in the WebSocket's initial state dump.
- `changed` is a value state: **unix seconds** of the last rule change (it
  ticked when rules were re-saved in the app), not of the last run.
- `history` is a text state: `|`-separated runs, each
  `YYYY-MM-DD HH:MM:SS <rule uuid> <rule name>` (local time), the recent runs only.
- A rule re-saved in the app did not produce a new program file in `/prog`,
  and a download still had the old rule. Such edits live in the Miniserver's
  custom changes; an upload logs `Loading sps_new.zip - Remove old custom changes`.
  So before uploading, check `lox autopilot changed` against the time of the
  download the upload is based on.

## Background

Loxone's "Automatik-Regel" (Automatic Rule / Autopilot) feature lets users create
programmable automation rules via the Loxone app. These rules are stored in the
Miniserver's `LoxApp3.json` structure file under a top-level `autopilot` key — a
sibling of `controls`, `rooms`, etc.

Each entry looks like:

```json
"autopilot": {
  "<uuid>": {
    "name": "Turn off lights at midnight",
    "uuidAction": "<uuid>",
    "states": {
      "changed": "<state-uuid>",
      "history": "<state-uuid>"
    }
  }
}
```

The Loxone structure file documentation explicitly notes: *"The API isn't publicly
available."* There is no HTTP command endpoint to trigger or modify rules — they
are managed exclusively via the app and executed by the Miniserver when conditions
are met.

`automaticRule` also appears as a **trigger type string** in the history payloads
of other controls (e.g. a blind moved because an automatic rule fired it). This is
unrelated to the `autopilot` section itself.

## What We Can Do

| Operation | Feasibility |
|---|---|
| List all rules (name, UUID, active, JSON valid) | ✅ `AutopilotRule` objects in the program file |
| Show last-fired timestamp | ✅ `history` state (WebSocket) |
| When rules last changed | ✅ `changed` state (WebSocket) |
| Trigger / activate a rule | ❌ No public API |
| Create / edit a rule | ❌ App-only |

## CLI Design

### New subcommand: `lox autopilot`

```
lox autopilot list              # list all automatic rules
lox autopilot state <name>      # show when a rule last fired
```

### `autopilot list`

Reads `structure["autopilot"]` from the cached structure file (no extra HTTP calls).
Prints a table sorted by name:

```
NAME                                UUID
Morning blinds                      0abc1234-005c-7471-ffffed57184a0000
Turn off lights at midnight         0def5678-005c-7471-ffffed57184a0001
```

With `--json`: full JSON array including state UUIDs.

### `autopilot state <name>`

Resolves `<name>` to a rule via fuzzy substring match (same pattern as control
resolution). Fetches the `states.changed` UUID via the state API and displays the
value as a human-readable datetime (the value is a unix timestamp).

```
Rule:     Turn off lights at midnight
Last run: 2026-03-08 00:00:03
```

With `--json`: raw API response.

## Implementation Plan

### `src/client.rs`

Add `AutopilotRule` struct:

```rust
pub struct AutopilotRule {
    pub name: String,
    pub uuid: String,           // uuidAction
    pub state_changed: String,  // states.changed UUID
    pub state_history: String,  // states.history UUID
}
```

Add `list_autopilot_rules()` method that parses `structure["autopilot"]`. The
structure key is expected to be a map keyed by UUID (same layout as `controls`).

### `src/main.rs`

Add to `Cmd` enum:

```rust
/// Manage automatic rules (Automatik-Regel / Autopilot)
Autopilot {
    #[command(subcommand)]
    action: AutopilotCmd,
},
```

```rust
#[derive(Subcommand)]
enum AutopilotCmd {
    /// List all automatic rules
    List,
    /// Show last-run state of a rule
    State { name_or_uuid: String },
}
```

The `state` handler fetches `changed` via `/jdev/sps/io/<state_uuid>/state`,
interprets the numeric value as a unix timestamp, and formats it with `chrono`
(already a dependency).

## Open Questions

1. **State endpoint** — uncertain whether `states.changed` UUID works with
   `/jdev/sps/io/<uuid>/state` or requires WebSocket. Needs verification against
   a real Miniserver. Fallback: `/dev/sps/io/<uuid>/all`.
2. **Structure key** — need to confirm the exact key name (`autopilot` vs
   `autopilotrules`) against a real `LoxApp3.json`. Won't know until tested.
3. **History state format** — value format is undocumented. Could be a unix
   timestamp, a text blob, or structured data. Deferred until confirmed.
4. **Empty section** — if no autopilot rules have been created, the `autopilot`
   key may be absent or an empty object. Handle gracefully.

## Out of Scope

- Triggering or activating rules (no API exists)
- Creating, editing, or deleting rules (app-only)
- History state display (deferred, format unknown)
- Cross-referencing which controls were last triggered by an autopilot rule
