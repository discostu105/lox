# Examples

Every language construct of the Lox Logic ([docs/language.md](../docs/language.md))
is visible in a committed, test-pinned example. Where to look:

| Example | Shows |
|---|---|
| [logic/beschattung.loxl](logic/beschattung.loxl) | the starter module: `page`, `let`, externs (`iname:` / `title:`), comparator and gate blocks, wires onto extern ports, an extern parameter write (`=`) |
| [logic/pool.loxl](logic/pool.loxl) | the narrative showcase: a template with an instance, an expression argument, a `min` unit value, an expression wire, arithmetic folded into a `Formula` block (D35), two sim scenarios |
| [logic/sprachtour.loxl](logic/sprachtour.loxl) | the systematic language tour — every construct the two above do not cover (see below) |
| [logic/lifecycle/](logic/lifecycle/) | `moved` and `removed`, in three staged modules — the two transient statements no permanent module can carry; narrated by `cargo run --example lifecycle` |
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
wire (D33); mirror routing (D34); a second `page` section; attribute
claims on externs — `.title`, `.room` with followers, `.category` on
an Air device and one of its inputs (D39); and `test` scenarios with
`clock`, per-step `dt`, and `~=`.

## Base configs

`configs/haus.Loxone` and `configs/pool.Loxone` are small synthetic
fixtures (hand-written test data — real Miniserver configs are never
committed). `pool.Loxone` carries two pages, rooms, categories, five
virtual inputs, one GUI-style mirror ref, and an Air device with three
children (two in the device's room, one elsewhere), so the matcher,
mirror and attribute-claim constructs have something real to resolve
against. `rohbau.Loxone` is an empty house to build on: rooms,
categories, the built-in operating modes, empty folders for virtual
inputs and outputs and one page; the agent cases (`tools/agent`) mint
their fixture into it.

## How they are kept honest

Every module is pinned by the test suite (`tests/ir.rs`:
`pool_showcase_compiles_end_to_end`, `sprachtour_compiles_end_to_end`,
`lifecycle_stages_move_and_remove`, `project_example_is_current`;
`tests/attrs.rs` for the attribute-claim lifecycle):
canonical formatting, compile assertions down to `Def=` values and wire
targets, and byte-identical recompiles. The committed artifacts are
regenerated when compiler output changes: `cargo run --example compile`
for `out/`, and `loxl compile --time 1767225600` inside
`examples/project/` for the project's lock and output. [ci.sh](ci.sh)
re-checks all invariants in CI
([workflow](../.github/workflows/ci.yml)).

## Simulated tests

`loxl test` runs the `test … end` scenarios in the Lox Logic simulator
([docs/simulator.md](../docs/simulator.md)); `tests/simulate.rs` runs the
examples' scenarios in `cargo test`, and `ci.sh` runs a module's tests on
every check. With `LOX=` / `--lox` they run in
[lox-cli](https://github.com/eisber/lox-cli)'s simulator instead
(`lox` ≥ 0.12.0), which addresses signals by title: there, inject only
signals whose title is unique in the compiled document. The examples'
scenarios follow that rule and pass against lox-cli 0.12.0 too.
