# D17: Decompile is a view, grouped by page

Status: accepted (2026-08-25), amended by [D28](0028-page-statement.md),
[D29](0029-full-and-adoptable-view.md) and [D30](0030-observed-defaults.md)

## Context

A real house config has 19 pages and about 1900 objects. A decompile that lifts only builtin-type blocks and
their direct neighbors showed 23 of them, in one flat module.

## Decision

`loxl decompile` defaults to a full view:

- Every page object with connectors is lifted. Managed types become block declarations, everything else
  `extern`s.
- Every wire between lifted objects is shown.
- Output is grouped by logic page: `page` sections in one module, or one module per page with `--out-dir`.
  In a per-page module, objects the page references but does not contain become externs annotated with
  their origin.
- Slugs form one namespace across the document, the same in the single and per-page views. Statement
  keywords are never used as slugs.

The full view is for reading. Compiled against the same base it would mint duplicates of the managed blocks
and claim every wire. The compilable adoption subset is `--managed-only` (`DecompileScope::ManagedOnly`,
[D7](0007-decompile-managed-wires.md)).

The report counts what the view leaves out:

- Extern `Def=` parameters are never lifted. A `target.Port = value` line would claim ownership of the
  value.
- Wiring between periphery objects stays raw. The view covers page logic, not the device tree.
- Objects whose type or port keys are not identifiers (Loxone's numeric type ids) stay raw.
