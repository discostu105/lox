# D21: The drift baseline is a fingerprint of what the diff compares

Status: accepted (2026-08-25)

## Context

A drift check asks whether Loxone Config or the Miniserver changed something since the last adopt or
compile. It should not need the last compiled output. It must not report what every GUI save touches anyway:
positions, display residue, the save's own changes and locale renames of built-ins.

## Decision

`semantic_fingerprint(doc)` hashes exactly what `diff` compares: objects by UUID with type and title, ports
that carry `Def=`, attribute parameters, and the wire set. It lives next to `diff` in the same module, so the
two cannot drift apart. Titles that may be locale renames are left out. Two documents have the same
fingerprint exactly when their diff is empty apart from such renames.

- Adopt and compile record it in the lock (`target.semantic_fingerprint`).
- `loxl drift <cfg> --lock <lock>` answers from one parse of one file. `loxl diff` says what changed.
- Each time the diff learns to compare more, the fingerprint gets a new version and covers it too. Examples
  are room and category bindings ([D39](0039-attribute-claims.md)), program text
  ([D40](0040-program-code.md)) and tile programs ([D41](0041-touch-pure-flex-tiles.md)). `loxl drift`
  accepts the previous version and notes it.
- The config a compile started from is in sync too: its bytes match the lock's
  `target.source_config_sha256`, so nothing changed it since; only the compiled output is not pushed
  yet. `loxl drift` says so and exits 0 (2026-11-01).

The corpus tests require every adopted rebuild to reproduce the fingerprint recorded at adoption exactly.
