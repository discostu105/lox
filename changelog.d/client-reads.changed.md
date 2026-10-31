- **lox status --diag**: shows the PLC's CPU share next to the CPU load; the JSON reports `cpu`, `sps`, `tasks` and
  the counters as numbers (`null` when the Miniserver leaves them empty). `lox status`, the TUI, `lox otel` and the
  MCP `system_status` tool now read the system state through the same loxone-client calls.
