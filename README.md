# lox — Loxone Miniserver CLI

**Fast, scriptable command-line interface for Loxone Miniserver.**  
Single binary. No runtime. No cloud. Works in scripts, cron jobs, and AI agent pipelines.

---

## Why this exists

The Loxone app is great for everyday use — but it offers no API or scripting support for automation, CI/CD pipelines, or headless environments.

`lox` gives you a proper CLI so you can:

- **Script your home** — bash, Python, cron, whatever
- **Connect AI agents** — Claude, GPT, or any LLM tool can control your home via shell commands
- **Chain commands** — `lox if "Temperatur" gt 25 && lox blind "Südseite" pos 80`
- **Integrate with anything** — exit codes, JSON output, stdin/stdout

```bash
# Turn off all lights when leaving
lox off "Licht Wohnzimmer Zentral" && lox blind "Südseite" full-up

# AI agent can call these:
lox ls --type LightControllerV2 -o json | jq '.[].name'
lox light mood "Wohnzimmer" off
lox status -o json | jq '.plc_running'

# Disambiguate sensors with the same name using --room or bracket syntax
lox get "Temperatur" --room "Schlafzimmer"
lox get "Temperatur [OG Kinderzimmer Bastian]"

# Quick energy overview
lox status --energy

# Conditionally close blinds
lox if "Temperatur Außen" gt 28 && lox blind "Beschattung Süd" pos 70
```

---

## For AI Agents

This CLI was designed for AI agent integration. Every command:

- Exits `0` on success, non-zero on error
- Has `-o json` flag for structured output
- Uses fuzzy name matching — agents don't need UUIDs
- Supports `--room` flag and `[Room]` bracket syntax to resolve ambiguous names
- Returns readable errors with suggestions
- `--dry-run` validates without executing — preview what would happen
- `--trace-id` correlates agent actions across logs
- `--non-interactive` fails instead of prompting (implied by `-o json`)
- `lox schema` lets agents discover available commands programmatically
- Errors return structured JSON envelopes with categorized error codes

**Example: give an LLM a shell tool**
```json
{
  "name": "lox",
  "description": "Control Loxone smart home. Use -o json for structured output.",
  "parameters": {
    "command": { "type": "string", "description": "e.g. 'on Wohnzimmer', 'blind Südseite pos 50', 'status --energy'" }
  }
}
```

The agent calls `lox <command>` as a shell tool and reads stdout. That's it.

An agent can discover your home (`lox ls -o json`), read sensor values, control devices, and check conditions — all without any custom integration layer.

**Agent-friendly workflow:**
```bash
lox schema -o json                    # discover available commands
lox ls -o json                        # discover controls
lox --dry-run on "Licht" -o json      # preview before executing
lox --trace-id "run-42" on "Licht"    # execute with tracing
lox health --problems -o json         # check device health
```

---

## Install

**Homebrew (macOS/Linux):**
```bash
brew tap discostu105/lox https://github.com/discostu105/lox
brew install discostu105/lox/lox
```

**Build from source:**
```bash
git clone https://github.com/discostu105/lox
cd lox
cargo build --release
cp target/release/lox ~/.local/bin/
```

**Requirements (source):** Rust 1.75+. No OpenSSL. No runtime dependencies.

## Setup

```bash
lox setup set --host https://192.168.1.100 --user USER --pass PASS

# With serial for correct TLS hostname (avoids cert warnings)
lox setup set --host https://192.168.1.100 --user USER --pass PASS --serial YOUR_SERIAL
```

Config: `~/.lox/config.yaml`

### Aliases

Add short names for frequently-used controls in `~/.lox/config.yaml`:

```yaml
host: https://192.168.1.100
user: admin
pass: secret
aliases:
  wz: "1d8af56e-036e-e9ad-ffffed57184a04d2"   # Lichtsteuerung Wohnzimmer
  kueche: "20236c09-0055-6e94-ffffed57184a04d2" # Licht Küche Insel
```

Then use short names directly: `lox on wz`, `lox off kueche`

### Shell Completions

**Homebrew** installs completions automatically — no extra steps needed.

**Manual install** (one command, auto-detects your shell):
```bash
lox completions --install
```

Or generate to stdout for custom setups: `lox completions bash|zsh|fish|powershell`

---

## Commands

Quick overview — see **[COMMANDS.md](COMMANDS.md)** for the full reference with all options.

```bash
lox ls                                  # List all controls
lox ls -t Jalousie -r "EG"             # Filter by type/room/category
lox get "Temperatur [Schlafzimmer]"     # Read a control's state
lox on "Licht Wohnzimmer"              # Turn on
lox off "Licht Wohnzimmer"             # Turn off
lox blind "Beschattung Süd" pos 50     # Blind to 50%
lox light mood "Licht" plus            # Next light mood
lox thermostat "Heizung" temp 22.5     # Set temperature
lox alarm "Alarmanlage" arm            # Arm alarm
lox stream --room "Kitchen" -o json    # Real-time WebSocket state stream
lox otel serve --endpoint http://..   # Push metrics, logs & traces via OTLP
lox if "Temperatur" gt 25 && echo hot  # Conditional logic
lox status --energy                    # Energy dashboard
lox config download --extract          # Download & extract Loxone Config
lox config diff old.Loxone new.Loxone  # Compare two configs
lox config init ~/loxone-config        # Init git repo for config versioning
lox config pull                        # Download, diff & git-commit config
lox config log                         # Show config change history
lox config restore abc123 --force      # Restore config from git history
lox run abend                          # Run a scene
lox health --problems                  # Device health (battery, signal, offline)
lox schema blind                       # Command schema for AI agent discovery
lox completions bash                   # Generate shell completions
```

---

## Scenes

YAML files in `~/.lox/scenes/`:

```yaml
# ~/.lox/scenes/abend.yaml
name: Abend
steps:
  - control: "Lichtsteuerung Wohnzimmer"
    cmd: on
  - control: "Beschattung Südseite"
    cmd: "pos 70"
  - control: "LED Küche"
    cmd: off
    delay_ms: 500
```

---

## Performance

Structure cache at `~/.lox/cache/structure.json` (24h TTL):

| Operation | Cold | Warm |
|-----------|------|------|
| `lox on "Licht"` | ~1.2s | ~80ms |
| `lox ls` | ~1.2s | ~80ms |
| `lox ls --values` | ~1.2s + N×req | slower (one HTTP request per control) |
| `lox status` | ~120ms | ~120ms |

---

## Supported Control Types

| Type | Commands |
|------|----------|
| `LightControllerV2` | `on`, `off`, `mood plus/minus/off/<id>` |
| `Jalousie` / `CentralJalousie` | `up`, `down`, `stop`, `pos <0-100>`, `shade`, `full-up`, `full-down` |
| `Switch` | `on`, `off`, `pulse` |
| `Dimmer` | `dimmer <name> <0-100>` |
| `Gate` / `CentralGate` | `gate <name> open/close/stop` |
| `ColorPickerV2` | `color <name> #RRGGBB` or `color <name> "hsv(h,s,v)"` |
| `IRoomControllerV2` | `thermostat <name> --temp/--mode/--override` |
| `Alarm` | `alarm <name> arm/disarm/quit` |
| `InfoOnlyAnalog` / `Meter` | `get` (read-only) |
| Any | `send <uuid> <raw-command>`, `lock`, `unlock` |

---

## Config Versioning (GitOps)

Track Miniserver configuration changes in a git repository — automated backups with meaningful commit messages:

```bash
# One-time setup
lox config init ~/loxone-config

# Pull current config, diff against previous, commit with semantic message
lox config pull

# View history
lox config log

# Restore a previous version
lox config restore abc123 --force
```

Each pull downloads the config via FTP, decompresses the proprietary LoxCC format to XML, generates a semantic diff (controls/rooms/users added/removed/renamed), and commits with a meaningful message like:

```
[504F94AABBCC] Config backup 2026-03-08 18:22:56 (v42)

+ Added control: "Garage Light" (Switch)
~ Light: "Licht EG" -> "Licht Erdgeschoss"
- Removed user: "guest"
```

**Cron-friendly:** `lox config pull --quiet` for automated nightly backups.

**Multi-Miniserver:** each Miniserver gets its own subdirectory (by serial number).

**Safe restore:** uploads the original backup ZIP from git history (no risky recompression).

---

## Architecture

```
~/.lox/
  config.yaml          # Host, credentials, serial, aliases
  cache/
    structure.json     # LoxApp3.json (24h TTL, ~150KB)
  token.json           # Token auth (optional)
  scenes/*.yaml        # Your scenes
```

Single static Rust binary ~4MB. TLS via rustls (no OpenSSL). Self-signed certs accepted.

---

## Requirements

- Loxone Miniserver Gen 1/2, firmware 12.0+
- Local network access (or DynDNS)
- For `lox log`: Admin user

---

## Status

This project is a an experiment. Expect rough edges. Not every Miniserver configuration has been tested. If you like the project, please help and contribute.

**Use at your own risk.** Commands that modify your Miniserver (e.g., `config upload`, `reboot`, `update`) can affect your live system. Always have a backup.

---

## License

MIT
