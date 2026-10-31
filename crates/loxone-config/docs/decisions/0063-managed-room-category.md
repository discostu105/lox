# D63: A managed block's room and category

Status: accepted (2026-10-30)

## Context

[D19](0019-gui-residue-carried-forward.md) leaves a managed block's `IoData` to Loxone Config. A block
made in source therefore showed up without a room in the app until someone opened the GUI.

## Decision

- `b.room = "…"` and `b.category = "…"` on a managed block write `IoData Cr=`/`Pr=` on every rebuild.
  When the block has no `IoData`, one with `Visu="true"` is added.
- No lock state: the source is the owner, as for the label.
- Attribute claims on externs ([D39](0039-attribute-claims.md)) are unchanged.
- `.title` on a managed block is an error: its label is its title.

Validated: oracle session 25 (room claims on nine minted blocks: empty diff); live, the probe blocks in
room "Zentral".
