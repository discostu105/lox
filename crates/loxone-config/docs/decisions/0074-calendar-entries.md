# D74: Calendar entries

Status: accepted (2026-10-31)

## Context

An entry of the *Betriebszeiten* calendar switches an operating mode on for a date. It is a
`CalendarEntry` object in the `CalendarCaption` folder (see
[loxone-format.md](../loxone-format.md#calendar-entries)). Issue: #211.

## Decision

A calendar entry is minted, adopted and edited like a virtual input ([D54](0054-virtual-inputs.md)):
`CalendarEntry("Title", CalType: …, Mode: …, …)`. Its parameters are the attributes of the corpus's 397
entries: `CalType` names the mode ([D69](0069-operating-modes.md)), `Mode` the kind of date (fixed,
Easter, period, yearly range). Amended by [D115](0115-calendar-entry-mode.md): the mode is
`switches: <mode>` now, and `CalType:` is refused.

Validated: oracle session 31, where fresh entries of the period, Easter and yearly-range kinds were saved
without change (see [oracle-sessions.md](../oracle-sessions.md)). Live on the house (2026-10-31), a
fixed-date entry for the day switched its mode on.
