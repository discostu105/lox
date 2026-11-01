- **loxl fmt**: a call with more than three arguments goes on one line when they are all numbers, unit
  values, times or `on`/`off` and it fits in 100 columns (`ModbusASensor(…)`, `defaults …`). A whole-line
  comment stays right above the statement it was written above, and a blank line below a comment is kept.
- **loxl lint --fix** respells only the values it fixes and leaves the rest of the file as written.
