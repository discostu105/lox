# D131: Source names for ports, default outputs and port aliases

Status: accepted (2026-11-03), language v3 part 1 (#374)

## Context

Source named a port by the key Loxone Config stores it under: `I1` on a gate, `Input1` on a comparator,
`Input1` on a `Formula` whose text says `I1`, `Pf`, `Mr1` and `OPf` on a meter, `TComfortC` on a room
controller. A reader needed `loxl types` open for anything beyond a gate. Nearly every reference ended in
`.Q`, and `x = a.Q` was refused because an alias is no block.

## Decision

**One source name per port.** The connector table carries a source name next to each key
(`port_names`): the key where it reads well (`Reset`, `AutoShade`), a curated name where it does not
(`Trigger`, `Power`, `LedRed`), and one name per family (`In`, `In1`, `In2` on every input family,
`Enable`, `Disable`, `Set`). Source uses the name and nothing else:

- The key of a renamed port is an error that names the port (`Write `Trigger` instead of
  `InputTrigger``). A port the table does not know, on an extern of an open-world type, is named by its
  key, and the family names read back to their keys there too (`In` on a Tree actuator is its `I`).
- Within a type a name is unique and never equals another port's key, so every lookup by "name or key"
  is unambiguous. The key-based tables (`port_meta`, `port_doc`, `gui_defaults`, `observed_defaults`)
  take either.
- The lockfile, the compiled config and the simulator keep the keys. The lock's `ports` map is unchanged,
  so no lock migrates. The blocks an expression generates are named after the sink's *key*
  (`nachlauf_inputtrigger__ge1`), so a renamed port keeps their identities.
- `loxl types` lists the names, and `Stored as `InputTrigger`.` under a port whose key differs. Its
  JSON carries `name` and `key`.
- `loxl fmt --write` rewrites the keys of v2 modules to the names. Decompile and adopt write names.

**A bare block name is its default output.** In an argument, an operand, the source of `<-` and a
test statement, a bare name that is a block, extern or signal stands for the type's default output:
its only output, or the one `port_names::default_output` lists (`Q` on switches and threshold
triggers, `AQ` on `Formula`, meters and analog sensors). A type with several meaningful outputs and no
default (`Memory`, `AutoJalousie`, `LightController2`) refuses the bare name and lists its outputs. The
explicit port stays legal: it is sometimes the clearer spelling.

- Resolution runs on the expanded module (`resolve_names`), after template expansion and before
  desugaring. The source keeps what the author wrote.
- Decompile and adopt write the bare name for a default output (`In1: sonne`,
  `pumpe.On <- wp_bedarf`).

**`let` may alias a port.** `let aussen = sensor.AQ` names the port wherever a port may stand and is
never a value. The rule the spec can now state: `let` never creates an object, `=` always does.

## Rejected

- **Keying the lock by the source names.** It would migrate every lock for no gain; the keys are the
  config's names and the lock describes the config.
- **Lowercase port names.** Lowercase keys are the language's own (`title:`, `device:`, `mirrors:`,
  `switches:`); PascalCase keeps the two apart.
- **Accepting a renamed port's key as a second spelling.** One spelling per port, as v2 did for values.
- **Making the bare name the only spelling of a default output.** `x.Q` next to `x.AQ` on a block
  with both is the clearer line; the formatter leaves the choice to the author.
