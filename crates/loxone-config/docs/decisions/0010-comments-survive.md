# D10: All comments survive the round trip

Status: accepted (2026-08-24)

## Context

`loxl fmt` must not destroy anything the author wrote. A comment moved away from its anchor loses its intent.

## Decision

Every comment survives parse and format:

- A whole-line comment is an item of the module.
- A trailing comment stays attached to its statement, its argument line or the closing `)`.
- A comment inside an argument list is an argument item.

## Rejected

- Moving a comment after the closing delimiter onto its own line. It detached the comment from its anchor
  for no gain.
