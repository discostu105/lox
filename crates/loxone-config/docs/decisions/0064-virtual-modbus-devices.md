# D64: Virtual and Modbus devices from source

Status: accepted (2026-10-31)

## Context

The virtual inputs and outputs and the Modbus registers sit below devices that only Loxone Config created.
A source could reference them, but not declare a new device or a new input of one.

## Decision

The devices and their I/O are declared like blocks.

- **Devices:** VirtualHttpIn, VirtualUdpIn, VirtualTextIn, VirtualOut, ModbusServer, ModbusDev.
- **I/O below them:** VirtualHttpInCmd, VirtualUdpInCmd, VirtualOutCmd, ModbusASensor, ModbusSensor,
  ModbusAActor.
- **Where an object goes follows from its type.** A device goes into its folder (see
  [loxone-format.md](../loxone-format.md#virtual-and-modbus-devices)). An I/O goes into the device its `device:` binding
  names: a slug of a managed block or an extern, of the one type (or two) that can hold it.
- **Settings** (`Address`, `Poll`, `Check`, `ModbusAddress`, …) are element attributes, bound like
  attribute parameters. A new device or I/O needs the ones that make it work (`connectors::periphery`).
- **The rest of the element is the GUI's and is carried**, as for page blocks
  ([D19](0019-gui-residue-carried-forward.md)): IoData, Display, `Cl`, `WF` and attributes no table names.
- **`IName=`** is minted per prefix, past the highest in use, and kept.
- **Order and ownership.** The rebuild puts a device's I/O back in base order. A managed device carries no
  I/O it does not manage:
  - `adopt` takes a device with everything below it, and promotes the externs the source has on them
    ([D62](0062-house-control-types.md));
  - an extern that resolves to a managed object is an error;
  - a device's teardown carries the I/O the GUI added below it, never a withdrawn one. The tombstone holds,
    also inside the captions a device groups its I/O in ([D31](0031-removal-tombstones.md)).
- A wire into a managed actuator that the base draws through an OutputRef on the writer's page is drawn
  the same way again ([D34](0034-mirror-routing.md)).

Validated:

- Oracle session 26: one of each type minted gave an empty diff, and a recompile of the save was
  byte-identical (see [oracle-sessions.md](../oracle-sessions.md)).
- The house's 17 devices with their 195 I/O adopted and rebuilt byte-identically.
- Live on the house Miniserver (17.2.8, 2026-10-31): a new HTTP input with a command, a command in an
  adopted device, a text input and a Modbus register read in an adopted device were pushed and removed
  again. The commands and the register delivered values. The push and the removal both reached a byte
  fixpoint.
