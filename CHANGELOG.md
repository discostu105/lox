# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
