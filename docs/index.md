---
title: Home
layout: default
nav_order: 1
---

<div class="hero">
  <div class="hero-badges">
    <span class="hero-badge">Single binary</span>
    <span class="hero-badge">No cloud</span>
    <span class="hero-badge">AI-ready</span>
    <span class="hero-badge">~4MB</span>
  </div>

  <h1 class="hero-title"><span class="hero-prompt">❯</span>lox</h1>
  <p class="hero-tagline">
    Fast, scriptable CLI for the Loxone Miniserver.<br>
    Control, monitor and version your smart home from the terminal.
  </p>

  <div class="install-block">
    <div class="install-row">
      <span class="install-label">macOS / Linux</span>
      <div class="install-cmd"><code>brew install discostu105/lox/lox</code><button class="install-copy" type="button" aria-label="Copy">Copy</button></div>
    </div>
    <div class="install-row">
      <span class="install-label">Windows (PowerShell)</span>
      <div class="install-cmd"><code>irm https://raw.githubusercontent.com/discostu105/lox/main/install.ps1 | iex</code><button class="install-copy" type="button" aria-label="Copy">Copy</button></div>
    </div>
  </div>

  <div class="hero-buttons">
    <a href="{{ site.baseurl }}/getting-started" class="btn-hero btn-hero-primary">Get Started</a>
    <a href="{{ site.baseurl }}/commands/" class="btn-hero btn-hero-secondary">Command Reference</a>
    <a href="https://github.com/discostu105/lox" class="btn-hero btn-hero-secondary">GitHub</a>
  </div>

  <figure class="hero-shot">
    <img src="{{ site.baseurl }}/media/lox-tui-demo.gif" alt="lox tui: rooms, live events and energy flow of a demo house" width="1280" height="720" loading="lazy">
  </figure>
</div>

<div class="features">
  <div class="feature-card">
    <span class="feature-icon"><svg viewBox="0 0 24 24" aria-hidden="true"><polyline points="4 17 10 11 4 5"/><line x1="12" x2="20" y1="19" y2="19"/></svg></span>
    <div class="feature-title">Script your home</div>
    <div class="feature-desc">Bash, Python, cron — use any tool. Chain commands with pipes and exit codes like any Unix CLI.</div>
  </div>
  <div class="feature-card">
    <span class="feature-icon"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 8V4H8"/><rect width="16" height="12" x="4" y="8" rx="2"/><path d="M2 14h2"/><path d="M20 14h2"/><path d="M15 13v2"/><path d="M9 13v2"/></svg></span>
    <div class="feature-title">AI agent ready</div>
    <div class="feature-desc">JSON output, schema discovery, fuzzy matching, dry-run mode, structured errors, and an MCP server (<code>lox mcp</code>).</div>
  </div>
  <div class="feature-card">
    <span class="feature-icon"><svg viewBox="0 0 24 24" aria-hidden="true"><rect width="7" height="9" x="3" y="3" rx="1"/><rect width="7" height="5" x="14" y="3" rx="1"/><rect width="7" height="9" x="14" y="12" rx="1"/><rect width="7" height="5" x="3" y="16" rx="1"/></svg></span>
    <div class="feature-title">Terminal UI</div>
    <div class="feature-desc"><code>lox tui</code>: rooms, live events, energy flow, system health and the wiring behind every control.</div>
  </div>
  <div class="feature-card">
    <span class="feature-icon"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="m10 13-2 2 2 2"/><path d="m14 17 2-2-2-2"/></svg></span>
    <div class="feature-title">Config as code</div>
    <div class="feature-desc">Write logic as <code>.lxir</code> text, review it in a pull request, deploy it verified with <code>lox config push</code>.</div>
  </div>
  <div class="feature-card">
    <span class="feature-icon"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4.9 19.1C1 15.2 1 8.8 4.9 4.9"/><path d="M7.8 16.2c-2.3-2.3-2.3-6.1 0-8.5"/><circle cx="12" cy="12" r="2"/><path d="M16.2 7.8c2.3 2.3 2.3 6.1 0 8.5"/><path d="M19.1 4.9C23 8.8 23 15.1 19.1 19"/></svg></span>
    <div class="feature-title">Real-time streaming</div>
    <div class="feature-desc">WebSocket state streaming with NDJSON output. Filter by room, type, or specific controls.</div>
  </div>
  <div class="feature-card">
    <span class="feature-icon"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M22 12h-4l-3 9L9 3l-3 9H2"/></svg></span>
    <div class="feature-title">OpenTelemetry</div>
    <div class="feature-desc">Push metrics, logs, and traces to Dynatrace, Datadog, Grafana Cloud, or any OTLP backend.</div>
  </div>
  <div class="feature-card">
    <span class="feature-icon"><svg viewBox="0 0 24 24" aria-hidden="true"><line x1="6" x2="6" y1="3" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/></svg></span>
    <div class="feature-title">Config versioning</div>
    <div class="feature-desc">GitOps for your Miniserver: every config change as a git commit with a logic diff.</div>
  </div>
  <div class="feature-card">
    <span class="feature-icon"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z"/><path d="m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65"/><path d="m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65"/></svg></span>
    <div class="feature-title">Multi-context</div>
    <div class="feature-desc">Several Miniservers as named contexts. Switch with <code>lox ctx use</code> or per command with <code>--ctx</code>.</div>
  </div>
  <div class="feature-card">
    <span class="feature-icon"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"/></svg></span>
    <div class="feature-title">Fast</div>
    <div class="feature-desc">~80ms warm, ~1.2s cold. A static Rust binary with zero runtime dependencies.</div>
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
<pre><code><span class="terminal-comment"># Discover your controls</span>
<span class="terminal-prompt">$</span> lox ls --type LightControllerV2 -o json | jq '.[].name'

<span class="terminal-comment"># Control devices</span>
<span class="terminal-prompt">$</span> lox on "Licht Wohnzimmer"
<span class="terminal-prompt">$</span> lox blind "Beschattung Süd" pos 50
<span class="terminal-prompt">$</span> lox thermostat "Heizung" temp 22.5

<span class="terminal-comment"># Conditional automation</span>
<span class="terminal-prompt">$</span> lox if "Temperatur" gt 28 && lox blind "Beschattung Süd" pos 70

<span class="terminal-comment"># Real-time monitoring</span>
<span class="terminal-prompt">$</span> lox stream --room "Kitchen" -o json

<span class="terminal-comment"># GitOps config backup, and why a light is on</span>
<span class="terminal-prompt">$</span> lox config pull
<span class="terminal-prompt">$</span> lox config wiring "Licht Wohnzimmer" --trace up</code></pre>
  </div>
</div>

---

<div class="section-label">Devices</div>

## Supported control types

| Type | Commands |
|:-----|:---------|
| `LightControllerV2` | `on`, `off`, `light mood <name> plus/minus/off/<id>`, `light moods <name>` |
| `Jalousie` / `CentralJalousie` | `blind <name> up/down/stop/shade/pos <0-100>` |
| `Switch` / `Pushbutton` | `on`, `off`, `input pulse` |
| `Dimmer` | `light dim <name> <0-100>` |
| `ColorPickerV2` | `light color <name> "#RRGGBB"` / `"hsv(h,s,v)"` / `"temp(brightness,kelvin)"` |
| `Gate` / `CentralGate` | `gate <name> open/close/stop` |
| `IRoomControllerV2` | `thermostat <name> temp <°C>` / `mode <auto…>` / `override <°C> [min]` |
| `Alarm` | `alarm <name> arm/arm-home/disarm/quit` |
| Door lock, intercom, wallbox | `door … lock/unlock/open`, `intercom … answer/hangup/open`, `charger … start/stop/pause` |
| Analog / virtual inputs | `input set <name> <value>` |
| Sensors, meters | `get`, `sensors`, `energy`, `history` (read-only) |
| Any | `send <name> <raw-command>`, `lock`, `unlock` |

---

<div class="section-label">Performance</div>

## Benchmarks

The structure (`LoxApp3.json`) is cached per context for 24 h:

| Operation | Cold | Warm |
|:----------|:-----|:-----|
| `lox on "Licht"` | ~1.2s | **~80ms** |
| `lox ls` | ~1.2s | **~80ms** |
| `lox ls --values` | ~1.2s + N reqs | slower |
| `lox status` | ~120ms | **~120ms** |

<script>
  document.querySelectorAll('.install-copy').forEach(function (btn) {
    btn.addEventListener('click', function () {
      var text = btn.parentElement.querySelector('code').textContent;
      navigator.clipboard.writeText(text).then(function () {
        btn.textContent = 'Copied';
        setTimeout(function () { btn.textContent = 'Copy'; }, 1500);
      });
    });
  });
</script>
