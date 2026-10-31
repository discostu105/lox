- **`lox logic decompile`** no longer prints a stale constant under a wire as a second argument for the same
  port (`Input1: 2, Input1: x.AQ`): the wire stays the argument and the constant becomes a comment on it, and
  `lox logic compile` keeps it in the config. **`lox logic check`** refuses a port bound to both a value and a
  wire, with the error on the port in the call (#226)
