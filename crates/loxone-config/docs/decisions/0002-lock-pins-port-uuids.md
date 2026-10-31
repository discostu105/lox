# D2: The lockfile pins port UUIDs

Status: accepted (2026-08-24)

## Context

Wires reference port UUIDs, not object UUIDs. If ports got new UUIDs on each compile, every compile would
rewrite every wire.

## Decision

The lockfile pins the UUID of each port of a managed block, not only the object UUID.
