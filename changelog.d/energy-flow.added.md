- **`lox energy flow`** breaks down the energy flow right now: production, consumption, grid import/export,
  battery charge/discharge and state of charge, read from the Energy Flow Monitor (`-o json` adds every node)
  - **`lox energy --top [N]`** ranks the meters by current power, highest first (default 10)
  - MCP: the new `get_energy_flow` tool returns the same breakdown; `top` adds the meter ranking
  - `lox tui` and `lox energy flow` share one interpretation of the monitor's nodes and roles (`roles:` in
    `~/.lox/tui.yaml` applies to both); `lox energy` now lists the flow monitor (type `EFM`) too
