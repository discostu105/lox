# D26: Expressions bind in argument lists too

Status: accepted (2026-08-25)

## Context

[D24](0024-expressions.md) allowed an expression only on the right of `<-`. So `ext.Port <- a.Q and b.Q`
worked, and `x = Monoflop(InputTrigger: a.Q and b.Q)` did not. The objection of
[D16](0016-constructor-syntax.md) to anonymous blocks is answered by D24's synthetic slugs.

## Decision

The value of an argument binding may be an expression. It desugars through the D24 machinery, and the
binding becomes a plain wire from the expression root's `Q`. Slugs follow `<sink>_<port>__<op><n>` with the
managed block's port as sink.

- A bare `slug.Port`, with or without parentheses, stays a wire binding. A bare value stays a parameter
  binding.
- The synthetic-slug counters run across the module's expressions per sink and operator. Two expressions
  into the same port continue the numbering instead of colliding.
