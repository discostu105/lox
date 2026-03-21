---
title: Config Versioning
layout: default
parent: Guides
nav_order: 3
---

# Config Versioning (GitOps)
{: .no_toc }

Track Miniserver configuration changes in a git repository with automated backups and semantic diffs.
{: .fs-5 .fw-300 }

## Table of contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Overview

The `lox config` commands let you version-control your Miniserver configuration using git. Each pull downloads the config via FTP, decompresses the proprietary LoxCC format to XML, generates a semantic diff, and commits with a meaningful message.

## Setup

Initialize a git repository for config tracking:

```bash
lox config init ~/loxone-config
```

This creates a git repository where configs will be stored. Multi-Miniserver setups use subdirectories by serial number.

## Pull workflow

Download the current config, diff against the previous version, and commit:

```bash
lox config pull
```

The pull workflow:
1. Download config ZIP via FTP
2. Decompress LoxCC format to XML
3. Generate semantic diff (controls/rooms/users added/removed/renamed)
4. Commit with a meaningful message

Example commit message:

```
[504F94AABBCC] Config backup 2026-03-08 18:22:56 (v42)

+ Added control: "Garage Light" (Switch)
~ Light: "Licht EG" -> "Licht Erdgeschoss"
- Removed user: "guest"
```

## View history

```bash
lox config log                  # show change history
lox config log -n 5             # last 5 entries
```

## Restore a previous version

```bash
lox config restore abc123 --force
```

This uploads the original backup ZIP from git history to the Miniserver. No risky recompression — uses the exact file that was downloaded.

{: .warning }
Restoring a config uploads it to your live Miniserver. Always verify the commit before restoring.

## Automated backups

For nightly cron-based backups:

```bash
# Crontab entry
0 2 * * * /usr/local/bin/lox config pull --quiet
```

The `--quiet` flag suppresses output unless there's an error.

## Config inspection

You can also download and inspect configs without git versioning:

```bash
lox config download                       # download ZIP
lox config download --extract             # download + decompress
lox config ls                             # list configs on Miniserver
lox config extract config.zip             # decompress to XML
lox config users file.Loxone              # list user accounts
lox config devices file.Loxone            # list hardware devices
lox config diff old.Loxone new.Loxone     # compare two configs
```
