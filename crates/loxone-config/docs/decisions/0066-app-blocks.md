# D66: Blocks minted from source in the app

Status: accepted (2026-10-31)

## Context

The push writes the app catalogue ([D61](0061-app-catalogue.md)). It lists a new control only when Lox
Logic reproduces the entry Loxone Config writes for its type. Most entries name states a save adds to the
block, such as a jalousie's seven special states (`SpStates=`). A minted block lacks them until a save (see
[loxone-format.md](../loxone-format.md#the-app-catalogue-loxapp3json)).

## Decision

- **A minted block gets what the first save would give it** (`connectors::save_defaults`):
  - the special states, minted once with the block and pinned in the lock (`states`, lockfile v8);
  - `<IoData Visu="true">`;
  - the history settings `<COHist>`.

  Once the base carries the block, these are the GUI's ([D19](0019-gui-residue-carried-forward.md)), like
  every `SpStates=`.
- **New entry types**, each reproduced byte for byte from three saves (98 entries hidden and written
  again):
  - the automatic jalousie (`Jalousie`);
  - the central light and shading blocks (`CentralLightController`, `CentralJalousie`), with their members
    from `rec=`;
  - the four meters (`Meter`), with details and `statisticV2` from the units, the power name and the
    `<statGrp>` groups.
- `defaultRating` is `<IoData Rating=>`.
- The push names a new control of a known type that has no room or no category: the app does not list it.
- **Not derived; these entries wait for a save:**
  - a light controller's circuits (`subControls`), which come from its circuit settings and the actuators
    it drives (see [D72](0072-light-controllers-catalogue.md));
  - a central block without members;
  - a meter with a unit of its own for the total;
  - the single-instance types: alarm, smoke alarm, irrigation, room controller.

Validated: oracle session 28 (a minted jalousie: the save kept its states, `<IoData>` and `<COHist>`, and a
recompile of the save was byte-identical; see [oracle-sessions.md](../oracle-sessions.md)). Live on the
house (2026-10-31): a jalousie minted from source and pushed was served in the app catalogue (`Jalousie`,
room and category, 16 states). Its seven minted states streamed values over the WebSocket, and `removed`
deleted entry and block again.
