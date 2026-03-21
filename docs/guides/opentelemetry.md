---
title: OpenTelemetry
layout: default
parent: Guides
nav_order: 4
---

# OpenTelemetry Export
{: .no_toc }

Push metrics, logs, and traces to any OTLP-compatible backend.
{: .fs-5 .fw-300 }

## Table of contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Overview

`lox otel` exports your smart home data via the OpenTelemetry Protocol (OTLP) to backends like Dynatrace, Datadog, Grafana Cloud, Prometheus, or any OTLP-compatible collector.

### What gets exported

**Metrics**: Control state gauges, system diagnostics (CPU, heap, tasks), network counters (CAN/LAN), weather data.

**Logs**: State change events, text-state messages, Miniserver system log entries.

**Traces**: Synthetic automation traces — correlates autopilot rule fires with temporally-close state changes.

## Continuous export

Run as a daemon that pushes data at regular intervals:

```bash
# Push metrics + logs + traces every 30 seconds
lox otel serve --endpoint http://localhost:4318 --interval 30s

# With auth header (Dynatrace, Datadog, etc.)
lox otel serve --endpoint https://otlp.example.com:4318 \
  --header "Authorization=Bearer xxx" --interval 1m

# Filter by room or control type
lox otel serve --endpoint ... --room "Kitchen" --type LightControllerV2

# Metrics only (disable logs and traces)
lox otel serve --endpoint ... --no-logs --no-traces

# Metrics + logs only
lox otel serve --endpoint ... --no-traces
```

| Flag | Description |
|:-----|:------------|
| `--endpoint` | OTLP endpoint URL |
| `-i`, `--interval` | Push interval (e.g., `30s`, `1m`, `5m`) |
| `-t`, `--type` | Filter by control type |
| `-r`, `--room` | Filter by room |
| `--header` | HTTP header for auth (`Key=Value`) |
| `--delta` | Use delta temporality for counters |
| `--no-logs` | Disable log export |
| `--no-traces` | Disable trace export |

## One-shot push

For cron jobs or periodic collection:

```bash
# Push once and exit
lox otel push --endpoint http://localhost:4318

# Metrics only
lox otel push --endpoint http://localhost:4318 --no-logs
```

## Cron example

```bash
# Push metrics every 5 minutes
*/5 * * * * /usr/local/bin/lox otel push --endpoint http://localhost:4318
```
