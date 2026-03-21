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

## Installation

### Homebrew (macOS / Linux)

```bash
brew tap discostu105/lox https://github.com/discostu105/lox
brew install discostu105/lox/lox
```

### Build from source

```bash
git clone https://github.com/discostu105/lox
cd lox
cargo build --release
cp target/release/lox ~/.local/bin/
```

**Build requirements:** Rust 1.91+. No OpenSSL. No runtime dependencies.

The release binary is ~4MB, statically linked with rustls for TLS.

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

Config is stored at `~/.lox/config.yaml`.

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
