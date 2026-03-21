---
title: Home
layout: default
nav_order: 1
---

<div class="hero">
  <div class="hero-badges">
    <span class="hero-badge"><span class="hero-badge-icon">&#9889;</span> Single binary</span>
    <span class="hero-badge"><span class="hero-badge-icon">&#128274;</span> No cloud</span>
    <span class="hero-badge"><span class="hero-badge-icon">&#129302;</span> AI-ready</span>
    <span class="hero-badge"><span class="hero-badge-icon">&#128230;</span> ~4MB</span>
  </div>

  <h1 class="hero-title">lox</h1>
  <p class="hero-tagline">
    Fast, scriptable CLI for Loxone Miniserver.<br>
    Control lights, blinds, and more from your terminal.
  </p>

  <div class="install-block">
    <code>brew install discostu105/lox/lox</code>
  </div>

  <div class="hero-buttons">
    <a href="/lox/getting-started" class="btn-hero btn-hero-primary">Get Started</a>
    <a href="/lox/commands/" class="btn-hero btn-hero-secondary">Command Reference</a>
    <a href="https://github.com/discostu105/lox" class="btn-hero btn-hero-secondary">GitHub</a>
  </div>
</div>

<div class="features">
  <div class="feature-card">
    <span class="feature-icon">&#128161;</span>
    <div class="feature-title">Script your home</div>
    <div class="feature-desc">Bash, Python, cron — use any tool. Chain commands with pipes and exit codes like any Unix CLI.</div>
  </div>
  <div class="feature-card">
    <span class="feature-icon">&#129302;</span>
    <div class="feature-title">AI agent ready</div>
    <div class="feature-desc">JSON output, schema discovery, fuzzy matching, dry-run mode, and structured errors designed for LLM tool use.</div>
  </div>
  <div class="feature-card">
    <span class="feature-icon">&#128225;</span>
    <div class="feature-title">Real-time streaming</div>
    <div class="feature-desc">WebSocket state streaming with NDJSON output. Filter by room, type, or specific controls.</div>
  </div>
  <div class="feature-card">
    <span class="feature-icon">&#128202;</span>
    <div class="feature-title">OpenTelemetry</div>
    <div class="feature-desc">Push metrics, logs, and traces to Dynatrace, Datadog, Grafana Cloud, or any OTLP backend.</div>
  </div>
  <div class="feature-card">
    <span class="feature-icon">&#128196;</span>
    <div class="feature-title">Config versioning</div>
    <div class="feature-desc">GitOps for your Miniserver. Track config changes with semantic diffs and git history.</div>
  </div>
  <div class="feature-card">
    <span class="feature-icon">&#9889;</span>
    <div class="feature-title">Fast</div>
    <div class="feature-desc">~80ms warm, ~1.2s cold. Structure cache with 24h TTL. Static Rust binary with zero runtime dependencies.</div>
  </div>
</div>

---

<div class="section-label">Quick start</div>

## See it in action

<div class="terminal">
  <div class="terminal-header">
    <span class="terminal-dot terminal-dot-red"></span>
    <span class="terminal-dot terminal-dot-yellow"></span>
    <span class="terminal-dot terminal-dot-green"></span>
    <span class="terminal-title">Terminal</span>
  </div>
  <div class="terminal-body">

```bash
# Discover your controls
lox ls --type LightControllerV2 -o json | jq '.[].name'

# Control devices
lox on "Licht Wohnzimmer"
lox blind "Beschattung Sud" pos 50
lox thermostat "Heizung" temp 22.5

# Conditional automation
lox if "Temperatur" gt 28 && lox blind "Beschattung Sud" pos 70

# Real-time monitoring
lox stream --room "Kitchen" -o json

# GitOps config backup
lox config pull
```

  </div>
</div>

---

<div class="section-label">Devices</div>

## Supported control types

| Type | Commands |
|:-----|:---------|
| `LightControllerV2` | `on`, `off`, `mood plus/minus/off/<id>` |
| `Jalousie` / `CentralJalousie` | `up`, `down`, `stop`, `pos <0-100>`, `shade`, `full-up`, `full-down` |
| `Switch` | `on`, `off`, `pulse` |
| `Dimmer` | `dimmer <name> <0-100>` |
| `Gate` / `CentralGate` | `gate <name> open/close/stop` |
| `ColorPickerV2` | `color <name> #RRGGBB` or `hsv(h,s,v)` |
| `IRoomControllerV2` | `thermostat <name> --temp/--mode/--override` |
| `Alarm` | `alarm <name> arm/disarm/quit` |
| `InfoOnlyAnalog` / `Meter` | `get` (read-only) |
| Any | `send <uuid> <raw-command>`, `lock`, `unlock` |

---

<div class="section-label">Performance</div>

## Benchmarks

Structure cache at `~/.lox/cache/structure.json` (24h TTL):

| Operation | Cold | Warm |
|:----------|:-----|:-----|
| `lox on "Licht"` | ~1.2s | **~80ms** |
| `lox ls` | ~1.2s | **~80ms** |
| `lox ls --values` | ~1.2s + N reqs | slower |
| `lox status` | ~120ms | **~120ms** |
