- **loxl** `fmt` writes short calls on one line (`b = And(I1: x.Q, I2: on)`: up to three arguments,
  no comments, within 100 columns) and puts a blank line around every multi-line item. Run
  `loxl fmt --write` once; the compile output does not change.
- **loxl** keyword rules: section kinds (`moods`, `schedule`, `tiles`, `rule`, …) are names outside
  their section head, `then wait|mode|notify|mail|message` is always the rule action, `tick` and
  `dt` take `let` constants, `when time` reads times like `clock`, and bulk statements and
  `mirrors:` take `inst.block`. Errors spell instance blocks as the source does, and a named signal
  that would share an expression's generated block names is an error.
