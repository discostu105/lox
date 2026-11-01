# D114: Compile refuses a value a save rewrites

Status: accepted (2026-11-01), follows [D112](0112-zero-is-no-attribute.md)

## Context

Oracle sessions 61 and 62 found that a save raises `Poll="0"` and `Timeout="0"` on a virtual HTTP input
to `"10"` and turns `MaxVal="0"` on a digital `VirtualIn` into `"1"`. Lox Logic compiled the values as
written, so the next save showed them as changed (#332). D112 left the other values open.

Sessions 63 and 64 checked them ([loxone-format.md](../loxone-format.md#values-a-save-rewrites)):

- `Poll`, `Timeout` and `Allowedtimeouts` are 32-bit whole numbers: a fraction is cut off, a larger
  number wraps around. `Poll` is then held to 10 to 604800, `Timeout` to at least 10.
- A digital `VirtualIn` always has `MaxVal="1"` and no `MinVal`. Its `DefVal` and an analog input's
  range stay as written.

## Decision

- Compile refuses a value a save would rewrite, and names the value the save writes:
  ``block `h`: `Poll: 5`: a virtual HTTP input polls in whole seconds, from 10 to 604800, so a Loxone
  Config save turns it into `Poll: 10`. Write that instead.`` (`connectors::save_rewrite`).
- `loxl check` catches the HTTP input's values and a `VirtualIn` with `Analog: off`. A `VirtualIn`
  without `Analog:` is checked by the compile, which knows whether the existing object is digital.
- Only what a save does counts. A value outside the range a Loxone page documents but kept by the save
  (`Timeout: 60000`, above the documented 8000) compiles.

## Rejected

- **Compile to the value the save writes**: `Poll: 5` would silently mean 10, and the source would say
  something the Miniserver does not do.
- **Refuse what is outside the documented range**: the save keeps those values, so refusing them would
  reject configs Loxone Config writes itself.
