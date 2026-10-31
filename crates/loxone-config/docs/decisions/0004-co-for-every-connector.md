# D4: Managed blocks write a `<Co>` for every connector

Status: accepted (2026-08-24)

## Context

Loxone writes every connector of a block, including unwired outputs and inputs without a value (see
[loxone-format.md](../loxone-format.md#objects-c)).

## Decision

A managed block gets a `<Co>` for every connector of its type, and `Nio=` is the connector count.
