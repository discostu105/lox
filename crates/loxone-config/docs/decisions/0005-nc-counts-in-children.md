# D5: `Nc` counts the `<In>` children

Status: accepted (2026-08-24)

## Decision

The compiler keeps `Nc=` on a `<Co>` equal to the number of its `<In>` children, and leaves it out when there
are none. It writes the attributes in Loxone's order `K, Def, Nc, U` (see
[loxone-format.md](../loxone-format.md#connectors-and-wires-co--in)).
