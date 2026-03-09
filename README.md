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
lox ls --type LightControllerV2 --json | jq '.[].name'
lox mood "Wohnzimmer" off
lox status --json | jq '.plc_running'

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
- Has `--json` flag for structured output
- Uses fuzzy name matching — agents don't need UUIDs
- Supports `--room` flag and `[Room]` bracket syntax to resolve ambiguous names
- Returns readable errors with suggestions

**Example: give an LLM a shell tool**
```json
{
  "name": "lox",
  "description": "Control Loxone smart home. Returns JSON on --json flag.",
  "parameters": {
    "command": { "type": "string", "description": "e.g. 'on Wohnzimmer', 'blind Südseite pos 50', 'status --energy'" }
  }
}
```

The agent calls `lox <command>` as a shell tool and reads stdout. That's it.

An agent can discover your home (`lox ls --json`), read sensor values, control devices, and check conditions — all without any custom integration layer.

---

## Install

**Homebrew (macOS/Linux):**
```bash
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
lox config set --host https://192.168.1.100 --user USER --pass PASS

# With serial for correct TLS hostname (avoids cert warnings)
lox config set --host https://192.168.1.100 --user USER --pass PASS --serial YOUR_SERIAL
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

---

## Commands

```bash
# ── System ────────────────────────────────────────────────────────
lox status                              # Miniserver health: firmware, PLC, memory
lox status --energy                     # + live energy panel (PV, grid, battery, consumption)
lox status --diag                       # CPU, tasks, context switches, SD card
lox status --net                        # Network config (IP, MAC, DNS, DHCP, NTP)
lox status --bus                        # CAN bus statistics
lox status --lan                        # LAN packet statistics
lox status --all                        # All diagnostic sections
lox status --json
lox time                                # Miniserver system date/time

# ── Discovery ─────────────────────────────────────────────────────
lox ls                                  # All controls
lox ls --type Jalousie                  # Filter by type
lox ls --room "Wohnzimmer"              # Filter by room
lox ls --cat "Beleuchtung"              # Filter by category
lox ls --favorites                      # Only favorites
lox ls --values                         # Include current values (slower)
lox ls --type LightControllerV2 --json  # JSON for agents/scripts
lox rooms                               # List all rooms
lox categories                          # List all categories
lox get "Lichtsteuerung Wohnzimmer"     # Full state of one control
lox info "Lichtsteuerung Wohnzimmer"    # Detailed info (sub-controls, states, moods)
lox globals                             # Global states (operating mode, sunrise, etc.)
lox modes                               # Operating modes
lox discover                            # Find Miniservers on local network (UDP)
lox discover --timeout 5               # Custom timeout in seconds
lox extensions                          # Connected extensions and devices

# ── Resolving ambiguous names ─────────────────────────────────────
# When multiple controls share the same name (e.g. "Temperatur"):
lox get "Temperatur" --room "Schlafzimmer"          # --room flag
lox get "Temperatur [OG Schlafzimmer]"              # bracket syntax
lox off "Lichtsteuerung" --room "Büro Christoph"    # works on all commands

# ── Lights ────────────────────────────────────────────────────────
lox on  "Lichtsteuerung Wohnzimmer"
lox off "Lichtsteuerung Wohnzimmer"
lox mood "Lichtsteuerung Wohnzimmer" plus     # Next mood
lox mood "Lichtsteuerung Wohnzimmer" minus    # Previous mood
lox mood "Lichtsteuerung Wohnzimmer" off      # Turn off (mood 778)
lox mood "Lichtsteuerung Wohnzimmer" 704      # Set by numeric mood ID
lox dimmer "Stehlampe" 75                     # Set dimmer 0-100%
lox color "LED Strip" "#FF0000"               # Set color (hex RGB or hsv())

# ── Blinds ────────────────────────────────────────────────────────
lox blind "Beschattung Süd" up
lox blind "Beschattung Süd" down
lox blind "Beschattung Süd" stop
lox blind "Beschattung Süd" pos 50      # Position 0-100%
lox blind "Beschattung Süd" full-up
lox blind "Beschattung Süd" full-down
lox blind "Beschattung Süd" shade       # Automatic shading

# ── Gates ────────────────────────────────────────────────────────
lox gate "Garagentor" open
lox gate "Garagentor" close
lox gate "Garagentor" stop

# ── Climate ──────────────────────────────────────────────────────
lox thermostat "Heizung Wohnzimmer" --temp 22.5     # Set comfort temp
lox thermostat "Heizung Wohnzimmer" --mode auto     # auto|manual|comfort|eco
lox thermostat "Heizung" --override 24 --duration 120  # Override for N minutes
lox weather                             # Current weather data
lox weather --forecast                  # 7-day forecast

# ── Security ─────────────────────────────────────────────────────
lox alarm "Alarmanlage" arm             # Arm alarm
lox alarm "Alarmanlage" arm --no-motion # Arm without motion detection
lox alarm "Alarmanlage" disarm          # Disarm
lox alarm "Alarmanlage" quit            # Acknowledge/silence
lox lock "Heizung" --reason "Wartung"   # Lock a control
lox unlock "Heizung"                    # Unlock a control

# ── Statistics & History ──────────────────────────────────────────
lox stats                               # Controls with statistics enabled
lox history "Temperatur" --month 2025-01  # Monthly statistics
lox history "Temperatur" --day 2025-01-15 # Daily statistics
lox history "Temperatur" --csv          # CSV output

# ── Conditions & Logic ────────────────────────────────────────────
lox if "Temperatur Außen" gt 25         # Exit 0=true, 1=false
lox if "Schalter" eq 1 && lox on "Licht"

# ── Analog / Virtual Inputs ───────────────────────────────────────
lox set "Sollwert Heizung" 21.5
lox pulse "Taster"

# ── Music Server ─────────────────────────────────────────────────
lox music play                          # Play zone 1
lox music pause 2                       # Pause zone 2
lox music stop                          # Stop zone 1
lox music volume 50                     # Set volume 0-100

# ── Scenes ────────────────────────────────────────────────────────
lox run abend
lox scene list
lox scene show abend
lox scene new abend

# ── System Administration ────────────────────────────────────────
lox update check                        # Check for firmware updates
lox update install --confirm            # Install firmware update
lox reboot --confirm                    # Reboot the Miniserver
lox files ls /                          # Browse Miniserver filesystem
lox files get /log/def.log              # Download a file
lox log                                 # Miniserver log (needs admin)

# ── Cache ─────────────────────────────────────────────────────────
lox cache info
lox cache check                         # Check if cache is current
lox cache refresh
lox cache clear

# ── Token Auth ────────────────────────────────────────────────────
lox token fetch                         # Fetch & save token (valid 20 days)
lox token info
lox token check                         # Verify token on Miniserver
lox token refresh                       # Extend token validity
lox token revoke                        # Revoke token on Miniserver
lox token clear                         # Delete local token file

# ── Raw ───────────────────────────────────────────────────────────
lox send <uuid> <command>
lox send <uuid> <command> --secured <hash>  # Secured command
lox watch "Temperatur Außen"
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

## License

MIT
