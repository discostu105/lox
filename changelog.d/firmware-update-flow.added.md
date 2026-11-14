- **`lox update install`** waits until the Miniserver runs the new firmware and prints progress. `--no-wait`
  returns right away.
- **`lox update check`** names the matching Loxone Config download and warns about a Remote Connect deadline.
  In a Lox Logic project it warns when the base was saved by another Loxone Config version.
- **`lox update check`** finds the serial without `lox setup set --serial`: from `lox.toml`, or from the
  Miniserver's MAC address.
