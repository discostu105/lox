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

- **`lox`** (the root package): the CLI, the TUI, the MCP server, contexts and config, risk levels and
  confirmations, scenes, output formatting.
- **[`loxone-client`](https://github.com/discostu105/lox/tree/main/crates/loxone-client)**
  (`crates/loxone-client`): everything that talks to a Miniserver — HTTP, token auth, the WebSocket state stream,
  FTP, statistics. It takes a `Connection` (host, credentials, token and cache paths), reads no config files and
  is published on [crates.io](https://crates.io/crates/loxone-client).
- **[`loxone-config`](https://github.com/discostu105/lox/tree/main/crates/loxone-config)** (`crates/loxone-config`):
  Loxone config as code. It holds a lossless `.Loxone` XML layer, the UUID model, the `.loxl` language and compiler,
  the lockfile and the semantic diff. It never touches the network and is published on
  [crates.io](https://crates.io/crates/loxone-config). Its CLI runs as the `loxl` binary and as `lox logic`. The
  logic diffs, `lox config show/wiring/find/lint/history` and the TUI's wiring view are built on it.

TLS is rustls (no OpenSSL). Certificate verification is off unless `verify_ssl: true` is set, since Miniservers use
self-signed certificates.

## Source structure

The source files and the rules between them (front-ends share layers, never each other) are listed in
[AGENTS.md](https://github.com/discostu105/lox/blob/main/AGENTS.md#source-files).

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

The Miniserver's `LoxApp3.json` (~150KB) is cached per context (`~/.lox/contexts/<name>/cache/structure.json`) with a
24-hour TTL. Every command that needs control UUIDs loads it first.

```bash
lox cache info      # where it is, how old
lox cache refresh   # force re-fetch
```

## Automatic rules

Automatic rules (autopilot) are made in the Loxone app. There is no API to run or edit them.

- The structure file's `autopilot` key holds one container for all rules. The rules themselves are
  `AutopilotRule` objects in the program file, so `lox autopilot ls` reads them through the `lox config` sources.
- The container's two states arrive only in the WebSocket's initial state dump. HTTP answers 404.
- `changed` is the unix time of the last rule change, not of the last run.
- `history` is text: recent runs separated by `|`, each `YYYY-MM-DD HH:MM:SS <rule uuid> <rule name>` in local time.
- A rule saved in the app lands in the Miniserver's custom changes, not in a new program file. An upload discards
  it, so compare `lox autopilot changed` with the time of the download the upload is based on.

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
WSS /ws/rfc6455                     → WebSocket: token auth key exchange, state streaming
FTP /prog, /sys/custom              → config download and upload, custom changes
UDP :7070                           → Miniserver discovery (broadcast)
HTTP :7091/zone/{n}/{cmd}           → Music server API (unofficial)
```
