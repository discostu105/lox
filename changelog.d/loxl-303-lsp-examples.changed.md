- **loxl drift** reports the config the last compile started from as in sync (its output is not pushed
  yet) instead of as changed, so `examples/ci.sh` no longer prints a drift note for the shipped examples.
- **loxl lsp**: argument-key completion leaves out the keys the call binds already, and hover on a block
  in a template body shows the `defaults` values it takes.
