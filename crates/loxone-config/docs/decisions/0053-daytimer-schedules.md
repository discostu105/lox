# D53: DayTimer schedules from source

Status: accepted (2026-10-29), amended by [D69](0069-operating-modes.md)

## Context

A DayTimer's switching times are its `<Entry>` list, its `Modes=` columns and the `N=` count (see
[loxone-format.md](../loxone-format.md#objects-c) and
[operating modes](../loxone-format.md#operating-modes)). They were GUI-owned content
([D19](0019-gui-residue-carried-forward.md)). Automations an agent writes need them ("lights on at 17:00 on
weekdays"), and lox-cli writes schedules too.

## Decision

A `schedule` set and a `<timer>.schedule = <set>` claim, in the shape of the tiles of
[D41](0041-touch-pure-flex-tiles.md).

- **Modes.** A schedule line names the built-in day modes by keyword. Any other operating mode, built-in or
  user-defined, is named as [D69](0069-operating-modes.md) describes.
- **All seven weekdays, always.** Every corpus `Modes=` list has them, and a save fills a missing weekday
  column with an all-day "on" entry. A weekday the schedule does not name is written as an empty column
  (off).
- **The claim owns the whole schedule and nothing else.** It writes `Modes=`, `UserModes=`, `N=` and every
  `<Entry>`. `Analog=`, `DefValue=` and `<IoData>` stay GUI-owned.
- **Values are checked against the timer.** An analog timer needs one value per interval; a digital one takes
  none. Intervals of one day must not overlap, since the GUI's editor cannot represent that.
- **Extern or managed.** A DayTimer is mintable, unlike a Touch Pure Flex, so the claim applies to both. It
  runs as a post-pass over the compiled document, by UUID. It keeps no lock state and restores nothing
  ([D40](0040-program-code.md), [D41](0041-touch-pure-flex-tiles.md)).
- **Schedules are semantics** (lox#162). The diff compares every DayTimer's schedule per mode column, in the
  source's terms: intervals, `activate`, `= value` on an analog timer. It groups the columns that changed
  alike under the day words. `N=` and `UserModes=` count the columns and add nothing. A column keys on its
  mode, not its index, so a reordered `Modes=` list is the same schedule.
- The fingerprint includes the columns (v6). `loxl drift` accepts a v5 fingerprint recorded before.

The syntax of `schedule` sets is in [language.md](../language.md).

Validated: oracle session 18, see [oracle-sessions.md](../oracle-sessions.md).
