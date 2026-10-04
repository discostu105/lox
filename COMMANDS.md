# Command Reference

All commands support `-o`/`--output` for format selection (`json`, `csv`, `table`) and `-q`/`--quiet` to suppress non-essential output. Controls can be referenced by name (fuzzy matched), UUID, or alias.

When multiple controls share the same name, disambiguate with `-r`/`--room` or bracket syntax:
```bash
lox get "Temperatur" -r "Schlafzimmer"
lox get "Temperatur [OG Schlafzimmer]"
```

Global flags: `-o`/`--output json|csv|table`, `-q`/`--quiet`, `--no-color` (also respects `NO_COLOR` env var), `--no-header`, `-v`/`--verbose` (request logging), `--dry-run`, `--non-interactive`, `--trace-id`, `--ctx <name>` (use a specific context)

---

## Verbose & Dry-Run

```bash
lox -v ls                              # show HTTP requests
lox -vv get "Temperatur"               # show requests + response bodies
lox --dry-run on "Licht Wohnzimmer"    # preview without executing
lox --dry-run on "Licht" -o json       # dry-run returns JSON envelope
lox --non-interactive reboot           # fail instead of prompting (implied by -o json)
lox --trace-id "abc-123" on "Licht"    # correlation ID for agent tracing
```

`--dry-run` validates and resolves inputs without sending commands. With `-o json`, it returns a structured envelope:
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

---

## Setup

```bash
lox setup set --host https://192.168.1.100 --user admin --pass secret
lox setup set --serial YOUR_SERIAL     # enables correct TLS hostname
lox setup set --verify-ssl             # enable cert verification
lox setup set --no-verify-ssl          # disable (default, self-signed)
lox setup show                         # show config (password redacted)
```

All config fields also support env vars: `LOX_HOST`, `LOX_USER`, `LOX_PASS`, `LOX_SERIAL`
```bash
LOX_HOST=https://192.168.1.100 LOX_USER=admin LOX_PASS=secret lox status
```

### Aliases

```bash
lox alias add wz "1d8af56e-036e-e9ad-ffffed57184a04d2"
lox alias remove wz
lox alias ls
```

Then use directly: `lox on wz`

---

## Discovery & Inspection

```bash
lox ls                                 # all controls
lox ls -t Jalousie                     # filter by type
lox ls -r "Wohnzimmer"                 # filter by room
lox ls -c "Beleuchtung"                # filter by category
lox ls -f                              # only favorites
lox ls -v                              # include live values (slower)

lox get "Licht Wohnzimmer"            # full state of one control
lox info "Licht Wohnzimmer"           # detailed: sub-controls, states, moods

lox rooms                             # list all rooms
lox categories                        # list all categories
lox globals                           # global states (operating mode, sunrise, etc.)
lox modes                             # operating modes
lox extensions                        # connected extensions and devices
lox discover                          # find Miniservers on local network (UDP)
lox discover --timeout 5

lox health                            # device health dashboard (battery, signal, offline)
lox health --type tree                # filter by device type (tree or air)
lox health --problems                 # show only devices with issues

lox schema                            # list all commands with metadata (for AI agents)
lox schema blind                      # show schema for a specific command
```

---

## Lights

```bash
lox on "Licht Wohnzimmer"
lox off "Licht Wohnzimmer"
lox on --all-in-room "Wohnzimmer"     # turn on all controls in a room
lox off --all-in-room "Schlafzimmer"  # turn off all controls in a room
lox light mood "Licht Wohnzimmer" plus      # next mood
lox light mood "Licht Wohnzimmer" minus     # previous mood
lox light mood "Licht Wohnzimmer" off       # turn off (mood 778)
lox light mood "Licht Wohnzimmer" 704       # set by mood ID
lox light moods "Licht Wohnzimmer"          # list available moods (via WebSocket)
lox light moods "Licht" -o json             # moods as JSON with control metadata
lox light dim "Stehlampe" 75                # set dimmer 0-100% (Dimmer controls; use a mood for a lighting controller)
lox light color "LED Strip" "#FF0000"       # hex RGB
lox light color "LED Strip" "hsv(120,100,100)"  # HSV
```

---

## Blinds

```bash
lox blind "Beschattung Süd" up
lox blind "Beschattung Süd" down
lox blind "Beschattung Süd" stop
lox blind "Beschattung Süd" pos 50    # position 0-100%
lox blind "Beschattung Süd" full-up
lox blind "Beschattung Süd" full-down
lox blind "Beschattung Süd" shade     # automatic shading
```

---

## Gates

```bash
lox gate "Garagentor" open
lox gate "Garagentor" close
lox gate "Garagentor" stop
```

---

## Climate

```bash
lox thermostat "Heizung" temp 22.5                 # set comfort temp
lox thermostat "Heizung" mode auto                  # auto|manual|auto-heat|auto-cool|manual-heat|manual-cool
lox thermostat "Heizung" override 24 120            # override 24°C for 120 min
lox thermostat "Heizung"                            # show current state
lox weather                                         # current weather data
lox weather --forecast                              # 7-day forecast
```

---

## Security

```bash
lox alarm "Alarmanlage" arm            # arm alarm
lox alarm "Alarmanlage" arm --no-motion  # arm without motion detection
lox alarm "Alarmanlage" disarm
lox alarm "Alarmanlage" quit           # acknowledge/silence
lox door "Haustür" lock                # lock a door lock
lox door "Haustür" unlock              # unlock
lox door "Haustür" open                # open (e.g. electric strike)
lox lock "Heizung" --reason "Wartung"  # lock a control (admin)
lox unlock "Heizung"
```

---

## Statistics & History

```bash
lox stats                              # controls with statistics (legacy or Statistics V2)
lox history "Temperatur" --month 2025-01
lox history "Temperatur" --day 2025-01-15
lox history "Temperatur" -o csv        # CSV output
```

---

## Conditions

```bash
lox if "Temperatur Außen" gt 25        # exit 0=true, 1=false
lox if "Schalter" eq 1 && lox on "Licht"
```

Operators: `eq`, `ne`, `gt`, `ge`, `lt`, `le`

---

## Analog / Virtual Inputs

```bash
lox input set "Sollwert Heizung" 21.5
lox input set "Sollwert Heizung" 21.5 -r "Wohnzimmer"  # disambiguate with room
lox input pulse "Taster"
```

---

## Music Server

```bash
lox music play                         # play zone 1
lox music pause 2                      # pause zone 2
lox music stop
lox music volume 50                    # volume 0-100
```

---

## Scenes

```bash
lox run abend                          # run a scene
lox run abend --dry-run                # preview without executing
lox scene ls                           # list all scenes
lox scene show abend                   # print YAML definition
lox scene new abend                    # create empty scene file
```

Scenes are YAML files in `~/.lox/scenes/`:
```yaml
name: Abend
steps:
  - control: "Licht Wohnzimmer"
    cmd: on
  - control: "Beschattung Süd"
    cmd: "pos 70"
    delay_ms: 500
```

---

## Autopilot (Automatic Rules)

```bash
lox autopilot ls                       # list all automatic rules
lox autopilot state "Rule Name"        # show when a rule last fired
```

---

## Loxone Config

```bash
lox config download                    # download latest config ZIP via FTP
lox config download --extract          # download + decompress to .Loxone XML
lox config download --save-as config.zip  # custom output filename
lox config ls                          # list all configs on the Miniserver
lox config extract config.zip          # decompress LoxCC → .Loxone XML
lox config extract config.zip --save-as out.Loxone
lox config upload config.zip --force   # upload to Miniserver (dangerous)
lox config users file.Loxone           # list user accounts from config XML
lox config devices file.Loxone         # list hardware devices (Tree/Air/Network)

# Git-based config versioning
lox config init ~/loxone-config        # initialize a git repo for config tracking
lox config pull                        # download, decompress, diff & git-commit
lox config pull --quiet                # cron-friendly (no output unless error)
lox config log                         # show config change history
lox config log -n 5                    # last 5 entries
lox config restore abc123 --force      # restore config from git history & upload
```

The `pull` workflow: FTP download → LoxCC decompress → logic diff → git commit with meaningful message.
Multi-Miniserver: each serial gets its own subdirectory in the repo.

### Inspecting the logic (read-only)

The program inside a config — blocks, parameters and wires — read with [lxir](https://github.com/discostu105/lxir).
Hardware references on the pages are folded away: a wire goes from the motion sensor to the light, not to an input ref.

```bash
lox config diff                        # the last change in the config repo
lox config diff v271                   # from version 271 to the newest config
lox config diff 2026-08-25 HEAD        # snapshots by save date, commit, version or file
lox config diff a1b2c3d..e4f5a6b       # a range of commits
lox config diff old.Loxone new.zip     # two files (.Loxone or backup ZIP)
lox config show                        # the logic pages: blocks and wires per page
lox config show -p Wohnzimmer          # one page as lxir source (--all: every page)
lox config wiring "Licht Wohnzimmer"   # inputs, outputs and non-default parameters of a block
lox config wiring Licht -r Küche --trace up      # the whole path up to the sensors (down, both)
lox config wiring Licht --trace down --depth 6   # hops to follow (default 4)
lox config wiring Licht --all-params   # parameters at their default value, too
lox config find bewegung küche         # blocks by name, type, device, room or page (all words match)
lox config lint                        # dead blocks, unwired inputs, duplicate names, broken refs
lox config history "Licht Wohnzimmer"  # how one block changed across the snapshots
lox config history Licht -n 200        # scan more snapshots (default 50)
```

`show`, `wiring`, `find` and `lint` read the newest config without the network: the config repo's working copy,
else the cached download (`lox tui`'s wiring view keeps one). `--at SNAPSHOT` reads a snapshot of the repo
(commit, `v273`, `2026-08-25`), `--file PATH` a `.Loxone` or backup ZIP, `--download` fetches the current config
via FTP first (read-only). Names match like controls do: substring, `Name [Room]` (room, page or device) or UUID.

The diff is per logic page, with totals first; layout-only saves (moved blocks) show as "No logic changes":

```
b11f0c7 · saved 2026-08-25 21:25:27 → 303191c · saved 2026-08-28 06:39:25

= 1 added · 1 renamed · 1 parameter · wires +2 −0

Hallway
  + block  Stairs pulse (Monoflop)
  ~ rename Night → Night mode
  ~ param  Hallway light · MoveOn  600 s (10 min) → 900 s (15 min)
  + wire   Motion.Q → Stairs pulse.InputTrigger
  + wire   Night mode.Q → Hallway light.DisP
```

All of them take `-o json`.

---

## System

```bash
lox status                             # firmware, PLC state, memory
lox status --diag                      # CPU, tasks, interrupts, SD card health
lox status --net                       # network config (IP, MAC, DNS1/2, DHCP, NTP)
lox status --bus                       # CAN bus statistics (incl. parity errors)
lox status --lan                       # LAN statistics (incl. exhaustion, no-buffer)
lox status --all                       # all diagnostic sections
lox time                               # Miniserver system date/time
lox log                                # system log (admin only)
lox log -n 100                         # custom line count
```

---

## Device Health

```bash
lox health                            # device health dashboard
lox health --type tree                # filter: tree or air devices only
lox health --problems                 # show only devices with warnings/offline
lox health -o json                    # JSON output
```

Shows battery levels, signal strength, online/offline status, and bus errors for Tree and Air devices.

---

## Command Schema (AI Agents)

```bash
lox schema                            # list all commands with metadata
lox schema blind                      # schema for a specific command
lox schema -o json                    # JSON output for programmatic use
```

Returns command structure, arguments, subcommands, control type hints, and valid actions. Designed for AI agent discovery — agents can introspect what commands exist and what parameters they accept.

---

## MCP Server (AI Assistants)

```bash
lox mcp serve                         # Model Context Protocol server on stdio (MCP 2026-07-28 + older)
lox mcp serve --read-only             # read tools only, no actions
lox mcp serve --allow-risky           # doors, gates, alarm without asking the user first
lox mcp serve --allow-raw             # also expose send_command (raw Loxone commands)
lox mcp config                        # print config for Claude Desktop / Cursor and Claude Code
lox mcp config -o json                # just the JSON "mcpServers" snippet
lox mcp tools                         # list the tools a client would see (-o json: full definitions)
lox --ctx home mcp serve              # pin the server to one Miniserver context
lox --dry-run mcp serve               # every action is a dry run
```

MCP clients launch `lox mcp serve` themselves; you normally only run `lox mcp config` and paste the result.
Tools: `list_rooms`, `list_controls`, `get_control`, `list_sensors`, `list_light_moods`, `list_scenes`,
`system_status`, `get_wiring` (the config logic around a control: what drives it, what it drives, parameters, `trace` up/down), `switch`, `blind`, `light`, `thermostat`, `gate`, `alarm`, `door`, `run_scene`
(and `send_command` with `--allow-raw`). Every tool has input and output schemas; every action tool accepts `dry_run`.
High-risk actions (the ones `lox tui` asks to confirm, and any action on a door lock, gate or alarm) are confirmed by the user
through the MCP client; clients that cannot ask are refused with `action_not_allowed` unless the server runs with `--allow-risky`.
Door openers wired as push-buttons are recognized by their door/gate/lock/key icons. To have any other control confirmed
(e.g. a pool cover), list it under `confirm:` in the config (UUID, alias, or name with an optional `[Room]`; shown by `lox setup show`).
`lox tui` follows the same rules and asks before sending:

```yaml
confirm:
  - Pool Abdeckung [Pool]
```

Tool errors use the same envelope as `-o json`, plus `invalid_arguments`, `action_not_allowed`, `declined_by_user`,
`confirmation_expired` and `confirmation_mismatch`.

---

## Error Envelopes

When using `-o json`, errors return structured envelopes instead of plain text:

```json
{
  "ok": false,
  "error": "control_not_found",
  "message": "No control matching 'Nonexistent'"
}
```

Error codes: `control_not_found`, `ambiguous_control`, `config_not_found`, `confirmation_required`, `unauthorized`, `forbidden`, `not_found`, `http_error`, `connection_error`, `error`.

---

## Filesystem

```bash
lox files ls /                         # browse Miniserver filesystem
lox files get /log/def.log             # download a file
lox files get /log/def.log --save-as local.log
```

---

## Cache

```bash
lox cache info                         # show cache age and path
lox cache check                        # check if cache is current
lox cache refresh                      # force re-fetch
lox cache clear                        # delete local cache
```

---

## Token Auth

More secure than Basic Auth. Token is valid ~20 days.

```bash
lox token fetch                        # fetch & save token
lox token info                         # show token status
lox token check                        # verify token on Miniserver
lox token refresh                      # extend validity
lox token revoke                       # revoke on Miniserver
lox token clear                        # delete local token file
```

---

## Raw Commands

```bash
lox send <uuid> <command>              # send raw command to a control
lox send <uuid> <command> --secured <hash>  # secured command
lox watch "Temperatur"                 # poll state and print changes (Ctrl+C to stop)
lox watch "Temperatur" -i 5            # custom poll interval in seconds
```

---

## Terminal UI

A full-screen, live view of the house: rooms and controls, a live event feed, energy flow, Miniserver health,
config history and all your Miniservers — and you can act on it. Design: [docs/design-tui.md](docs/design-tui.md).

```bash
lox tui                                # start on the last screen (Home the first time)
lox tui --screen energy                # start on a screen: home|rooms|events|energy|system|sites
lox tui -r Kitchen                     # start in Rooms with a room selected
lox tui --read-only                    # no actions at all (wall display, screen sharing)
lox tui --theme mono                   # theme: night (default) | day | mono | neon
lox tui --icons nerd                   # Nerd Font icons (plain Unicode by default)
lox tui --no-mouse --no-motion         # no mouse capture, no animations (drag-select + auto-copy works with the mouse on)
lox tui --demo                         # a synthetic demo house, no Miniserver needed
lox --ctx office tui                   # another context; switch live with C or on the Sites screen
```

Global `--no-color` (or `NO_COLOR`) selects the `mono` theme. Harmless actions (lights, blinds, moods) are
instant; doors, gates, alarm, wallbox, door openers wired as push-buttons and controls on the config's `confirm:` list ask `y/N`; reboot and firmware update ask you to type the context name.

Preferences live in `~/.lox/tui.yaml`:

```yaml
theme: night          # night | day | mono | neon
icons: plain          # plain | nerd
motion: true          # animated energy flow, pulses
mouse: true
transparent: false    # keep the terminal background
roles:                # energy role overrides: meter name or UUID → grid | pv | battery | load
  Balcony PV: pv
```

UI state (pins, mutes, palette history, last screen) is kept per context in `tui-state.yaml`.

### Keys

`?` shows the keys for where you are; every pane's bottom border shows the ones in reach. `:` (or `Ctrl-K`) opens
the palette: fuzzy go-to for rooms, controls and scenes, plus any `lox` command.

<!-- keys:start (generated from src/tui/keymap.rs) -->
| Global | Action |
|---|---|
| `1` | Home |
| `2` | Rooms |
| `3` | Events |
| `4` | Energy |
| `5` | System |
| `6` | Sites |
| `⇥` | next pane |
| `⇤` | previous pane |
| `h` `←` | focus pane left |
| `l` `→` | focus pane right |
| `j` `↓` | move down |
| `k` `↑` | move up |
| `g` `Home` | first |
| `G` `End` | last |
| `^d` `PageDown` | half page down |
| `^u` `PageUp` | half page up |
| `[` | previous sub-view |
| `]` | next sub-view |
| `/` | filter |
| `:` `^k` | palette: go to / run |
| `?` | help |
| `C` | switch context |
| `!` | message log |
| `p` | pause live updates |
| `^r` | refresh structure + reconnect |
| `q` `^c` | quit |
| `Esc` | back |

| Selected item | Action |
|---|---|
| `␣` | primary action |
| `+` | step up |
| `-` | step down |
| `<` | minimum |
| `>` | maximum |
| `=` | set exact value |
| `s` | stop |
| `m` | mode / mood |
| `a` | all actions |
| `⏎` | inspect / open |
| `w` | wiring |
| `*` | pin to Home |
| `e` | events of this item |
| `c` | history chart (timeframes) |
| `y` | copy lox command |
| `Y` | copy UUID |
| `v` | mark |
| `V` | mark all visible |

| Rooms | Action |
|---|---|
| `b` | group by room · category · type |
| `f` | facets: room, type, state… |
| `o` | sort rooms: name · activity · temperature |

| Events | Action |
|---|---|
| `f` | facets: room, control, type… |
| `x` | mute this control |
| `X` | show muted / noisy |
| `F` | follow newest |

| System › Log | Action |
|---|---|
| `⏎` | full line |
| `n` | next match |
| `N` | previous match |

| System › Config | Action |
|---|---|
| `⏎` | browse the snapshot (diff pane: the full line) |
| `m` | mark: compare another commit with this one |
| `w` | wiring of the diff line's block at that snapshot |
| `n` | next change |
| `N` | previous change |
| `P` | pull: download the newest backup, commit it to git (read-only) |

| System › Update | Action |
|---|---|
| `R` | reboot (typed confirmation) |
| `U` | install update (typed confirmation) |

<!-- keys:end -->

---

## Streaming

```bash
lox stream                             # stream all state changes via WebSocket (NDJSON with --json)
lox stream --room "Kitchen"            # filter by room
lox stream --type LightControllerV2    # filter by control type
lox stream --control "Kitchen Light"   # filter by control name
lox stream --initial                   # include initial state snapshot
lox stream --json                      # output as NDJSON (one JSON object per line)
```

---

## OpenTelemetry Export

Pushes metrics, logs, and traces to any OTLP-compatible backend (Dynatrace, Datadog, Grafana Cloud, etc.).

**Metrics**: control state gauges, system diagnostics (CPU, heap, tasks), network counters (CAN/LAN), weather data.
**Logs**: state change events, text-state messages, Miniserver system log (`def.log`).
**Traces**: synthetic automation traces — correlates autopilot rule fires with temporally-close state changes.

```bash
# Continuous daemon — push metrics + logs + traces via OTLP every 30s
lox otel serve --endpoint http://localhost:4318 --interval 30s

# With auth header (Dynatrace, Datadog, etc.)
lox otel serve --endpoint https://otlp.example.com:4318 \
  --header "Authorization=Bearer xxx" --interval 1m

# Filter by room or control type
lox otel serve --endpoint ... --room "Kitchen" --type LightControllerV2

# Metrics only (disable logs and traces)
lox otel serve --endpoint ... --no-logs --no-traces

# Metrics + logs only (disable traces)
lox otel serve --endpoint ... --no-traces

# One-shot push (for cron jobs)
lox otel push --endpoint http://localhost:4318

# One-shot push, metrics only
lox otel push --endpoint http://localhost:4318 --no-logs
```

---

## Context Management

Manage multiple Miniserver connections, similar to `kubectl config use-context`:

```bash
lox ctx add home --host https://192.168.1.100 --user admin --pass secret
lox ctx add office --host https://10.0.0.50 --user admin --pass secret
lox ctx use home                    # switch active context
lox ctx home                        # shortcut for `lox ctx use home`
lox ctx list                        # list contexts (* = active)
lox ctx current                     # show active context
lox ctx remove office               # remove a context
lox ctx rename home house           # rename a context
```

### One-off context override

```bash
lox --ctx office status             # run against 'office' without switching
lox --ctx home ls -o json           # query 'home' context
```

### Project-local config

```bash
lox ctx init                        # create .lox/ in current directory
lox ctx init --host ... --user ... --pass ...  # with connection details
```

Project-local `.lox/config.yaml` is auto-discovered by walking up from cwd (like `.git`). Secrets are excluded via `.lox/.gitignore`.

### Migration

```bash
lox ctx migrate                     # convert flat config to 'default' context
```

Existing flat `~/.lox/config.yaml` files continue to work unchanged. Multi-context format:

```yaml
active_context: home
contexts:
  home:
    host: https://192.168.1.100
    user: admin
    pass: secret
  office:
    host: https://10.0.0.50
    user: admin
    pass: secret
```

### Config resolution order

1. `LOX_CONFIG` env var (absolute priority)
2. Project-local `.lox/config.yaml` (walk up from cwd)
3. Global `~/.lox/config.yaml` (flat or multi-context)
4. `--ctx` flag overrides context selection within global config

---

## Firmware Update

```bash
lox update check                       # check for updates
lox update install --yes               # install update
lox reboot --yes                       # reboot Miniserver
```

---

## Shell Completions

```bash
lox completions bash                   # generate bash completions
lox completions zsh                    # generate zsh completions
lox completions fish                   # generate fish completions

# Install (bash):
lox completions bash > /etc/bash_completion.d/lox

# Install (zsh):
lox completions zsh > ~/.zfunc/_lox

# Install (fish):
lox completions fish > ~/.config/fish/completions/lox.fish
```
