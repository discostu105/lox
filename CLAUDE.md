# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.
See [AGENTS.md](AGENTS.md) for issue tracking workflow (beads/bd).

## Commands

```bash
# Build
cargo build           # debug
cargo build --release # production binary (~4MB)

# Run
cargo run -- <args>   # e.g. cargo run -- ls --json
./target/release/lox <args>

# Check / Lint
cargo check
cargo clippy

# Test
cargo test
cargo test <test_name>  # run a single test
```

## Architecture

Single Rust binary with two execution modes: **synchronous CLI commands** (reqwest blocking) and an **async daemon** (tokio + WebSocket).

### Source files

| File | Purpose |
|------|---------|
| `src/main.rs` | All CLI commands, clap argument parsing, `LoxClient` (HTTP), control resolution, scenes, automation list |
| `src/config.rs` | `Config` struct — loads/saves `~/.lox/config.yaml`, provides `Config::dir()` path |
| `src/daemon.rs` | Automation daemon: `Automations`/`Rule` structs, `StateRegistry`, rule engine (`eval_rule`/`fire_rule`), WebSocket and polling loop variants |
| `src/ws.rs` | `LoxWsClient` — async WebSocket connection with token auth (RSA+AES handshake), binary protocol parser for Loxone `ValueEventTable` messages |
| `src/token.rs` | Token auth flow: RSA key exchange, AES-encrypted credential exchange, token storage at `~/.lox/token.json` |

### Key design points

**Control resolution** (`LoxClient::resolve_with_room`): Names are matched against the structure cache using fuzzy substring matching. Resolution order: alias → exact UUID → bracket room qualifier (`"Name [Room]"`) → `--room` flag → fuzzy substring. Ambiguous matches are an error.

**Structure cache**: `LoxApp3.json` (~150KB) is cached at `~/.lox/cache/structure.json` with a 24h TTL. All commands that need control UUIDs load this cache first; `lox cache refresh` forces a re-fetch.

**Mixed sync/async**: The CLI commands use `reqwest::blocking`; the daemon uses `tokio` + `reqwest` async. `main.rs` uses `#[tokio::main]` and calls blocking reqwest from within it — this works because the blocking client spawns its own thread pool.

**WebSocket protocol**: Loxone uses a binary framing format (8-byte header: magic `0x03`, type byte, flags, reserved, u32le length). Type `0x02` = `ValueEventTable`: repeated 24-byte records of UUID (binary, little-endian parts) + f64le value. UUID binary parsing order: u32le, u16le, u16le, then 8 raw bytes.

**Token auth** (used by daemon/WS): RSA public key fetched from Miniserver → encrypt AES session key → send encrypted credentials → receive token. Token stored in `~/.lox/token.json`, valid ~20 days.

### User data layout

```
~/.lox/
  config.yaml          # host, user, pass, serial, aliases
  token.json           # optional token auth
  cache/
    structure.json     # LoxApp3.json cache (24h TTL)
  scenes/*.yaml        # multi-step scene definitions
  automations.yaml     # daemon rule definitions
```

### Loxone HTTP API used

```
GET /data/LoxApp3.json              → full structure (controls, rooms)
GET /jdev/sps/io/{uuid}/{cmd}       → send command → JSON response
GET /dev/sps/io/{uuid}/all          → XML: all state outputs for a control
GET /dev/sps/io/{name}/state        → input state by name
GET /dev/sys/heap, /dev/sps/state, /dev/cfg/version, /data/status  → status info
WSS /ws/rfc6455                     → WebSocket for real-time state push
```

TLS: `danger_accept_invalid_certs(true)` is used throughout (Miniserver uses self-signed certs). When `serial` is set in config, `Config::tls_host()` generates the DynDNS hostname for valid cert matching.

## Agent Workflow

### Non-Interactive Shell Commands

**ALWAYS use non-interactive flags** with file operations to avoid hanging on confirmation prompts.

Shell commands like `cp`, `mv`, and `rm` may be aliased to include `-i` (interactive) mode on some systems, causing the agent to hang indefinitely waiting for y/n input.

```bash
cp -f source dest    # NOT: cp source dest
mv -f source dest    # NOT: mv source dest
rm -f file           # NOT: rm file
rm -rf directory     # NOT: rm -r directory
```

Other commands that may prompt: `scp`/`ssh` — use `-o BatchMode=yes`; `apt-get` — use `-y`; `brew` — set `HOMEBREW_NO_AUTO_UPDATE=1`.

### Session Completion

**Work is NOT complete until `git push` succeeds.**

1. File issues for remaining work (`bd create`)
2. Run quality gates — `cargo test`, `cargo clippy`
3. Update issue status — close finished work
4. Push:
   ```bash
   git pull --rebase
   bd backup        # export issues to .beads/backup/ and commit
   git push
   git status       # must show "up to date with origin"
   ```
5. Verify all changes committed and pushed before ending the session
