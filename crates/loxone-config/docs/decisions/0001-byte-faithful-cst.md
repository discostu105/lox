# D1: A byte-faithful syntax tree, not a DOM

Status: accepted (2026-08-24)

## Context

A conforming XML parser rejects or corrupts real `.Loxone` files. They have attribute names that start with a
digit and raw newlines in attribute values (see [loxone-format.md](../loxone-format.md#not-actually-xml)).

## Decision

The crate parses `.Loxone` with its own parser into a lossless concrete syntax tree, and writes it with a
canonical writer. A parse and write is byte-identical on six real configs. Everything else in the crate builds
on this.

## Rejected

- A DOM-level writer, as in lox-cli. It cannot prove that it changed nothing else.
