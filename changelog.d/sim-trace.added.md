- **`lox logic test --trace`** (also `loxl test --trace`) prints each test cycle by cycle: what a step sets,
  every change of the ports its expectations depend on, named by slug, and each `expect` with its actual value.
  A failing run without it points to the flag.
