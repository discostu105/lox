# D71: Several objects in one statement, device settings, Air serials

Status: accepted (2026-10-31)

## Context

Issues #199 and #197 asked for three things: setting one value on many objects, the settings of an Air or
Tree device itself, and telling Air devices apart by serial. Air I/Os narrowed by `device:`
([D55](0055-device-narrowed-matching.md)) only to the Air base, where all of them look alike.

## Decision

- **Several objects in one statement.** `(a, b, c).MinTime = 600` is the setting, or `title`, `room`,
  `category`, on each object.
  - The parser writes one statement per object, each remembering its line. Compile, lint, adopt and rename
    see only the single form.
  - `fmt` writes the run back as the one line (`SetDecl::bulk`, `AttrDecl::bulk`).
  - A slug listed twice is an error. `tiles`, `schedule` and `moods` take one object.
- **An Air or Tree device's own settings** (`settings::DEVICE_TYPES`), with the rules of
  [D65](0065-physical-io-settings.md):
  - `Desc` is added after `Type`, as on an input;
  - where it is installed (`Installation`, `SwitchBoard`, `SwitchBoardRow`, `SwitchBoardPos`) is rewritten,
    never added. A board written onto a Tree room device is dropped by the save (see
    [loxone-format.md](../loxone-format.md#hardware-identity-serial)).
  - Not claimable: `PWMFrequency`, `UseDimmer`, `UseRGBDimmer`. The corpus shows one value each, so what
    another value does is not known.
  - Pairing and firmware stay in Loxone Config.
  - The semantic diff compares these settings. They joined the drift fingerprint in its v9, and a lock
    recorded with v8 still reads as in sync.
- **Air serials.** An Air device's serial is the MAC its link-local `IP=` is formed from (see
  [loxone-format.md](../loxone-format.md#hardware-identity-serial)). `device:` narrows Air I/Os by it.
  `device:` also takes an object's own serial, so a device itself matches by `device:`. Serials compare
  without case and without `:`.

Validated: oracle session 30. Description, place and board on an Air relay and a Tree dimmer, and
description and place on a Tree room sensor, gave an empty diff; a recompile of the save was
byte-identical. See [oracle-sessions.md](../oracle-sessions.md).
