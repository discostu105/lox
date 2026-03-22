# Agent Instructions

This file provides guidance to Claude Code and other AI agents working with this repository.
See [API_DESIGN_GUIDELINES.md](API_DESIGN_GUIDELINES.md) for CLI command naming, flag conventions, and output format rules.

## Commands

```bash
# Build
cargo build           # debug
cargo build --release # production binary (~4MB)

# Run
cargo run -- <args>   # e.g. cargo run -- ls -o json
./target/release/lox <args>

# Check / Lint
cargo check
cargo clippy

# Test
cargo test
cargo test <test_name>  # run a single test
```

## Pre-push CI Checklist

**ALWAYS run these four checks before committing/pushing.** They mirror `.github/workflows/ci.yml` (ubuntu + macos):

```bash
cargo fmt --check            # formatting
cargo clippy -- -D warnings  # lints (warnings are errors)
cargo build --release        # release build
cargo test                   # all tests
```

Do not push until all four pass. Fix issues and re-run before committing.

## Architecture

Single Rust binary. CLI commands use reqwest blocking; token auth uses tokio + WebSocket for the RSA/AES key exchange.

### Source files

| File | Purpose |
|------|---------|
| `src/main.rs` | All CLI commands (30+), clap argument parsing, helper functions (RGB→HSV, weather/stats binary parsing) |
| `src/client.rs` | `LoxClient` (HTTP) — control resolution, structure cache, categories, global states, operating modes |
| `src/config.rs` | `Config` struct — loads/saves `~/.lox/config.yaml`, provides `Config::dir()` path |
| `src/gitops.rs` | Git-based config versioning — init, pull (FTP→LoxCC→diff→commit), log, restore workflows |
| `src/scene.rs` | Scene loading/listing from `~/.lox/scenes/*.yaml` |
| `src/ws.rs` | `LoxWsClient` — async WebSocket connection used by token auth (RSA+AES key exchange handshake) |
| `src/token.rs` | Token auth flow: RSA key exchange, AES-encrypted credential exchange, token storage, HMAC token hashing |

### Key design points

**Control resolution** (`LoxClient::resolve_with_room`): Names are matched against the structure cache using fuzzy substring matching. Resolution order: alias → exact UUID → bracket room qualifier (`"Name [Room]"`) → `--room` flag → fuzzy substring. Ambiguous matches are an error.

**Structure cache**: `LoxApp3.json` (~150KB) is cached at `~/.lox/cache/structure.json` with a 24h TTL. All commands that need control UUIDs load this cache first; `lox cache refresh` forces a re-fetch.

**Mixed sync/async**: The CLI commands use `reqwest::blocking`. `main.rs` uses `#[tokio::main]` because `lox token fetch` needs async (WebSocket for the key exchange). The blocking reqwest client spawns its own thread pool so both modes coexist.

**Token auth**: RSA public key fetched from Miniserver → encrypt AES session key → send encrypted credentials via WebSocket → receive token. Token stored in `~/.lox/token.json`, valid ~20 days.

### User data layout

```
~/.lox/
  config.yaml          # host, user, pass, serial, aliases
  token.json           # optional token auth
  cache/
    structure.json     # LoxApp3.json cache (24h TTL)
  scenes/*.yaml        # multi-step scene definitions
```

### Loxone HTTP API used

```
GET /data/LoxApp3.json              → full structure (controls, rooms, categories, globalStates)
GET /jdev/sps/io/{uuid}/{cmd}       → send command → JSON response
GET /dev/sps/io/{uuid}/all          → XML: all state outputs for a control
GET /dev/sps/io/{name}/state        → input state by name
GET /dev/sys/heap, /dev/sps/state, /dev/cfg/version, /data/status  → status info
GET /jdev/sys/lastcpu, numtasks, contextswitches, sdtest           → diagnostics
GET /jdev/sys/ints, comints, contextswitchesi                      → additional diagnostics
GET /jdev/cfg/ip, mac, mask, gateway, dns1, dns2, dhcp, ntp       → network config
GET /jdev/bus/packetssent, packetsreceived, ..., parityerrors      → CAN bus stats
GET /jdev/lan/txp, txe, txc, txu, rxp, rxo, eof, exh, nob        → LAN stats
GET /jdev/sys/date, /jdev/sys/time                                 → system clock
GET /jdev/sps/LoxAPPversion3        → structure file version check
GET /binstatisticdata/{uuid}/{period} → binary statistics (u32 ts + f64[] values)
GET /data/weatheru.bin              → binary weather data (108-byte entries)
GET /dev/fsget/{path}               → filesystem access
GET /jdev/sys/checktoken, refreshtoken, killtoken                  → token management
GET /jdev/sys/reboot                → reboot Miniserver
GET /jdev/sys/updatetolatestrelease → firmware update
WSS /ws/rfc6455                     → WebSocket for token auth key exchange
UDP :7070                           → Miniserver discovery (broadcast)
HTTP :7091/zone/{n}/{cmd}           → Music server API (unofficial)
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

<!-- BEGIN BEADS INTEGRATION -->
## Issue Tracking with bd (beads)

**IMPORTANT**: This project uses **bd (beads)** for ALL issue tracking. Do NOT use markdown TODOs, task lists, or other tracking methods.

### Why bd?

- Dependency-aware: Track blockers and relationships between issues
- Version-controlled: Built on Dolt with cell-level merge
- Agent-optimized: JSON output, ready work detection, discovered-from links
- Prevents duplicate tracking systems and confusion

### Quick Start

**Check for ready work:**

```bash
bd ready --json
```

**Create new issues:**

```bash
bd create "Issue title" --description="Detailed context" -t bug|feature|task -p 0-4 --json
bd create "Issue title" --description="What this issue is about" -p 1 --deps discovered-from:bd-123 --json
```

**Claim and update:**

```bash
bd update <id> --claim --json
bd update bd-42 --priority 1 --json
```

**Complete work:**

```bash
bd close bd-42 --reason "Completed" --json
```

### Issue Types

- `bug` - Something broken
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature with subtasks
- `chore` - Maintenance (dependencies, tooling)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (default, nice-to-have)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Workflow for AI Agents

1. **Check ready work**: `bd ready` shows unblocked issues
2. **Claim your task atomically**: `bd update <id> --claim`
3. **Work on it**: Implement, test, document
4. **Discover new work?** Create linked issue:
   - `bd create "Found bug" --description="Details about what was found" -p 1 --deps discovered-from:<parent-id>`
5. **Complete**: `bd close <id> --reason "Done"`

### Sync

Issues are committed to git as JSONL in `.beads/backup/`. `bd backup` exports and commits; runs automatically on `git push` via the pre-push hook.

### Important Rules

- Use bd for ALL task tracking
- Always use `--json` flag for programmatic use
- Link discovered work with `discovered-from` dependencies
- Check `bd ready` before asking "what should I work on?"
- Do NOT create markdown TODO lists
- Do NOT use external issue trackers
- Do NOT duplicate tracking systems

<!-- END BEADS INTEGRATION -->
