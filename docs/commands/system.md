---
title: System
layout: default
parent: Command Reference
nav_order: 3
---

# System Commands
{: .no_toc }

## Table of contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Status

```bash
lox status                   # firmware, PLC state, memory
lox status --diag            # CPU, tasks, interrupts, SD card health
lox status --net             # network config (IP, MAC, DNS1/2, DHCP, NTP)
lox status --bus             # CAN bus statistics (incl. parity errors)
lox status --lan             # LAN statistics (incl. exhaustion, no-buffer)
lox status --all             # all diagnostic sections
lox status -o json | jq .plc_running
```

| Flag | Description |
|:-----|:------------|
| `--diag` | CPU, tasks, context switches, interrupts, SD card (read/write speed, errors, usage) |
| `--net` | Network configuration (IP, MAC, DNS 1 & 2, DHCP, NTP) |
| `--bus` | CAN bus statistics (packets, receive/frame/parity errors, overruns) |
| `--lan` | LAN statistics (TX/RX packets, errors, collisions, underruns, exhaustion, no-buffers) |
| `--all` | All diagnostic sections |

Energy meters moved to [`lox energy`](inspection#energy).

---

## System log

```bash
lox log                      # last 50 lines
lox log -n 100               # last 100 lines
```

Requires admin user credentials. For the full file: `lox files get /log/def.log`.

---

## System time

```bash
lox time                     # show Miniserver date and time
```

---

## Discover

Find Miniservers on the local network via UDP broadcast:

```bash
lox discover
lox discover --timeout 5     # custom timeout in seconds
```

---

## Extensions

```bash
lox extensions               # list connected extensions and devices
```

Shows Tree extensions, Air devices, and other connected hardware with serial numbers, versions, and parent relationships.

---

## Device health

```bash
lox health                   # full device health dashboard
lox health --type tree       # filter: tree devices only
lox health --type air        # filter: air devices only
lox health --problems        # show only devices with issues
lox health -o json           # JSON output
```

Shows battery levels, signal strength, online/offline status, and bus errors for Tree and Air devices.

---

## Firmware updates

```bash
lox update check             # check for available updates
lox update install --yes     # install update (requires --yes)
```

---

## Reboot

```bash
lox reboot --yes             # reboot Miniserver (requires --yes)
```

{: .warning }
The `reboot` and `update install` commands affect your live system. Always have a backup (`lox config pull`).

After `lox config upload` or `restore`, the reboot is what loads the new config.

---

## Filesystem

Browse and download files from the Miniserver:

```bash
lox files ls /               # list root directory
lox files ls /log            # list log directory
lox files get /log/def.log   # download into the current directory (./def.log)
lox files get /log/def.log --save-as local.log
lox files get /log/def.log --save-as - | grep -i error   # to stdout
```

`lox files ls` without a path lists `/`. The confirmation shows where the file went; with `-o json`:
`{"ok": true, "path": …, "saved_as": …, "bytes": …}`.

---

## Terminal UI

```bash
lox tui                      # live dashboard: rooms, events, energy, system
lox tui --demo               # without a Miniserver
```

See the [Terminal UI guide](/lox/guides/tui).
