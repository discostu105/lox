# D27: Values with a unit suffix, scaled at compile

Status: accepted (2026-08-25), amended by [D52](0052-port-facts.md)

## Decision

A number may carry a unit suffix written right after it: `Time: 90min`, `TimeHigh: 250ms`,
`DayMinTemp: 2700K`, `TargetPos: 70%`.

- Time units (`ms`, `s`, `min`, `h`) scale exactly into the port's unit, with decimal integer arithmetic and
  no floats. A port without a documented unit is in seconds. [D52](0052-port-facts.md) has the per-port
  units, and where `%` and `K` bind.
- On an undocumented port, `K` and `%` are annotations with factor 1.
- The suffix is the canonical spelling: `1.5h` stays `1.5h` through `fmt`. The compiled `Def=` is
  byte-identical to the plain number.
- A quoted string that looks like a unit value (`"40s"`) stays a string.
- Decompile lifts plain numbers. It cannot know a port's unit.
