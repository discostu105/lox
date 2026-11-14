- **`lox update check`** reads the release version from Loxone's answer. It printed "Update available: 1.0" or
  "check Loxone Config for details" instead of the version on offer.
- **The global `~/.lox/config.yaml`** is no longer mistaken for a project-local config when the home directory
  path contains a symlink, as on macOS under `/var`.
