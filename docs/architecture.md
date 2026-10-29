---
title: Architecture
layout: default
nav_order: 5
---

# Architecture
{: .no_toc }

## Table of contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Overview

`lox` is a single static Rust binary (~4MB), built from a Cargo workspace:

- **`lox`** (the root package): the CLI, the TUI, the MCP server and everything that talks to a Miniserver (HTTP,
  WebSocket, FTP).
- **[`lxir`](https://github.com/discostu105/lox/tree/main/crates/lxir)** (`crates/lxir`): Loxone config as code — a
  lossless `.Loxone` XML layer, the UUID model, the `.lxir` language and compiler, lockfile and semantic diff. It
  never touches the network and is published on [crates.io](https://crates.io/crates/lxir). `lox ir` runs its CLI;
  the logic diffs, `lox config show/wiring/find/lint/history` and the TUI's wiring view are built on it.

TLS is rustls (no OpenSSL). Certificate verification is off unless `verify_ssl: true` is set, since Miniservers use
self-signed certificates.

## Source structure

| File | Purpose |
|:-----|:--------|
| `src/main.rs` | clap definitions and command dispatch; `lox ir` hands its arguments to `lxir::cli` |
| `src/commands/` | Command implementations: `control`, `inspect`, `system`, `config_cmd` (download/extract/compress/upload), `config_insight` (diff/show/wiring/find/lint/history), `config_push`, `ctx` |
| `src/client.rs` | `LoxClient` (HTTP): control resolution, structure cache |
| `src/config.rs` | `Config` / `GlobalConfig`: flat or multi-context config, project-local `.lox/` discovery |
| `src/actions.rs` | Per-control-type command mapping and risk levels, shared by the CLI, TUI and MCP server |
| `src/stream.rs`, `src/ws.rs`, `src/token.rs` | WebSocket state streaming, the connection, token auth |
| `src/otel.rs` | OpenTelemetry metrics, logs and traces |
| `src/ftp.rs`, `src/loxcc.rs`, `src/loxone_xml.rs` | Config transfer, LoxCC (de)compression, users/devices in the XML |
| `src/gitops.rs`, `src/snapshot.rs`, `src/logic.rs` | Config repo (init/pull/log/restore), snapshot resolution, the logic views over lxir |
| `src/statv2.rs` | Statistics V2 (energy flow monitor meters) |
| `src/scene.rs` | Scenes from YAML |
| `src/mcp/` | `lox mcp`: MCP server on rmcp |
| `src/tui/` | `lox tui` (cargo feature `tui`, on by default) |

## Control resolution

`LoxClient::resolve_with_room` resolves names to control UUIDs against the structure cache:

1. Alias lookup (from config)
2. Exact UUID match
3. Bracket room qualifier (`"Name [Room]"`)
4. `--room` flag filtering
5. Case-insensitive substring match

Among several matches, a single exact name wins (`Zentral` over `Jalousie Zentral`). Otherwise an ambiguous match is
an error that lists the candidates.

## Structure cache

The Miniserver's `LoxApp3.json` (~150KB) is cached per context with a 24-hour TTL. Every command that needs control
UUIDs loads it first.

```bash
lox cache info      # where it is, how old
lox cache refresh   # force re-fetch
```

## Mixed sync/async runtime

CLI commands use `reqwest::blocking`. `main()` runs on tokio because the WebSocket sessions (token auth, `stream`,
`otel`, TUI) are async; the blocking client has its own thread pool, so both coexist.

## Token authentication

1. Fetch the Miniserver's RSA public key over HTTP
2. Send an RSA-encrypted AES session key over WebSocket (`keyexchange`)
3. `getkey2`, then hash the credentials with HMAC (SHA-1 or SHA-256, as the Miniserver reports)
4. `gettoken`: the token is stored per context in `token.json` (mode 0600), valid ~20 days

WebSocket sessions reuse the stored token (`authwithtoken`) and only request a new one when it is missing, expired
or rejected. HTTP requests use Basic Auth.

## User data layout

```
~/.lox/
  config.yaml          # flat (one Miniserver) or multi-context
  tui.yaml             # lox tui preferences
  contexts/<name>/     # multi-context: per-context data
    cache/structure.json
    token.json
    scenes/*.yaml
  cache/structure.json # flat mode keeps these directly in ~/.lox/
  token.json
  scenes/*.yaml
```

Project-local (auto-discovered by walking up from cwd):

```
./project/.lox/
  config.yaml          # connection settings
  .gitignore           # excludes secrets and cache
  cache/
  scenes/
```

## Loxone interfaces used

| Endpoint | Purpose |
|:---------|:--------|
| `GET /data/LoxApp3.json` | Full structure (controls, rooms, categories, global states) |
| `GET /jdev/sps/io/{uuid}/{cmd}` | Send a command to a control |
| `GET /dev/sps/io/{uuid}/all` | All state outputs of a control (XML) |
| `GET /dev/sys/heap`, `/jdev/sys/…`, `/jdev/cfg/…`, `/jdev/bus/…`, `/jdev/lan/…` | Status and diagnostics |
| `GET /binstatisticdata/{uuid}/{period}` | Legacy statistics |
| `GET /dev/sps/getStatistic/…` | Statistics V2 |
| `GET /data/weatheru.bin` | Weather data |
| `GET /dev/fsget/{path}` | Filesystem access |
| `WSS /ws/rfc6455` | WebSocket: token auth, state streaming |
| FTP | Config download and upload (`/prog`) |
| `UDP :7070` | Miniserver discovery |
| `HTTP :7091/zone/{n}/{cmd}` | Music server (unofficial) |
