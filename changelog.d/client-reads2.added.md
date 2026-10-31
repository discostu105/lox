- **loxone-client 0.1.2** (on crates.io): more typed reads — `LoxClient::network()` (IP, mask, gateway, DNS, MAC,
  DHCP, NTP), `bus_stats()` (CAN bus counters), `lan_stats()` (LAN counters), `sdtest()` (the parsed SD card test)
  and `log()` (the system log, with a clear error when the user isn't an administrator), plus `check_ll()` for the
  `Code` of a command's answer. `lox status`, the TUI and `lox otel` now read these through the client instead of
  each keeping its own list of endpoints.
