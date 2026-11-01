# D115: A calendar entry names its mode with `switches:`

Status: accepted (2026-11-01)

## Context

A `CalendarEntry` ([D74](0074-calendar-entries.md)) named the mode it switches on by the stored
attribute, `CalType: 10`: the mode's `Mode=` number ([loxone-format.md](../loxone-format.md#calendar-entries)).
Rules and schedules name a mode one way since [D110](0110-mode-reference-spelling.md): a built-in one
by its day word, one the module declares or externs by its slug, any other by its quoted title. The
calendar was the last place that took a raw mode number. Issue: #326.

The facts the mapping rests on are in loxone-format.md: the twelve built-in modes have fixed UUIDs
`00000000-0000-000N-1500000000000000` and `Mode=` N − 1, user modes have negative numbers, and an
entry's `CalType=` is that number, absent for 0 (the holiday mode). The house config has two entries,
`CalType: 10` (heating period) and `CalType: 11` (cooling period).

## Decision

**`switches: <mode>`** names the mode, in D110's one spelling: `switches: heating`,
`switches: party`, `switches: "Urlaub"`. Only a day word that names one mode counts (`weekdays` is
five). It compiles to the same `CalType=`, and the holiday mode to no attribute.

- **The number is refused** with the replacement: "Write `switches: heating` instead of
  `CalType: 10`". A built-in mode's number is fixed, so `loxl check` names it (and the language server
  offers it as a quick fix). A user mode's number is only in the config, so `loxl compile` names it:
  the module's slug for the mode, else its title. `loxl check` lets a negative number through to the
  compile.
- **Another spelling of the mode** is refused as in D110: a title where the module has a slug, a
  title or extern slug of a built-in mode where it has a day word.
- **`CalType=` is written once the compiled document numbers every mode**, so `switches:` can name a
  `Mode` minted in the same compile.
- **Without `switches:`** a fresh entry has no `CalType=` (the holiday mode, as before) and an
  adopted one keeps the base's. The source does not state the mode then.
- **`loxl fmt`** rewrites a built-in mode's number to its day word, and a title to the module's slug.
  **`adopt`** writes `switches:` always, `switches: holiday` included. `decompile` does not lift
  calendar entries. Amended by [D127](0127-calendar-entry-gone-mode.md): for a mode the config no
  longer has, adopt writes a comment instead of `switches:`.

The compile output and the lock do not change.

## Rejected

- **`mode:`**, as the issue proposed. `Mode:` is the entry's kind of date (fixed, Easter, period,
  range), and two arguments that differ only by case read as one. `mode:` and `Mode: heating` are
  errors that name `switches:`.
- **Renaming `Mode:`** to free the name. It is the stored attribute, like every other setting of the
  entry; renaming it is a separate change.
- **A positional mode** (`CalendarEntry("Heizperiode", heating, …)`). The first positional argument
  is the title; a second one says nothing about what it is.
- **Keeping `CalType:` next to `switches:`.** Two spellings for one fact, which D110 removed
  everywhere else.
