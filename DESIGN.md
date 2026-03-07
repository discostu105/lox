# lox — Design Document

> CLI + Automation Daemon for Loxone Miniserver

## Status

Working prototype. Core commands functional, polling daemon tested end-to-end.

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│  lox CLI (single binary)                                 │
│                                                          │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐              │
│  │ Commands │  │  Scenes  │  │  Daemon  │              │
│  │ on/off   │  │ YAML     │  │ poll/WS  │              │
│  │ blind    │  │ run      │  │ rules    │              │
│  │ if/watch │  │ new      │  │ fire     │              │
│  └────┬─────┘  └─────┬────┘  └────┬─────┘              │
│       │              │             │                     │
│  ┌────▼──────────────▼─────────────▼────┐               │
│  │            HTTP Client               │               │
│  │   reqwest + Basic Auth + TLS         │               │
│  └────────────────────┬─────────────────┘               │
└───────────────────────┼─────────────────────────────────┘
                        │ HTTPS / WSS
              ┌─────────▼──────────┐
              │  Loxone Miniserver │
              │   /jdev/sps/io/    │
              │   /dev/sps/io/all  │
              │   /data/LoxApp3    │
              │   /ws/rfc6455      │
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
| `lox daemon --poll` | ✅ | Polling daemon, rules tested E2E |
| `lox daemon` (WS) | ⚠️ | Connects/auth OK, needs Monitor rights for `enablestatusupdate` |
| `lox log` | ⚠️ | Needs admin user |
| `--json` output | ✅ | All commands |

---

## Next Steps

### 1. WebSocket — Real-time State (High Priority)

**Problem:** `enablestatusupdate` returns 400 for non-admin users.  
**Fix:** In Loxone Config → User Management → enable "Monitor" right for the user.

Once that's enabled:
- Remove polling fallback as primary recommendation
- WS gives instant (<100ms) state changes vs 2-3s polling lag
- Enables automation on fast events (motion sensors, doorbells)

**Loxone WS binary protocol** (already implemented, needs live testing):
```
Header (8 bytes): 0x03 | type | flags | reserved | uint32_le length
type 0x02 = ValueEventTable: repeated [UUID(16) + double(8)] records
```

**TODO:**
- Test with Monitor rights enabled
- Handle reconnect gracefully (currently reconnects but state is lost)
- State cache: persist last-known values across reconnects

---

### 2. `lox watch` via WebSocket (Medium Priority)

Currently polling. Once WS works:

```bash
lox watch "Lichtsteuerung"     # real-time, <100ms latency
lox watch --all                # stream all state changes
```

With `--all` useful for discovering what changes when you operate physical switches.

---

### 3. Daemon: systemd Integration (Medium Priority)

```bash
lox daemon install    # writes /etc/systemd/system/lox-daemon.service
lox daemon status     # shows systemd status
lox daemon logs       # journalctl output
```

Service file template:
```ini
[Unit]
Description=Loxone Automation Daemon
After=network.target

[Service]
ExecStart=/usr/local/bin/lox daemon --poll
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

---

### 4. Automation Rule Improvements (Medium Priority)

**Current:** Single `when` + `op` + `value`

**Planned:**
```yaml
rules:
  # Multiple conditions (AND)
  - when:
      - control: "Temperatur Wohnzimmer"
        op: gt
        value: "25"
      - control: "Beschattung"
        op: eq
        value: "0"      # not already shaded
    run: "lox blind 'Beschattung Zentral EG' shade"

  # Time window
  - when: "Türklingel"
    op: eq
    value: "1"
    only_between: "08:00-22:00"
    run: "lox on 'Licht Eingang'"

  # Consecutive triggers (debounce vs require N hits)
  - when: "Bewegungsmelder"
    op: eq
    value: "1"
    require_count: 1
    cooldown_secs: 300
    run: "lox run alarm"
```

**Environment variables in run scripts:**
```bash
# Pass current value to script
run: "notify.sh $LOX_CONTROL $LOX_VALUE $LOX_PREV_VALUE"
```

---

### 5. `lox set` — Analog/Virtual Inputs (Medium Priority)

Send analog values to virtual inputs:

```bash
lox set "Solltemperatur" 21.5     # send analog value
lox set "Modus" "Urlaub"          # send text to virtual text input
```

API: `/jdev/sps/io/{uuid}/{value}` already supports this, just needs a dedicated command + type detection.

---

### 6. `lox mood` — LightControllerV2 Moods (Low Priority)

Loxone light controllers have named moods (Stimmungen):

```bash
lox mood "Wohnzimmer" list           # list available moods + IDs
lox mood "Wohnzimmer" set "Abend"   # activate mood by name
lox mood "Wohnzimmer" set 778       # activate mood by ID
```

The `/all` output for LightControllerV2 includes `moodList` — needs parsing.

---

### 7. `lox rooms` — Room-scoped Commands (Low Priority)

```bash
lox room "Wohnzimmer" off    # turn off everything in room
lox room "Wohnzimmer" ls     # list all controls in room
```

Already have room data in structure, just needs a Room command that iterates.

---

### 8. TLS Improvements (Low Priority)

Currently using `danger_accept_invalid_certs`. Options:

**Option A:** Use dyndns hostname (serial-based):
```
https://192-168-20-24.{SERIAL}.dyndns.loxonecloud.com
```
Already implemented in `Config::tls_host()`, just needs to be used everywhere.

**Option B:** Token auth (newer Loxone firmware)  
Loxone supports JWT tokens via `/jdev/sys/gettoken` — longer-lived, more secure than Basic Auth per-request.

---

### 9. `lox backup` — Structure/Config Backup (Low Priority)

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

Automations (~/.lox/automations.yaml)
  rules[]
    rule: {when, op, value, run, cooldown_secs, description}
```

---

## Known Limitations

| Issue | Impact | Fix |
|-------|--------|-----|
| `enablestatusupdate` needs Monitor rights | WS daemon falls back to polling | Enable Monitor right in Loxone Config |
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
