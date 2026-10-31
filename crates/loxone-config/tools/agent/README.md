# Agent cases: a request in, a module out, graded by `loxl test`

How well does an agent write `.loxl` with the skill? Each case is a
request as a house owner would phrase it, hidden `test` blocks that
check the behavior, and a reference solution. `run.py` gives an agent
each request in a fresh project, then runs the hidden tests against
what it wrote, in the Lox Logic simulator. Everything here is written for
this repo (lox-cli's eval set is AGPL; [../eval](../eval/README.md)
grades solutions against it from a checkout).

## Layout

- [`examples/configs/rohbau.Loxone`](../../examples/configs/rohbau.Loxone) — a synthetic config (shared with the templates): rooms, categories, the
  first ten built-in operating modes, empty folders for virtual inputs and
  outputs, one page "Logik".
- `fixture.loxl` — the house: sensors as virtual inputs (motion,
  button, brightness, temperatures, CO2, window, rain, wind, door,
  "Abwesend", water, humidity) and actuators as virtual output commands
  (lights, ventilation, heating, irrigation, awning, siren,
  dehumidifier, a door hint). Compiled against `rohbau.Loxone` into the
  config the agent works on.
- `cases/<id>/request.md` — the request, German, as given to the agent.
- `cases/<id>/tests.loxl` — hidden. Its own externs (`t_…`) name the
  fixture objects by title, so the tests do not depend on the slugs a
  solution picks. The expectations read the actuators' command inputs
  (`VirtualOutCmd.I`): what the logic sends, whichever blocks compute
  it.
- `cases/<id>/reference.loxl` — one solution that passes.

`cargo test -p loxone-config --test agent_cases` checks every case: the hidden
tests fail on the bare fixture (else they grade nothing) and pass with
the reference.

## Running

```sh
cargo build --release
tools/agent/run.py --reference                  # the references, no agent
tools/agent/run.py [--case c03 …] [--model …] [--json results.json]
```

Per case the agent (Claude Code, `claude -p`) starts in a new
directory with `fixture.Loxone`, a `lox.toml` (so `loxl check`,
`compile` and `test` need no flags), the Lox Logic skill under
`.claude/skills/`, and read access to `crates/loxone-config/docs`. Its tools are
`loxl` on the shell, file reading and editing; no MCP servers, no user
settings. It writes `solution.loxl`. The grader compiles that module
with the hidden tests next to it against the fixture and counts the
case passed when every hidden test passes; the agent's own tests do not
count. `--work` keeps the directories (default: a new temp dir, kept),
`agent.json` in each holds the agent's transcript summary.

## Results

2026-10-31, Claude Code 2.1.316 with its default model, Lox Logic 0.39
(this branch):

| Cases | Pass | Turns (median) | Time per case | Cost per case |
|---|---|---|---|---|
| c01–c12 (one or two blocks per actuator) | 12/12 | 13.5 | 26–69 s | $0.29–0.40 |
| c13–c16 (timers combined, schedule + override, latch + delay) | 4/4 | 15 | 28–73 s | $0.31–0.40 |
| c17–c19 (roller shutter with wind alarm, lighting controller, fixed value meter; Lox Logic 0.6) | 3/3 | 13 | 62–66 s | $0.32–0.35 |

Every solution compiled; the agents read `loxl types <Type>` for port
names and wrote tests of their own before finishing. Findings:

- c16 first failed in grading, not in its logic: the agent had named a
  test of its own like a hidden one. The grader now drops the
  solution's `test` blocks before adding the hidden ones.
- The schedule cases (c12, c14) worked once the base config had the
  operating-mode objects: without them a DayTimer has no columns, in
  the Miniserver as in the simulator.
- c17–c19 need the blocks the simulator models since loxone-config 0.6
  (AutoJalousie, LightController2, MeterDig). The agents found the
  right inputs (`EndUp`/`EndDown` for complete open/close, `Safety`,
  `Move`, `A`/`Ppwr`) from the skill and `loxl types`, and gave the
  motion mood a percentage.
- The simulator used to refuse expectations on an actuator's command
  input (a block without a model); an input now reads what its wires
  carry on any block.
