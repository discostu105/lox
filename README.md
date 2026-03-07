# lox — Loxone Miniserver CLI

Fast, scriptable CLI for controlling your Loxone Miniserver.

## Install

```bash
git clone https://github.com/discostu105/lox
cd lox
cargo build --release
cp target/release/lox ~/.local/bin/
```

## Setup

```bash
lox config set --host https://loxone.int.neumueller.net --user USER --pass PASS
```

Config: `~/.lox/config.yaml`

## Commands

```bash
# Discovery
lox rooms
lox ls
lox ls --room "Wohnzimmer"
lox ls --type LightControllerV2

# Control
lox on  "Licht Wohnzimmer Zentral"
lox off "Licht Wohnzimmer Zentral"
lox pulse "Tür öffnen"
lox send "Lichtsteuerung" 778         # raw Loxone command

# State
lox get "Lichtsteuerung"

# Watch (live polling, Ctrl+C to stop)
lox watch "Lichtsteuerung" --interval 2

# Scripting — exit code 0=match, 1=no match
lox if "Lichtsteuerung" eq 1 && echo "Licht an"
lox if "Temperatur"     gt 22.5       # numeric comparison

# Scenes
lox scene list
lox scene new abend
lox scene show abend
lox run abend
```

## Scenes

Scenes live in `~/.lox/scenes/*.yaml`:

```yaml
name: "Abend Wohnzimmer"
description: "Gemütliches Abendlicht"
steps:
  - control: "Lichtsteuerung"   # name or UUID
    cmd: "on"
  - control: "Mitte Wohnzimmer Licht"
    cmd: "off"
    delay_ms: 200               # optional delay after step
```

Run with: `lox run abend`

## Shell Automation

```bash
# Cron: Licht aus um 23:00
0 23 * * * lox run alles_aus

# Conditional
lox if "Schalter Küchenstrom Links" eq 1 && lox off "Schalter Küchenstrom Links"

# JSON output for jq
lox ls --json | jq '.[] | select(.type == "LightControllerV2") | .name'
```

## Operators for `lox if`

| Op | Meaning |
|----|---------|
| `eq` / `==` | equal |
| `ne` / `!=` | not equal |
| `gt` / `>` | greater than (numeric) |
| `lt` / `<` | less than (numeric) |
| `ge` / `>=` | greater or equal |
| `le` / `<=` | less or equal |
| `contains` | substring match |
