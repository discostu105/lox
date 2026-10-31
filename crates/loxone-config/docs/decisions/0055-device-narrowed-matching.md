# D55: Device-narrowed extern matching

Status: accepted (2026-10-29), amended by [D71](0071-multi-object-statements.md)

## Context

Identical hardware repeats its I/O titles. Every Tree push-button interface has a "Taster 1", and every
Extension has the same input names. `title:` is then ambiguous, and decompile falls back to `uuid:`, which
tells a reader nothing. Where a room tells the objects apart, `room:` narrows. The I/Os of an Extension in the
distribution board share its room or have none, so `room:` does not help there.

The device's serial number does. It is printed on the hardware, scanned at installation, and never changed by
the GUI. Devices nest the way the hardware is cabled, each with its own serial (see
[loxone-format.md](../loxone-format.md#hardware-identity-serial)).

## Decision

A `device: "<serial>"` constraint, like `room:` and `category:`.

- It narrows an `iname` or `title` match to objects whose nearest enclosing device carries that serial. The
  I/O belongs to the innermost device: a Tree device sits in a Tree Extension inside a Miniserver.
- The Miniserver's own I/Os and virtual folders narrow by the Miniserver's serial. This tells the Miniservers
  of a client/gateway project apart.
- An Air device's serial is the MAC its `IP=` encodes ([D71](0071-multi-object-statements.md)).
- An object's own serial also counts, so a device itself matches by `device:`.
- Serials compare without case and without `:`.
- Not combined with `uuid:` (already pinned) or `mirrors:` (narrowed by page).
- **Decompile** emits `title:` plus `device:` where the title is unique within its device but not overall.
  This comes before the `uuid:` fallback (27 externs across the corpus's wired views).
- A serial is personal data, and `loxl scan` reports it. Modules of a real house belong in a private
  repository anyway.

Evidence (web corpus, titled I/Os below a device whose type and title repeat in their config):

| Below | I/Os | Told apart by device | By room |
|---|---|---|---|
| Extensions | 34 | 34 | 0 |
| Tree devices | 204 | 45 | 151 |
| The Miniserver | 4017 | not counted | not counted |

Below the Miniserver, most repeats are within one Miniserver (call and virtual commands), which neither
constraint resolves.
