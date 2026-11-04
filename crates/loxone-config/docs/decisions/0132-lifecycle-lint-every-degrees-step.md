# D132: A stale lifecycle statement is a lint, `every:`, `°`, and `step` next to `tick`

Status: accepted (2026-11-03), language v3 part 2

## Context

Four small rough edges of v2, each found by writing modules:

- A `removed` or `moved` statement outlived its job as a hard compile error: once the removal reached
  the deployed config, or the rename had its lock entry, the next compile refused the whole module until
  the line was deleted. A compile that works today failed tomorrow with no change to the source. A
  reviewer of a config repo saw a red build over a line that did nothing.
- A calendar entry's day every year was `on: 12-31`. Read aloud it says the opposite of a period
  (`from:`/`to:`) badly, and `on` is also the boolean, so `on: on` was legal.
- `Dir: 180` on a blind, `TComfort: 21` on a room controller: the angle and temperature ports had no
  unit suffix while every time port had one. `loxl types` documents them in `°`.
- `tick 3` meant three program cycles and `tick 3s` three seconds. One word, two quantities, and
  `tick 3` read as a time to everyone who had not read the spec.

## Decision

- **A stale `removed` or `moved` compiles and does nothing; `loxl lint` reports it** as
  `stale-lifecycle`. The compile still refuses what is wrong today: a slug both declared and `removed`,
  a `moved` whose old slug is still declared, chained moves. What is merely finished is a lint
  finding naming the line to delete, with the lockfile as the judge: a `removed` is stale when the
  lockfile has no managed block, pinned rule, created device or pending removal of that slug; a
  `moved` when the old slug has no lock entry (the finding says whether the new one has it). The
  typo guard moves with it: a misspelt slug is the same finding. Outside a project the kind is not
  judged, as with the other kinds that need the lockfile.
- **`every:` names a calendar entry's day every year.** `on:` is an error naming `every:`; `loxl fmt
  --write` rewrites it with the other calendar spellings.
- **`°` is a unit suffix** with factor 1, accepted on a port documented in `°` or an undocumented one,
  like `%` and `K`. Decompile and adopt keep writing those ports bare.
- **`tick` takes a duration, `step` a count of program cycles.** `tick 5min`, `step 3`, each with an
  optional `dt`. A bare number after `tick` is an error naming `step`, a duration after `step` one
  naming `tick`; a `let` constant must hold the matching kind. `loxl fmt --write` rewrites a v2
  `tick <n>` to `step <n>` on the text, before the parse refuses it. `step` is a reserved word.

## Rejected

- *Keep the stale statement an error but only after a grace period.* Any rule by time or by number of
  compiles makes the same source pass and fail on different days. The lint says the same thing every
  time and never blocks a deploy.
- *Have the compile delete the stale line.* The compile does not write source. `fmt` could, but a
  rewrite that depends on the lockfile would make `fmt` differ by project state.
- *Keep `tick <n>` and add `tick <duration>` only.* Both already existed; that was the problem. One
  keyword per quantity, and the error names the other.
- *Write `on:` as `yearly:` or `annually:`.* `every: 12-31` reads as a sentence next to `from:` and
  `to:`.
