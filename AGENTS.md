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
| `src/config.rs` | `Config` + `GlobalConfig` — loads/saves config (flat or multi-context), context resolution, project-local `.lox/` discovery |
| `src/commands/ctx.rs` | `lox ctx` commands — add/use/list/remove/rename/init/migrate contexts |
| `src/gitops.rs` | Git-based config versioning — init, pull (FTP→LoxCC→diff→commit), log, restore workflows |
| `src/statv2.rs` | Statistics V2 (`statisticV2` groups, `getStatistic` paths, record parsing) — meters of the energy flow monitor |
| `src/scene.rs` | Scene loading/listing from `~/.lox/scenes/*.yaml` |
| `src/ws.rs` | `LoxWsClient` — async WebSocket connection used by token auth (RSA+AES key exchange handshake) |
| `src/token.rs` | Token auth flow: RSA key exchange, AES-encrypted credential exchange, token storage, HMAC token hashing |
| `src/actions.rs` | Shared action layer: per-control-type command mapping and risk levels (used by the CLI and the TUI) |
| `src/mcp/` | `lox mcp`: MCP server on rmcp (MCP 2026-07-28 + older) — CLI/runtime (`mod.rs`), `#[tool]` definitions, risky-action confirmation via MRTR/elicitation (`server.rs`), blocking Miniserver ops + output schemas (`ops.rs`), rmcp client/server protocol tests (`tests.rs`). Design: `docs/design-mcp.md` |
| `src/logic.rs` | Adapter over lxir (always built): config program blocks, wires with hardware refs folded, neighborhood/trace (`WiringView`), lint, block history, the logic diff used by `config diff`, pull commit messages and the TUI |
| `src/snapshot.rs` | Where a `.Loxone` comes from: config repo commits (by hash, `v273`, save date, `<hash>^`), working copy, cache, FTP download, files; block history across commits |
| `src/commands/config_insight.rs` | Read-only `lox config diff/show/wiring/find/lint/history` |
| `src/tui/` | `lox tui` (feature `tui`): Elm-style `update()` over `App`, pure rendering (`ui/`, `widgets/`), live/demo backends (`exec.rs`), terminal runtime (`run.rs`), journey tests (`tests.rs`). Design: `docs/design-tui.md` |

### Key design points

**Control resolution** (`LoxClient::resolve_with_room`): Names are matched against the structure cache using fuzzy substring matching. Resolution order: alias → exact UUID → bracket room qualifier (`"Name [Room]"`) → `--room` flag → fuzzy substring; among several matches a single exact (case-insensitive) name wins. Ambiguous matches are an error.

**Multi-context config**: `~/.lox/config.yaml` supports both flat (single-Miniserver, backward compatible) and multi-context format. `Config::load()` resolution: `LOX_CONFIG` env → project-local `.lox/` (walk up from cwd) → global config → `--ctx` flag override. Each context gets isolated data under `~/.lox/contexts/<name>/`.

**Structure cache**: `LoxApp3.json` (~150KB) is cached per-context (e.g. `~/.lox/contexts/<name>/cache/structure.json`) with a 24h TTL. All commands that need control UUIDs load this cache first; `lox cache refresh` forces a re-fetch.

**Mixed sync/async**: The CLI commands use `reqwest::blocking`. `main.rs` uses `#[tokio::main]` because `lox token fetch` needs async (WebSocket for the key exchange). The blocking reqwest client spawns its own thread pool so both modes coexist.

**Token auth** (`src/token.rs`): RSA public key fetched over HTTP → RSA-encrypted AES session key sent via WS `keyexchange` → `getkey2` → HMAC credential hash (SHA-1 or SHA-256, following the `hashAlg` the Miniserver reports) → `gettoken`. Token stored per-context (e.g. `~/.lox/contexts/<name>/token.json`, mode 0600), valid ~20 days. WebSocket sessions (`lox stream`, `otel`, TUI) reuse the stored token via `authwithtoken` and only fall back to `gettoken` when it is missing, expired or rejected. HTTP requests still use Basic auth.

### User data layout

```
~/.lox/
  config.yaml          # flat (single-Miniserver) or multi-context format
  contexts/            # per-context data (multi-context mode)
    <name>/
      cache/
        structure.json # LoxApp3.json cache (24h TTL)
      token.json       # optional token auth
      scenes/*.yaml    # multi-step scene definitions
  # Legacy flat mode (backward compatible):
  cache/
    structure.json     # LoxApp3.json cache (24h TTL)
  token.json           # optional token auth
  scenes/*.yaml        # multi-step scene definitions
```

Project-local config (auto-discovered by walking up from cwd):
```
project/
  .lox/
    config.yaml        # connection settings
    .gitignore         # excludes secrets and cache
    cache/
    scenes/
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
GET /dev/sps/getStatistic/{uuid}/raw/{fromUtc}/{toUtc}/all/{group}[/{output}] → Statistics V2 (u32 unix ts + f64[])
GET /data/weatheru.bin              → binary weather data (108-byte entries)
GET /dev/fsget/{path}               → filesystem access
GET /jdev/sys/checktoken, refreshtoken, killtoken                  → token management
GET /jdev/sys/reboot                → reboot Miniserver
GET /jdev/sys/updatetolatestrelease → firmware update
WSS /ws/rfc6455                     → WebSocket for token auth key exchange
UDP :7070                           → Miniserver discovery (broadcast)
HTTP :7091/zone/{n}/{cmd}           → Music server API (unofficial)
```

TLS: certificate verification is off unless `verify_ssl: true` is set (Miniservers use self-signed certs); the WebSocket connector (`src/ws.rs`) always skips verification. `serial` is only used for the OTel `device.id` and the gitops directory name; there is no DynDNS hostname generation.

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

1. File GitHub issues for remaining work (`gh issue create`)
2. Run quality gates — `cargo test`, `cargo clippy`
3. Close or update the GitHub issues the work resolves
4. Push:
   ```bash
   git pull --rebase
   git push
   git status       # must show "up to date with origin"
   ```
5. Verify all changes committed and pushed before ending the session

## Issue Tracking

Issues live in GitHub Issues (`gh issue list`, `gh issue create`). Do not keep TODO lists in markdown files.
