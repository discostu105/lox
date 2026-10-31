# D47: A rebuilt block keeps the base's order

Status: accepted (2026-10-29)

## Context

A Loxone Config save orders the objects of a page, the attributes of a block and its connectors. The rebuild
wrote its own order:

- It appended managed blocks at the end of their page.
- It wrote `Desc=` behind the position attributes. Loxone Config writes it right after `Type=`.
- It wrote connectors in table order. Loxone Config 17.2 writes AnalogThresholdTrigger's `Remanence` after
  `Off` (see [loxone-format.md](../loxone-format.md#connectors-and-wires-co--in)).

None of this has meaning, but each difference kept a recompile of a saved config from reproducing it.
[D45](0045-wire-order-kept.md) solves the same problem for wires.

## Decision

The teardown records each block's attribute names and connector keys in base order, with its
[D19](0019-gui-residue-carried-forward.md) residue. It also records each page's objects in base order. The
rebuild follows them:

- Attributes and connectors go into base order. An item the base did not have, such as a new parameter or
  connector, stays right behind the item it followed as emitted.
- A block goes back to its place in the page: before the first object the base listed after it (or did not
  have), otherwise behind the page's last object.
- A page's `<IoData>` stays where it is. Loxone Config writes it before the objects on some pages and after
  them on others.
- A block new to the page, minted or moved from another page, is appended.

Compiling the compiler's own output is still a fixpoint. With [D45](0045-wire-order-kept.md),
[D46](0046-block-version.md) and [D48](0048-text-ref-flag.md), compiling a config that Loxone Config 17.2
saved reproduces it byte for byte.
