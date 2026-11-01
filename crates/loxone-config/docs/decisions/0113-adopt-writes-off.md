# D113: Adopt writes `off` where a new object would tick the box

Status: accepted (2026-11-01), amends [D91](0091-off-is-no-attribute.md), follows
[D112](0112-zero-is-no-attribute.md)

## Context

D91 kept decompile and adopt as they were: a missing `Signed=`, `Analog=` or `CloseAfterSend=` lifts as
nothing, and they never produce `off` from a real config. Compiled onto the same config that is right,
since the attribute stays absent. But a new object of that source gets the mint defaults:
`Signed="true"` and `Analog="true"` on `VirtualHttpInCmd` and `VirtualUdpInCmd`, `CloseAfterSend="true"`
on `VirtualOut`. The box turned on when the adopted source was compiled onto another config (#333).

D112 fixed the same gap for numbers: a missing setting lifts as 0 where a new object would get another
default.

## Decision

- Decompile and adopt write `off` for a missing box that a new object would tick
  (`periphery::missing_value`). `off` on these compiles to no attribute (D91), so the output onto the
  same config does not change.
- A box a new object also leaves unticked (`Analog` on `VirtualOutCmd`) still lifts as nothing.

## Rejected

- **Keep lifting nothing**: the source would mean one thing on its own config and another on a fresh
  one.
- **Always lift `off`** for a missing box: it adds lines that say nothing a new object would not do.
