- **loxl** knows the documented units, canvas labels and ranges of the alarms, the room controller, the
  meters, the NFC Code Touch, the intercom, the irrigation and the math and counter blocks. `adopt` and
  `decompile` write their time parameters with a suffix (`PulseTime: 3s`), and `loxl lint` flags bare
  numbers there; `loxl lint --fix` rewrites them without changing the compiled output.
