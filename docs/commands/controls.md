---
title: Controls
layout: default
parent: Command Reference
nav_order: 1
---

# Control Commands
{: .no_toc }

## Table of contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## On / Off

Turn controls on or off by name, UUID, or alias. Add `--dry-run` to any control command to see what would be sent.

```bash
lox on "Licht Wohnzimmer"
lox off "Licht Wohnzimmer"
```

Apply to all controls in a room:

```bash
lox on --all-in-room "Wohnzimmer"
lox off --all-in-room "Schlafzimmer"
```

| Flag | Description |
|:-----|:------------|
| `-r`, `--room` | Disambiguate by room name |
| `--all-in-room` | Apply to all controls in the specified room |

---

## Lights

### Moods

```bash
lox light mood "Licht Wohnzimmer" plus      # next mood
lox light mood "Licht Wohnzimmer" minus     # previous mood
lox light mood "Licht Wohnzimmer" off       # all off (mood 778)
lox light mood "Licht Wohnzimmer" 704       # set by mood ID
lox light moods "Licht Wohnzimmer"          # list available moods (via WebSocket)
lox light moods "Licht" -o json             # moods as JSON with control metadata
```

### Dimmer

```bash
lox light dim "Stehlampe" 75               # set dimmer 0-100% (Dimmer controls; use a mood for a lighting controller)
```

### Color

```bash
lox light color "LED Strip" "#FF0000"              # hex RGB
lox light color "LED Strip" "hsv(120,100,100)"     # HSV
lox light color "LED Strip" "temp(80,2700)"        # white: brightness %, color temperature K
```

{: .tip }
`light dim` is for `Dimmer` controls. A lighting controller (`LightControllerV2`) is driven by moods: list them with
`lox light moods`, then set one by ID.

---

## Blinds

```bash
lox blind "Beschattung Süd" up
lox blind "Beschattung Süd" pos 50          # position: 0 = open, 100 = closed
lox blind "Beschattung Süd" 50              # same
lox blind "Beschattung Süd" shade           # automatic shading
lox blind "Beschattung Süd" shade 40        # slat angle
lox blind "Beschattung Süd" stop
```

| Action | Aliases | Sends |
|:-------|:--------|:------|
| `up` | `open`, `full-up` | `FullUp`: all the way up |
| `down` | `close`, `full-down` | `FullDown`: all the way down |
| `stop` | | `off` |
| `pos <0-100>` | `<0-100>` | `manualPosition/<n>` (0 = open, 100 = closed) |
| `shade` | `auto` | `AutomaticDown`: automatic shading |
| `shade <0-100>` | | `manualLamella/<n>`: slat angle (0 = horizontal, 100 = vertical) |

`up` and `down` run to the end position. For short jog steps send the raw commands:
`lox send "Beschattung Süd" up`, then `lox send "Beschattung Süd" UpOff`.

---

## Gates

```bash
lox gate "Garagentor" open
lox gate "Garagentor" close
lox gate "Garagentor" stop
```

---

## Thermostats

```bash
lox thermostat "Heizung" temp 22.5              # set comfort temperature
lox thermostat "Heizung" mode auto               # auto | manual | auto-heat | auto-cool | manual-heat | manual-cool
lox thermostat "Heizung" override 24 120         # override 24 °C for 120 minutes (default 60)
lox thermostat "Heizung"                         # show current state
```

---

## Alarm

```bash
lox alarm "Alarmanlage" arm
lox alarm "Alarmanlage" arm --no-motion    # arm without motion detection
lox alarm "Alarmanlage" arm-home
lox alarm "Alarmanlage" disarm
lox alarm "Alarmanlage" quit               # acknowledge / silence
```

| Flag | Description |
|:-----|:------------|
| `--no-motion` | Arm without motion detection |
| `--code` | PIN code for arm/disarm |
| `-r`, `--room` | Disambiguate by room |

---

## Door locks

```bash
lox door "Haustür" lock
lox door "Haustür" unlock
lox door "Haustür" open        # open (e.g. electric strike)
```

{: .note }
Doors, gates and the alarm act immediately on the command line. `lox tui` and the
[MCP server](/lox/guides/ai-agents#safety) ask for confirmation first.

---

## Intercom

```bash
lox intercom "Türklingel" answer
lox intercom "Türklingel" hangup
lox intercom "Türklingel" open
```

---

## EV Charger

```bash
lox charger "Wallbox" start
lox charger "Wallbox" stop
lox charger "Wallbox" pause
lox charger "Wallbox" start --limit 20     # stop after 20 kWh
```

---

## Analog / Virtual inputs

```bash
lox input set "Sollwert Heizung" 21.5
lox input set "Sollwert Heizung" 21.5 -r "Wohnzimmer"
lox input pulse "Taster"                      # momentary pulse, e.g. a push-button
```

---

## Music server

```bash
lox music play                    # play zone 1
lox music pause 2                 # pause zone 2
lox music stop
lox music volume 50               # volume 0-100
lox music next
lox music prev
lox music mute
lox music unmute
```

All music commands accept an optional zone number (default: 1): `lox music volume 30 2` sets zone 2. They use the
Music Server's unofficial API on port 7091 of the host.

---

## Lock / Unlock controls

Administrative lock to prevent changes (admin user, firmware 11.3.2.11+):

```bash
lox lock "Heizung" --reason "Wartung"
lox unlock "Heizung"
```

---

## Raw commands

Send any command the Miniserver understands directly to a control (name, alias or UUID):

```bash
lox send "Beschattung Süd" UpOff
lox send <uuid> <command> --secured <hash>   # secured command (visualization password hash)
```

---

## Scenes

Run multi-step automation scenes:

```bash
lox run abend                    # run a scene
lox run abend --dry-run          # preview without executing
```

See the [Scenes guide](/lox/guides/scenes) for defining scenes.
