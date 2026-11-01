# D110: One spelling per operating mode

Status: accepted (2026-11-01)

## Context

Three places name an operating mode, and they did not agree:

| Where | Before |
|---|---|
| `when mode <mode>`, `then mode <mode> on\|off` | a `Mode`'s slug or any mode's quoted title |
| a schedule line's days | a day word (`mon`, `holiday`, `heating`, …) or any mode's quoted title |
| `CalendarEntry(CalType: n)` | the mode's `Mode=` number |

So one mode had up to three spellings in one module. `party = Mode("Party")` could be `when mode party`
in a rule and `"Party"` in a schedule, and the built-in Monday could be `mon` or `"Montag"`.
Decompile and adopt always wrote the title.

The evidence for picking one:

- **A slug is the module's name for an object.** Every other reference (wires, claims, rule targets,
  tests) uses it. `loxl rename` follows it, and a title change in Loxone Config does not break it.
  The lock pins the object behind it.
- **Titles are locale-volatile for built-in modes** (`Montag`, `Feiertag`). A save in another language
  renames them. The day words are fixed (the built-in modes have fixed UUIDs in every config).
- **A title is the only name for a mode the module does not know.** A rule may test a mode made in
  Loxone Config that the module never declares, and declaring an extern just to name it would be noise.
- The house config names no mode in a rule or schedule. It declares its six user modes and externs two
  weekday modes, which logic reads by wire. Nothing there argued for another rule.

## Decision

A mode reference has one spelling, chosen in this order:

1. **A built-in mode in a schedule is its day word** (`mon` … `sun`, `holiday`, `vacation`, `dayoff`,
   `heating`, `cooling`). Rules have no day words for modes (`when weekday` is a different condition),
   so this step applies to schedules only.
2. **A mode the module declares or externs is its slug**: a managed `Mode`, or an `extern … = Mode(…)`.
   This now works in schedule lines too (`party 18:00-24:00`).
3. **Any other mode is its quoted title.**

A slug that is a day word cannot name a mode in a schedule, because the day word wins there. That mode
stays a title in schedules.

Any other spelling is refused with the line as it should read, "write `party 18:00-24:00` instead of
`"Party" 18:00-24:00`". The language server offers the replacement as a quick fix.

- **`loxl check`** knows what the source shows: a managed `Mode`'s title, an extern's `title:`, an
  extern by the fixed UUID of a built-in mode. A schedule day that is a slug must name a `Mode`.
- **`loxl compile`** also knows the document. It catches the rest: a title of a mode the module externs
  by UUID, a title or slug of a built-in mode in a schedule (`"Montag"`, an extern `montag` by title).
- **`loxl fmt`** rewrites what the source shows, using the `Mode`s of every file of a module directory
  (the language server uses the files of the unit). On a single file it only sees that file's modes.
- **Decompile and adopt** write a rule's mode by the slug of the module's object for it, else by title.
  They do not lift schedules.

The compile output and the lock do not change: every spelling resolves to the same `Mode=` number and
UUID. A schedule column is still ordered by `Mode=` number.

## Rejected

- **Always the title.** It needs no module context, but it breaks when Loxone Config renames the mode
  (or a save changes the locale), and it is the one reference in the language not by slug.
- **Always a slug, with an extern for every mode used.** A rule on a mode made in the app would need an
  extern the module has no other use for.
- **Day words in rules (`when mode holiday`).** They collide with slugs in the rule namespace, and
  `when weekday` already covers the weekdays. An extern gives a built-in mode a slug.
- **Make `fmt` the only migration.** `fmt` works on files and cannot see the base. Check and compile
  give the replacement in every case, and the quick fix applies it.

## Not covered

A `CalendarEntry` names the mode it switches on by `CalType:` number, the stored attribute. Naming it by
slug or day word there is a separate change.
