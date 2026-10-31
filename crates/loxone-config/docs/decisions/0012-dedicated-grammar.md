# D12: A dedicated grammar

Status: accepted (2026-08-24)

## Context

Wiring is the hard part of a Loxone config. The language also needs room for expressions and templates.

## Decision

Lox Logic has its own grammar, small enough to parse by hand ([language.md](../language.md)).

## Rejected

- A YAML, KDL or HCL host syntax. YAML would have been the fastest start, but wiring written in YAML has
  the ergonomics this project exists to remove. Expressions and templates want first-class syntax.
