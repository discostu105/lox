# D91: `off` is no attribute, except on a virtual input

Status: accepted (2026-11-01), amends [D80](0080-v2-one-spelling.md), amended by
[D113](0113-adopt-writes-off.md)

## Context

D80 made `on`/`off` the only spelling of the text-boolean attributes (`connectors::bool_params`) and
compiled `off` to `"false"` on all of them. Issue #298 asked whether that matches Loxone Config, on the
assumption that a save of a digital `VirtualIn` leaves `Analog=` out.

The corpus and the oracle say the opposite for `VirtualIn`, and confirm it for the others
([loxone-format.md](../loxone-format.md#virtual-and-modbus-devices), oracle session 37):

- Every `VirtualIn` in the corpus has `Analog=`, `"false"` on a digital one, in every Config version.
  A save keeps `Analog="false"` and adds it where it is missing.
- On `VirtualHttpInCmd`/`VirtualUdpInCmd` `Signed=`/`Analog=`, `VirtualOutCmd` `Analog=` and `VirtualOut`
  `CloseAfterSend=`, no real config has `"false"`: an unticked box has no attribute. A save drops an
  explicit `"false"`.

So the compile was right for `VirtualIn` and wrong for the rest: each GUI save after a compile showed
those attributes as changed, and the next compile wrote them back.

## Decision

- `VirtualIn` `Analog: off` stays `Analog="false"`; no `Analog:` on a new one also writes it.
- On the other text-boolean attributes, a bound `off` writes no attribute. On an existing object it
  removes a ticked one. `connectors::omitted_when_off` names them.
- Decompile and adopt are unchanged: an absent attribute lifts as nothing, which leaves the box with the
  GUI. They never produce `off` on these attributes from a real config. (Since D113 they write `off`
  where a new object would tick the box.)

## Rejected

- **Omit `Analog=` on a digital `VirtualIn` too**, as #298 proposed: a save adds it back, so the
  output would differ from every save.
- **Treat absent and `"false"` as equal in the semantic diff only**: it would hide the difference
  without making the output what Config writes.
