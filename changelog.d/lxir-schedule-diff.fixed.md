- `lox ir diff` shows DayTimer schedule changes, per timer and day group in the terms of a `schedule` block
  (`DayTimer "Abendlicht" schedule weekdays: 17:00-22:00 -> 17:00-23:00`), also with `--summary`, `--json` and
  `--exit-code`; `lox ir drift` and the drift check of `lox config push` now see a schedule changed in Loxone Config
  or the app. Fingerprints recorded by older versions stay accepted (#162)
