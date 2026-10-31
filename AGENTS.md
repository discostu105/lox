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
cargo test -p loxone-config      # only the loxone-config crate
cargo test -p loxone-client
```

The commands above cover the whole workspace (`default-members`): the `lox` package at the root and the
library crates `crates/loxone-config` and `crates/loxone-client`. `cargo run` runs `lox` (`default-run`); `cargo run --bin loxl -- help` runs loxl, the Lox Logic CLI.

## Pre-push CI Checklist

**ALWAYS run these four checks before committing/pushing.** They mirror `.github/workflows/ci.yml` (ubuntu + macos):

```bash
cargo fmt --check            # formatting
cargo clippy -- -D warnings  # lints (warnings are errors)
cargo build --release        # release build
cargo test                   # all tests
```

Do not push until all four pass. Fix issues and re-run before committing.

## Releases

Every push to main that passes CI is released automatically (`.github/workflows/auto-release.yml`): the version is
bumped, the changelog entries are written into CHANGELOG.md under it, `Release vX.Y.Z` is committed and tagged,
and `release.yml` builds the binaries, the Docker image and the Homebrew formula.

**Changelog: never edit CHANGELOG.md in a PR.** Add the entry as a new file `changelog.d/<branch>.<section>.md`
(section `added`, `changed`, `deprecated`, `removed`, `fixed` or `security`; content: the bullet as it should read
in the changelog, leading with the command in bold). New files never conflict; the release merges them into
CHANGELOG.md and deletes them. CI fails a PR that writes under `[Unreleased]`. A notable change also adds
`changelog.d/<branch>.summary.md`: a short lowercase tagline on the first line (the release title becomes
`vX.Y.Z — tagline`), then a sentence or two of intro. The GitHub release renders the sections with emoji
headings (`--notes vX.Y.Z` shows it). See [changelog.d/README.md](changelog.d/README.md).

- Bump: PR label `release:major`, `release:minor`, `release:patch` or `no-release` (in a direct push: the same
  words in brackets in the commit message, `[release:minor]`). Without a `release:*` label: minor when
  there is an `added` entry, otherwise patch. Changes to docs, CI or the formula alone are not released.
- Several merges in a row become one release (only the newest green commit of main releases).
- A Cargo.toml version above the last tag is released as it is. A hand-pushed `vX.Y.Z` tag still releases; a
  failed release reruns with *Actions › Release › Run workflow* and the tag.
- The library crates keep their own versions (`crates/loxone-config/Cargo.toml`, `crates/loxone-client/Cargo.toml`):
  bump one (and its requirement in the root Cargo.toml when the bump leaves the old one) and the next
  release publishes it to crates.io, loxone-config first — that needs the `CARGO_REGISTRY_TOKEN` repository secret.
  Without it, publish by hand: `cargo publish -p loxone-config`, then `cargo publish -p loxone-client`. A breaking
  change to a public API is a minor bump while the crate is 0.x.
- `python3 .github/scripts/prepare_release.py --dry-run` shows what the next release would be, with its notes.

## Architecture

Cargo workspace: the `lox` binary (root package) and two library crates that `lox` uses by path, both
published on crates.io:

- `crates/loxone-client`: talks to the Miniserver (HTTP, token auth, WebSocket stream, FTP, statistics).
- `crates/loxone-config`: the program file (`.Loxone`, backup ZIPs, the `.loxl` language). Never touches the network.

Policy stays in lox: contexts and config files, aliases, risk levels and confirmations (`actions.rs`), scenes,
the CLI, TUI and MCP server, output formatting. HTTP is reqwest blocking; the WebSocket parts are tokio.

### The loxone-client crate (`crates/loxone-client`)

`Connection` (host, credentials, `verify_ssl`, token and structure cache paths) is all it needs — lox builds
one with `Config::connection()`, a `LoxClient` with the aliases with `Config::client()`. Its errors are its
own `loxone_client::Error` (thiserror); in lox they become anyhow errors, so find an HTTP status with
`util::http_status(&e)`, not `downcast_ref::<HttpStatusError>()`. It prints nothing unless `set_verbose`
asks for request logs. Modules: `client.rs` (`LoxClient`: control resolution, structure cache, categories,
global states, operating modes, retries), `connection.rs`, `error.rs`, `ws.rs` (WebSocket connector),
`token.rs` (token auth, `TokenStore`), `stream.rs` (event tables → `StateEvent`, sessions), `ftp.rs`,
`statv2.rs` (Statistics V2), `stats.rs` (statistics and weather files), `sdtest.rs`, `time.rs` (Loxone
timestamps).

### The loxone-config crate (`crates/loxone-config`)

Loxone config as code: lossless `.Loxone` XML, the UUID/identity model, the `.loxl` language,
compile/decompile/adopt, the lockfile, semantic diff, and (feature `archive`) backup ZIPs with the LoxCC codec. It
never touches the network — that is loxone-client's job. Its CLI is `crates/loxone-config/src/cli.rs`, run by the `loxl` binary and by `lox logic`
(arguments passed through as-is). Before changing it, read `crates/loxone-config/AGENTS.md` (format facts are
documented, never guessed; the connector table only takes live-verified entries; never commit real configs).
`lox` itself is not published (the crate name is taken).

The crate was called lxir until v0.3.0 (the language: "lxir" → Lox Logic, `.lxir` → `.loxl`). It came from the
standalone repo github.com/discostu105/lxir via `git subtree` (history kept); that repo is archived now.

### Source files

| File | Purpose |
|------|---------|
| `src/main.rs` | The top-level `Cli`/`Cmd` clap definitions (help template, global flags), dispatch to the command modules, the JSON error envelope (`categorize_error`); `lox logic` hands its arguments to `loxone_config::cli` |
| `src/commands/*.rs` | One module per command group, each owning its clap subcommand enum next to its handler: `control` (on/off, blind, light, input, music, …), `inspect` (ls, get, stream, if, autopilot, …), `energy`, `system` (status, update, files, otel, …), `access` (user, access), `config_cmd` (setup, alias, scene, cache, token, config), `ctx`; `completions.rs` (`lox completions`), `schema.rs` (`lox schema`). `lox mcp`'s `McpCmd` lives in `src/mcp/mod.rs` |
| `src/util.rs` | Small shared helpers: `xml_attr`/`json_val_str`, `http_status`, RGB→HSV, path encoding, terminal formatting |
| `src/config.rs` | `Config` + `GlobalConfig` — loads/saves config (flat or multi-context), context resolution, project-local `.lox/` discovery |
| `src/commands/ctx.rs` | `lox ctx` commands — add/use/list/remove/rename/init/migrate contexts |
| `src/gitops.rs` | Git-based config versioning — init, pull (FTP→LoxCC→diff→commit), log, restore workflows |
| `src/scene.rs` | Scene loading/listing from `~/.lox/scenes/*.yaml` |
| `src/actions.rs` | Shared action layer: per-control-type command mapping and risk levels (used by the CLI, the TUI and MCP) |
| `src/inventory.rs` | Shared read layer: which controls are sensors or energy meters, controls with their live `/all` answer (`read_live`), a light controller's moods (used by the CLI and MCP) |
| `src/mcp/` | `lox mcp`: MCP server on rmcp (MCP 2026-07-28 + older) — CLI/runtime (`mod.rs`), `#[tool]` definitions, risky-action confirmation via MRTR/elicitation (`server.rs`), blocking Miniserver ops + output schemas (`ops.rs`), rmcp client/server protocol tests (`tests.rs`). Design: `docs/design-mcp.md` |
| `src/energy.rs` | Energy flow shared by `lox energy flow`/`--top` (`src/commands/energy.rs`), MCP `get_energy_flow` and the TUI: EFM node roles (+ `tui.yaml` overrides), flow breakdown, `/all` parsing, meter ranking |
| `src/logic.rs` | Adapter over loxone-config (always built): config program blocks, wires with hardware refs folded, neighborhood/trace (`WiringView`), lint, block history, the logic diff used by `config diff`, pull commit messages and the TUI |
| `src/snapshot.rs` | Where a `.Loxone` comes from: config repo commits (by hash, `v273`, save date, `<hash>^`), working copy, cache, FTP download, files; block history across commits |
| `src/commands/config_insight.rs` | Read-only `lox config diff/show/wiring/find/lint/history` |
| `src/commands/config_push.rs` | `lox config push`: deploy a Lox Logic project — drift check, compile against the live download (loxone-config library), byte-exact staging, upload as `sps_new.zip`, program restart (`--reboot`: reboot), verify, move `base`; `Miniserver` trait with a fake for tests |
| `src/commands/push_users.rs` | The push's user guard: live users (user management API) against the users of the program file — the changes an upload would discard |
| `crates/loxone-client/` | The Miniserver client (see above) |
| `crates/loxone-config/` | The loxone-config crate (see above): `src/xml.rs`, `uuid.rs`, `doc.rs`, `connectors.rs`, `lock.rs`, `diff.rs`, `project.rs`, `logic/` (parser, compile, decompile, adopt, lint, simtest), `cli.rs`; its own `docs/`, `tests/`, `examples/` (`examples/ci.sh` runs in CI) |
| `src/tui/` | `lox tui` (feature `tui`): Elm-style `update()` over `App`, pure rendering (`ui/`, `widgets/`), live/demo backends (`exec.rs`), terminal runtime (`run.rs`), journey tests (`tests.rs`). Design: `docs/design-tui.md` |

### Key design points

**Front-ends share layers, never each other**: the CLI (`commands/`), the TUI (`tui/`), the MCP server (`mcp/`) and
the OTel exporter (`otel.rs`) each call the shared layers — loxone-client for anything about the Miniserver's API
(e.g. `system_status()`, `diagnostics()`), `actions.rs` for writes, `inventory.rs`, `logic.rs`, `energy.rs` for lox's
reads — and never import one another's modules. Fetch, then format: a read that a second front-end could use returns
data from a shared layer, and the front-end does the printing. When a command needs such a read, move it there first.

**Control resolution** (`LoxClient::resolve_with_room`): Names are matched against the structure cache using fuzzy substring matching. Resolution order: alias → exact UUID → bracket room qualifier (`"Name [Room]"`) → `--room` flag → fuzzy substring; among several matches a single exact (case-insensitive) name wins. Ambiguous matches are an error.

**Multi-context config**: `~/.lox/config.yaml` supports both flat (single-Miniserver, backward compatible) and multi-context format. `Config::load()` resolution: `LOX_CONFIG` env → project-local `.lox/` (walk up from cwd) → global config → `--ctx` flag override. Each context gets isolated data under `~/.lox/contexts/<name>/`.

**Structure cache**: `LoxApp3.json` (~150KB) is cached per-context (e.g. `~/.lox/contexts/<name>/cache/structure.json`) with a 24h TTL. All commands that need control UUIDs load this cache first; `lox cache refresh` forces a re-fetch.

**Mixed sync/async**: The CLI commands use `reqwest::blocking`. `main.rs` uses `#[tokio::main]` because `lox token fetch` needs async (WebSocket for the key exchange). The blocking reqwest client spawns its own thread pool so both modes coexist.

**Token auth** (`crates/loxone-client/src/token.rs`): RSA public key fetched over HTTP → RSA-encrypted AES session key sent via WS `keyexchange` → `getkey2` → HMAC credential hash (SHA-1 or SHA-256, following the `hashAlg` the Miniserver reports) → `gettoken`. Token stored per-context (e.g. `~/.lox/contexts/<name>/token.json`, mode 0600), valid ~20 days. WebSocket sessions (`lox stream`, `otel`, TUI) reuse the stored token via `authwithtoken` and only fall back to `gettoken` when it is missing, expired or rejected. HTTP requests still use Basic auth.

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
GET /jdev/sps/restart, /jdev/sps/state → restart the program (loads sps_new.zip), PLC state (5 = running)
GET /jdev/sys/updatetolatestrelease → firmware update
WSS /ws/rfc6455                     → WebSocket for token auth key exchange
UDP :7070                           → Miniserver discovery (broadcast)
HTTP :7091/zone/{n}/{cmd}           → Music server API (unofficial)
```

TLS: certificate verification is off unless `verify_ssl: true` is set (Miniservers use self-signed certs); the WebSocket connector (`crates/loxone-client/src/ws.rs`) always skips verification. `serial` is only used for the OTel `device.id`, the gitops directory name and the `config push` target check; there is no DynDNS hostname generation.

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

## External references

The official Loxone documents (Communicating with the Miniserver, Structure File, User Management, ...) and
the community sources are listed in [docs/references.json](docs/references.json): links only, never the
files. `tools/fetch-refs.py` downloads them into `refs/` (gitignored) and checks each pinned sha256, so a
silently republished document shows up as `DRIFT`. Read a protocol fact there before guessing it; write it
into our docs in our own words, citing the reference id and section. The public test corpus of real configs
is `crates/loxone-config/tools/fetch-corpus.py` (see that crate's AGENTS.md).

## Issue Tracking

Issues live in GitHub Issues (`gh issue list`, `gh issue create`). Do not keep TODO lists in markdown files.
