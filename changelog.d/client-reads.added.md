- **loxone-client 0.1.1** (on crates.io): typed reads of the system state — `LoxClient::system_status()` (firmware,
  PLC state, heap), `heap()`, `diagnostics()` (CPU and PLC load, tasks, interrupts, context switches) and
  `value(path)` for any `LL` value, with the parsers public in `loxone_client::system`.
