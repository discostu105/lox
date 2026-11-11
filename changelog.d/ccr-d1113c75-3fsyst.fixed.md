- **`verify_ssl: true`** works. The HTTPS client had no root certificates, so a verified connection always
  failed. `lox update check` and `lox otel` with an HTTPS endpoint failed for the same reason.
- **Commands are sent once.** After a lost answer lox sent `pulse` and other commands again, up to three times.
  `lox reboot`, `lox update install` and the restart of `lox config push` are sent once too.
- **A refused command is an error.** `lox on`, `off`, `blind`, `gate` and the other control commands printed `✗`
  and exited 0 when the Miniserver answered with a code other than 200.
- **`lox ctx add`** keeps a single-Miniserver config: it asks for `lox ctx migrate` first instead of replacing
  it. `lox ctx` and `lox setup set` leave a config they cannot read as it is.
- **`lox setup set`** with another host or user drops the structure cache and the token of the old one.
- **`lox otel serve`** stops on SIGTERM and sends what is pending, and reconnects when the Miniserver stalls
  during login.
  - The SD card test runs every 10 minutes at most. It ran on every export and wrote to the card each time.
  - `--interval 0` is rejected.
- **`lox config pull`, `push` and `download`** no longer hang on a Miniserver that stops answering over FTP. A
  failed listing is an error instead of "no configs", and a rejected login is tried once.
- **`lox reboot`, `lox update install` and `lox music`** honour `--dry-run`.
- **`lox update check -o json`** prints JSON only.
- **`lox history`** reports an answer that is not a statistics file instead of crashing.
- **`lox light color`** rejects a malformed value. `#1ä234` crashed.
- **`lox schema`** names flags as the command line takes them (`--all-in-room`) and lists every global flag.
- **`lox --help`** lists `tui`.
- **`lox logic … | head`** and `loxl … | head` end quietly instead of panicking.
- **`lox watch -i 0`** is rejected instead of polling without a pause.
- **`lox logic compile`** finds the ports of an extern by the names `lox logic adopt` writes. On block types
  such as AnalogScaler, RSFlipFlop or Memory it failed with "has no port `In`", although the list it printed
  named `In`.
- **`lox logic compile`** keeps `Valid="true"` on a formula whose text did not change. Loxone Config checks a
  formula only when its text is edited, so every compiled formula showed as invalid.
