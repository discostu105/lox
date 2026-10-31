# D56: Creating pages from source

Status: accepted (2026-10-29)

## Context

[D28](0028-page-statement.md) makes a page title authoritative but leaves creating the page to Loxone Config.
An agent that needed room for a block chain had to stop and ask a human to click "Neue Seite". A nine-block
feedback loop needs 17 088 twips in one row; the full page it belonged on had 10 560 free on the A3 sheet
([D50](0050-placement-paper-sheet.md)).

## Decision

`page "Title" (room: "…"[, category: "…"])` creates the page when no page carries the title.

- **Opt-in by `room:`.** Without it, a missing title stays the D28 error, so a typo never creates a page. The
  error names the syntax. `room:` is required because the GUI requires it: its dialog keeps Save disabled
  while the room is "Nicht zugeordnet". Lox Logic likewise refuses the unassigned `Place` (`First="true"`).
  The category defaults to the unassigned one, as in the dialog.
- **The element is what the GUI writes** (see [loxone-format.md](../loxone-format.md#container)), with `V=` the
  document's block version ([D46](0046-block-version.md)). The compiler leaves out `LtE=`, and a GUI save of a
  compiled page does not add it.
- **Position.** The GUI files a new page among the program's pages in case-insensitive title order, but never
  re-sorts, so real lists are not sorted. The page goes behind the last page that sorts at or before it, next
  to its namesake. It never goes ahead of the special pages (`SpNo`).
- **Identity in the lock** (v5 `pages`, title → UUID). The UUID is minted like a block's (`NextObj` + 1).
  This makes the compile → push → download window a fixpoint: a recompile against the old base re-creates the
  same page, and against the downloaded one creates nothing. A base that titles the UUID differently means the
  page was renamed in Loxone Config. That is an error, not a second page.
- **Ownership ends at creation.** Once the base has the page, it is Loxone Config's like any other. Room,
  category, title and position are not enforced; a claim on them would be [D39](0039-attribute-claims.md)'s
  job, and nothing needs it yet. A removed statement releases the lock entry without deleting the page, since
  deleting a page deletes whatever else the GUI drew on it.

Validated: oracle session 19, see [oracle-sessions.md](../oracle-sessions.md).
