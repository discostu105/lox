# D36: Tests live in the module

Status: accepted (2026-08-26), partly superseded by [D68](0068-own-simulator.md) (the simulator), amended by [D80](0080-v2-one-spelling.md)

## Context

Logic should be testable next to its source, and the tests should run in CI. A test injects values, advances a
simulated clock and checks outputs.

## Decision

`test "Name" … end` is a top-level item. Its body is line-oriented like the rest of the language:

- injections: `slug.Port = value`;
- `tick <n> [dt <v>]`;
- `expect slug.Port <cmp> <value>`;
- `clock …` (bare, [D80](0080-v2-one-spelling.md)).

Statements group into simulator steps at `tick` boundaries. An `expect` attaches to the preceding `tick`.

- **Compile ignores tests.** A module with tests compiles to the same bytes as one without.
- **`loxl test` runs them** in Lox Logic's own simulator ([D68](0068-own-simulator.md)). It compiles in memory
  against the lockfile and addresses ports by slug through the lock, so titles need not be unique.
- **`--lox` runs them in lox-cli's `lox sim`** instead. That backend is title-addressed: slugs map to the compiled
  `Title`, room-qualified (`"Title [Room].Port"`) when titles collide. Refs mirroring the same signal may share a
  title. Check results are mapped back to `expect` lines step by step. `lox sim` has no `!=`, so `--lox` refuses it.
- **References validate on the flattened form**, with templates expanded and expressions desugared. Tests address
  what the simulator sees, including slugs that expansion and desugaring create. Validation of the unexpanded module
  skips test object references (the `flattened` guard), because those slugs do not exist yet. `check`, `compile`
  and `test` all re-validate the flattened form.
- **Injections may drive any port, outputs included.** Driving a sensor's `Q` is the point: an injection is a
  persistent override.

## Rejected

- The roadmap's `given {…} after N ticks` block syntax: braces and keywords in a language of line-oriented
  statements.
- Compiling tests to XML test configs and round-tripping through files. An in-memory compile is simpler and leaves
  no files behind.
