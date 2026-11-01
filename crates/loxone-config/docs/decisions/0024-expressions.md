# D24: Expressions are sugar over discrete blocks, owned by their statement

Status: accepted (2026-08-25), amended by [D26](0026-expressions-in-arguments.md),
[D35](0035-arithmetic-formula.md), [D81](0081-named-signals.md) and [D123](0123-decompile-lifts-expressions.md)

## Context

A rule like "shade when sunny and warmer than 28 degrees" takes three blocks and four wires to write out.
The blocks should stay readable on the Loxone Config canvas.

## Decision

An expression may stand on the right of a wire statement:

```
jal_sued.AutoShade <- sonne.Q and aussentemp.Q >= schwelle
```

- `<-` stays the one wiring operator; `=` on a port stays the `Def=` write. A bare `slug.Port` is a plain
  wire. Anything more desugars before compile, like template expansion.
- It desugars into the live-verified discrete blocks:
  - `and`/`or` become `And`/`Or`, fixed at two inputs ([D8](0008-fixed-gate-inputs.md)). Chains cascade
    left-associatively.
  - `not` becomes `Not`.
  - Comparisons become the comparator family. Operand order is kept: left to `Input1`, right to `Input2`.
  - Constants become `Def=` parameters, ports become wires.
- Each generated block's label is its sub-expression text, so the rule reads on the canvas.
- Precedence: `or` < `and` < `not` < comparison. Parentheses group. Comparisons take plain operands and do
  not chain. `and`, `or` and `not` are reserved words.
- **Identity.** Synthetic slugs are `<sink>_<port>__<op><n>`, numbered in post-order with a counter per
  operator. The lockfile keys them like hand-written blocks, marked `expr_owned`. An unchanged expression
  never mints again. An edited one derives new slugs, and the compiler removes the orphaned `expr_owned`
  entries without a `removed` statement: the expression is their only source. A hand-written slug that
  collides with a synthetic one is an error naming the expression.
- Desugaring runs after template expansion, so a body expression's sink prefix uses the instance's extern.
- **Minting.** The minter's counter restarts at 0 in each run. With an identical mint time, a block added in
  a later session could reuse a (time, sequence) pair and collide. Compiles therefore seed the minter past the
  lock's high-water mark (`counters.next_mint`) and past every locked UUID's sequence.

Deferred: parenthesized comparison operands.

## Rejected

- Content-hash slugs. They are stable across edits but unreadable, and they show on the canvas. With
  positional slugs an edit mints the edited expression's blocks again; the blast radius stays inside that
  expression.
- One `Formula` block per boolean expression. It is compact but opaque on the canvas and capped at four
  inputs. Arithmetic uses it ([D35](0035-arithmetic-formula.md)).
