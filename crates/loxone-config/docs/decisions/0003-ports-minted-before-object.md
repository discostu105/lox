# D3: Ports are minted before their object

Status: accepted (2026-08-24)

## Context

In real Loxone Config output the ports of a block take the lower counter values and the object the next one,
for example ports `…0749`, `…074a`, `…074b`, then object `…074c` (see
[loxone-format.md](../loxone-format.md#uuid-anatomy)).

## Decision

The compiler mints a block's port UUIDs first and its object UUID after them, in the same counter order.
