- **`lox mcp`** keeps every command on its control. A value such as `../<uuid>/open`, given as a color, a raw
  command or an alarm code, reached another control or a system endpoint without the confirmation. `lox send`,
  `lox light color` and the TUI are covered too.
- **`lox run`** and the MCP `run_scene` tool take a scene name only. A path ran any YAML file as a scene.
- **`lox cache refresh`** honours `verify_ssl: true`. It sent the password without checking the certificate.
- **Dependencies** are updated to patched versions of rustls, tokio, rmcp and others. `cargo audit` runs in CI.
- **`install.ps1`** checks the download against the release's SHA256 file before it installs.
- **`config.yaml`**, the token and the caches are written readable by you only, in one step. Before, the config
  was readable by others for a moment each time it was saved, and the cached program file stayed readable.
