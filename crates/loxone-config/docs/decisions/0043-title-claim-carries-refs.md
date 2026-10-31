# D43: A title claim carries the refs that copy the title

Status: accepted (2026-10-29)

## Context

Every `InputRef` and `OutputRef` holds its own `Title=`, and Loxone Config rewrites it to the mirrored object's
title on every save. The exception is the inputs an alarm block collects into its sensor list: refs with `WF` bit
0x1000, whose titles the GUI keeps ([loxone-format.md](../loxone-format.md#refs-c-typeinputrefoutputref-ref)).

A `.title` claim ([D39](0039-attribute-claims.md)) that rewrote only the object left its refs on the old title. The
next GUI save showed up as drift. The house config carried two stale relay refs from such a claim.

## Decision

A title claim has followers, like a room or category claim:

- **Every ref whose `Ref=` is the claimed object** follows, whatever its current title. The GUI would rewrite a
  stale one too.
- **Except collected inputs.** An `InputRef` with `WF` bit 0x1000 keeps its title. The bit is GUI-derived, so it
  marks exactly the refs whose title the GUI does not own. The `OutputRef`s such a block uses carry 0x1000 too, and
  they do follow.
- Managed refs and refs claimed explicitly are skipped, as for [D39](0039-attribute-claims.md) followers.
- Lock, teardown and tombstones are D39's. Each follower is restored to the claim's original while it still shows
  the written value. That is also what the GUI derives once the object is back.
- Created `mirrors:` refs ([D33](0033-minted-mirrors.md)) take the title this compile leaves on their target: the
  claimed one, or the restored one after a withdrawal. The base may still show a withdrawn claim.

Validated: oracle session 12 ([oracle-sessions.md](../oracle-sessions.md)). The existing relay claims fix their stale
refs. Claims on a target with visible, hidden and collected refs, and their withdrawal, save with an empty semantic
diff.
