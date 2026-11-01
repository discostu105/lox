# D101: Hardware declarations

Status: accepted (2026-11-01)

## Context

Loxone Config pairs Tree and Air devices and creates their inputs and outputs. Lox Logic could reference
them as externs and claim some I/O settings ([D65](0065-physical-io-settings.md)), one statement per
setting, each restored when it leaves the source. Taking over a whole device took dozens of statements,
and its title, room, category and statistics stayed with the GUI.

## Decision

- **A declaration takes over existing hardware.** `taster = TreeDevice("Taster", serial: "…", …)` and
  `klingel = TreeSensor("Klingel", device: taster, iname: "I0", …)`. Devices: `TreeDevice`,
  `LoxAIRDevice`, `LoxTree`, `LoxAIR`. I/Os: the four Tree and four Air sensor and actor types, below
  their device (`hardware::DEVICES`).
- **Matching.** A device by `serial:` (an Air device's from `IP=`) or `uuid:`. An I/O by `iname:` inside
  its device, which needs the device matched by serial, or by `uuid:`. A new extern matcher `serial:`
  matches a device by its own serial.
- **Lowered in expansion.** Each declaration becomes an extern, its port bindings `<-`/`=` statements on
  it, and an owned record (title, room, category, settings). The rest of the compiler sees externs.
- **Owned, not claimed.** The owned values are written on every compile, and the lock keeps no original.
  A declaration that leaves the source releases the hardware: the element keeps what the source wrote
  last. Nothing is restored and nothing is deleted. The lock marks the externs `owned`.
- **One spelling.** The title is the label; ports are bound in the argument list. `.title`, `<-` and `=`
  on a declaration are errors. `.room`/`.category` fold into the declaration, as on a managed block
  ([D63](0063-managed-room-category.md)), but only rewrite a binding the element has.
- **Statistics.** `Statistics: on` writes the type's `StatsType` (7 digital, 11 analog), `off` removes
  it with `StatsAutoDel`. Other kinds are not written.
- **Extensions.** `LoxTree` and `LoxAIR` get `Installation` and the `SwitchBoard` settings, rewritten
  only.
- **Adopt.** `adopt --uuid <device>` declares the device and every I/O below it with what the config
  holds. Externs among them are promoted. An I/O alone is refused.
- The semantic diff compares the extension settings and the `StatsType` of hardware I/O. They joined
  the drift fingerprint in its v11; a lock recorded with v10 still reads as in sync.

Rejected:

- **Claims with restore** (D65 for everything): removing a device declaration would roll back titles and
  rooms the house has used for months. Owning a device means the source has the last word, then lets go.
- **Teardown and rebuild** like virtual devices ([D64](0064-virtual-modbus-devices.md)): pairing lives
  in the element, so Lox Logic must never delete or create a paired one.
- **I/Os nested in the device's argument list:** the I/Os are referenced on their own (`klingel.Q`), so
  each needs its own slug.

Validated:

- Oracle session 51: a save keeps `StatsType` added (7, 11) and removed, with and without
  `StatsAutoDel`, and the extension settings rewritten.
- Oracle session 52: a module over one Tree device of the house config, four of its I/Os and its
  extension compiled to an empty diff after a save; the recompile of the save was byte-identical.
