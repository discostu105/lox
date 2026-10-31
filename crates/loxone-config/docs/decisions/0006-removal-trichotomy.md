# D6: Removal is a trichotomy

Status: accepted (2026-08-24), amended by [D13](0013-removed-moved-statements.md) and
[D31](0031-removal-tombstones.md)

## Context

A managed slug that is in the lock but missing from source is ambiguous. It may be a typo or an intended
delete.

## Decision

The compiler refuses by default. The user resolves it in one of three ways: fix the source, delete the block,
or stop managing it. The table is in [design.md](../design.md#the-removal-trichotomy).
[D13](0013-removed-moved-statements.md) puts delete and rename into the language.
