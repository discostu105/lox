# D50: Placement keeps to the paper sheet

Status: accepted (2026-10-29)

## Context

Loxone Config draws every page on a paper sheet, set by the Document's `PaperFormat=` (see
[loxone-format.md](../loxone-format.md#container)). Objects past its edge sit on the grey canvas beyond.
[D42](0042-placement-gui-geometry.md) placed clusters on an unbounded canvas, so on a full page the best clear
spot could lie past the sheet's right edge. After a relayout of a kitchen page of the house, a chain of nine
blocks sat at x 24192–35904, past the sheet's edge at 23808.

## Decision

- **The sheet comes from `PaperFormat`**, one for all pages. A3 landscape (`2`) is x 0..23808 × y 0..16032:
  the paper in twips on the grid, less the title block along the bottom edge. Pages of other formats stay
  unbounded, since their sizes are not known.
- **A cluster goes to the best clear spot on the sheet**, with the room its labels and tags need. The sheet's
  edges are candidate positions, next to the obstacle edges.
- **Tighter before farther.** Without room at the GUI's spacing, the cluster is laid out tighter and tried
  again: columns 960 apart, then 576, then 384, with stacking 384 → 192 at the last step. The GUI's spacing
  comes first, because a far spot with room reads better than a cramped one close by.
- **Past the edge only when nothing fits.** A cluster that fits nowhere on the sheet, even at the tightest
  spacing, goes where D42 puts it, at the GUI's spacing.
- **Ref tags.** A tag goes beside its port if it stays on the sheet there. Otherwise it takes the best clear
  spot on the sheet, like a cluster of one. A tag whose block is past the edge stays beside the block.
- **Compile names what is past the edge.** After writing, `compile` warns per page about managed objects past
  the sheet (by slug), and names the `--relayout` that lays the page out afresh. An object is past the edge if
  its footprint or one of its off-page labels is. Placement and `render` use the same test, and `render` draws
  the sheet's outline.
- **Less than a grid unit over counts as on the sheet.** The GUI draws a block as tall as its rows wherever it
  was dropped. A GUI-placed block on the kitchen page ends 24 below the title-block line.
- `lox config push` deploys the placement the lock holds and places nothing, so it does not warn.

A3 is the limit, since Loxone Config offers no larger format. A full page stays full. Moving logic to a page
of its own is a module change. Moving GUI-owned objects to make room is not placement's job.

Result on the kitchen page: the chain sits in the top right, the only free region left, at the tightest
spacing, and the whole page is on the sheet. The price is wires. The chain runs in a loop from two light
controllers back into them. Past the edge, right of the controllers, only the four wires back into them ran
right to left (5 backward wires on the page). Above them, the four wires out of them do too (9).
