- **`lox` run in the home directory** uses `~/.lox` as the global config when the home directory is reached
  through a symlink, as on macOS. It read it as a project's config instead, and `lox setup set` could replace a
  config with several contexts.
