# D122: A named signal's `.title` labels its top block

Status: accepted (2026-11-01), amends [D81](0081-named-signals.md)

## Context

A named signal's blocks are labelled with their expression text (D24). For the top block that is
often a long line (`sonne.Q and aussentemp.Q >= schwelle and not wind.Q`) where a person would
write "Beschatten Süd" on the Loxone Config canvas. A signal could not take any attribute (D81),
so the only way to a custom label was declaring the blocks by hand (#288).

## Decision

**`<signal>.title = "…"` on its own line** sets the top block's label.

- The value is a string, number or `let` reference, the same as an extern's `.title`.
- Desugaring writes it into the top block and drops the statement. It is the label, not a claim, so
  it keeps no lock state and the lock keys do not change.
- The sub-blocks keep their expression text, which still says what each one computes.
- `.room` and `.category` on a signal stay errors; they would be claims on a block the compile owns.
- One `.title` per signal (the duplicate-claim check). `fmt` keeps the line directly below its
  signal, so the two read as one.
- A template body may give its signal a `.title`, with `{param}` interpolation like a block label.

Decompile writes a signal whose label is not its expression text as the signal plus its `.title`.

## Rejected

- **A label inside the statement** (`beschatten = "Beschatten" : sonne.Q and …`, or a trailing
  string). A new form of the statement for one optional field, and a string next to an expression
  reads like an operand.
- **Taking the label from the trailing comment.** Comments are for readers and never change the
  output elsewhere in the language.
- **A `title:` argument in a `Signal(…)` call.** A signal is not a block call; it would be a second
  spelling for the same thing.
