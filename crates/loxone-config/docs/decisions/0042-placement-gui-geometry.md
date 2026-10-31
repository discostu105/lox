# D42: Placement in the GUI's geometry, shared with the GUI

Status: accepted (2026-10-29), amended by [D50](0050-placement-paper-sheet.md)

## Context

A block without a position needs one, and a block the user dragged in Loxone Config should stay where it was
dragged. Placing every new block in one column below the lowest managed block, and writing positions from the lock
only, failed both:

- On a kitchen page of the house config, 11 new objects sat in one column at y 18552–29040, below everything else.
  15 of the page's 76 wires ran right to left.
- On "Automatik-Regeln" the content ends near y 6000, and two managed blocks sat at y 15192.
- The next compile reverted a drag made in the GUI.

## Decision

### Positions are shared state

Teardown reads each managed block's rectangle from the base, with the page it is drawn on, like other GUI residue
([D19](0019-gui-residue-carried-forward.md)). When lock and base disagree, the base wins: a drag in the GUI goes into
the lock. Positions stay outside the diff and the fingerprint ([D21](0021-drift-baseline.md)), so a drag is not
drift.

- Only a move of a grid unit (96) or more is a drag. The GUI recomputes `Px2`/`Py2` on every save. A block on the
  same spot takes the GUI's rectangle, so a save moves nothing.
- A rectangle on another page than the module's (a cut and paste in the GUI) is ignored. The block is rebuilt on
  its own page, where it was.

### The GUI's geometry

`src/layout.rs` uses the GUI's block sizes, port rows and ref tag offsets
([loxone-format.md](../loxone-format.md#objects-c)). Compile writes grid positions with the GUI's footprint, so a
save moves nothing.

### Placement runs after wiring

Placement runs per page, on the page as it then stands. Free blocks joined by wires form a cluster, laid out as a
layered graph:

- feedback is broken in source order by a depth-first walk;
- columns come from the longest path, and the order within a column from barycenter sweeps;
- each block sits level with the ports that feed it;
- columns are spaced for the labels and tags beside them.

The cluster goes to the free spot, cleared by a margin, that keeps its wires to fixed objects shortest and running
left to right. The spot must be on the page's paper sheet ([D50](0050-placement-paper-sheet.md)). A cluster without
such a wire goes below the page's content. A created ref tag goes beside the port it serves, 960 off and stacked by
row, as the GUI places them. Integer arithmetic and sorted candidates make placement deterministic.

### `--relayout <page>`

`--relayout` places the page's managed blocks afresh, as if new.

- A block stays where it is if it is wired to a ref tag the compile does not own (a GUI-drawn ref) and drawn beside
  it, level and at most two gaps off. Someone arranged those two together in the GUI.
- A wire routed through a GUI ref elsewhere on the page ([D34](0034-mirror-routing.md)) pins nothing.
- A cluster's old position (that of its first block) is worth one row in the cost, so a spot about as good keeps it.
- A title that names no page is an error.

### A stale base must not revert a placement

After a relayout, the lock holds the new position, and the base keeps the old one until it is uploaded and
downloaded again. Reading the base would take the old position for a GUI drag. So the lock records it as
`superseded_layout`:

- a base position on the same spot is ignored;
- any other base position is taken over (the new one once it arrives, or a drag) and clears the field;
- every placement writes it anew: the base position it replaced, or nothing.

This is the lifecycle of the removal tombstones ([D31](0031-removal-tombstones.md)). Lockfile v4 added the field, so
an older binary refuses the lock instead of dropping it. A block that changes pages is placed afresh. Its old page's
position is not on the new page, so nothing is superseded.

### `loxl render`

`loxl render` draws one page as SVG: managed objects blue, GUI objects grey, wires orthogonal with backward ones red,
labels for other pages purple. It prints the metrics placement is judged by: overlaps, backward wires (listed), long
wires (over 8000), wire crossings and total wire length. This shows a layout without opening Loxone Config.

### Results on the house config

Without `--relayout`, compiling the house gives the same bytes and the same lock as before placement existed. With
`--relayout`:

- The kitchen page: backward wires 15 → 5, long wires 11 → 4, crossings 34 → 18, total wire length 333k → 210k.
  Four of the remaining backward wires are real feedback, from a scene block back into the two light controllers it
  follows. The fifth runs between two GUI objects.
- "Automatik-Regeln": the page's extent shrinks from y 768–16584 to 768–7416.

These numbers are for an unbounded canvas. Keeping the kitchen chain on the paper sheet
([D50](0050-placement-paper-sheet.md)) costs four more backward wires there (9).

### Not covered

- Moving objects the compile does not own (GUI refs, extern blocks).
- Wire routing: the GUI routes wires itself.
- Negative coordinates: never seen, so x and y stay ≥ 0. A chain that feeds a block at the page's left edge runs
  backward.
- Lifting positions into source: they live in the lock.

## Rejected

- One column below the lowest managed block: the old placement, with the results above.
- The size formula `504 + 192·(ports−2)` for every block. It is wrong for function blocks: a minted PushButton came
  out 1344 × 1848, where the GUI draws 2688 × 1272.
