# D11: `NextObj` advances by one per minted object

Status: accepted (2026-08-24)

## Context

The `Next*` counters on `<ControlList>` must never decrease (see
[loxone-format.md](../loxone-format.md#container)). Loxone Config itself raises `NextObj` on every save, even
when no object was added. Whether ports also consume `NextObj` is unknown.

## Decision

The compiler raises `NextObj` by one per minted managed object, and never lowers it.
`Lockfile::absorb_counters` takes the maximum of lock and document, so a counter the GUI raised is kept.
Counting objects only is the conservative reading of the observed files.
