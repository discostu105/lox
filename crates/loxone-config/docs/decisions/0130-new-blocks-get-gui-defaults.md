# D130: A new block gets Loxone Config's defaults

Status: accepted (2026-11-01), amends [D30](0030-observed-defaults.md) and
[D128](0128-no-def-at-zero.md)

## Context

A missing `Def=` is 0 ([D128](0128-no-def-at-zero.md)). Lox Logic created blocks without `Def=` on the
ports the source left out, as D30 had decided: a new `Monoflop` without `Time` pulsed for 0 s, a new
`LightController2` had `Max` 0 and `MoveTimeout` 0 (#362). Loxone Config writes its default on every
connector of a block it places, so a block drawn in the GUI never has this problem.

Where the defaults come from:

- **The corpus** holds what users left or changed. The modal value is the default only for a few types
  (D30 admitted 26 ports of `AutoJalousie` and `LightController2`). Elsewhere users changed it: the
  corpus mode of `OffDelay.Time` is 600, and `AutoJalousie.Dir` is 180 in ten of twelve configs.
- **The documentation** gives defaults for some ports of about 15 types, and none for `Irrigation`,
  `Alarm`, the room controller and others.
- **Oracle session 92** took one block of each of 54 builtin page types, removed every connector without
  a `Def=` or a wire, and saved. The save added each connector back with its default: 162 values on 24
  types, among them ports the source wires, like `StairwayLS.TimeHigh` 180 and `LightController2.MoveOn`
  900. They agree with every default the documentation gives, `AutoJalousie.Dir` -1 included. The other
  30 types got no `Def=` at all.
- **Oracle session 93** compiled the 54 blocks with these values, and a `Monoflop` with `Time: 0`, a
  `StairwayLS` with a wire on `TimeHigh` and a `LightController2` with two values set. The save changed
  none of the connectors.

## Decision

- `src/gui_defaults.rs` holds the table of session 92: every default that is not 0.
- **A new block** gets the GUI default on every port the source gives no value, also under a wire,
  as Loxone Config does. "New" is recorded in the lock: the compile that creates the block sets
  `gui_defaults` on its entry (only for a type with defaults), and every later compile writes the same
  values, whether the base carries the block yet or not.
- **A block from before** (a lock entry without `gui_defaults`, or an adopted block) keeps a missing
  `Def=`, which is 0. Its compile output does not change.
- **`implicit-zero`** (from `lint`, needs the project) reports such a block: a port with a GUI default,
  no `Def=` and no wire in the compiled config, and no value in the source. `loxl lint --fix` writes
  `Port: 0` into the block, so the source says what Loxone Config runs. The fix proves the compiled
  config byte-identical, as for `unit-suffix`. A block a template creates is reported but not fixed.
- Decompile and adopt lift `Port: 0` where a GUI default is not 0 ([D128](0128-no-def-at-zero.md) used
  the observed and documented defaults). The full view still hides values at the observed default (D30).
- `loxl types` and hover show the GUI default and say that a block that already exists has 0 there.
  `types --json` adds `gui_default`.
- The simulator reads a port the object has no connector for at its GUI default, since a save adds it
  at that value. A missing `Def=` stays 0.
- The lock is written as v9 only when an object carries `gui_defaults`. A lock without one stays v8, so
  an older binary still reads it in full; one with the flag refuses an older binary, which would drop
  the flag and compile 0.

## Rejected

- **Absence means the GUI default on every block**: it changes the output of every existing project.
  The house config would get `Tv1`/`Tv2` 600 on its irrigation, which runs at 0 today.
- **Keep creating blocks without defaults, and only lint**: every new block would need every parameter
  in the source, and `loxl types` would document a default the compiler never writes.
- **The documented or observed default as the source**: neither covers the room controller, the alarm
  or the irrigation, and the corpus mode is a user value for several ports. The save is Loxone Config's
  own answer.
- **New means "not in the base"**: once pushed, the block would lose its defaults on the next compile.
- **Bump the lock to v9 always**: an older `lox` would refuse every project, for a field it does not
  need.
