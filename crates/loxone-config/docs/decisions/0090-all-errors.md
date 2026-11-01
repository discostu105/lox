# D90: One check run reports every error

Status: accepted (2026-11-01)

## Context

`loxl check`, the language server and `loxl compile` stopped at the first error. A migrated or
hand-edited module often has a dozen, and each one cost a full round: fix, check, read the next.
Agents pay for every round, so they gained most from seeing all of them at once.

Reporting everything naively floods the output: one missing declaration turns every use of the
name into an error, and one unknown block type turns every wire on that block into an unknown port.

## Decision

**Errors are collected, then returned together.** `Error::Many` holds the errors of one stage;
a single error is still returned on its own, so callers that match on `Error::Parse` or
`Error::Compile` keep working. `Error::into_list` flattens it.

**Syntax errors recover at the next statement.** The parser is line based: a failed line is
reported and skipped. When the failed line opened an argument list, the lines up to its `)` are
skipped with it. When it opened a body (`rule`, `test`, `template`, a named set), the lines up to
its `end` are. A failed argument line inside an open list ends the list if it holds the `)`.

**Stages gate each other.** Syntax, then template expansion, then names (`validate`), then types and
ports (`validate_ports`). A stage with errors stops the later ones. A file that does not parse
declares nothing, so name resolution after it would only report its names as undeclared; a module
whose names do not resolve would make the port checks guess.

**Within a stage, each statement is checked on its own.** Two rules keep follow-on errors out:

- A statement whose only undeclared names were already reported by an earlier failing statement is
  skipped. The missing declaration is reported once, at its first use.
- Statements about a block of an unknown type are not checked against the port table. The type
  error covers them.

**Sorted by file and line, at most 50.** The CLI and the language server place each error the way
they placed the one error before (the backtick spans of the message, searched in the fragments). The
search also finds a quoted statement (`` `b.I2 <- sw.Q` ``) where it is written, and the second
declaration for a duplicate name. `check --json` keeps its shape: `ok`, `path` (the first error's
file) and `errors`, where each entry has its own `path`, `line` and `message`. When there are more
than 50, the first 50 are shown and `truncated` gives the count left out.

**Instance names stay respelled after the fact.** Errors raised after template expansion name
expanded slugs (`kueche_lc`), and `Module::respell_error` turns them into the source spelling
(`kueche.lc`) for each error of the list. The respelling is unambiguous: source never writes an
expanded slug (expansion refuses one), and a port follows a dot. Giving every error its source
spelling at creation would mean passing a spelling table to every message that names a block, and
the expanded slug cannot simply be the source spelling, because lock keys, synthetic block names and
the default `Title` are built from it.

The compile output does not change.

## Rejected

- **Continuing past a failed stage.** Port checks on a module with unresolved names report
  guesses, and name checks after a syntax error report names the broken line would have declared.
- **No cap.** A file pasted in the wrong language yields an error per line; fifty is enough to see
  the pattern.
- **Deduplicating by message text.** Two errors about the same missing name differ in their
  context (`in a.I1`, `in b.I2`); the skip uses the statement's references instead.
