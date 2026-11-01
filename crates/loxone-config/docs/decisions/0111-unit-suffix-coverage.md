# D111: Unit suffixes on extern ports, template parameters and device settings

Status: accepted (2026-11-01), extends [D87](0087-v2-tooling.md)

## Context

The `unit-suffix` lint ([D87](0087-v2-tooling.md)) looked at block and `defaults` parameters only.
Two places with bare time values were left out:

- port assignments on externs (`x.TimeEnd = 120`);
- template value parameters (`zeit_hoch = 75`) and the instances' arguments for them. On the house
  config these were the 34 remaining bare durations: every blind instance passes its run times
  this way.

The settings of virtual and Modbus devices (`Poll: 10`, `Timeout: 4000`, `MaxTime: 300000`) had no
documented unit at all. The TechDoc scrape behind the port facts ([D52](0052-port-facts.md))
covers function blocks, not the device tree.

## Decision

**Extern assignments take the extern's declared type.** That is the type the compile scales a
suffix with, so the lint uses the same rule as on a block. A bulk line `(a, b).Port = 300` changes
only as a whole, when every member gets the same spelling: the line stays one line.

**A template value parameter takes a suffix when its uses agree.** The fix collects every use of
the parameter in the body: a block parameter (`Time: zeit`) or a port assignment on an object
parameter or extern (`j.TimeEnd = zeit`). The default and each instance's argument get a suffix
only when all uses give the same spelling, and only when the body names the parameter nowhere
else. A title, an expression or a port without a unit would read a suffixed value differently,
so such a parameter keeps its numbers. An instance argument that names no parameter goes to the
primary block, and the lint uses that block's port.

**`lint --fix` sees the whole module.** In a directory module, templates and externs often live
in another file than the instances and assignments that use them. Each file is fixed with the
whole module as context.

**Device settings get units from the knowledge base, never by guess.** A hand-written table
(`src/settings_meta.rs`, documented in loxone-format.md) holds the unit and range of a setting
where a Loxone page states it:

- `VirtualHttpIn` `Poll` in seconds and `Timeout` in milliseconds, with their ranges (reference
  `kb-virtual-http-input`). The corpus agrees: every `Timeout` is the mint default 4000, every
  `Poll` at least the documented minimum 10.
- No Loxone page gives the unit of the Modbus server's `Timeout` or the Modbus sensors'
  `ModbusPollingCycle`, `MinTime` and `MaxTime` (reference `kb-modbus-tcp` names them without a
  unit). They stay bare numbers until a document states one.

**Milliseconds are a port unit.** A time suffix scales exactly into milliseconds there (`4s` →
`4000`); decompile, adopt and the lint write `4s` for `4000`. Before, a suffix on such a port
scaled into seconds, the default for an undocumented port, so `Timeout: 4s` compiled to `4`. No
config in the repository or the house config wrote one.

## Rejected

- **Reading the unit off the value range.** `MaxTime: 300000` looks like milliseconds, but
  `MinTime` is 1000 on one Modbus sensor type and 10 on the other. A guessed unit would compile a
  suffix to the wrong value.
- **Suffixing a parameter by its first use.** A parameter used on a seconds port and a minutes port
  would compile differently from its bare number.
