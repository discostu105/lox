- **Switching times from source** (lxir D53): a `schedule <name> … end` block in a `.lxir` module is a DayTimer's
  week (`weekdays 06:00-07:30, 17:00-22:30`, `holiday vacation off`, values and `activate` for analog timers), and
  `<timer>.schedule = <name>` writes it onto a DayTimer, extern or managed
- `lox ir compile` mints virtual inputs (`party = VirtualIn("Partymodus")`) in the Virtual Inputs folder, with the
  next free `VI<n>` (lxir D54), and eight more block types verified against Loxone Config saves (delays,
  staircase light, edge detection, flip-flop, multiplexer, state); `lox ir types <Type>` shows each port's
  documented unit, label and range, unit suffixes scale into the port's unit, and `lox ir lint` flags constants out
  of range (lxir D52)
- `lox ir scan <file|dir>`: lists the personal data in a config (address, users, codes, credentials, remote access,
  serials) by element and attribute, never the value, and exits 1 on any finding; CI runs it over every committed
  `.Loxone`
- `lox ir diff --summary`: the semantic diff as a commit message, by block title and port name, users/access
  changes included
- `extern … (device: "<serial>")` narrows a repeated title to the objects of one Extension or Tree device (lxir D55)
- An agent skill for writing `.lxir` modules (`crates/lxir/skills/lxir/SKILL.md`)
