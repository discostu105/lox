# D16: A block's inputs live at its declaration

Status: accepted (2026-08-25, language v1), amended by [D24](0024-expressions.md) and
[D26](0026-expressions-in-arguments.md)

## Context

A parameter and a wire onto a block's port both target the same `<Co>` in the XML. Spread over a type
statement, a parameter body and separate wire lines, one block's behavior is hard to read.

## Decision

A block is one call-shaped declaration:

```
slug = Type("Label", Port: value, Port: source.Q, …)
```

- The argument's value decides its meaning. A literal or constant is a `Def=` parameter. A port reference
  is a wire.
- Externs are not constructed, so wires onto extern ports keep a statement: `target.Port <- source.Port`.
  A value on an extern port is `target.Port = value` ([D14](0014-set-for-extern-ports.md)).
- Every fact has one spelling. A managed sink binds only in its argument list; `<-` or `=` on a managed
  slug is an error that points there.
- The formatter writes one argument per line, so a diff stays one line per fact.
- Names stay mandatory, so the lockfile, `moved`/`removed` and the compile strategy are unchanged.

The v1 example compiles byte-identically to its v0 counterpart.

## Rejected

- An expression language with anonymous intermediate blocks (`node x = a >= 28 and sonne`). Path-based
  identity of anonymous blocks breaks the lockfile's rename stability, as Terraform's `count` showed.
  [D24](0024-expressions.md) adds expressions with deterministic synthetic slugs, which answers this.
- A general-purpose host language such as Starlark. It belongs above the source as a generator. The flat
  declarative form stays the total representation that every hand-drawn config decompiles into.
