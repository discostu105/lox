# D18: Adoption is decompile with a lock, verified per block

Status: accepted (2026-08-25), amended by [D19](0019-gui-residue-carried-forward.md),
[D20](0020-inv-connector-gui-owned.md) and [D62](0062-house-control-types.md)

## Context

Existing managed-type blocks must come under source control without being minted again. Matching by title
does not work: real configs have duplicate titles, such as four blocks titled "O1657".

## Decision

`loxl adopt <cfg>` writes the `--managed-only` view together with a new lockfile. The lock pins each block's
existing identity: object UUID, port UUIDs, layout and page. The first compile rebuilds the blocks in place.
Identity comes from the lift, not from matching.

Acceptance: adopt, then compile, gives an empty semantic diff against the real house config. Compiling that
output again is a byte-identical fixpoint.

What a faithful rebuild needs:

- **Page pinning.** Every managed block is pinned to a page (`LockedObject.page_uuid`). An adopted block
  keeps the page it was drawn on. A new block gets the options' page on its first compile. A lock without
  the field still works; the next compile fills it in.
- **Attribute parameters.** Some block logic lives in element attributes, not in connectors, such as
  `Formula="I1+I2"` on a Formula block. These are language surface: `Formula: "I1+I2"` binds like a
  parameter, never a wire. They are declared per type in `connectors::attr_params`. The compiler writes them
  with the observed `Valid="false"` companion, decompile lifts them, and `loxl diff` compares them.
- **Verification, not translation.** Adopt allows exactly what the rebuild writes, plus known
  normalizations:
  - `WF=` becomes the value Loxone Config itself writes on save.
  - `LtE=` and the color are display state.
  - `<Co>` element order is cosmetic. The port-UUID index tails prove the type's order; the GUI sometimes
    writes canvas order.
- **Skip, don't translate.** A block that fails verification stays unmanaged. Where adopted logic touches
  it, it comes back as a pinned extern. The reason goes to `AdoptReport::refused`.
- **Empty elements.** Loxone output is not always self-closing (`<IoData></IoData>`, see
  [loxone-format.md](../loxone-format.md#container)). The compiler makes an element self-closing only when
  its own removals emptied it.

Adopt never modifies the config and refuses to overwrite existing outputs. `loxl adopt --uuid <uuid> --as
<slug>` adopts one object ([D62](0062-house-control-types.md)).

## Rejected

- All-or-nothing adoption. One GUI flag on one block (an `Inv=` input inversion on a PulseGen) would have
  blocked the other 22 blocks.
- Adoption by title. Titles are not unique.
