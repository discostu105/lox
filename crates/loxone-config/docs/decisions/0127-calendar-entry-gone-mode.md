# D127: Adopt takes over a calendar entry whose mode is gone

Status: accepted (2026-11-01), amends [D115](0115-calendar-entry-mode.md)

## Context

[D115](0115-calendar-entry-mode.md) has `adopt` write a calendar entry's mode as `switches:`. An
entry's `CalType=` can name a mode the config no longer has: a user mode deleted since, or the heating
or cooling mode in a config from before they existed. The public corpus has one such entry
(`wiki_bewaesserung`, `CalType="-7"`, no `Mode` with `Mode="-7"`). `adopt --uuid` refused it, so the
entry could not be taken over at all, not even to delete it or point it at another mode. Issue: #357.

## Decision

**Adopt writes no `switches:` for such an entry, and a comment in its place** that names the number:
`# Its operating mode (CalType -7) is not in the config. Without `switches:` it stays; name a mode to
switch that one.` Without `switches:`, D115 already has the compile keep the base's `CalType=`, so
the entry compiles back to the same bytes. Writing `switches:` then points it at a mode the config has,
and removing the block deletes it, as for any adopted entry.

A built-in mode counts as gone only for heating and cooling, the two that older configs lack (the
compile refuses `switches: heating` there for the same reason).

## Rejected

- **Keep refusing, and name the fix** (point the entry at another mode in Loxone Config first). It
  makes a dead entry the one thing adopt cannot take over, and the fix is a GUI round trip for what one
  line of source does.
- **A `switches:` value for a missing mode** (`switches: -7`, or a placeholder word). It would bring
  back the raw number D115 removed, or invent a name the config does not have.
