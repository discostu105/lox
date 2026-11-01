# D112: 0 is no attribute on a number a save leaves out

Status: accepted (2026-11-01), extends [D91](0091-off-is-no-attribute.md) and
[D65](0065-physical-io-settings.md)

## Context

Oracle session 37 found that a save drops `MinVal="0"` on an analog `VirtualIn` (#312). Lox Logic wrote
every bound number as it stands, so `MinVal: 0` showed as changed after each save, and the next compile wrote
it back. The question was which other numbers behave the same.

- **Corpus** (131 `.Loxone` files, `ControlList Version` 74 to 267): on `VirtualIn`, the virtual commands,
  the Modbus objects, calendar entries and the physical inputs and outputs of D65, no numeric setting the
  source can set holds `"0"`. Many are absent: `MinVal` on 112 of 171 `VirtualIn`s, `ModbusDataType` on 149
  of 339 Modbus sensors, `SourceValLow`/`DestValLow` on every virtual command.
- **Oracle sessions 61 and 62** set each of these settings to 0 on fresh virtual and Modbus objects, on
  calendar entries, and on the analog and digital inputs of a library config. The save dropped every one,
  with three exceptions: a Modbus server keeps `Timeout="0"`, a virtual HTTP input raises `Poll="0"` and
  `Timeout="0"` to 10, and a digital `VirtualIn` turns `MaxVal="0"` into `"1"`.

## Decision

- A bound 0 on a setting in `connectors::omitted_when_zero` writes no attribute. On an existing object it
  removes the attribute. The setting names are the ones the oracle checked; a name counts on every type
  that has it as a setting. Month and day of a calendar entry are not in it, since 0 is no month.
- On a physical input or output (D65), `x.MinVal = 0` removes the attribute, and a 0 on a setting Lox
  Logic never adds is allowed where it is missing: it adds nothing. A withdrawn 0 restores the original,
  with a tombstone that reads the missing attribute as the 0 written.
- Decompile and adopt read a missing setting as 0. They write it (`ModbusDataType: 0`) where a fresh
  object needs the setting or would get another default (`ModbusDataType="1"`), so the source means the
  same when it is compiled onto another config. Elsewhere a missing setting lifts as nothing.

## Rejected

- **Leave out every attribute whose value is 0**: real configs hold `"0"` on other attributes (`Typ="0"`
  on `PulseAt`, `Sec="0"` among them), and a Modbus server keeps `Timeout="0"`.
- **Treat absent and `"0"` as equal in the semantic diff only**: as in D91, it would hide the difference
  without making the output what Config writes.
- **Clamp `Poll` and `Timeout` to 10 and `MaxVal` to 1 on a digital `VirtualIn`**: what Config does with
  other small values is not checked yet (#332).
