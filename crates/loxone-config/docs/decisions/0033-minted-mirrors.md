# D33: Minted mirrors

Status: accepted (2026-08-25), amended by [D48](0048-text-ref-flag.md)

## Context

Refs carry signals across pages. To drop ref plumbing from source, Lox Logic must be able to create refs, not
only match them ([D32](0032-mirrors-matching.md)).

## Decision

A managed block of type `InputRef` or `OutputRef` takes a mandatory `mirrors:` binding:
`spiegel = InputRef(mirrors: quelle)`. The target is any extern or managed block of the module, including one
created in the same compile.

- **Mirror identity.** The compiler emits what the GUI writes on every ref it creates:
  - `Ref=`: the target's UUID;
  - `LinkRefType=`: Loxone Config's type-registry code for the target's XML type, from
    `connectors::ref_link_type`;
  - `Analog="true"` where the target type is analog, `Text="true"` where it is text
    ([D48](0048-text-ref-flag.md)).

  A target type without a verified code is refused. The table grows from corpus evidence.
- **These attributes are compiler-owned** on ref blocks. They are not carried as GUI residue
  ([D19](0019-gui-residue-carried-forward.md)), so a retargeted `mirrors:` wins over the base's copy.
- **Ports.** `InputRef` has `AI`/`I` in and `AQ`/`Q` out; `OutputRef` has `AI` in and `AQ` out (189 InputRefs and
  154 OutputRefs in the house config).
- **Feed wires stay explicit in source.** The target's connector index 0 feeds `AI`, index 1 feeds `I`. The GUI
  draws missing feed wires itself, but the compiler requires them so rebuilds converge.
- **The title comes from the target.** The GUI rewrites a ref's `Title=` to its target's title on save. The
  compiler emits the target's title and refuses a label on a ref block.
- **Footprint.** Refs are drawn as flat 2112 × 192 tags.
- **The `OutputRef` → actor distribution wire stays in source.** The GUI does not draw it.

What the GUI heals on a minted ref is in [loxone-format.md](../loxone-format.md#refs-c-typeinputrefoutputref-ref).

Validated: oracle session 11. Minted refs survive open and save; recompiling onto the saved file is a semantic
no-op, and a second save is empty in both directions ([oracle-sessions.md](../oracle-sessions.md)).
