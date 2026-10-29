---
title: Configuration
layout: default
parent: Command Reference
nav_order: 4
---

# Configuration Commands
{: .no_toc }

## Table of contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Setup

```bash
lox setup set --host https://192.168.1.100 --user admin --pass secret
lox setup set --serial 504F94112233        # config repo folder, OTel device.id; checked by config push
lox setup set --verify-ssl                 # enable cert verification
lox setup set --no-verify-ssl              # disable (default, self-signed)
lox setup set --visu-password-file ~/.lox/visu-password  # visualization password for secured controls
lox setup show                             # show config (password redacted)
```

`setup set` changes only the fields you pass. All fields also read environment variables (`LOX_HOST`, `LOX_USER`,
`LOX_PASS`, `LOX_SERIAL`); `read -rs LOX_PASS && export LOX_PASS` keeps the password out of the process list and
your shell history.

Secured controls (an NFC Code Touch's learn mode, for one) need the **visualization password** of the user lox
connects with. lox reads it from `LOX_VISU_PASSWORD` or from the first line of the file `visu_password_file:`
names (`~/` expands; set it with `--visu-password-file`), never from an argument, and never prints it. Keep the
file private (`chmod 600`).

To have `lox tui` and the MCP server ask before acting on a control, list it under `confirm:` in the config
(UUID, alias, or name with an optional `[Room]`):

```yaml
confirm:
  - Pool Abdeckung [Pool]
```

---

## Aliases

```bash
lox alias add wz "1d8af56e-036e-e9ad-ffffed57184a04d2"
lox alias remove wz
lox alias ls
```

Then use directly: `lox on wz`, `lox get wz`. Find a UUID with `lox ls -o json` or `lox info <name>`.

---

## Cache

The structure cache stores the Miniserver's `LoxApp3.json` per context with a 24-hour TTL. Run `lox cache refresh`
after you change the config, so new controls and names show up.

```bash
lox cache info               # show cache age and path
lox cache check              # check if cache is current
lox cache refresh            # force re-fetch
lox cache clear              # delete local cache
```

---

## Token auth

More secure than Basic Auth. Tokens are valid ~20 days.

```bash
lox token fetch              # fetch and save new token
lox token info               # show token status
lox token check              # verify token on Miniserver
lox token refresh            # extend validity
lox token revoke             # revoke token on Miniserver
lox token clear              # delete local token file
```

The token is acquired with an RSA+AES key exchange over WebSocket and used by WebSocket sessions (`stream`, `tui`,
`otel`, `light moods`), which request and save one on their own when needed. HTTP commands use Basic Auth.

---

## Scenes

```bash
lox scene ls                 # list all scenes
lox scene show abend         # print YAML definition
lox scene new abend          # create empty scene file
```

Scene files are stored in `~/.lox/scenes/*.yaml` (with contexts: `~/.lox/contexts/<name>/scenes/`). See the [Scenes guide](/lox/guides/scenes) for details.

---

## Loxone Config management

Download, inspect, and manage Loxone Config files:

```bash
lox config download                       # download latest config ZIP via FTP
lox config download --extract             # download + decompress to .Loxone XML
lox config download --save-as config.zip  # custom output filename
lox config ls                             # list available configs
lox config extract config.zip             # decompress LoxCC to .Loxone XML
lox config extract config.zip --save-as out.Loxone
lox config compress out.Loxone --template config.zip  # upload ZIP: config.zip with only sps0.LoxCC replaced
lox config upload config.zip --force      # upload as sps_new.zip, loaded on the next reboot
lox config users file.Loxone              # list user accounts
lox config devices file.Loxone            # list hardware devices
```

`upload` refuses anything that is not a config ZIP and only stages it: `lox reboot --yes` loads it. To upload,
reboot and verify in one go, deploy with [`lox config push`](/lox/guides/config-as-code#deploy-with-lox-config-push).

{: .warning }
An upload replaces the whole program and deletes the Miniserver's custom changes: what the app, the web UI or the
API changed and Loxone Config has not saved (light moods, timers, autopilot rules). `upload` lists them first.

### Inspecting the logic

Read-only views of the program in a config — blocks, parameters and wires, with hardware refs folded to
the device and room they live in:

```bash
lox config diff                           # the last change in the config repo
lox config diff v271 HEAD                 # two snapshots: commit, version, save date — or two files
lox config show                           # logic pages with block and wire counts
lox config show -p Wohnzimmer             # one page as lxir source
lox config wiring "Licht" --trace up      # what drives a block, back to the sensors
lox config find bewegung küche            # search blocks
lox config lint                           # dead blocks, unwired inputs and motors, duplicate names
lox config history "Licht"                # how one block changed over time
```

They read the config repo's working copy or the cached download; `--at SNAPSHOT`, `--file PATH` or
`--download` (read-only FTP) pick another config.

### Git-based config versioning

Track configuration changes in a git repository:

```bash
lox config init ~/loxone-config           # initialize git repo
lox config pull                           # download, diff, and git-commit
lox config pull --quiet                   # cron-friendly
lox config log                            # show change history
lox config log -c 5                       # last 5 entries
lox config restore abc123 --force         # restore from git history
```

See the [Config Versioning guide](/lox/guides/config-versioning) for the full workflow.

### Config as code

```bash
lox ir check                              # validate .lxir modules (in a lox.toml project)
lox ir compile                            # base + module + lock → out.Loxone
lox config diff base.Loxone out.Loxone    # review the change by name
lox config push                           # drift check, compile, stage (dry run)
lox config push --force                   # upload, reboot, verify, move base
lox ir help                               # every lxir command and flag
```

See the [Config as Code guide](/lox/guides/config-as-code).

---

## Context management

Manage multiple Miniserver connections, similar to `kubectl config use-context`:

```bash
lox ctx add home --host https://192.168.1.100 --user admin --pass secret
lox ctx add office --host https://10.0.0.50 --user admin --pass secret --serial 504F94AABBCC
lox ctx use home                # switch active context
lox ctx home                    # shortcut for `lox ctx use home`
lox ctx list                    # list all contexts (* = active)
lox ctx current                 # show active context
lox ctx remove office           # remove a context
lox ctx rename home house       # rename a context
```

Use `--ctx <name>` on any command for a one-off override:
```bash
lox --ctx office status         # run against 'office' without switching
```

### Project-local config

```bash
lox ctx init                    # create .lox/ in current directory
lox ctx init --host https://192.168.1.100 --user admin --pass secret  # with connection details
```

Project-local `.lox/config.yaml` is auto-discovered by walking up from cwd (like `.git`). Each context gets its own cache, token, and scenes directory. Secrets are excluded via `.lox/.gitignore`.

### Migration from flat config

```bash
lox ctx migrate                 # convert existing config to 'default' context
```

Existing flat `~/.lox/config.yaml` files continue to work unchanged.

### Multi-context config format

```yaml
active_context: home
contexts:
  home:
    host: https://192.168.1.100
    user: admin
    pass: secret
  office:
    host: https://10.0.0.50
    user: admin
    pass: secret
```

### Config resolution order

1. `LOX_CONFIG` env var (absolute priority)
2. Project-local `.lox/config.yaml` (walk up from cwd)
3. Global `~/.lox/config.yaml` (flat or multi-context)
4. `--ctx` flag overrides context selection within global config

---

## Shell completions

```bash
lox completions bash                      # generate bash completions
lox completions zsh                       # generate zsh completions
lox completions fish                      # generate fish completions
lox completions powershell                # generate PowerShell completions
lox completions --install                 # auto-detect and install
```

---

## Command schema

For AI agent integration — discover available commands programmatically:

```bash
lox schema                                # list all commands with metadata
lox schema blind                          # schema for a specific command
lox schema -o json                        # JSON for programmatic use
```

---

## MCP server

Expose `lox` to MCP clients (Claude Desktop, ChatGPT, Cursor, Claude Code, …):

```bash
lox mcp serve                             # stdio server (MCP 2026-07-28 + older), launched by the client
lox mcp serve --read-only                 # read tools only
lox mcp serve --allow-risky               # doors, gates, alarm without asking the user
lox mcp serve --allow-raw                 # also expose send_command
lox mcp config                            # print client config snippets (takes the same policy flags)
lox mcp tools                             # list exposed tools
```

See the [AI agent guide](../guides/ai-agents#mcp-server) for tools and the safety model.
