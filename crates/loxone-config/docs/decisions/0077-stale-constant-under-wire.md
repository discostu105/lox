# D77: A stale constant under a wire

Status: accepted (2026-10-31)

## Context

Loxone Config keeps a connector's `Def=` when a wire is drawn onto it (see
[loxone-format.md](../loxone-format.md#connectors-and-wires-co--in)). Decompile printed it as a second
argument, `Input1: 2, Input1: x.AQ`. That read as a fan-in or an override, and passed `loxl check`.
Issue: #226.

## Decision

- **Decompile and adopt print the wire only**, with the constant as a comment on it:
  `Input1: x.AQ, # stale constant 2 kept under the wire`. A constant at the observed GUI default is
  elided like any other ([D30](0030-observed-defaults.md)).
- **The compile carries the base's `Def=`** onto a port the source still wires. It is read from the base
  with the block's other GUI residue ([D19](0019-gui-residue-carried-forward.md)), so an adoption rebuilds
  byte-identically.
- **Unwiring the port in source drops the constant with the wire.** The source states what the port
  carries; a value it wants is written as `Input1: 2`.
- **A value next to a wire on one port is an error**, like a duplicate parameter. This holds also with an
  expression. Fan-in, several different wires, stays allowed.
- The messages of the duplicate family name the block first (`` block `b`: … ``), so the language server
  puts them on the port key in the call.
