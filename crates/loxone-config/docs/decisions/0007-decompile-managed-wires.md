# D7: The managed-only view lifts only wires that touch a managed block

Status: accepted (2026-08-24), amended by [D17](0017-decompile-view-by-page.md)

## Context

A wire between two externs belongs to the config, even when the compiler drew it.

## Decision

`loxl decompile --managed-only` lifts only wires that touch a managed block. So `decompile(compile(m))` is a
faithful subset of `m`, not its inverse. The default full view ([D17](0017-decompile-view-by-page.md)) also
shows wires between lifted extern page objects.
