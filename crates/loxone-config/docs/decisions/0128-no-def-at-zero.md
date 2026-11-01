# D128: A connector's 0 is no `Def=`

Status: accepted (2026-11-01), amends [D30](0030-observed-defaults.md), extends
[D112](0112-zero-is-no-attribute.md)

## Context

Oracle session 83 compiled `p1.Q < 0` into a `Less` block with `<Co K="Input2" Def="0" …/>`. The save wrote
the connector without `Def=`, so `loxl diff` showed `Input2: Some("0") -> None` and the next compile wrote it
back (#352). Lox Logic wrote every bound value, from a block parameter, an expression's operand and an extern
`x.Port = 0` alike. The question was which block types drop a 0, and what a missing `Def=` means.

- **Corpus** (131 `.Loxone` files): about 3000 `Def=` values on block connectors, across 60 types, and not one
  is 0. Where the GUI default is not 0, the GUI writes it: every `Monoflop` holds `Def=` on `Time`, every
  `LightController2` on `Max`, `Step`, `MoveTimeout` and the rest.
- **Oracle session 90** set 22 connectors on 15 types to 0: the six comparators, `And` and `Or`, `Formula`,
  `Monoflop`, `PulseGen`, `AnalogThresholdTrigger`, `OffDelay`, `OnDelay`, `PButtonT`, `StairwayLS`,
  `EdgeDetection` and `AnalogMultiplexer2`, `0` and `0.0`. The save dropped every one, on ports whose GUI
  default is 0 and on ports whose default is not (`Monoflop.Time` 2, `AnalogThresholdTrigger.On` 5). A
  `Monoflop` and an `AnalogThresholdTrigger` without `Def=` stayed without one.
- **Oracle session 91** showed that `Monoflop` in the property panel: *Duration* 0. A missing `Def=` is 0, not
  the type's default.

## Decision

- A bound 0 on a block's connector writes no `Def=`: a parameter, an expression's operand, a `defaults`
  value, and an extern `x.Port = 0`, which removes the attribute. A withdrawn extern 0 restores the original
  with the tombstone of D112.
- The simulator reads a missing `Def=` as 0. The documented default counts only for a port the object has no
  connector for.
- Decompile and adopt lift `Port: 0` where `Def=` is missing on an unwired port whose observed GUI default
  (D30) is not 0. A reader of the source would otherwise assume the default. Elsewhere a missing `Def=`
  lifts as nothing, which compiles back to the same 0.
- A `Def=` under a wire stays as the base carries it (D77).

D30 said a missing `Def=` means the default on both sides. It means 0. A block Lox Logic mints without a value
for a parameter therefore has that parameter at 0 in Loxone Config, not at the GUI default (#362).

## Rejected

- **Leave out a 0 only on comparators**: the save drops it on every type tested, and no real config holds one.
- **Treat `Some("0")` and none as equal in the semantic diff only**: as in D91 and D112, it would hide the
  difference without making the output what Config writes.
- **Keep the documented default in the simulator for a missing `Def=`**: a `Time: 0` would then simulate as
  2 s, and Config's own simulation already disagreed with it on `LightController2` (`AlarmPeriod`).
