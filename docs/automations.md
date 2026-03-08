# Automations

`~/.lox/automations.yaml` defines rules that the `lox daemon` evaluates on every
state change received over WebSocket (or HTTP polling with `--poll`).

## File structure

```yaml
rules:
  - when: "Control name or UUID"
    op: changes         # operator (see below)
    value: "1"          # target value (omit for "changes")
    also:               # optional: additional AND conditions
      - control: "Another control"
        op: eq
        value: "0"
    only_between: "08:00-22:00"   # optional: time window (local time)
    cooldown_secs: 30             # optional: minimum seconds between re-triggers
    run: "lox on 'Entrance Light'"  # shell command to run
    description: "Turn on light when doorbell rings"  # optional
```

## Operators

| Op | Aliases | Meaning |
|----|---------|---------|
| `changes` | — | Fire on any value transition (ignores `value` field) |
| `eq` | `==` | Equal (numeric or string) |
| `ne` | `!=` | Not equal |
| `gt` | `>` | Greater than (numeric) |
| `lt` | `<` | Less than (numeric) |
| `ge` | `>=` | Greater than or equal (numeric) |
| `le` | `<=` | Less than or equal (numeric) |

## Fields

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `when` | yes | — | Control name (fuzzy), UUID, or alias |
| `op` | no | `changes` | Comparison operator |
| `value` | no | — | Target value for `eq`/`ne`/`gt`/`lt`/`ge`/`le` |
| `also` | no | `[]` | Additional AND conditions (control + op + value) |
| `only_between` | no | — | Time window `"HH:MM-HH:MM"` in local time |
| `cooldown_secs` | no | `5` | Minimum seconds between rule re-triggers |
| `run` | yes | — | Shell command to execute when rule fires |
| `description` | no | — | Human-readable description (displayed in `lox automation list`) |

## `changes` semantics

`changes` triggers whenever the current value differs from the previous value.
On first observation (no previous value), it always triggers.
For non-`changes` rules, the daemon only fires on a **false→true transition**
(i.e. it won't re-fire while the condition remains true).

## Time windows

`only_between: "HH:MM-HH:MM"` uses **local time** (or the timezone set in
`config.yaml` via `lox config set --timezone Europe/Vienna`).

Overnight windows are supported: `"22:00-06:00"` means "after 22:00 or before 06:00".

## Examples

```yaml
rules:
  # Turn on entrance light when doorbell rings
  - when: "Doorbell"
    op: eq
    value: "1"
    run: "lox on 'Entrance Light'"
    cooldown_secs: 30

  # Shade south windows when it gets hot (only during the day)
  - when: "Temperature Living Room"
    op: gt
    value: "25"
    only_between: "08:00-20:00"
    run: "lox blind 'South Blind' shade"
    cooldown_secs: 600

  # Log all light controller changes
  - when: "Living Room Lights"
    op: changes
    run: "echo \"$(date): light changed\" >> /tmp/lox.log"

  # Multi-condition: shade only if hot AND blinds not already down
  - when: "Temperature"
    op: gt
    value: "26"
    also:
      - control: "South Blind"
        op: ne
        value: "1"
    run: "lox blind 'South Blind' full-down"
    cooldown_secs: 900
```

## Commands

```bash
lox daemon              # start WebSocket daemon
lox daemon --poll       # start HTTP polling daemon (no Monitor rights needed)
lox daemon --poll --interval 5  # poll every 5s (default 3s)
lox automation list     # show loaded rules
lox automation check    # verify rule targets resolve correctly
lox automation edit     # print path to automations file
```
