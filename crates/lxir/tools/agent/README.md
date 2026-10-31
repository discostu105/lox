# Agent cases: a request in, a module out, graded by `lxir test`

How well does an agent write `.lxir` with the skill? Each case is a
request as a house owner would phrase it, hidden `test` blocks that
check the behavior, and a reference solution. `run.py` gives an agent
each request in a fresh project, then runs the hidden tests against
what it wrote, in lxir's own simulator. Everything here is written for
this repo (lox-cli's eval set is AGPL; [../eval](../eval/README.md)
grades solutions against it from a checkout).

## Layout

- [`examples/configs/rohbau.Loxone`](../../examples/configs/rohbau.Loxone) — a synthetic config (shared with the templates): rooms, categories, the
  first ten built-in operating modes, empty folders for virtual inputs and
  outputs, one page "Logik".
- `fixture.lxir` — the house: sensors as virtual inputs (motion,
  button, brightness, temperatures, CO2, window, rain, wind, door,
  "Abwesend", water, humidity) and actuators as virtual output commands
  (lights, ventilation, heating, irrigation, awning, siren,
  dehumidifier, a door hint). Compiled against `rohbau.Loxone` into the
  config the agent works on.
- `cases/<id>/request.md` — the request, German, as given to the agent.
- `cases/<id>/tests.lxir` — hidden. Its own externs (`t_…`) name the
  fixture objects by title, so the tests do not depend on the slugs a
  solution picks. The expectations read the actuators' command inputs
  (`VirtualOutCmd.I`): what the logic sends, whichever blocks compute
  it.
- `cases/<id>/reference.lxir` — one solution that passes.

`cargo test -p lxir --test agent_cases` checks every case: the hidden
tests fail on the bare fixture (else they grade nothing) and pass with
the reference.

## Running

```sh
cargo build --release
tools/agent/run.py --reference                  # the references, no agent
tools/agent/run.py [--case c03 …] [--model …] [--json results.json]
```

Per case the agent (Claude Code, `claude -p`) starts in a new
directory with `fixture.Loxone`, a `lox.toml` (so `lxir check`,
`compile` and `test` need no flags), the lxir skill under
`.claude/skills/`, and read access to `crates/lxir/docs`. Its tools are
`lxir` on the shell, file reading and editing; no MCP servers, no user
settings. It writes `solution.lxir`. The grader compiles that module
with the hidden tests next to it against the fixture and counts the
case passed when every hidden test passes; the agent's own tests do not
count. `--work` keeps the directories (default: a new temp dir, kept),
`agent.json` in each holds the agent's transcript summary.

## Results

2026-10-31, Claude Code 2.1.316 with its default model, lxir 0.39
(this branch):

| Cases | Pass | Turns (median) | Time per case | Cost per case |
|---|---|---|---|---|
| c01–c12 (one or two blocks per actuator) | 12/12 | 13.5 | 26–69 s | $0.29–0.40 |
| c13–c16 (timers combined, schedule + override, latch + delay) | 4/4 | 15 | 28–73 s | $0.31–0.40 |

Every solution compiled; the agents read `lxir types <Type>` for port
names and wrote tests of their own before finishing. Findings:

- c16 first failed in grading, not in its logic: the agent had named a
  test of its own like a hidden one. The grader now drops the
  solution's `test` blocks before adding the hidden ones.
- The schedule cases (c12, c14) worked once the base config had the
  operating-mode objects: without them a DayTimer has no columns, in
  the Miniserver as in the simulator.
- The simulator used to refuse expectations on an actuator's command
  input (a block without a model); an input now reads what its wires
  carry on any block.
