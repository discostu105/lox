- **`lox ir test` / `lxir test`** runs the `test … end` blocks in a simulator of its own: no lox-cli install needed,
  and `examples/ci.sh` now always runs a module's tests
  - 31 block types are modelled from Loxone's documentation (logic gates, comparators, arithmetic, formulas, memory
    flags, refs, monoflop, on/off delays, switch, push-button, flip-flop, edge detection, threshold switch,
    operating modes, schedules); `crates/lxir/docs/simulator.md` lists each with its source
  - A test that depends on any other block reports `ERROR` and names it instead of passing on a guessed value;
    setting a port between that block and the expectation cuts it out
  - Ports are addressed by UUID through the lockfile, so blocks no longer need unique titles
  - `--lox <path>` still runs the tests in lox-cli's `lox sim`; `--show-spec` now needs it
