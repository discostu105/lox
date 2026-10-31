# D48: A minted ref of a text value carries `Text="true"`

Status: accepted (2026-10-29)

## Context

A minted ref ([D33](0033-minted-mirrors.md)) carries its mirror identity: `Ref=`, `LinkRefType=`, and
`Analog=` where the target is analog. Refs of text-valued objects carry `Text="true"` in place of `Analog=`
(see [loxone-format.md](../loxone-format.md#refs-c-typeinputrefoutputref-ref)). A minted ref without it gained
it on the next GUI save, so no recompile of a saved config could reproduce it.

## Decision

- `connectors::ref_link_type` returns the value kind with the code (`RefValue`: digital, analog or text).
  `ApiActor` and `VirtualTextIn` are text.
- `Text=` is compiler-owned on refs, like the other identity attributes. A ref retargeted to a non-text object
  loses it.
- `Memory` and `VirtualOutCmd` stay out of the table. Only a few of their refs carry `Text=`, so the flag
  follows the target's configuration, not its type. `Analog=` on `Memory` refs is handled the same way.

With this and [D45](0045-wire-order-kept.md) to [D47](0047-block-order-kept.md), compiling a config saved by
Loxone Config 17.2 reproduces it byte for byte.
