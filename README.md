# lox — Loxone Miniserver CLI

Fast, scriptable CLI for the Loxone Miniserver. Single binary, no runtime.

## Install

```bash
git clone https://github.com/discostu105/lox
cd lox
cargo build --release
cp target/release/lox ~/.local/bin/
```

## Setup

```bash
# Basic
lox config set --host https://192.168.1.100 --user USER --pass PASS

# With serial for proper TLS (avoids cert warning)
lox config set --host https://192.168.1.100 --user USER --pass PASS --serial YOUR_SERIAL
```

Config: `~/.lox/config.yaml`

## Commands

```bash
# System
lox status                              # Miniserver health (firmware, PLC, memory, connections)

# Discovery
lox rooms
lox ls
lox ls --room "Wohnzimmer"
lox ls --type Jalousie

# Read state
lox get "Fenster Ost Groß"             # value + position for blinds, outputs for lights

# Control
lox on  "Licht Wohnzimmer Zentral"
lox off "Licht Wohnzimmer Zentral"
lox pulse "Tür öffnen"
lox send "Lichtsteuerung" 778          # raw Loxone command

# Blinds / Jalousie
lox blind "Fenster Ost Groß" up        # PulseUp
lox blind "Fenster Ost Groß" down      # PulseDown
lox blind "Fenster Ost Groß" stop
lox blind "Fenster Ost Groß" shade     # automatic shading
lox blind "Fenster Ost Groß" full-up
lox blind "Fenster Ost Groß" full-down

# Scenes
lox scene list
lox scene new abend
lox run abend

# Watch state (polling loop)
lox watch "Lichtsteuerung" --interval 2

# Shell scripting — exit 0 = match, exit 1 = no match
lox if "Fenster Ost Groß" eq 0         # closed?
lox if "Temperatur"       gt 22.5      # hot?
lox if "Zustand"          contains "an"

# JSON output (pipe to jq)
lox status --json
lox ls --json | jq '.[] | select(.type == "Jalousie") | .name'
lox get "Fenster Ost Groß" --json
```

## Scenes (`~/.lox/scenes/*.yaml`)

```yaml
name: "Abend Wohnzimmer"
description: "Gemütliches Abendlicht"
steps:
  - control: "Lichtsteuerung"
    cmd: "on"
  - control: "Mitte Wohnzimmer Licht"
    cmd: "off"
    delay_ms: 200
```

## Shell Automation Examples

```bash
# Cron: alle Jalousien runter bei Sonnenuntergang
0 20 * * * lox blind "Beschattung Zentral EG" full-down

# Conditional in script
if lox if "Schalter Küchenstrom Links" eq 1; then
  lox off "Schalter Küchenstrom Links"
fi

# JSON + jq pipeline
lox ls --json | jq '.[] | select(.type == "Jalousie") | .name' | \
  xargs -I{} lox blind "{}" shade
```

## Operators for `lox if`

| Op | Meaning |
|----|---------|
| `eq` / `==` | equal |
| `ne` / `!=` | not equal |
| `gt` / `>` | greater than (numeric) |
| `lt` / `<` | less than (numeric) |
| `ge` / `>=` | ≥ (numeric) |
| `le` / `<=` | ≤ (numeric) |
| `contains` | substring match |
