# D38: UUIDs are unique; a `uuid:` edit re-points

Status: accepted (2026-10-18)

## Context

No two `<C>` objects or `<Co>` connectors share a UUID in a config the GUI wrote. Loxone Config treats a repeat as
damage and heals it on open by creating new UUIDs for the later twins. That re-points their wires
([loxone-format.md](../loxone-format.md#uuid-anatomy)).

The house config had 38 duplicate UUIDs. An upload tool had mapped every ref's ports by `Ref=` target plus
connector key, so all mirrors of one target got the same `AQ`/`Q` UUID. Every UUID-keyed step in Lox Logic
(extern matching, wiring, routing, the semantic diff and the fingerprint) silently picks one twin of such a pair.

A second gap: an edited `uuid:` value on an extern with a lock pin was ignored, because the pin wins while its
object exists.

## Decision

- **`compile` and `adopt` refuse a base with repeated UUIDs** among `<C>` objects and `<Co>` connectors. The error
  names the remedy: one open and save in Loxone Config, which heals the twins. `<SObj>` entries reuse their
  object's UUID by design and are not counted.
- **`compile` checks its own output.** A repeat there can only come from this compile: a new UUID colliding with
  a base UUID created in the same second under the same serial, or a lock mapping two identities onto one UUID.
  Such output must never reach a push.
- **`diff` warns instead of refusing.** Diffing a damaged config is how the damage gets investigated. `drift`
  adds a note.
- **An edited `uuid:` matcher resolves afresh.** The pin tolerates a drifting title or iname, but a `uuid:` value
  is the identity. A value that differs from the pin is a source edit; a typo is a `NoMatch`. Changing the matcher
  kind still confirms the pinned object ([D32](0032-mirrors-matching.md)).

Mirror routing ([D34](0034-mirror-routing.md)) treats every mirror as having its own ports. The wire drawn before
pins which one a consumer reads.
