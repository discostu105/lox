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
lox status --diag            # CPU, tasks, context switches, SD card
lox status --net             # network config (IP, MAC, DNS, DHCP, NTP)
lox status --bus             # CAN bus statistics
lox status --lan             # LAN packet statistics
lox status --all             # all diagnostic sections
lox status --energy          # energy dashboard
```

| Flag | Description |
|:-----|:------------|
| `--diag` | CPU, tasks, context switches, SD card test |
| `--net` | Network configuration (IP, MAC, DNS, DHCP, NTP) |
| `--bus` | CAN bus statistics |
| `--lan` | LAN packet statistics |
| `--all` | All diagnostic sections |
| `--energy` | Energy meter dashboard |

---

## System log

```bash
lox log                      # last 50 lines
lox log -n 100               # last 100 lines
```

Requires admin user credentials.

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
The `reboot` and `update install` commands affect your live system. Always have a backup.

---

## Filesystem

Browse and download files from the Miniserver:

```bash
lox files ls /               # list root directory
lox files ls /log            # list log directory
lox files get /log/def.log   # download a file
lox files get /log/def.log --save-as local.log
```
