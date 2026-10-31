# D54: Minting virtual inputs

Status: accepted (2026-10-29), amended by [D64](0064-virtual-modbus-devices.md) and
[D80](0080-v2-one-spelling.md)

## Context

Agent-written automations often need a switch the user flips in the app, such as "party mode" or a setpoint.
The source could only reference a virtual input the GUI had created. A VirtualIn is not a page block. It is
an object of the "Virtual Inputs" folder (`<C Type="VirtualInCaption">`), wired across to the pages, which
[D34](0034-mirror-routing.md) established as legal.

## Decision

`VirtualIn` is a mintable builtin type whose element lives in that folder.

- **Shape.** Connectors `[Q, Qm]`, both sources. Attributes and children are those a Loxone Config save writes
  for a fresh one (see [loxone-format.md](../loxone-format.md#virtual-and-modbus-devices)).
- **Placement.** Appended to the base's first `VirtualInCaption`. It is never pinned in the lock (`page_uuid`
  and `layout` stay empty), never laid out, and not governed by `page` statements. A base without the folder
  is refused.
- **`IName=`** (`VI<n>`, the designation the GUI shows) is minted once, one past the highest number in the
  base and in carried residue. After that it is carried verbatim, like the other input-dialog attributes
  (`connectors::carried_attrs`).
- **Parameters.** `Analog:` (`on`/`off`), `MinVal:`, `MaxVal:` and `DefVal:` are attribute parameters with the
  claim semantics of [D40](0040-program-code.md): written while bound, the GUI's otherwise. The `<IoData>` room
  and category binding and `<Display>` stay GUI-owned.
- **Adopted one at a time.** `loxl adopt --uuid` takes an existing VirtualIn the way it takes a device
  ([D64](0064-virtual-modbus-devices.md)). It promotes an extern on it, carries its input-dialog settings, and
  rebuilds it in its base place in the folder (the tree's order, as for a device's I/O). A whole-config adopt
  leaves virtual inputs as externs. The house's nine adopted and rebuilt byte-identically (#198).

Validated: oracle session 18, see [oracle-sessions.md](../oracle-sessions.md).
