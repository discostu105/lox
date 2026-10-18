# Examples

Every language construct of the IR ([docs/ir-spec.md](../docs/ir-spec.md))
is visible in a committed, test-pinned example. Where to look:

| Example | Shows |
|---|---|
| [ir/beschattung.lxir](ir/beschattung.lxir) | the starter module: `page`, `let`, externs (`iname:` / `title:`), comparator and gate blocks, wires onto extern ports, an extern parameter write (`=`) |
| [ir/pool.lxir](ir/pool.lxir) | the narrative showcase: a template with an instance, an expression argument, a `min` unit value, an expression wire, arithmetic folded into a `Formula` block (D35), two sim scenarios |
| [ir/sprachtour.lxir](ir/sprachtour.lxir) | the systematic language tour — every construct the two above do not cover (see below) |
| [ir/lifecycle/](ir/lifecycle/) | `moved` and `removed`, in three staged modules — the two transient statements no permanent module can carry; narrated by `cargo run --example lifecycle` |
| [project/](project/) | a `lox.toml` project: fragment directory with one file per page (D25/D28), committed lockfile and output |

The tour covers, in order: string and numeric `let` constants; the full
extern matcher palette (`uuid:`, `iname:`, `title:` narrowed by `room:`
and by `category:`); block labels; fan-in (two wires on one port); the
`Formula:` attribute parameter; unit values `ms`/`min`/`h`/`%`;
`==` in an expression; a template with title interpolation
(`"{titel}: …"`, overridden with a string `let`), value-parameter
overrides and port forwarding; an expression wire with `or`, `and` and
meaning-bearing parentheses; both mirror forms — a `mirrors:` extern on
a GUI-drawn ref (D32) and a minted `InputRef(mirrors: …)` with its feed
wire (D33); mirror routing (D34); a second `page` section; and `test`
scenarios with `clock`, per-step `dt`, and `~=`.

## Base configs

`configs/haus.Loxone` and `configs/pool.Loxone` are small synthetic
fixtures (hand-written test data — real Miniserver configs are never
committed). `pool.Loxone` carries two pages, rooms, categories, five
virtual inputs, and one GUI-style mirror ref, so the matcher and mirror
constructs have something real to resolve against.

## How they are kept honest

Every module is pinned by the test suite (`tests/ir.rs`:
`pool_showcase_compiles_end_to_end`, `sprachtour_compiles_end_to_end`,
`lifecycle_stages_move_and_remove`, `project_example_is_current`):
canonical formatting, compile assertions down to `Def=` values and wire
targets, and byte-identical recompiles. The committed artifacts are
regenerated when compiler output changes: `cargo run --example compile`
for `out/`, and `lxir compile --time 1767225600` inside
`examples/project/` for the project's lock and output. [ci.sh](ci.sh)
re-checks all invariants in CI
([workflow](../.github/workflows/ci.yml)).

## Simulated tests

`lxir test` runs the `test … end` scenarios through
[lox-cli](https://github.com/eisber/lox-cli)'s simulator (`lox` ≥ 0.12.0
on PATH, or `LOX=` / `--lox`). One practical rule, learned against the
real simulator: inject only signals whose title is unique in the
compiled document — a title that a mirror ref shares with its target is
not a stable address for the simulator's name table. The examples'
scenarios follow that rule and pass against lox-cli 0.12.0.
