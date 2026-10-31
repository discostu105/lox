# D20: A connector with `Inv=` is wholly GUI-owned

Status: accepted (2026-08-25)

## Context

`Inv="true"` on a `<Co>` is the GUI's input inversion. Unwired, it turns the constant-0 input into a 1. This
is how the GUI stores checkboxes such as an enabled Remanenz (see
[loxone-format.md](../loxone-format.md#connectors-and-wires-co--in)). All 23 LightController2 blocks of the
house carry it on their unwired `Remanence`. A rebuild that dropped the flag would turn off state retention
across reboots. Carried as plain residue, the flag could contradict the source: a declared wire into an
inverted connector means its negation.

## Decision

A connector that carries `Inv=` is GUI-owned as a whole:

- The rebuild writes its entire `<Co>` back verbatim, with the flag, `Def=` and `<In>` wires.
- Wiring or setting such a connector from source is a hard error, on managed and extern sinks.
- Decompile and adopt keep its `Def=` and wires out of the lifted source.

The faithfulness rule of [D19](0019-gui-residue-carried-forward.md) is unchanged; ownership is per connector.
With this, all 100 managed-type blocks of the house adopt. The last 31 refusals were 23 LightController2,
5 PushButton, both DayTimers and one PulseGen.

## Rejected

- Inversion in the language (`!source.Q` or an `inv:` marker). Lox Logic would become a second writer of a
  flag the GUI edits by checkbox. In Lox Logic source, an explicit `Not` block expresses inversion.
