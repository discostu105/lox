# D68: `loxl test` runs in a simulator of its own

Status: accepted (2026-10-31), supersedes the simulator part of [D36](0036-tests-in-module.md)

## Context

[D36](0036-tests-in-module.md) put tests in the module and ran them through lox-cli's `lox sim`. That had
three problems:

- lox-cli is AGPL and a separate install, so no CI and no agent could rely on it.
- Its title addressing refused tests whenever two blocks share a title.
- It answers with a value for a block it does not know, so a passing test could rest on a guess.

## Decision

The tests run in `loxone_config::sim`. The external `lox sim` stays an option (`--lox <path>`,
`--show-spec`). The core rule: **a test only passes on documented behavior.**

- **Models come from documentation.** Every model is written from Loxone's documentation of the block (the
  KB page and its timing diagram), or from a format fact in [loxone-format.md](../loxone-format.md) such
  as `Inv=`, `Def=` defaults or InputRef mirroring. [simulator.md](../simulator.md) lists each model with
  its source and what it leaves out. No code or behavior was taken from lox-sim (clean room).
- **Unknown behavior is an error, not a value.** Reading a port checks the logic it depends on, given what
  the test sets at that moment. Each of these is an error naming the object:
  - a block without a model;
  - an output the model does not compute;
  - several wires into one input of a block whose fan-in is undocumented (only And is documented as
    AND-linked, Or and Xor as OR-linked);
  - an operating mode switched by logic;
  - a timer with two of its modes active at once.

  The test then reports `ERROR`, not a pass or a failure. Setting a port between such a block and the
  expectation makes the test independent of it. A failing block elsewhere in the program never touches a
  test that does not depend on it.
- **Addressing is by UUID through the lock**, so titles need not be unique.
- **Each test starts fresh:** all values 0, clock 2026-01-01 00:00 (a Thursday).
- **One `tick`** advances clock and time by `dt`, then evaluates the operating modes, then the blocks in
  wiring order. Feedback loops run in document order and read the previous cycle.
- Weekday modes follow the clock; other modes are off unless set.
- `==` allows the slack of decimal arithmetic, `~=` 5 %.

Validated: the shipped examples' tests (`tests/simulate.rs`, also in CI through `examples/ci.sh`) and the
house's two scenarios, both of which ran in lox-cli before. The house's alarm test is refused until it sets
`heimkommen` from the start, because the NFC Code Touch behind it has no model.

## Rejected

- Modelling the remaining blocks from forum posts or from lox-sim's behavior. It is unverifiable, and a
  wrong model makes a test pass that the house fails.
- Keeping lox-cli as the default. An optional external binary cannot gate CI.
