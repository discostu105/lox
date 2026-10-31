# D39: Attribute claims on externs

Status: accepted (2026-10-18), amended by [D43](0043-title-claim-carries-refs.md) and
[D63](0063-managed-room-category.md)

## Context

Repurposing a device, such as a sensor moved to another room and renamed, touches objects Lox Logic can match but
not change: the device, its inputs and outputs, their titles and room bindings. The edit needed a byte patch on the
base plus an oracle check. Creating devices stays with Loxone Config, because pairing, serials and channel layout
belong to the GUI and the Miniserver. Assigning an existing device is a plain fact a source file can hold.

## Decision

Three top-level statements claim an attribute of an extern `x`:

```
x.title = "…"
x.room = "…"
x.category = "…"
```

- **Rooms and categories are named by title** and must exist in the base exactly once. They are created in the
  GUI, like every other taxonomy entry. The claim writes the `Place` or `Category` UUID into `<IoData Pr=/Cr=>`
  ([loxone-format.md](../loxone-format.md#rooms-and-categories-iodata-cr-pr)).
- **A claim only rewrites an attribute the object already carries.** An object without an `<IoData>` binding is
  refused with the remedy: set it once in the GUI. Element shape and attribute order are never invented.
- **Managed blocks:** `.title` is an error, since the label is the title. `.room` and `.category` on a managed block
  are [D63](0063-managed-room-category.md).
- **An extern may not claim what it is matched by** (`title:` with `.title`, `room:` with `.room`, …). After the
  push the match would no longer find its object. Pin it by `uuid:` or `iname:` instead.

### Followers

A device's inputs and outputs carry their own `Pr=`. In the house config 673 of 687 device children share their
device's room. Loxone Config has no cascade for this, so Lox Logic defines one:

- A room or category claim also rewrites every descendant whose binding equals the object's original value.
- It skips managed blocks and descendants claimed explicitly for the same attribute, with their subtrees.
- A child bound elsewhere stays where it is.
- A title claim has followers too: the refs that copy the title ([D43](0043-title-claim-carries-refs.md)).

### Lock, teardown, withdrawal

- The lock records the original, the written value and the follower list (`attr_claims`).
- Teardown restores the object unconditionally. It restores each follower only while it still shows the written
  value. A follower moved in the GUI after the claim keeps the GUI's choice and leaves the list on the next compile.
- A claim gone from source leaves a `removed_attrs` tombstone ([D31](0031-removal-tombstones.md)). It keeps
  restoring while the base shows the written value and retires once it does not.
- Claims are planned against the torn-down document before anything is written. Nested claims never record each
  other's write as their original.

### Diff

The semantic diff and the fingerprint include room and category bindings; a diff blind to them would call a moved
device empty. Fingerprint v2 added them. `loxl drift` accepts an older fingerprint and notes what it cannot see;
the next compile records the current one. Lockfile v3 added `attr_claims`.

Validated on the house config: the compiled claims equal the hand patch they replace, and Loxone Config saves the
output without a semantic change.

Not covered: membership lists such as the smoke alarm's device list. They live in the alarm block, not in the
device.
