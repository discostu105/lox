# D79: A PulseAt's fire time from source; what the compile carries

Status: accepted (2026-10-31)

## Context

A PulseAt keeps its fire time in `Sec=`, the seconds since midnight. `Typ="0"` is the fixed-time mode;
other `Typ=` values are time functions (see [loxone-format.md](../loxone-format.md#objects-c)). A source
without the fire time hid when a block fires: on the house, blinds opening at 18:58 and closing at 19:00.
Issue: #248.

## Decision

- **`At: 19:00` on a PulseAt claims `Sec=`.**
  - A time of day is a value of its own: `HH:MM[:SS]`, canonical with two-digit hours and without zero
    seconds. Only `connectors::clock_params` binds it.
  - The parameter is named for what it means; `connectors::attr_name` maps it to the attribute.
  - Like `Code:` ([D40](0040-program-code.md)) it is a claim. While unbound, `Sec=` is carried, so a
    source written without it stays correct. While bound, it is written from source, at the base's
    attribute position.
  - Unlike `Code:`, decompile lifts it: a fire time is one short value.
  - A block on a time function (`Typ=` other than 0) refuses `At:` and is lifted without it. The source
    cannot say what a fixed time would mean there.
  - A mint writes `Sec=` alone and leaves `Typ=` to the save, as a bare mint does (oracle session 8).
  - The diff names the change `At` and shows times of day.
  - `Sec=` joined the drift fingerprint in its v10 ([D21](0021-drift-baseline.md)). A lock recorded with
    v9 still reads as in sync.
- **`loxl coverage` lists what the compile carries.** Per managed type, it lists the attributes the
  compile carries from the base ([D19](0019-gui-residue-carried-forward.md), `carried_attrs`). It names
  the parameter that claims one where the language has it, and leaves out the blocks whose source claims
  it. This is the remaining inventory of behaviour that lives in Loxone Config rather than in the source
  (#253).
