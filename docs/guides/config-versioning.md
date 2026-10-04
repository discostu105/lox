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
3. Diff the logic against the previous snapshot: blocks, parameters and wires, page by page
4. Commit with a meaningful message

Example commit message:

```
[504F94AABBCC] Config backup 2026-03-08 18:22:56 (v42)

= 1 added · 1 renamed · 1 parameter · wires +1 −0

# Garage
+ block  Garage light (Switch)
~ rename Licht EG → Licht Erdgeschoss
~ param  Garage door · Runtime  20 s → 25 s
+ wire   Motion Garage.Q → Garage light.Tg
```

Hardware references are folded away (a wire runs from the motion sensor to the light, not to an
input ref), re-placed refs and moved blocks are no change at all, and saves that only touch the
layout say "No logic changes (layout or metadata only)." Long diffs are capped in the message;
`lox config diff` shows the whole thing.

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

### Push to a remote repository

Pair `lox config pull` with `git push` to keep a remote backup on GitHub:

```bash
0 2 * * * cd ~/loxone-config && /usr/local/bin/lox config pull --quiet && git push
```

### Kubernetes CronJob

Run config backups as a Kubernetes CronJob that pushes changes to a GitHub repository. This is useful when you already run a k8s cluster (e.g., for `lox otel serve`).

{: .note }
The `ghcr.io/discostu105/lox` Docker image is a scratch image (no shell). The CronJob uses `alpine` and downloads the `lox` binary at runtime.

**Prerequisites:**

1. A GitHub repository for config history
2. A GitHub PAT with **Contents read/write** scope
3. A PersistentVolumeClaim for the git repo (survives between runs)

**Secrets:**

```bash
# Miniserver credentials (reuse from lox otel if already deployed)
kubectl create secret generic lox-config \
  --from-file=config.yaml=/path/to/your/config.yaml

# GitHub PAT
kubectl create secret generic loxone-config-backup-github \
  --from-literal=token=ghp_your_token_here
```

**Deploy:**

```bash
kubectl apply -f k8s/config-backup-cronjob.yaml
```

The CronJob ([`k8s/config-backup-cronjob.yaml`](https://github.com/discostu105/lox/blob/main/k8s/config-backup-cronjob.yaml)):
- Runs every 6 hours (configurable via `schedule`)
- Downloads config, diffs the logic, commits (no-op if unchanged)
- Pushes to your GitHub remote
- Works on both `amd64` and `arm64` nodes

**Manual trigger:**

```bash
kubectl create job --from=cronjob/loxone-config-backup loxone-config-backup-manual
```

## Inspecting snapshots

Every snapshot in the repository can be inspected and compared:

```bash
lox config diff                           # the last change
lox config diff v271                      # from version 271 to the newest config
lox config diff 2026-08-25 HEAD           # by save date, commit, version — or two files
lox config diff a1b2c3d..e4f5a6b          # a range of commits
lox config show --at v271                 # the logic pages of an old snapshot
lox config show -p Garage --at v271       # a page as lxir source
lox config wiring "Garage light" --at v271 --trace up   # its wiring back then
lox config history "Garage light"         # every snapshot that changed this block
lox config lint                           # dead blocks, unwired inputs, duplicate names
```

In `lox tui`, *System › Config* lists the snapshots with their diffs: `⏎` browses a snapshot (pages,
blocks, lxir source, `/` search, `⏎` on a block opens its wiring at that snapshot), `m` marks a commit
to compare any two, and `w` on a diff line opens that block's wiring.

## Config inspection

You can also download and inspect configs without git versioning:

```bash
lox config download                       # download ZIP
lox config download --extract             # download + decompress
lox config ls                             # list configs on Miniserver
lox config extract config.zip             # decompress to XML
lox config users file.Loxone              # list user accounts
lox config devices file.Loxone            # list hardware devices
lox config diff old.Loxone new.Loxone     # compare two configs (.Loxone or backup ZIP)
lox config wiring "Licht" --file config.zip   # any of the logic views on a file
```
