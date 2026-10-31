# D30: The full view hides what the GUI wrote by default

Status: accepted (2026-08-25)

## Context

Loxone Config writes a `Def=` for nearly every port when a block is placed, at the GUI default. A decompiled
block buries its two configured values in twenty default ones.

## Decision

The observed default of a (type, port) is the most common `Def=` value across the local corpus. It counts
only at a share of at least 90% and at least 10 occurrences.

- `tools/extract-defaults.py` writes the evidence (every candidate, share, counts, the rejected tail) to
  `docs/data/param-defaults.json`. It generates `src/observed_defaults.rs` for the builtin types, the only
  ones whose parameters decompile lifts.
- The full view leaves out a lifted parameter whose value equals the observed default. `--all-params` shows
  everything. The report counts what was left out.
- The adoptable `ManagedOnly` view never leaves out a value; the rebuild writes the exact `Def=` back.
- Minted blocks write no `Def=` for unbound parameters. On both sides, absence means default.

Corpus statistics are enough for view-only knowledge. A wrong entry can hide a value from a reader but never
change what compiles. What the compiler may write still needs live verification. On the house config 486
parameters were left out; with [D28](0028-page-statement.md) and [D29](0029-full-and-adoptable-view.md) the
full view went from 2667 to 1332 lines.
