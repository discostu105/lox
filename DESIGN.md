# lox — Design Document

> CLI for Loxone Miniserver

## Status

Working. Core commands functional and tested end-to-end.

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│  lox CLI (single binary)                                 │
│                                                          │
│  ┌──────────┐  ┌──────────┐                             │
│  │ Commands │  │  Scenes  │                             │
│  │ on/off   │  │ YAML     │                             │
│  │ blind    │  │ run      │                             │
│  │ if/watch │  │ new      │                             │
│  └────┬─────┘  └─────┬────┘                             │
│       │              │                                   │
│  ┌────▼──────────────▼────┐                              │
│  │      HTTP Client       │                              │
│  │  reqwest + Basic Auth  │                              │
│  └──────────┬─────────────┘                              │
└─────────────┼───────────────────────────────────────────┘
              │ HTTPS
    ┌─────────▼──────────┐
    │  Loxone Miniserver │
    │   /jdev/sps/io/    │
    │   /dev/sps/io/all  │
    │   /data/LoxApp3    │
    └────────────────────┘
```

---

## What Works Today

| Feature | Status | Notes |
|---------|--------|-------|
| `lox ls / rooms` | ✅ | Structure from LoxApp3.json |
| `lox on/off/pulse/send` | ✅ | Via `/jdev/sps/io/{uuid}/{cmd}` |
| `lox get` | ✅ | Via `/dev/sps/io/{uuid}/all` XML |
| `lox blind` | ✅ | PulseUp/Down/FullUp/FullDown/AutomaticDown |
| `lox status` | ✅ | Firmware, PLC state, memory |
| `lox if` | ✅ | Exit codes for shell scripting |
| `lox watch` | ✅ | HTTP polling loop |
| `lox run <scene>` | ✅ | Multi-step YAML scenes |
| `lox log` | ⚠️ | Needs admin user |
| `--json` output | ✅ | All commands |

---

## Next Steps

### 1. `lox watch` via WebSocket (Medium Priority)

Currently polling. The WS infrastructure exists in `ws.rs` (`connect_raw`). Once wired up:

```bash
lox watch "Lichtsteuerung"     # real-time, <100ms latency
lox watch --all                # stream all state changes
```

Requires Monitor rights enabled in Loxone Config → User Management.

---

### 2. `lox set` — Analog/Virtual Inputs (Medium Priority)

Send analog values to virtual inputs:

```bash
lox set "Solltemperatur" 21.5     # send analog value
lox set "Modus" "Urlaub"          # send text to virtual text input
```

API: `/jdev/sps/io/{uuid}/{value}` already supports this, just needs a dedicated command + type detection.

---

### 3. `lox mood` — LightControllerV2 Moods (Low Priority)

Loxone light controllers have named moods (Stimmungen):

```bash
lox mood "Wohnzimmer" list           # list available moods + IDs
lox mood "Wohnzimmer" set "Abend"   # activate mood by name
lox mood "Wohnzimmer" set 778       # activate mood by ID
```

The `/all` output for LightControllerV2 includes `moodList` — needs parsing.

---

### 4. `lox rooms` — Room-scoped Commands (Low Priority)

```bash
lox room "Wohnzimmer" off    # turn off everything in room
lox room "Wohnzimmer" ls     # list all controls in room
```

Already have room data in structure, just needs a Room command that iterates.

---

### 5. TLS Improvements (Low Priority)

Currently using `danger_accept_invalid_certs`. Options:

**Option A:** Use dyndns hostname (serial-based):
```
https://192-168-20-24.{SERIAL}.dyndns.loxonecloud.com
```
Already implemented in `Config::tls_host()`, just needs to be used everywhere.

**Option B:** Token auth (newer Loxone firmware)  
Loxone supports JWT tokens via `/jdev/sys/gettoken` — longer-lived, more secure than Basic Auth per-request.

---

### 6. `lox backup` — Structure/Config Backup (Low Priority)

```bash
lox backup          # save LoxApp3.json + current state snapshot
lox backup restore  # restore from backup (readonly — shows diff)
```

Also: `/dev/fslist/` and `/dev/fsget/` allow SD card file access (needs admin).

---

## Data Model

```
Config (~/.lox/config.yaml)
  host, user, pass, serial

Structure (cached from LoxApp3.json)
  controls: {uuid → {name, type, room, states}}
  rooms:    {uuid → name}

Scenes (~/.lox/scenes/*.yaml)
  name, description, steps[]
    step: {control, cmd, delay_ms}

```

---

## Known Limitations

| Issue | Impact | Fix |
|-------|--------|-----|
| `enablestatusupdate` needs Monitor rights | `lox watch` can't use WS | Enable Monitor right in Loxone Config |
| `CentralLightController` has no numeric `/all` value | Can't watch central lights via polling | Use LightControllerV2 UUIDs directly |
| State values only via WS (not HTTP) for most types | `lox get` shows limited info | WS with Monitor rights |
| `lox log` needs admin | Can't read Miniserver logs | Use admin user |
| No structured error codes | Rule engine uses string matching | Add error enum |

---

## API Reference (Loxone HTTP)

```
GET /data/LoxApp3.json                     → structure (controls, rooms)
GET /jdev/sps/io/{uuid}/{cmd}              → send command, returns JSON
GET /dev/sps/io/{uuid}/all                 → all outputs as XML
GET /dev/sps/io/{name}/state               → input state (works by name)
GET /dev/sps/io/{name}/astate              → output state
GET /dev/sys/cpu                           → CPU load (admin only)
GET /dev/sys/heap                          → memory usage
GET /dev/sps/state                         → PLC state (0-8)
GET /dev/cfg/version                       → firmware version
GET /data/status                           → full status XML
GET /dev/fsget/log/def.log                 → system log (admin)
WSS /ws/rfc6455                            → WebSocket API
  → jdev/sps/enablestatusupdate            → subscribe to state push
  → keepalive                              → keepalive ping
```

---

## Loxone WebSocket Protocol

```
Connection: WSS /ws/rfc6455
Auth: Basic Auth in HTTP Upgrade header

Binary message format:
  Header (8 bytes):
    [0] = 0x03 (magic)
    [1] = message type
          0x00 = text
          0x02 = ValueEventTable  ← state updates
          0x06 = keepalive
    [2] = flags (bit0 = estimated value)
    [3] = reserved
    [4-7] = uint32_le payload length

  ValueEventTable payload:
    repeated 24-byte records:
      [0-15]  = UUID (uint32_le + uint16_le + uint16_le + 8 bytes)
      [16-23] = double (float64_le) = current value

UUID binary → string:
  bytes[0..4]  → uint32_le → 8 hex chars  (part 1)
  bytes[4..6]  → uint16_le → 4 hex chars  (part 2)
  bytes[6..8]  → uint16_le → 4 hex chars  (part 3)
  bytes[8..16] → raw       → 16 hex chars (part 4)
  → "{p1}-{p2}-{p3}-{p4}"
```
