---
title: Inspection
layout: default
parent: Command Reference
nav_order: 2
---

# Inspection Commands
{: .no_toc }

## Table of contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## List controls

```bash
lox ls                                 # all controls
lox ls -t Jalousie                     # filter by type
lox ls -r "Wohnzimmer"                 # filter by room
lox ls -c "Beleuchtung"                # filter by category
lox ls -f                              # only favorites
lox ls --values                        # include live values (slower)
lox ls -o json | jq -r '.[].name'      # names, for scripts
```

Filters combine and match substrings, case-insensitively: `lox ls -t Jalousie -r OG`.

| Flag | Description |
|:-----|:------------|
| `-t`, `--type` | Filter by control type |
| `-r`, `--room` | Filter by room name |
| `-c`, `--cat` | Filter by category |
| `-f`, `--favorites` | Only show favorites |
| `--values` | Include live state values (requires one HTTP request per control) |

---

## Get control state

```bash
lox get "Licht Wohnzimmer"
lox get "Temperatur" --room "Schlafzimmer"
lox get "Temperatur [OG Kinderzimmer]"
```

Returns all state outputs for the control. To see *why* it has that value, follow its wiring in the config:
`lox config wiring "Licht Wohnzimmer" --trace up` (see [Config Versioning](/lox/guides/config-versioning#inspecting-snapshots)).

---

## Control info

```bash
lox info "Licht Wohnzimmer"
```

Shows detailed information: sub-controls, all state keys, moods (for light controllers), flags, and type metadata.

---

## Watch state changes

Poll a control's state and print changes:

```bash
lox watch "Temperatur"
lox watch "Temperatur" -i 5           # poll every 5 seconds
```

| Flag | Description |
|:-----|:------------|
| `-i`, `--interval` | Poll interval in seconds (default: 2) |

Press Ctrl+C to stop.

---

## Stream real-time changes

Stream state changes via WebSocket (more efficient than polling):

```bash
lox stream                             # stream all changes
lox stream --room "Kitchen"            # filter by room
lox stream --type LightControllerV2    # filter by type
lox stream --control "Kitchen Light"   # filter by control name
lox stream --initial                   # include initial state snapshot
lox stream -o json                     # NDJSON output
lox stream --reconnect                 # keep going across Miniserver reboots
```

| Flag | Description |
|:-----|:------------|
| `-t`, `--type` | Filter by control type |
| `-r`, `--room` | Filter by room |
| `-c`, `--control` | Filter by control name |
| `--initial` | Include initial state snapshot (with `--reconnect`: again after every reconnect) |
| `--reconnect` | Wait for the Miniserver after a reboot or dropped connection (backs off 1 s → 30 s, gives up after 30 min), then resume; the gap is reported on stderr and, with `-o json`, as `disconnected`/`reconnected` event lines |

```bash
# A change log of one room, one JSON object per line
lox stream -r Küche --reconnect -o json >> kueche.ndjson
```

---

## Conditional checks

Check a control's state value. Returns exit code 0 (true) or 1 (false):

```bash
lox if "Temperatur Aussen" gt 25
lox if "Schalter" eq 1 && lox on "Licht"
```

Operators: `eq`, `ne`, `gt`, `ge`, `lt`, `le`

Use `-r <room>` to disambiguate. Useful for scripting conditional automation:

```bash
# Close blinds if temperature exceeds 28
lox if "Temperatur Aussen" gt 28 && lox blind "Beschattung Sud" pos 70
```

---

## Rooms & Categories

```bash
lox rooms                             # list all rooms
lox categories                        # list all categories
lox globals                           # global states (operating mode, sunrise, etc.)
lox modes                             # operating modes
```

---

## Sensors

```bash
lox sensors                           # all sensor readings
lox sensors --type temperature        # temperature sensors only
lox sensors --type door-window        # door/window sensors
lox sensors --type motion             # motion sensors
lox sensors --type smoke              # smoke detectors
lox sensors -r "Wohnzimmer"          # filter by room
```

---

## Energy

```bash
lox energy                            # show energy meter readings
lox energy -r "Keller"               # filter by room
```

---

## Weather

```bash
lox weather                           # current weather data
lox weather --forecast                # 7-day forecast
```

---

## Statistics & History

```bash
lox stats                             # controls with statistics enabled (legacy or Statistics V2)
lox history "Temperatur" --month 2025-01
lox history "Temperatur" --day 2025-01-15
lox history "Temperatur" -o csv       # CSV output for spreadsheets
```

---

## Autopilot rules

```bash
lox autopilot ls                      # rules in the program file: active, JSON valid
lox autopilot ls --file backup.Loxone # same sources as `lox config` (--at, --file, --download)
lox autopilot state "Rule Name"       # when a rule last ran (the Miniserver's rule history)
lox autopilot changed                 # when the rules were last changed on the Miniserver
lox autopilot changed -o json         # {"changed": <unix seconds>, "changed_at": …, "uuid": …}
```

Rules created or edited in the app can live only in the Miniserver's *custom changes*, not in the
program file: `lox autopilot ls` and a config download don't see them, and uploading a config discards
them (the Miniserver logs `Remove old custom changes`). Before an upload, check that
`lox autopilot changed` is older than the download the upload is based on.
