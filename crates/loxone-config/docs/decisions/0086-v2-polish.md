# D86: Contextual words, one-line calls and the v2 polish

Status: accepted (2026-11-01)

## Context

After the v2 language changes ([D80](0080-v2-one-spelling.md) to [D85](0085-defaults.md)), a set of
small inconsistencies was left:

- Which words are reserved had grown case by case. `tiles`, `schedule` and `moods` were reserved,
  `access`, `recipients`, `rule` and `defaults` were contextual.
- `then wait …` in a rule meant the wait action or a block named `wait`, depending on the line.
  `mode`, `notify`, `mail` and `message` had the same problem.
- `fmt` wrote every call with an argument one argument per line. `b = And(I1: x.Q, I2: y.Q)` took
  four lines, and the r50 house config ended up two thirds argument lines.
- `tick` took a number or a duration but no `let` constant, though `dt` did.
- `when time` had its own time parser, separate from `clock` and `At:`.
- The bulk statement `(a, b).X = v` and `mirrors:` took bare slugs only, so a block of a template
  instance had to be named by its lock key, which [D82](0082-template-instance-names.md) refuses
  everywhere else.
- Errors found after template expansion named blocks by their lock key (`kueche_x`), a name the
  source does not contain.
- A named signal `a_i1 = …` and an expression on `a.I1` both generate blocks `a_i1__<op><n>`; the
  second silently reused the first's counter.

## Decision

**One rule for keywords.** A word is either reserved (never a name: the statement keywords, `on`,
`off`, the expression operators, v0's keywords) or contextual (a keyword only in its place). The
section kinds `tiles`, `schedule`, `moods`, `access`, `recipients`, `rule` and `defaults` are
contextual: a keyword at the start of a line in front of a name or type, and after the dot of a
claim. Everywhere else they are names (`moods = Not()`, `moods.Q`). A declaration named by a
reserved word gets the reserved-word error, not the error of the statement the word would start.
Decompile and adopt never pick a contextual word as a slug, so their output reads the same in any
place.

**Rule words win their place.** After `then`, `wait`, `mode`, `notify`, `mail` and `message` are
always the action. A block of such a name as an action target is an error that names
`loxl rename`. A condition on a block state carries a dot (`when mode.Q == 1`), so it is tried
first and takes a block of any name.

**Short calls on one line.** `fmt` writes a call on one line when it has at most three arguments
(the label counts), no comments, and the line fits in 100 columns with its indentation (a tab counts
four). Any other call keeps one argument per line. Three arguments covers the common two-input gate
with a label; a fourth makes the one-line form hard to scan. The width check counts the indentation,
so the same call can be one line at the top and multi-line in a template body. A call with a comment
stays multi-line because the comment belongs to one argument line.

**Blank lines around multi-line items.** A blank line goes where the item kind changes and before
and after every item that spans several lines. Single-line items of one kind stay adjacent. The
earlier rule (blank after a multi-line block only) left a one-line call glued to the multi-line
call before it.

**`tick` and `dt` take constants.** `tick warten` reads the count or the duration from `let warten`.
The parser checks literal values; the simulator and `check` resolve constants and give the same
errors for a constant that holds neither a whole count nor a time, or a duration that `dt` does not
divide.

**One time-of-day lexer.** `when time` uses the lexer of `clock` and `At:`. `7:30` reads as `07:30`.
Seconds other than `:00` are an error, since the app stores minutes. A quoted time is an error naming
the bare one, like `clock`.

**Scoped names everywhere.** `(inst.block, x).room = …`, `extern r = InputRef(mirrors: inst.block)`
and a minted ref's `mirrors: inst.block` take the instance spelling, and expansion resolves them as
it does wires. The lock-key spelling is refused there with the error D82 gives elsewhere.

**Errors spell names like the source.** Compile, check, test and LSP diagnostics found after
expansion are rewritten by one helper: a lock key inside a backtick span becomes the scoped name
(`kueche_x` → `kueche.x`, a primary block `kueche_lc` → `kueche`). Errors from expansion itself are
left alone, since they quote the source as written.

**The prefix collision is an error.** Before desugaring, every expression sink and named signal
computes its generated prefix. Two different statements with one prefix are an error naming both
and `loxl rename`.

The compile output of every file is byte-identical; only `fmt` changes the text.

## Rejected

- **Reserving every section kind.** `moods` and `rule` are good block names in a house config, and
  reserving them would break existing modules.
- **Keeping `then <block> <cmd>` for a block named `wait` by a lookahead.** `then wait 5s` and
  `then wait on` would differ in a token's shape, and an agent could not tell which one it wrote.
- **A width-only rule for one-line calls.** A long list of short arguments fits in 100 columns but
  reads badly on one line; the argument limit keeps those multi-line.
- **Respelling expansion errors too.** They already quote the source spelling; rewriting them
  would replace the spelling the user must change.
