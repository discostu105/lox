---
title: Home
layout: home
nav_order: 1
---

# lox — Loxone Miniserver CLI

**Fast, scriptable command-line interface for Loxone Miniserver.**
Single binary. No runtime. No cloud. Works in scripts, cron jobs, and AI agent pipelines.
{: .fs-6 .fw-300 }

[Get Started](/lox/getting-started){: .btn .btn-primary .fs-5 .mb-4 .mb-md-0 .mr-2 }
[Command Reference](/lox/commands/){: .btn .fs-5 .mb-4 .mb-md-0 }

---

## Why this exists

The Loxone app is great for everyday use — but it offers no API or scripting support for automation, CI/CD pipelines, or headless environments.

`lox` gives you a proper CLI so you can:

- **Script your home** — bash, Python, cron, whatever
- **Connect AI agents** — Claude, GPT, or any LLM tool can control your home via shell commands
- **Chain commands** — `lox if "Temperatur" gt 25 && lox blind "Sudseite" pos 80`
- **Integrate with anything** — exit codes, JSON output, stdin/stdout

```bash
# Turn off all lights when leaving
lox off "Licht Wohnzimmer Zentral" && lox blind "Sudseite" full-up

# AI agent can call these:
lox ls --type LightControllerV2 -o json | jq '.[].name'
lox light mood "Wohnzimmer" off
lox status -o json | jq '.plc_running'

# Conditionally close blinds
lox if "Temperatur Aussen" gt 28 && lox blind "Beschattung Sud" pos 70
```

---

## Quick overview

```bash
lox ls                                  # List all controls
lox get "Temperatur [Schlafzimmer]"     # Read a control's state
lox on "Licht Wohnzimmer"              # Turn on
lox off "Licht Wohnzimmer"             # Turn off
lox blind "Beschattung Sud" pos 50     # Blind to 50%
lox light mood "Licht" plus            # Next light mood
lox thermostat "Heizung" temp 22.5     # Set temperature
lox alarm "Alarmanlage" arm            # Arm alarm
lox stream --room "Kitchen" -o json    # Real-time state stream
lox status --energy                    # Energy dashboard
lox health --problems                  # Device health
lox config pull                        # GitOps config versioning
```

---

## Supported control types

| Type | Commands |
|:-----|:---------|
| `LightControllerV2` | `on`, `off`, `mood plus/minus/off/<id>` |
| `Jalousie` / `CentralJalousie` | `up`, `down`, `stop`, `pos <0-100>`, `shade`, `full-up`, `full-down` |
| `Switch` | `on`, `off`, `pulse` |
| `Dimmer` | `dimmer <name> <0-100>` |
| `Gate` / `CentralGate` | `gate <name> open/close/stop` |
| `ColorPickerV2` | `color <name> #RRGGBB` or `hsv(h,s,v)` |
| `IRoomControllerV2` | `thermostat <name> --temp/--mode/--override` |
| `Alarm` | `alarm <name> arm/disarm/quit` |
| `InfoOnlyAnalog` / `Meter` | `get` (read-only) |
| Any | `send <uuid> <raw-command>`, `lock`, `unlock` |

---

## Performance

Structure cache at `~/.lox/cache/structure.json` (24h TTL):

| Operation | Cold | Warm |
|:----------|:-----|:-----|
| `lox on "Licht"` | ~1.2s | ~80ms |
| `lox ls` | ~1.2s | ~80ms |
| `lox ls --values` | ~1.2s + N reqs | slower |
| `lox status` | ~120ms | ~120ms |
