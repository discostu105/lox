---
title: Scenes
layout: default
parent: Guides
nav_order: 2
---

# Scenes
{: .no_toc }

Multi-step automation sequences defined as YAML files.
{: .fs-5 .fw-300 }

## Table of contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Overview

Scenes let you define a sequence of commands that run in order. They are YAML files in `~/.lox/scenes/`, or
`~/.lox/contexts/<name>/scenes/` when you use [contexts](https://github.com/discostu105/lox/blob/main/COMMANDS.md#context-management), so each
Miniserver has its own.

## Creating a scene

Create a new scene file:

```bash
lox scene new abend
```

This creates `~/.lox/scenes/abend.yaml`. Edit it to define your steps:

```yaml
# ~/.lox/scenes/abend.yaml
name: Abend
steps:
  - control: "Lichtsteuerung Wohnzimmer"
    cmd: "on"
  - control: "Beschattung Südseite"
    cmd: "pos 70"
  - control: "LED Küche"
    cmd: "off"
    delay_ms: 500
```

## Scene format

| Field | Required | Description |
|:------|:---------|:------------|
| `name` | Yes | Display name for the scene |
| `steps` | Yes | List of command steps |
| `steps[].control` | Yes | Control name (fuzzy matched) |
| `steps[].cmd` | Yes | Command to send: `on`, `off`, `pos 70`, `plus`, or any raw Loxone command (as with `lox send`) |
| `steps[].delay_ms` | No | Delay in milliseconds before this step |

## Running scenes

```bash
lox run abend                    # run the scene
lox run abend --dry-run          # preview without executing
```

The `--dry-run` flag resolves all control names and shows what would execute, without actually sending commands.

## Managing scenes

```bash
lox scene ls                     # list all scenes
lox scene show abend             # print YAML definition
```

## Tips

- Controls are resolved using the same fuzzy matching as regular commands
- Disambiguate with the bracket syntax: `control: "Licht [Schlafzimmer]"`
- Quote `on`/`off` in YAML (`cmd: "on"`): unquoted, YAML may read them as booleans
- `lox tui` lists your scenes in the palette (`:`), and AI agents can run them through the [MCP server](ai-agents#mcp-server)
- Add `delay_ms` between steps when the Miniserver needs time to process
- Scene files can be version-controlled alongside your other dotfiles
