# D126: A calendar entry states its date as a date

Status: accepted (2026-11-01)

## Context

Since [D115](0115-calendar-entry-mode.md) a `CalendarEntry` names its mode with `switches:`, but its
date was still the stored attributes ([D74](0074-calendar-entries.md)): `Mode: 4` (the kind), `Month`,
`Day`, `Month2`, `Day2`, and for a period `From: 6393, To: 6456`, days since 2009-01-01. A reader
cannot tell what that means without the table, and `Mode:` next to `switches:` reads like the
operating mode. The rest of the language writes dates and times as such (`when date 2026-12-24`,
`clock 2026-01-03 07:30`, `At: 19:00`). Issue: #349.

The facts are in [loxone-format.md](../loxone-format.md#calendar-entries): four kinds (no `Mode`, 1, 3,
4) and no other in the corpus's 431 entries. A kind change keeps the old kind's attributes: three
fixed dates carry an `EasterOffset`, Easter entries carry a date. The house config has two yearly
ranges.

## Decision

The date in the source's own terms, written bare like `when date`:

| Source | Kind | Compiles to |
|---|---|---|
| `on: 12-31` | a day every year | no `Mode=`; `Month`, `Day` |
| `easter: 1` | days after Easter Sunday | `Mode="1"`, `EasterOffset` |
| `from: 2026-07-04, to: 2026-09-05` | one period | `Mode="3"`, `From`, `To` |
| `from: 10-01, to: 04-30` | a range every year | `Mode="4"`, `Month`, `Day`, `Month2`, `Day2` |

- **A year or none tells the kind of a range.** `from:`/`to:` with `YYYY-MM-DD` is one period, with
  `MM-DD` every year; mixing them is an error. `on:` takes no year (a one-day period is
  `from: d, to: d`, and the error says so).
- **Only the attributes the kind reads are written**, with `Mode=`. The others keep what they hold,
  as a Loxone Config save does, so an adopted entry compiles to the same bytes, stale attributes
  included. A date attribute an adopted entry gains goes where a save writes it.
- **The stored attributes are refused**: `Mode`, `Month`, `Day`, `Month2`, `Day2`, `From`, `To`,
  `EasterOffset` and `WeekDayInMonth`, with the replacement ("Write `from: 10-01, to: 04-30` instead
  of `Mode: 4, Month: 10, …`"), which the language server offers as a fix. `WeekDayInMonth` is 1
  wherever it occurs and a fresh entry gets it, so it has no source form. An undocumented `Mode`,
  an impossible day or a `WeekDayInMonth` other than 1 is refused without a replacement, and adopt
  refuses such an entry.
- **`loxl fmt` rewrites** the stored attributes where their numbers give a date, and drops
  `WeekDayInMonth: 1`. **`adopt`** writes the date.
- `MM-DD` and `YYYY-MM-DD` are a value only for these three keys, where the parser reads `12-31` as a
  date, not a subtraction. Two digits each, as in `when date`; `02-29` is a day of the year. A period
  starts on 2009-01-01 or later (day 0). `to` before `from` is allowed: the corpus has two such
  periods.

The compile output and the lock do not change.

## Rejected

- **`yearly: 10-01..04-30`**, as the issue proposed. `..` is no token of the language, a schedule's
  range is `06:00-07:00`, and `from:`/`to:` already say start and end; the year says the rest.
- **`easter: +1`.** A number has no `+` anywhere else; `easter: 1` and `easter: -2` read the same.
- **Writing 1 into the attributes the kind does not use.** That is what a fresh entry has, but an
  adopted entry with stale attributes would change on its first compile.
- **Keeping the raw attributes next to the date.** Two spellings for one fact.
