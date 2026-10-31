# D19: GUI-owned residue is carried forward from the base

Status: accepted (2026-08-25), amended by [D20](0020-inv-connector-gui-owned.md)

## Context

Blocks created in the GUI carry content the compiler does not model:

- display attributes such as `Tp=`, `Sun=` and `SpStates=`;
- child elements such as `<IoData>` (with the room and category binding `Pr=`/`Cr=`), `<Display>`,
  `<PSD>` and `<COHist>`.

Loxone Config adds the missing children on every save. A compile that dropped them deleted them again, so
each save and compile churned. Under the strict check of [D18](0018-adoption.md), this content blocked 20 of
the house's 43 managed-type blocks from adoption.

## Decision

When the compiler rebuilds a block that already exists in the base, it takes an allowlisted set of
attributes and child elements from the base element. It writes them back verbatim: same values, same order,
same self-closing form. A newly minted block gets the defaults. After its first GUI save the added content is
carried forward, so the churn ends.

- **`Cl=`, `LtE=` and `WF=` are carried.** A fixed default would repaint adopted blocks that are not green
  (Memory is grey, AutoJalousie blue) and fight the GUI over `WF=`. What the base lacks, the rebuild lacks.
- **The allowlist is the boundary of faithfulness.** Content is carried only if writing it back verbatim
  cannot contradict the source. Unknown attributes and children still refuse adoption. Growing the allowlist
  takes evidence.
- **`Inv=` is not plain residue.** It inverts a wire the source declares uninverted.
  [D20](0020-inv-connector-gui-owned.md) makes such a connector GUI-owned as a whole.
- **`FLG=` on `<In>` wires is carried.** The Miniserver and the app write it; Loxone Config treats it as
  inert (see [loxone-format.md](../loxone-format.md#connectors-and-wires-co--in)). It decorates a wire the
  source declares, so it passes the faithfulness test. It is keyed by sink port and source port. A wire whose
  source changes in the module is written without it. 113 of the house's 880 wires carry it.

## Rejected

- A snapshot of the residue in the lockfile. It would revert later GUI edits, because the stale copy wins.
  The GUI owns this content, and owner state is read from the config, as for externs.
- Refusing such blocks. It blocked adoption of most real blocks.
