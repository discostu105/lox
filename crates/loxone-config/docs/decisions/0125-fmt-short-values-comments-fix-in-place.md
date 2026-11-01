# D125: One-line calls of short values, attached comments, `lint --fix` in place

Status: accepted (2026-11-01), amends [D86](0086-v2-polish.md) and [D87](0087-v2-tooling.md)

## Context

#303 named three formatting rough edges:

- D86 puts a call on one line only up to three arguments. A Modbus sensor or a `defaults` line of
  four or five numbers (`ModbusAddress: 1100, ModbusDataType: 0, DestValHigh: 100`) then takes six
  lines, though it reads fine as one.
- A whole-line comment got a blank line between it and the statement below whenever the statement
  was of another kind or spanned several lines, so `fmt` pulled a comment away from what it
  describes. Blank lines between two comments were dropped.
- `loxl lint --fix` wrote the whole file in canonical form, so fixing one value reformatted a file
  the user had not run `fmt` on.

## Decision

**Short values may exceed three arguments.** A call goes on one line when it fits in 100 columns,
has no comments and either has at most three arguments or binds only numbers, unit values, times
and `on`/`off` (the label aside). Names, strings, wires and expressions still count against the
three: they are what a reader has to find in a line.

**A comment stays where it was written.** The parser keeps a blank line below a whole-line comment
as an item (`Item::Gap`, many blank lines become one). `fmt` writes a comment directly above the
next statement when the source had no blank line between them, and keeps the blank line when it
had one. The blank line above a comment follows the statement below it, so a comment block moves
with its statement. `# lint: allow` comments always worked this way.

**`lint --fix` patches the value.** It replaces each `<port>: <number>` (or `= <number>`) the fix
names in place, in source order, outside comments and strings, and leaves every other byte of the
file. The patched file must parse to the fixed module; if not (a quoted number, a value the scan
cannot find), the file is written in canonical form as before. The compile check in a project is
unchanged.

## Rejected

- **A longer argument limit** (five, six): long names and wires then pack into lines that are hard to
  scan. The kind of value is what makes a line readable, not the count.
- **Keeping every blank line**: `fmt` would no longer put a blank line where the kind changes, the
  rule most files rely on. Only the blank line below a comment carries intent.
- **Source spans in the AST** for `lint --fix`: every node would carry positions only this fix uses.
  The scan plus the parse check gives the same result for the values the lint fixes.
