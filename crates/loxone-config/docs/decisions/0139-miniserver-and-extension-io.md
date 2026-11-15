# D139: The Miniserver's and its extensions' I/O as hardware declarations

Status: accepted (2026-11-15)

## Context

After [D138](0138-declarations-carry-fan-in-and-tiles.md) the house config declares every Tree and
Air device, yet 86 externs were left. 66 of them are inputs and outputs that no declaration could
take over:

- 30 `Actor`: seven relay outputs of the Miniserver itself, 23 of two Relay Extensions.
- 27 `OvertempShutdown`: the Miniserver's, and one on each Air relay and Nano IO Air.
- 4 `DigitalIn`: three of a DI Extension, the Intercom's bell button.
- 5 `ApiActor`: the API connectors of the three Touch Pure Flex, the Code Touch and the Intercom.

[D101](0101-hardware-declarations.md) knew only Tree and Air types. Adopt also refused an I/O alone
("comes with its device"), so a device declared before its I/O types were known could not gain
them.

The Miniserver is a special parent. It stays an extern whose settings are claims
([D135](0135-project-and-miniserver-settings.md)), and every extension and network device sits
below it in the tree. Their designations repeat its own: the Relay Extension's `Q1`, the Intercom's
`I1` and every `TSD` sit below the Miniserver as well. A match by designation within the Miniserver,
or by the serial the nearest ancestor carries, is ambiguous.

## Decision

- **New devices:** `LoxREL` (`Actor`, `OvertempShutdown`), `LoxDigin` (`DigitalIn`) and
  `NetworkDevice` (`DigitalIn`, `OvertempShutdown`, `ApiActor`). They are matched by `serial:` or
  `uuid:` like the Tree and Air devices. An Intercom has no serial, so adopt matches it by `uuid:`.
- **New I/O types on known devices:** `ApiActor` below a `TreeDevice`; `OvertempShutdown` and
  `ApiActor` below a `LoxAIRDevice`.
- **The Miniserver's onboard I/O** (`Actor`, `DigitalIn`, `OvertempShutdown`) names the Miniserver's
  extern as its `device:` and is matched by `uuid:` only. `LoxLIVE` does not become a declaration:
  it stays the extern that carries the system settings.
- **Adopt takes an I/O alone** when its device is declared already, or when it sits on the
  Miniserver and the Miniserver is an extern of the module. It matches by designation when the
  device goes by serial and no sibling of the type shares the designation, else by `uuid:`. Without
  a declared device the refusal names the `adopt --uuid` to run first.

The settings stay those of [D65](0065-physical-io-settings.md): `Actor` and `DigitalIn` carry them,
`OvertempShutdown` and `ApiActor` own only their title, room and category. `Statistics:` stays with
the Tree and Air types, so the drift fingerprint stays at v16.

## Consequences

- The house config adopts its 2 Relay Extensions, the DI Extension and both Intercoms with all
  their I/O, and the remaining I/O of the Miniserver and of the declared Air and Tree devices one by
  one. The recompile is byte-identical, and no hardware I/O is an extern any more.
- An `Actor` or `DigitalIn` below a device type not listed (a Dimmer Extension, a Modbus device)
  still cannot be declared. Adopt refuses it and names the device types it knows.
- A `LoxDigin` or `LoxREL` declaration can be created by `serial:` only after Loxone Config paired
  the extension. Pairing stays in Loxone Config.
