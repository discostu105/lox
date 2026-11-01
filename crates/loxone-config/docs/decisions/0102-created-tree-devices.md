# D102: Tree devices created from source

Status: accepted (2026-11-01)

## Context

[D101](0101-hardware-declarations.md) takes over hardware that Loxone Config paired. Planning a house
starts earlier: the devices are known before anyone runs a Tree search. Loxone Config lets you add a
Tree device offline, unpaired, and pairs it later. Lox Logic can do the same, for device types whose
element is known exactly.

## Decision

- **A declaration with `kind:` creates the device.** `melder = TreeDevice("Melder Flur", kind:
  "PresenceSensorTree", tree: tree1, branch: "left")`. `kind:` is the type's `TId`, `tree:` names the
  declaration of its Tree extension, and `branch:` picks the left or right branch. It can be left out
  when the extension has one branch. The I/Os below it are declared as in D101, by `iname:` inside the
  device.
- **A table of verified kinds** (`tree_kinds.rs`), each element as a save of Loxone Config writes it.
  Four kinds: Touch Tree, Presence Sensor Tree, LED Ceiling Light RGBW Tree, Valve Actuator Tree. Any
  other kind is refused by name. A kind is admitted when every instance in the public corpus has the same
  I/O, at least three instances exist, and a config with a created one opens and saves with an empty
  semantic diff ([connector-db.md](../connector-db.md#tree-devices)).
- **Created once, then matched.** The compile adds the element, unpaired (`Serial="00000000"`), at the
  end of the branch, and the lock's `hardware` map keeps its UUID. Later compiles match that UUID. A base
  without the element (an older one, or the device deleted in Loxone Config) gets it back under the same
  UUID. Changing `kind:` of a created device is refused.
- **Deleted only by `removed`.** A created device that leaves the source is refused until `removed
  <slug>` says so. Then it becomes a tombstone, like a managed block. A device that feeds a wire Loxone
  Config drew is refused. Declaring it by `uuid:` or `serial:` instead keeps it and ends the creation.
- **A guard in the compiler.** A compile whose output lacks a device of the base without a tombstone
  fails. `lox config push` compiles, so it never deletes hardware the source did not remove.
- **Not paired, and said so.** `loxl compile` and `lox config push` print a note for each created
  device that is still unpaired. How a Miniserver runs a device it has not searched for is not verified.
  Pairing stays in Loxone Config: a Tree search assigns the serial to the planned device.

Rejected:

- **Kinds with instance-specific content.** The LED Spot RGBW Tree (`32790`): its corpus instances come
  from one installation, and the title of its output follows the device's. The Nano 2 Relay Tree
  (`32793`): `ApiActor` carries an instance-specific `AiT`, and a configuration blob.
- **Air devices.** Their identity is a MAC that only pairing gives.
- **Link extensions.** Not covered yet.

Validated:

- Oracle sessions 53 and 54: corpus shapes of the candidate kinds added by hand to a copy of the house
  config. The save added `TId`, reordered the attributes, wrote `<IoData></IoData>` and, on a Valve Actuator Tree,
  `SwitchBoard=""` and `SwitchBoardPos="1"`. The table carries that saved form.
- Oracle session 55: a compile created one device of each kind on the house config's Tree extensions,
  one into an empty branch. The save gave an empty semantic diff and left the created elements byte for
  byte as compiled; a recompile of the save was byte-identical.
- Oracle session 56: `removed` on two of them; the save gave an empty semantic diff.
