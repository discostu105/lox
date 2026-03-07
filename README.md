# lox — Loxone Miniserver CLI

A fast CLI for controlling your Loxone Miniserver from the terminal.

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

Config is stored at `~/.lox/config.yaml`.

## Usage

```bash
# List all controls
lox ls

# Filter by room or type
lox ls --room "Wohnzimmer"
lox ls --type LightControllerV2

# List rooms
lox rooms

# Control by name (case-insensitive substring match)
lox on "Wohnzimmer Zentral"
lox off "Wohnzimmer Zentral"

# Raw command
lox send "Lichtsteuerung" pulse

# JSON output
lox ls --json
lox on "Licht" --json
```

## Name Resolution

Pass a name substring — if it's unique, it gets used. If ambiguous, you'll see the matches. Alternatively, pass a UUID directly.
