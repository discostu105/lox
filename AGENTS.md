# Agent Instructions

Guidance for Claude Code and other AI agents working in this repository.

- [API_DESIGN_GUIDELINES.md](API_DESIGN_GUIDELINES.md): command names, flags and output formats.
- [WRITING_GUIDELINES.md](WRITING_GUIDELINES.md): all text people read (help, errors, comments, docs, changelog).
  Keep it short and plain.
- [docs/architecture.md](docs/architecture.md): control resolution, caches, token auth, the data layout and the
  Loxone endpoints lox uses.

## Commands

```bash
cargo build                   # debug
cargo build --release         # production binary (~4MB)
cargo run -- <args>           # run lox, e.g. cargo run -- ls -o json
cargo run --bin loxl -- help  # run loxl, the Lox Logic CLI
cargo check
cargo clippy
cargo test                    # all tests
cargo test <test_name>        # one test
cargo test -p loxone-config   # one crate (also -p loxone-client)
```

These cover the whole workspace (`default-members`): the `lox` package at the root and the library crates
`crates/loxone-config` and `crates/loxone-client`.

## Pre-push CI checklist

**Always run these four checks before committing or pushing.** They mirror `.github/workflows/ci.yml` (ubuntu and
macOS):

```bash
cargo fmt --check            # formatting
cargo clippy -- -D warnings  # lints (warnings are errors)
cargo build --release        # release build
cargo test                   # all tests
```

Do not push until all four pass.

## Releases

Every push to main that passes CI is released by `.github/workflows/auto-release.yml`. It bumps the version, writes
the changelog entries into CHANGELOG.md, commits and tags `Release vX.Y.Z`. Then `release.yml` builds the binaries,
the Docker image and the Homebrew formula.

**Never edit CHANGELOG.md in a PR.** CI fails a PR that writes under `[Unreleased]`. Instead:

- Add `changelog.d/<branch>.<section>.md`. The section is `added`, `changed`, `deprecated`, `removed`, `fixed` or
  `security`. The content is the bullet as it should read, leading with the command in bold.
- For a notable change, also add `changelog.d/<branch>.summary.md`. Its first line is a short lowercase tagline
  (the release title becomes `vX.Y.Z — tagline`), followed by a sentence or two of intro.
- The release merges the files into CHANGELOG.md and deletes them. See [changelog.d/README.md](changelog.d/README.md).

The version bump:

- A PR label picks it: `release:major`, `release:minor`, `release:patch` or `no-release`. In a direct push, put the
  same word in brackets in the commit message (`[release:minor]`).
- Without a label, it is minor when there is an `added` entry, otherwise patch.
- Changes to docs, CI or the formula alone are not released.
- Several merges in a row become one release: only the newest green commit of main releases.
- A Cargo.toml version above the last tag is released as it is.

Other cases:

- A hand-pushed `vX.Y.Z` tag still releases.
- A failed release reruns with *Actions › Release › Run workflow* and the tag.
- `python3 .github/scripts/prepare_release.py --dry-run` shows the next release and its notes.

The library crates keep their own versions in `crates/<crate>/Cargo.toml`:

- Bump one, and its requirement in the root Cargo.toml if the old one no longer matches. The next release
  publishes it to crates.io, loxone-config first.
- Publishing needs the `CARGO_REGISTRY_TOKEN` repository secret. Without it, run `cargo publish -p loxone-config`,
  then `cargo publish -p loxone-client`.
- A breaking change to a public API is a minor bump while the crate is 0.x.

## Architecture

A Cargo workspace: the `lox` binary (root package) and two library crates that `lox` uses by path. Both crates are
published on crates.io.

- `crates/loxone-client` talks to the Miniserver: HTTP, token auth, the WebSocket stream, FTP, statistics.
- `crates/loxone-config` handles the program file: `.Loxone`, backup ZIPs, the `.loxl` language. It never touches
  the network.

Policy stays in lox: contexts and config files, aliases, risk levels and confirmations (`actions.rs`), scenes, the
CLI, TUI and MCP server, and output formatting. HTTP is reqwest blocking, and `main()` runs on tokio for the
WebSocket parts. The blocking client has its own thread pool, so both coexist.

### loxone-client

lox builds a `Connection` (host, credentials, `verify_ssl`, token and structure cache paths) with
`Config::connection()`, and a `LoxClient` with the aliases with `Config::client()`.

- Its errors are its own `loxone_client::Error` (thiserror). In lox they become anyhow errors, so find an HTTP
  status with `util::http_status(&e)`, not `downcast_ref::<HttpStatusError>()`.
- It prints nothing unless `set_verbose` asks for request logs.
- Modules: `client.rs` (`LoxClient`: control resolution, structure cache, categories, global states, operating
  modes, retries), `connection.rs`, `error.rs`, `ws.rs`, `token.rs` (`TokenStore`), `stream.rs` (event tables,
  `StateEvent`, sessions), `ftp.rs`, `statv2.rs` (Statistics V2), `stats.rs` (statistics and weather files),
  `sdtest.rs`, `time.rs`.

### loxone-config

Loxone config as code: lossless `.Loxone` XML, the UUID model, the `.loxl` language, compile/decompile/adopt, the
lockfile, semantic diff and (feature `archive`) backup ZIPs with the LoxCC codec. Its CLI is
`crates/loxone-config/src/cli.rs`, run by the `loxl` binary and by `lox logic` (arguments passed through as-is).

**Before changing it, read `crates/loxone-config/AGENTS.md`.** In short: format facts are documented, never guessed.
The connector table only takes live-verified entries. Never commit real configs.

`lox` itself is not published, because the crate name is taken. The crate was called lxir until v0.3.0 (the
language: "lxir" became Lox Logic, `.lxir` became `.loxl`). It came from github.com/discostu105/lxir via
`git subtree` with its history. That repo is archived.

### Source files

| File | Purpose |
|------|---------|
| `src/main.rs` | Top-level `Cli`/`Cmd` (help template, global flags), dispatch, the JSON error envelope (`categorize_error`). `lox logic` hands its arguments to `loxone_config::cli` |
| `src/commands/*.rs` | One module per command group, each with its clap enum next to its handler: `control` (on/off, blind, light, input, music, …), `inspect` (ls, get, stream, if, autopilot, …), `energy`, `system` (status, update, files, otel, …), `access` (user, access), `config_cmd` (setup, alias, scene, cache, token, config), `ctx`, `completions`, `schema` |
| `src/commands/config_insight.rs` | Read-only `lox config diff/show/wiring/find/lint/history` |
| `src/commands/config_push.rs` | `lox config push`: drift check, compile against the live download, byte-exact staging, upload, restart, verify, move `base`, the app-changes guard; `lox config sync`. A `Miniserver` trait with a fake for tests |
| `src/commands/push_users.rs` | The push's user guard: live users against the program file's users |
| `src/config.rs` | `Config`, `GlobalConfig`: flat or multi-context config, context resolution, project-local `.lox/` |
| `src/util.rs` | Shared helpers: `xml_attr`/`json_val_str`, `http_status`, RGB→HSV, path encoding, terminal formatting |
| `src/actions.rs` | Writes: per-control-type commands and risk levels (CLI, TUI, MCP) |
| `src/inventory.rs` | Reads: sensors, energy meters, a light controller's moods (CLI, MCP) |
| `src/hardware.rs` | Hardware for `lox hw status` and MCP `hw_status`: `/data/status` against the program file's serials |
| `src/energy.rs` | Energy flow for `lox energy`, MCP `get_energy_flow` and the TUI: EFM node roles, `/all` parsing, meter ranking |
| `src/logic.rs` | Adapter over loxone-config: program blocks, wires, `WiringView`, lint, block history, the logic diff |
| `src/snapshot.rs` | Where a `.Loxone` comes from: config repo commits (hash, `v273`, date, `<hash>^`), working copy, cache, FTP, files |
| `src/gitops.rs` | Config repo: init, pull (FTP → LoxCC → diff → commit), log, restore |
| `src/scene.rs` | Scenes from YAML |
| `src/mcp/` | `lox mcp` on rmcp. Config-as-code tools run the CLI as a child process (`config_tools.rs`). Design: `docs/design-mcp.md` |
| `src/tui/` | `lox tui` (feature `tui`), Elm-style `update()` over `App`. Design: `docs/design-tui.md` |

### Key design points

**Front-ends share layers, never each other.** The CLI (`commands/`), the TUI (`tui/`), the MCP server (`mcp/`)
and the OTel exporter (`otel.rs`) never import one another's modules. They call the shared layers:

- loxone-client for anything about the Miniserver's API (`system_status()`, `diagnostics()`).
- `actions.rs` for writes.
- `inventory.rs`, `logic.rs` and `energy.rs` for lox's reads.

Fetch, then format. A read that a second front-end could use returns data from a shared layer, and the front-end
prints it. When a command needs such a read, move it there first.

**TLS.** Certificate verification is off unless `verify_ssl: true` is set (Miniservers use self-signed certs).
The WebSocket connector always skips verification. `serial` is only used for the OTel `device.id`, the gitops
directory name and the `config push` target check.

## Agent workflow

### Non-interactive shell commands

**Always use non-interactive flags.** `cp`, `mv` and `rm` may be aliased to `-i` and hang on a prompt:

```bash
cp -f source dest    # NOT: cp source dest
mv -f source dest    # NOT: mv source dest
rm -f file           # NOT: rm file
rm -rf directory     # NOT: rm -r directory
```

Other commands that may prompt: `scp`/`ssh` (use `-o BatchMode=yes`), `apt-get` (use `-y`), `brew` (set
`HOMEBREW_NO_AUTO_UPDATE=1`).

### Session completion

**Work is not complete until `git push` succeeds.**

1. File GitHub issues for remaining work (`gh issue create`).
2. Run the quality gates: `cargo test`, `cargo clippy`.
3. Close or update the issues the work resolves.
4. Push, and check that `git status` shows "up to date with origin":
   ```bash
   git pull --rebase
   git push
   git status
   ```

## External references

[docs/references.json](docs/references.json) lists the official Loxone documents (Communicating with the
Miniserver, Structure File, User Management, …) and community sources. It holds links only, never the files.

- `tools/fetch-refs.py` downloads them into `refs/` (gitignored) and checks each pinned sha256. A silently
  republished document shows up as `DRIFT`.
- Read a protocol fact there before guessing it. Write it into our docs in our own words, citing the reference id
  and section.
- `docs/research/` holds dated research notes: facts that no other doc records yet. Check them before you build
  on them, and move a fact into the real docs once a feature uses it.
- The public test corpus of real configs is `crates/loxone-config/tools/fetch-corpus.py` (see that crate's
  AGENTS.md).

## Issue tracking

Issues live in GitHub Issues (`gh issue list`, `gh issue create`). Do not keep TODO lists in markdown files.
