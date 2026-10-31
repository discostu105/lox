# D41: Touch Pure Flex tiles from source

Status: accepted (2026-10-29)

## Context

The tiles of a Touch Pure Flex are a `<btns>` child of its `TreeDevice`: one `<btn>` per tile with three functions
each ([loxone-format.md](../loxone-format.md#touch-pure-flex-tiles-btns-on-a-treedevice)). They were GUI-only
state. In the house, three Flex devices in one room share a tile layout whose shade tiles carry no `ctrls=`. Such a
command reaches every shade linked to the device, so a shade tile moved all linked shades.

## Decision

### Syntax

A named set lists tile programs, and a claim writes the set onto an extern `TreeDevice`:

```
tiles <name>
  tile <i>(click: …, up: …, down: …, text: …, to: …)
end
<device>.tiles = <name>
```

- Several devices may claim one set.
- `click`, `up` and `down` are `f1`, `f2` (`bT="1"`) and `f3` (`bT="2"`).
- `text:` and `to:` apply to every function with a command. `<fn>_text:` and `<fn>_to:` override one function.
- `to:` is repeated rather than a list, like a fan-in.
- The device is a plain extern. Tree devices have free-text titles, so it is pinned by `uuid:`.
- Commands and texts are quoted strings, written as Loxone Config shows them. Lox Logic does not parse the command
  language beyond the abbreviations below.

### Rewrite existing tiles only

- Tiles are addressed by `i`. An index the device lacks is an error that lists the indices it has.
- Unlisted tiles are carried byte for byte. Creating tiles and the layout (`x`/`y`/`h`/`w`, `ci`, icons) stay with
  Loxone Config.
- A written function emits `api`, `cT`, `bT`, `ctrls` in the GUI's order. It keeps the raw bytes of a value that
  decodes unchanged and carries unknown attributes after them. An empty click is written as `<f1></f1>`.
- A `bT=` that is not the slot's is refused.

### `to:` targets must be API-linked

A target must be linked to the device in the compiled output:

- a wire, in either direction, between the target's `OutputAPI` and the device's `ApiActor.I`; or
- the `AI` of an `OutputRef` mirroring that actor, whose `AQ` distributes into `ApiActor.I`.

Both directions count because API distribution wires are recorded in reverse. The check runs after all wires of the
compile are drawn, so a link written in the same source counts. A target resolves like any wire endpoint: extern,
managed block, or template body block by flattened name.

### New API links through an explicit mirror

When a page already holds several mirrors of the actor, `OutputAPI: flex_api.I` leaves mirror routing
([D34](0034-mirror-routing.md)) no choice. The source then creates the mirror and draws the link's three wires:

- the forward wire: `OutputRef(mirrors: flex_api, AI: blk.OutputAPI)`;
- the distribution: `flex_api.I <- ref.AQ`;
- the reverse wire: `OutputAPI: ref.AI`.

The reverse wire names an input port as its source. The direction check admits that for exactly one shape: the
`AI` of an `OutputRef` whose `mirrors:` target is an `ApiActor`, read by an `OutputAPI` connector that takes wires.
An extern ref's `AI` is accepted as a source anyway, since extern ports are open-world. A created reverse wire
carries no `FLG=`; a flag already present is carried as residue. All 61 links through a mirror in the corpus and the
house config have these three wires
([loxone-format.md](../loxone-format.md#refs-c-typeinputrefoutputref-ref)).

### Type check from abbreviations

The first argument of a multi-argument call (`SET(As;Co;pulse)`) names the addressed block type. Lox Logic knows
`As` = `AutoJalousie` and `Lico` = `LightController2`, both seen in the house config. When every abbreviation in a
command is known, a target of another type is refused. Unknown abbreviations are written unchecked. The corpus fan
tile's `Sts` sits on a Flex linked to a `StepSel`; a single observation, not added.

### Warnings are lint

Compile has no warning channel. The advisory checks are `loxl lint` kinds over the compiled config, for every Flex,
claimed or not. Both need a known abbreviation.

| Kind | Fires when |
|---|---|
| `tile-broadcast` | a command without `ctrls` reaches two or more linked blocks of its type |
| `tile-dead-command` | no linked block of its type, among `ctrls` when given |

### Lock, diff, decompile

- **No lock state, no restore.** Like `Code:` ([D40](0040-program-code.md)), a withdrawn claim leaves the last
  written programs in the device, and the base holds them. There is nothing to tombstone.
- **Diff and fingerprint see tile programs.** Each non-empty function becomes a parameter keyed
  `tile <i> click|up|down` (attributes except `bT`). Fingerprint v4 added them; drift notes an older fingerprint
  that cannot see them. The layout stays outside the diff, as GUI state.
- **Decompile and adopt do not lift tiles.** A set is per-device data whose `ctrls` name blocks that need slugs,
  and the device needs an extern. Lifting would add externs to every decompiled view for state that works
  unclaimed. Programs move into source on purpose; an unclaimed `<btns>` is carried verbatim.

Not covered:

- creating or moving tiles;
- display-only tiles (a text without a command; no corpus evidence);
- abbreviations beyond `As` and `Lico`. The house also has `TpfController` blocks on Flex API connectors, whose
  commands Lox Logic writes unchecked.
