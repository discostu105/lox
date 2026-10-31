# D65: Settings of physical inputs and outputs from source

Status: accepted (2026-10-31)

## Context

Only Loxone Config creates the inputs and outputs of the extensions, Tree and Air devices and 1-Wire
sensors (`settings::SETTING_TYPES`). Their settings are plain attributes, though: description, display
unit, analog correction, range and transmission timing (see
[loxone-format.md](../loxone-format.md#settings-of-physical-inputs-and-outputs)).

## Decision

- **A setting on an extern is a claim.** `x.MinTime = 600` on an extern of such a type claims the
  attribute, the way `x.Port = v` claims a `Def=`. The original goes into the lock under
  `<object>#<setting>`. It comes back when the statement leaves the source, with the same tombstone until
  the restore is deployed ([D31](0031-removal-tombstones.md)).
- `Unit` is `Unit=` of the object's `<Display>`.
- **Added only where a save would omit it.** A setting the element lacks is added only if a save omits it
  at its default: `Desc`, the low ends of the correction, `MinVal`, `ResetTime`, `AverageTime`. It goes
  where a save writes it: `Desc` right after `Type`, the rest in Loxone's order.
- **The others are rewritten, never added.** An analog correction on a digital input is an error, not an
  attribute.
- `Desc` is the name the app shows, so a description renames the app control. The push carries a new unit
  into the catalogue's value formats ([D61](0061-app-catalogue.md)).
- The semantic diff compares these settings. The drift fingerprint hashes them together with the device
  settings of [D64](0064-virtual-modbus-devices.md) ([D21](0021-drift-baseline.md)). They joined the
  fingerprint in its v8; a lock recorded with v7 still reads as in sync.

Validated:

- Oracle session 27: description, unit, low end, averaging and time on two Air sensors gave an empty diff.
  The save kept every attribute where Lox Logic wrote it, and a recompile of the save was byte-identical.
- Live on the house Miniserver (2026-10-31): the description and unit of a garden sensor were pushed. The
  Miniserver formatted the value as `8.2 °C`, the app served `%.1f °C`. Withdrawn, the element was
  byte-identical to before (`8.2°`).
