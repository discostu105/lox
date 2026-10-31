# D31: Removal tombstones

Status: accepted (2026-08-25)

## Context

A compile withdraws three kinds of edit: a managed block (`removed`), an extern wire gone from source, and a
`set` gone from source. The base config keeps each withdrawn edit until the compiled output is pushed and
downloaded again. In that window CI recompiles against the old base.

If the lock forgets a withdrawal at once, the recompile sees the old object as foreign and passes it through.
On the house config, the recompiled output carried both an old gate cascade and its expression-owned
replacement, with the alarm input driven twice. That output must never be pushed.

## Decision

Everything the compiler withdraws leaves a tombstone in the lockfile, if the base carried it:

| Withdrawal | Tombstone | Recognition marker |
|---|---|---|
| managed block | `removed` (object UUID) | the object |
| extern wire | `removed_wires` | the wire |
| extern `set` | `removed_sets` | the original and the written value |

- Every compile re-applies pending withdrawals: delete the object, delete the wire, restore the original.
- A tombstone retires when a base without the withdrawn edit is seen. Deployment is witnessed, never assumed.
- A `removed_sets` port that shows a third value was taken over by another writer. The tombstone retires and
  drift reports the port.
- A `removed <slug>` statement whose tombstone has retired is an error. This gets the statement cleaned up.
- `Lockfile::remove_object` creates no tombstone. It stays the escape hatch that leaves an orphan.

The compile → push → download window is a lock fixpoint: committable, CI-green and byte-reproducible.
Lockfile v2 introduced tombstones, so an older binary refuses such a lock instead of dropping them.

[D39](0039-attribute-claims.md) adds `removed_attrs` for attribute claims, and
[D42](0042-placement-gui-geometry.md) uses the same lifecycle for superseded positions.
