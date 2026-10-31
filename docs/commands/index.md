---
title: Command Reference
layout: default
nav_order: 3
has_children: true
---

# Command Reference
{: .no_toc }

All commands support these global flags:
{: .fs-5 .fw-300 }

| Flag | Description |
|:-----|:------------|
| `-o`, `--output` | Output format: `table` (default), `json`, `csv` |
| `-q`, `--quiet` | Suppress non-essential output |
| `--no-color` | Disable colored output (also respects `NO_COLOR` env var) |
| `--no-header` | Suppress table headers |
| `-v`, `--verbose` | Verbose logging (`-v` HTTP requests, `-vv` + response bodies) |
| `--dry-run` | Validate and resolve without executing |
| `--non-interactive` | Fail instead of prompting (implied by `-o json`) |
| `--trace-id` | Correlation ID for agent tracing |
| `--ctx <name>` | Use a specific context (overrides active context) |

## Control resolution

Controls can be referenced by **name** (fuzzy matched), **UUID**, or **alias**.

When multiple controls share the same name, disambiguate with:

```bash
# --room flag
lox get "Temperatur" --room "Schlafzimmer"

# Bracket syntax
lox get "Temperatur [OG Schlafzimmer]"
```

Resolution order: alias > exact UUID > bracket room qualifier > `--room` flag > fuzzy substring.
When several controls match, one whose name matches exactly wins (`Zentral` over `Jalousie Zentral`).

## Dry-run mode

Preview what a command would do without executing:

```bash
lox --dry-run on "Licht Wohnzimmer"
lox --dry-run on "Licht" -o json
```

JSON dry-run output:

```json
{
  "ok": true,
  "dry_run": true,
  "would_execute": {
    "uuid": "1d8af56e-...",
    "command": "on",
    "control": "Licht Wohnzimmer",
    "room": "Wohnzimmer"
  }
}
```

## Scripting

- Exit code `0` on success, non-zero on errors; `lox if` exits `1` when the condition is false.
- `-o json` on every command; errors become a JSON envelope (`{"ok": false, "error": "control_not_found", …}`).
- `--no-header` with `-o csv` or tables for clean pipes; `-q` drops progress messages.
- `lox schema -o json` describes every command and argument.

```bash
lox ls -t Jalousie -r OG -o json | jq -r '.[].uuid' | while read -r b; do lox blind "$b" up; done
```

## Pages

| Page | Commands |
|:-----|:---------|
| [Controls](controls) | `on`/`off`, lights, blinds, gates, climate, alarm, doors, intercom, charger, inputs, music, `send`, `run` |
| [Inspection](inspection) | `ls`, `get`, `info`, `watch`, `stream`, `if`, rooms, sensors, energy, weather, history, autopilot |
| [System](system) | `status`, `log`, `health`, `discover`, firmware, `reboot`, `files`, `tui` |
| [Configuration](configuration) | `setup`, aliases, cache, token, scenes, `config`, `logic`, contexts, completions, `schema`, `mcp` |
