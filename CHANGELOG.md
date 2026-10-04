# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.14.0] — 2026-10-04

### Added
- **Config logic insight** (read-only, built on lxir):
  - `lox config show` (logic pages; `-p PAGE` as lxir source), `lox config wiring NAME` (inputs, outputs, non-default parameters with units, devices and rooms; `--trace up|down|both`), `lox config find`, `lox config lint` (dead blocks, unwired inputs, duplicate names, broken refs), `lox config history NAME` (a block across the snapshots)
  - All of them read the config repo, the cached download, `--at SNAPSHOT`, `--file PATH` or `--download`; snapshots by commit, version (`v273`) or save date
- MCP tool `get_wiring` — the config logic around a control (what drives it, what it drives, parameters, traced paths), read-only
- TUI: System › Config snapshot browser (`⏎` on a commit: pages, blocks, lxir source, `/` search, wiring at that snapshot), compare any two commits (`m`), `w` on a diff line opens the block's wiring
- TUI wiring overlay: block type, page, room and parameters in the header, the selected wire's device/room and fan-out (`also drives`), formatted live values, `t` trace to the sensors / actuators, `o` open the page, `/` search, `H` block history, `p` all parameters

### Changed
- `lox config diff` is a logic diff (blocks, parameters, wires per page, hardware refs folded) and takes snapshots of the config repo (`lox config diff`, `diff v271`, `diff A..B`) as well as files
- `lox config pull` commit messages carry the logic diff with totals, grouped by page and capped at 80 lines; layout-only saves say "No logic changes"
- lxir is always built (also with `--no-default-features`)
- TUI keys, one pattern everywhere: `Esc` (or `⌫`) goes back one step — in the wiring the previously followed block (was `b`), in the config browser source → blocks → pages, in the chart the cursor, on a screen filter → pane → the screen a jump (`e`, `⏎` on a room or event, palette) came from (`Esc ← rooms` in the header); `q` closes a popup in one press and quits only from a screen, and a held `q` no longer quits after closing the popups
- TUI chart: `←`/`→` and `h`/`l` both move the cursor, which carries on into the previous/next period at the edges; `PgUp`/`PgDn` move a whole period (was `←`/`→`)
- TUI facet picker: `Esc` closes at once, like the other type-to-narrow popups (`C-u` clears the query)

### Fixed
- Piping output into a command that stops reading early (`lox ls | head -1`, `lox completions bash | head`) no longer panics with "Broken pipe"; `lox` exits quietly (#116)

## [0.13.0] — 2026-10-03

### Added
- **MCP server** (`lox mcp`) — turns `lox` into a Model Context Protocol server over stdio, so Claude Desktop, Claude Code, Cursor, ChatGPT connectors, n8n and other MCP clients can control a Loxone installation
  - `lox mcp serve [--read-only | --allow-risky] [--allow-raw]`, `lox mcp config` (client config snippets), `lox mcp tools`
  - Built on rmcp; speaks MCP 2026-07-28 (stateless `server/discover`, `tools/list` cache hints) and every revision back to 2024-11-05
  - Read tools (`list_rooms`, `list_controls`, `get_control`, `list_sensors`, `list_light_moods`, `list_scenes`, `system_status`), action tools (`switch`, `blind`, `light`, `thermostat`, `run_scene`) and high-risk tools (`door`, `gate`, `alarm`, raw `send_command` with `--allow-raw`)
  - Every action supports `dry_run` and returns the equivalent `lox` command line; every tool has an output schema and structured results
  - High-risk actions need the user's confirmation (multi-round-trip `input_required` on 2026-07-28, `elicitation/create` on older revisions); confirmation state is a single-use, server-side nonce bound to the exact commands
- `confirm:` list in the config — controls on it (UUID, alias or name, optional `[Room]`) always ask for confirmation, in `lox mcp` and `lox tui`

### Changed
- Door openers, locks and gates wired as generic switches or push buttons (door/gate/lock icon, `isSecured`) now need confirmation, too — in `lox mcp` and `lox tui`
- Control resolution: among several substring matches, a single exact (case-insensitive) name match wins
- MCP `get_control` returns a control's named outputs (e.g. LightControllerV2 relays) instead of flattening them into states
- `lox sensors --type temperature` classifies by unit, so CO2 and humidity sensors no longer show up as temperatures
- Dimming a LightControllerV2 to a bare level is refused with a hint to use moods (the Miniserver ignores it)

### Fixed
- Token auth: the HMAC algorithm follows the `hashAlg` the Miniserver reports (SHA-1 on firmware before 10.4), instead of always SHA-256
- WebSocket sessions (`lox stream`, `lox otel serve`, `lox tui`) reuse the stored token via `authwithtoken` instead of minting a new long-lived token on every connect
- `lox stream` and `lox otel serve` send keepalives and detect dead sockets, so the Miniserver's idle timeout no longer drops them; `lox otel serve` reconnects with backoff
- `token.json` is written with mode 0600

## [0.12.0] — 2026-09-27

### Added
- **Terminal UI** (`lox tui`) — full-screen, keyboard-first live view of your installation (built in by default; `--no-default-features` for a minimal build)
  - Home, Rooms, Events, Energy, System and Sites screens with live values
  - Control actions from the keyboard (toggle, dim, set value, moods), with confirmation for doors, alarms and gates
  - Wiring view (`w`) — the config program around a control, with live values on the wires (via [lxir](https://github.com/discostu105/lxir))
  - History view (`c`) — control statistics from 6 h to a year, with period compare
  - Fuzzy command palette (`:`), `y` copies the equivalent `lox` command
  - `--demo` (synthetic house, no Miniserver needed) and `--read-only` (wall display) modes
  - Miniserver log with full-line view and match highlighting; config history with semantic diffs

## [0.11.0] — 2026-03-23

### Added
- **Multi-Miniserver context management** (`lox ctx`) — `kubectl`-style context switching for multiple Miniservers
  - `lox ctx add <name> --host ... --user ... --pass ...` — add a named context
  - `lox ctx use <name>` / `lox ctx <name>` — switch active context
  - `lox ctx list` — list all contexts (`*` = active)
  - `lox ctx current` — show active context
  - `lox ctx remove <name>` / `lox ctx rename <old> <new>` — manage contexts
  - `lox ctx init` — create project-local `.lox/` directory (auto-discovered like `.git`)
  - `lox ctx migrate` — convert existing flat config to a `default` context
- `--ctx <name>` global flag — run any command against a specific context without switching
- Per-context data isolation — each context gets its own cache, token, and scenes directory under `~/.lox/contexts/<name>/`
- Project-local `.lox/` directory support — walks up from cwd, like `.git` resolution
- Backward-compatible config format: existing flat `~/.lox/config.yaml` files continue to work unchanged

## [0.8.0] — 2026-03-21

### Added
- **Windows support** — pre-built binaries for Windows x86_64 and aarch64 are now included in every release
- `lox config init <path>` — initialize a git repository for config version tracking (multi-Miniserver via serial subdirectories)
- `lox config pull [--quiet]` — download config via FTP, decompress LoxCC, generate semantic diff, and git-commit with meaningful change messages
- `lox config log [-n COUNT]` — show config change history from the git repository
- `lox config restore <commit> --force` — restore a previous config version from git history and upload to Miniserver
- `lox health` — device health dashboard showing battery, signal, offline status, and bus errors for Tree/Air devices (`--type tree|air`, `--problems`)
- `lox schema` — command schema introspection for AI agent discovery; lists commands with metadata, args, and valid actions
- `--dry-run` global flag — validates and resolves inputs without executing commands; returns structured JSON envelope with `-o json`
- `--non-interactive` global flag — fails instead of prompting for confirmation (implied by `-o json`)
- `--trace-id` global flag — correlation ID for tracking agent actions in logs
- `-v`/`--verbose` global flag — `-v` shows HTTP requests, `-vv` shows requests + response bodies
- `--all-in-room` flag on `lox on`/`lox off` — apply command to all controls in a room
- Structured JSON error envelopes when using `-o json` (categorized error codes: `control_not_found`, `ambiguous_control`, `unauthorized`, `connection_error`, etc.)

### Changed
- `lox extensions` now queries `/data/status` instead of `LoxApp3.json` — provides richer device information including Tree branch error counts, device parent relationships, and plugin versions
- CI: moved linting to a dedicated Ubuntu job; Windows builds no longer run redundant clippy/fmt checks

### Removed
- `lox daemon` — automation daemon (WebSocket/polling rule engine)
- `lox automation` — automation rule management
- `lox service` — systemd service management
- `timezone:` config field (was only used for automation time windows)

## [0.1.0] — 2024-01-01

### Added
- `lox ls` — list controls with optional `--type`, `--room`, `--values` filters
- `lox get <name>` — show full state of a control
- `lox on/off/pulse <name>` — send on/off/pulse commands
- `lox send <name> <cmd>` — send arbitrary raw command
- `lox blind <name> <action>` — control Jalousie blinds (up/down/stop/shade/pos)
- `lox mood <name> <action>` — control LightControllerV2 moods
- `lox set <name> <value>` — set analog/virtual input value
- `lox if <name> <op> <value>` — conditional state check (exit 0/1)
- `lox watch <name>` — poll state changes
- `lox status [--energy]` — Miniserver health; auto-discovers energy meters
- `lox rooms` — list all rooms
- `lox config set/show` — manage connection config
- `lox token fetch/info/clear` — RSA+AES token auth management
- `lox cache info/clear/refresh` — structure cache management
- `lox scene list/show/new` + `lox run <scene>` — multi-step scenes
- `--room` flag on all commands for disambiguation
- Bracket room qualifier: `"Name [Room]"` syntax
- Alias support in config (`aliases:` map)
- Structure cache (24h TTL) at `~/.lox/cache/structure.json`
- Token auth (acquired via `lox token fetch`) used for all HTTP requests
### Fixed
- `lox config set` no longer clobbers the alias list
- `lox set` percent-encodes values to avoid malformed URLs
- Token auth is now actually used for all requests (was always falling back to Basic Auth)
- `lox blind` and `lox mood` now support `--room` flag
- WebSocket nonce uses cryptographically random bytes
- Debug `eprintln!` removed from token RSA parsing

[Unreleased]: https://github.com/discostu105/lox/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/discostu105/lox/compare/v0.1.0...v0.8.0
[0.1.0]: https://github.com/discostu105/lox/releases/tag/v0.1.0
