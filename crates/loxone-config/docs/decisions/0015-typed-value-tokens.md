# D15: Values are typed tokens

Status: accepted (2026-08-25), amended by [D80](0080-v2-one-spelling.md)

## Context

A formatter that guesses a value's kind from its content prints a string like `"5+"` as a bare token. That
token does not parse again, which breaks the `parse(to_text(m)) == m` fixpoint.

## Decision

- The AST records how a value was written (number, string, reference, and since
  [D80](0080-v2-one-spelling.md) `on`/`off`). The formatter writes each kind as recorded.
- One canonicalization remains, at parse time: a quoted string that reads exactly as a number becomes the
  bare number.
- A bare identifier in value position is a `let` reference and must resolve.

## Rejected

- Sniffing the kind from the content when formatting.
- Reading an unknown bare identifier as a string.
