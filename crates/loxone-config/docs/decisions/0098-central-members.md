# D98: Central block members and energy units from source

Status: accepted (2026-11-01), extends [D95](0095-block-settings.md)

## Context

[D95](0095-block-settings.md) refused the member lists of alarms and central blocks because no save had
shown how Loxone Config keeps an edited list. It also left a set of settings refused for lack of
evidence: on the room controller, the irrigation, the energy flow monitor and the
`TpfController`. Issues: #278, #253.

Oracle sessions 47 to 50 ([oracle-sessions.md](../oracle-sessions.md)) and the corpus
([loxone-format.md](../loxone-format.md), *Objects*) showed:

- A save keeps a central block's `rec=` as written: its order, a UUID of another type, a duplicate. It
  drops a UUID of no object, removes an empty `rec=` and moves one written elsewhere before `SpStates=`.
- An alarm's `Objects=` does not own its hidden refs. A UUID dropped from the list leaves the ref in
  the file and wired; a ref removed from the file leaves a dangling UUID in the list.
- A save keeps an EFM's `UnSt=`, `Un1=` and `Un2=` as written, adds none and drops an empty one.

## Decision

- **`<central>.members = (a, b, …)`** sets a `CentralLight`'s or `CentralShade`'s `rec=` to those
  blocks, in the order written. Members are externs or managed blocks of the member type
  (`LightController2`, `AutoJalousie`). The compile checks the type and refuses a duplicate itself,
  since a save would keep either. `()` leaves `rec=` out. A new `rec=` goes before `SpStates=`.
  - A claim, not a parameter: it names objects, like `.moods` and `.schedule`. One claim per block,
    allowed in a template body.
  - No lock state. Withdrawing the claim leaves the list as last written.
  - Decompile and adopt lift the list of a lifted central block. A member not lifted joins as an extern.
    A list that names a missing object, another type or a duplicate stays with Loxone Config, with a
    `# members <slug>: not lifted — …` comment that says why.
  - The listed app catalogue follows: `details.controls` of the central entry is rewritten in the
    new order.
  - `loxl diff` compares `rec=` on central blocks.
- **`Un1:`, `Un2:` and `UnSt:` on `EFM`** take text, like a meter's units. Empty is refused, since a
  save drops it.
- **The drift fingerprint v13 includes both.** A lock recorded with v12 still reads as in sync.
- **Still refused by name** (`connectors::unclaimed_reason`):
  - An alarm's `Objects=`: the hidden refs it names stay in the file, wired, whatever the list says.
    `Entries=` and `CentralOut=`: no field, no known meaning.
  - An EFM's `StatsType=`: the statistics settings are not documented.
  - `HeatIRoomController2` `SourceCap=`, `ExT=`, `custRU=`, `custRI=`, `srcName=`, `PrioH=`, `PrioC=`,
    `Cap=`, `DefFan=`, `DefFanNP=`: nine blocks in six corpus configs, mostly one value each, and no
    documented meaning. `UUIDTimer=`, `Objects=`, `linkC=` name objects Loxone Config keeps.
  - `Irrigation` `TRK_NE=`/`TRK_MAX=` and `TpfController` `PRE=`: no corpus block at all.

## Rejected

- **A `members:` parameter holding a string.** A parameter holds a value; a list of blocks needs
  slugs, checked and renamed like any reference.
- **Write the alarm's `Objects=` the same way.** The list and the hidden refs would drift apart: the
  compiler would have to add, wire and remove refs whose layout no document covers.
