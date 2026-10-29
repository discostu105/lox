- `lox ir`: Automatic Designer rules from source (D57). `rule <slug>` on an `AutopilotRule` extern states whether the
  rule is `active`, its conditions (`when time 09:00`, `when weekday mon tue`) and its actions (`then pumpe "on"`);
  `decompile`/`adopt` lift every rule of these shapes, and the compile writes `APC=` only when the source changes the
  rule, as the app's own edit (`STA` follows `active`). Action codes come from the config itself or the observed
  PushButton/PButtonT ones; anything else is refused
