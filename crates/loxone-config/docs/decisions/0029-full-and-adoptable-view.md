# D29: The full view is for the reader; only the adoptable view must compile

Status: accepted (2026-08-25)

## Context

The two decompile scopes have different contracts. `ManagedOnly` output must compile back byte-identically.
The full view is documentation.

## Decision

Three changes make the full view easier to read. The adoptable view has none of them.

1. **Ref plumbing folds.** A ref's `Ref=` attribute names the object it mirrors; the wires between the two
   are GUI routing, not logic. They are left out, and the ref's extern declaration gets
   `# mirrors <Type> "<name>"`. A periphery object whose only connection was plumbing is not pulled in as an
   extern.
2. **Wires sort** by sink slug, sink port, source slug and source port, so a big fan-in reads as a table.
   The adoptable view keeps the `<In>` order within a connector, which is part of the compiled bytes.
3. **Redundant labels drop.** A label the slug already encodes (`slugify(title) == slug`, e.g.
   `"Temp über 28"` → `temp_ueber_28`) is left out. The adoptable view keeps every label, since the rebuild
   writes `Title=` back exactly.

The report counts folded wires (`ref_wires_folded`, printed by the CLI). On the house config the view shrank
from 2667 to 1818 lines. Adoption fidelity across the corpus is unchanged.
