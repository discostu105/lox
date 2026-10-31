# D67: Deleting a rule from source

Status: accepted (2026-10-31)

## Context

A rule deleted in the app lives only in the Miniserver's custom changes. The program file still has it,
and the next upload brings it back (see
[loxone-format.md](../loxone-format.md#automatic-designer-rules-c-typeautopilotrule-apc)). Removing a
`rule` declaration from source leaves the rule in place ([D60](0060-creating-rules.md)).

## Decision

- **`removed <slug>`** in place of a `rule <slug> "Name"` declaration deletes the rule the lock pins.
  - The pin becomes a removal tombstone ([D31](0031-removal-tombstones.md)) under the rule's UUID.
  - Every compile against a base that still holds the rule deletes it, with its connectors, which live
    inside it.
  - A deployed base retires the tombstone. After that the statement is an error, as for a block.
- A rule whose outputs feed a block is refused, since the wire would dangle.
- The app catalogue lists only the rules container, not single rules, so it is unchanged.
- **`lox config push --force`** lets a custom change to an object the upload deletes pass: the change goes
  with the object. A rule deleted in the app is such a custom change.

Validated live on the house (2026-10-31): the test rule of #181, deleted in the app, was deleted by the push
with its switch. `/sys/custom` was empty afterwards and the program kept running.
