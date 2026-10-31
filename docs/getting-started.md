---
title: Getting Started
layout: default
nav_order: 2
---

# Getting Started
{: .no_toc }

## Table of contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Requirements

- Loxone Miniserver Gen 1 or Gen 2, firmware 12.0+
- Network access to the Miniserver (local, VPN, or a DynDNS/port-forward)
- For `lox log`: Admin user required
- **Platforms:** Windows (x86_64, ARM64), macOS (x86_64, Apple Silicon), Linux (x86_64, ARM64)

## Installation

### Windows (PowerShell)

One-liner — downloads the latest release and adds it to your PATH:

```powershell
irm https://raw.githubusercontent.com/discostu105/lox/main/install.ps1 | iex
```

Or install manually: download `lox-windows-x86_64.exe` from the [latest release](https://github.com/discostu105/lox/releases/latest), rename to `lox.exe`, and place it somewhere on your PATH (e.g. `C:\Users\<YOU>\AppData\Local\lox\`).

### Homebrew (macOS / Linux)

```bash
brew tap discostu105/lox https://github.com/discostu105/lox
brew install discostu105/lox/lox
```

### Build from source (all platforms)

```bash
git clone https://github.com/discostu105/lox
cd lox
cargo build --release
```

The binary is at `target/release/lox` (or `lox.exe` on Windows). Copy it somewhere on your PATH.

**Build requirements:** Rust 1.91+. No OpenSSL needed.

---

## Setup

Configure your Miniserver connection:

```bash
lox setup set --host https://192.168.1.100 --user USER --pass PASS
```

{: .tip }
Use a dedicated Miniserver user for `lox` rather than your admin account; give it admin rights only if you need
`lox log`, config download/push or firmware updates.

`--serial` (optional) names this Miniserver's folder in a [config repo](/lox/guides/config-versioning), becomes its
OpenTelemetry `device.id`, and lets `lox config push` refuse the wrong Miniserver. Certificate checks are off by
default (Miniservers use self-signed certs); `--verify-ssl` turns them on.

Verify the connection:

```bash
lox status
```

Config is stored at:
- **macOS/Linux:** `~/.lox/config.yaml`
- **Windows:** `C:\Users\<YOU>\.lox\config.yaml`

### Environment variables

All config fields can be overridden via environment variables:

```bash
LOX_HOST=https://192.168.1.100 LOX_USER=admin LOX_PASS=secret lox status
```

| Variable | Config field |
|:---------|:-------------|
| `LOX_HOST` | `host` |
| `LOX_USER` | `user` |
| `LOX_PASS` | `pass` |
| `LOX_SERIAL` | `serial` |
| `LOX_VISU_PASSWORD` | the visualization password itself, for secured controls (or `visu_password_file`) |

---

## First commands

```bash
lox rooms                              # what's there
lox ls -r Wohnzimmer                   # controls in a room (-t type, -c category)
lox get "Licht Wohnzimmer"             # live state of one control
lox on "Licht Wohnzimmer"              # act on it
lox --dry-run blind "Süd" pos 50       # see what would be sent, without sending
lox tui                                # or browse everything live
```

Names are case-insensitive substrings: `lox on wohnz` works if only one control matches. When several do,
`lox` lists them; pick one with `-r <room>`, `"Name [Room]"` or the UUID.

Every command takes `-o json` for scripts (`lox ls -o json | jq '.[].name'`) and exits non-zero on errors.

---

## Multiple Miniservers

Manage multiple Miniserver connections with named contexts (similar to `kubectl config use-context`):

```bash
lox ctx add home --host https://192.168.1.100 --user admin --pass secret
lox ctx add office --host https://10.0.0.50 --user admin --pass secret
lox ctx use home              # switch active context
lox ctx home                  # shortcut for `lox ctx use home`
lox --ctx office status       # one-off command against a different context
```

Existing single-Miniserver configs continue to work. Use `lox ctx migrate` to convert a flat config to a named context when you're ready.

See the [Context Management](https://github.com/discostu105/lox/blob/main/COMMANDS.md#context-management) reference for all options.

---

## Aliases

Add short names for frequently-used controls:

```bash
lox alias add wz "1d8af56e-036e-e9ad-ffffed57184a04d2"
lox alias add kueche "20236c09-0055-6e94-ffffed57184a04d2"
lox alias ls
```

Then use them directly:

```bash
lox on wz
lox off kueche
```

Aliases are stored in `~/.lox/config.yaml`:

```yaml
host: https://192.168.1.100
user: admin
pass: secret
aliases:
  wz: "1d8af56e-036e-e9ad-ffffed57184a04d2"
  kueche: "20236c09-0055-6e94-ffffed57184a04d2"
```

---

## Shell completions

**Homebrew** installs completions automatically.

**Manual install** (auto-detects your shell):

```bash
lox completions --install
```

Or generate to stdout for custom setups:

```bash
lox completions bash
lox completions zsh
lox completions fish
lox completions powershell
```

Manual installation paths:

```bash
# Bash
lox completions bash > /etc/bash_completion.d/lox

# Zsh
lox completions zsh > ~/.zfunc/_lox

# Fish
lox completions fish > ~/.config/fish/completions/lox.fish
```

**PowerShell** — add this to your `$PROFILE`:

```powershell
lox completions powershell | Out-String | Invoke-Expression
```

---

## Token authentication

Live connections (`lox stream`, `lox tui`, `lox otel`, `lox light moods`) log in over WebSocket with a token,
which `lox` requests on first use, saves, and reuses while it is valid (~20 days). To manage it yourself:

```bash
lox token fetch    # acquire and save token
lox token info     # check token status
lox token refresh  # extend validity
```

Plain HTTP commands keep using Basic Auth. See [Token auth](https://github.com/discostu105/lox/blob/main/COMMANDS.md#token-auth) for all token
commands.

---

## Discover Miniservers

Find Miniservers on your local network:

```bash
lox discover
lox discover --timeout 5
```

---

## Next steps

- Browse the [Command Reference](/lox/commands/) for all available commands
- Explore your house in the [Terminal UI](/lox/guides/tui)
- Learn about [Scenes](/lox/guides/scenes) for multi-step automation
- Set up [Config Versioning](/lox/guides/config-versioning) for GitOps backups
- Manage logic as text with [Config as Code](/lox/guides/config-as-code)
- Integrate with [AI Agents](/lox/guides/ai-agents)
- Export data with [OpenTelemetry](/lox/guides/opentelemetry)
