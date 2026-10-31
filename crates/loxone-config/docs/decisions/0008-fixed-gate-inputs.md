# D8: Gate inputs are fixed

Status: accepted (2026-08-25)

## Context

`And` and `Or` have the ports `I1`, `I2` and `Q`. Loxone Config 17 offers no way to grow a gate. A compiled
`I3` loads, but the next save deletes it and its wire without a warning (see
[loxone-format.md](../loxone-format.md#uuid-anatomy)). No corpus config has a gate with more than two inputs.

## Decision

Wiring `I3` or higher on a gate is a compile error. The error suggests cascading 2-input gates.

## Rejected

- Growing inputs with indexes after the builtin ports. It was the first assumption, and Loxone Config deletes
  them on save. Silent logic loss is the worst outcome a compiler can produce.
