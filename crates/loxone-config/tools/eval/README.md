# Eval harness: lox-cli's tasks, solved in the Lox Logic

[lox-cli](https://github.com/eisber/lox-cli) maintains ~475 natural-language
automation tasks ("close the blind when it's sunny and above 25 °C"), each
with the blocks, wires and parameters a solution should contain and
simulator scenarios it should pass. `loxl-eval.py` grades `.loxl`
solutions against them. It is an end-to-end check of Lox Logic as an
agent's tool: can a task be written, does it compile, does it behave?

Nothing of lox-cli is copied here (it is AGPL-3.0). The harness reads
the cases and the fixture from a lox-cli checkout and runs its simulator
binary. The solutions in `solutions/` are written for this repo and name
their case only by id.

## Setup

```sh
git clone https://github.com/eisber/lox-cli ~/repos/lox-cli
(cd ~/repos/lox-cli && cargo build --release)      # the `lox` simulator
cargo build --release                               # loxl
E=~/repos/lox-cli/tests/eval
export LOX=~/repos/lox-cli/target/release/lox
```

## The loop (agent or human)

```sh
tools/eval/loxl-eval.py cases --evals $E --feasible     # tasks Lox Logic can mint
tools/eval/loxl-eval.py fixture --evals $E --out /tmp/fx.Loxone
loxl decompile /tmp/fx.Loxone                           # what exists to reference
tools/eval/loxl-eval.py prompt --evals $E <case-id>     # the task + what is graded
$EDITOR tools/eval/solutions/<case-id>.loxl
tools/eval/loxl-eval.py run --evals $E --solutions tools/eval/solutions [--case <id>]
```

`cases --feasible` keeps the tasks whose every expected block type is in
Lox Logic's verified table (`loxl types`); a `+` marks them in the full list.

`run` compiles each solution against a working copy of the fixture and
reports per case:

- **blocks**: each expected new block present (type, title or title
  fragment). Required for a pass.
- **wires**, **params**: the expected wires (direct or through a ref
  mirror) and `Def=` values. Informative only: a correct solution may
  route differently or choose another value where the task leaves room.
- **sim**: the case's scenarios through `lox sim run`. Required for a
  pass. Scenarios that already pass on the untouched fixture are counted
  separately as non-discriminating, since they say nothing about the
  solution.

`--json <file>` writes the full report; `--keep` keeps the work dir
(compiled configs, locks, sim specs).

## Fixture normalization

The working copy differs from lox-cli's fixture in two ways, both of
which Lox Logic refuses rather than guessing past:

- **XML comments are stripped.** A config written by Loxone Config has
  none.
- **Repeated UUIDs are re-minted** after their first occurrence, the way
  a Loxone Config save repairs them. The fixture defines two objects with
  `a0000008-0008-0001-…`. Wires keep naming the first.

## Known simulator gaps

- **DayTimer schedules.** lox-sim reads each `<Entry>` as "value until
  `To=`", ignores `From=` and `Modes=`, and treats `Ix=` as a day of the
  week. Real configs, and Lox Logic's D53 claims, which an oracle save
  confirmed, use `From`–`To` intervals and `Ix` as an index into
  `Modes=`. So a correct `schedule` fails scenarios that check the
  timer's off times (`kr09-outdoor-light-schedule`).

- **`AnalogMultiplexer2`.** lox-sim passes `Input1` at `Select=0` and
  `Input2` at `1`; on a Miniserver `Select` is reported 1-based (`0`
  gives 0). A solution that drives it from a digital flag can pass the
  simulator and fail live ([loxone-format.md](../../docs/loxone-format.md)).

## Results

2026-10-29, 10 sampled feasible cases (`solutions/`): 9 pass. Every
solution compiled on the first try. The failure is the DayTimer gap above.
`s18-door-open-warning` passes but deliberately differs from one expected
parameter: it delays by `Delay: 5min` and flashes for `Time: 6s`, where
the case expects the pulse length `Time = 300`. `kr15-comfort-temp-memory`
was skipped, because its scenarios read a port (`Raumregler.Input`) that
the fixture's room controller does not have.
