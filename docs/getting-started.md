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
- Local network access (or DynDNS with serial configured)
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

**Build requirements:** Rust 1.91+. No OpenSSL. No runtime dependencies.

---

## Setup

Configure your Miniserver connection:

```bash
lox setup set --host https://192.168.1.100 --user USER --pass PASS
```

With serial number for correct TLS hostname (avoids cert warnings):

```bash
lox setup set --host https://192.168.1.100 --user USER --pass PASS --serial YOUR_SERIAL
```

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

For better security than Basic Auth, use token authentication (valid ~20 days):

```bash
lox token fetch    # acquire and save token
lox token info     # check token status
lox token refresh  # extend validity
```

Once fetched, the token is used automatically for all requests. See the [Token Auth](/lox/commands/configuration#token-auth) section for all token commands.

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
- Learn about [Scenes](/lox/guides/scenes) for multi-step automation
- Set up [Config Versioning](/lox/guides/config-versioning) for GitOps backups
- Integrate with [AI Agents](/lox/guides/ai-agents)
- Export data with [OpenTelemetry](/lox/guides/opentelemetry)
