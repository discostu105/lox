# D120: Compile refuses a title a save rewrites

Status: accepted (2026-11-01), extends [D114](0114-refuse-what-a-save-rewrites.md)

## Context

Oracle session 63 found that a save turned `<` in two `VirtualIn` titles into `_` and kept `>`. Lox
Logic wrote the title as given, so the next save showed the object as renamed (#342).

Sessions 80 to 82 saved titles with every printable ASCII character, a few non-ASCII ones, tabs, line
breaks and spaces at the ends, on every type the compiler creates
([loxone-format.md](../loxone-format.md#titles-a-save-rewrites)). The save drops `{`, `}`, tabs and
line breaks, turns spaces at either end into `_`, and turns a set of characters into `_` that depends on
the type: `%`, `<` and `|` on most objects, other sets on `LightController2`, `Mode` and
`CalendarEntry`. A page and a Modbus server keep any title.

## Decision

- `loxl check` and `loxl compile` refuse such a title on a managed block, a hardware declaration and a
  `.title` claim, and name the title the save writes: ``block `d`: title "R Ana Max<Min": a Loxone
  Config save writes it as "R Ana Max_Min". Write that instead.`` (`connectors::title_save_rewrite`).
- An empty title is refused too: the save names the object `noname <UUID>`.
- An expression's blocks are labeled with the sub-expression, which the user does not write as a
  title. Their labels spell what a save would rewrite with characters it keeps: `<=` as `≤`, `<` as
  `＜`, `%` as `％` (`p1.Q ≤ 5 and p2.Q ＜ 70％`, oracle session 83). Before, every save turned
  `p1.Q < p2.Q` into `p1.Q _ p2.Q` and the next compile turned it back.
- Only the types a save was checked on are refused. A type no oracle run has saved (Tree and Air
  devices and their I/Os, other extern types) keeps its title as written.

## Rejected

- **Compile to the title the save writes**: the source would say a name the app does not show, and a
  reference by title (`title:` matchers, `when mode "…"`) would then miss.
- **Write an expression label as the save does** (`p1.Q _ p2.Q`): stable, but the label no longer
  says what the block compares.
- **Mirror the comparison in the label** (`p2.Q > p1.Q` for `p1.Q < p2.Q`): plain ASCII, but the label
  reads the other way round than the source and the wiring, and `%` still needs a spelling.
- **One rule for every type**: a `Mode` keeps `<` but loses `:`, a page keeps everything. A single set
  would refuse titles the save keeps.
