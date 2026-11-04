- **Lox Logic v3, part 2: a leftover `removed` or `moved` is a lint, not an error.** Once the removal
  has reached the deployed config or the rename is applied, the statement compiles to nothing and
  `loxl lint` reports it as `stale-lifecycle` with the line to delete. A misspelt slug is the same
  finding.
- **Lox Logic: `step` counts program cycles, `tick` takes a duration.** `step 3` (was `tick 3`),
  `tick 5min`. A bare number after `tick` is an error naming `step`; `loxl fmt --write` rewrites a v2
  module.
- **Lox Logic: a calendar entry's day every year is `every: 12-31`** (was `on: 12-31`). `on:` is an
  error naming `every:`; `loxl fmt --write` rewrites it.
- **Lox Logic: `°` is a unit suffix** (`Dir: 180°`, `TComfort: 21°`), accepted on a port documented in
  degrees like `%` and `K` on theirs.
