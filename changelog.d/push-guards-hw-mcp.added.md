- **`lox config push`** refuses `--force` when the Automatic Designer's rules changed since the last verified push,
  since the upload would delete rules edited in the app. It records the change time in `autopilot.changed`;
  `--allow-app-changes` overrides. The saved lock now matches the verified download.
- **`lox config sync`** checks, read-only, that a project's source, lock and the live config agree, and exits 1
  when they don't.
- **`lox hw status`** compares the devices on the bus (`/data/status`) with the program file: missing, offline,
  replaced (serial changed) or not in the program. Table or `-o json`.
- **`lox mcp`** gets read-only tools for hardware and config as code: `hw_status`, `logic_check`, `logic_diff`,
  `logic_drift`, `logic_lint`, `logic_compile` (into a scratch directory), `logic_test`, `config_sync`,
  `config_push_dry_run` and more.
