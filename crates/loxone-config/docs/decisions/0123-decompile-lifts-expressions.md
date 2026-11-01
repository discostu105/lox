# D123: Decompile and adopt lift expressions back

Status: accepted (2026-11-01), amends [D24](0024-expressions.md) and [D81](0081-named-signals.md)

## Context

An expression compiles to gate blocks and `Formula` blocks labelled with their (sub-)expression text.
Decompile and adopt wrote those blocks back as declared blocks with synthetic-looking slugs
(`jal_sued_autoshade__ge1` became `aussentemp_q_schwelle = GreaterEqual(…)`), so a compiled and
adopted module read worse than the source it came from, and the lock lost the expression-owned
marks (#303).

## Decision

**Decompile lifts a chain back into an expression when it compiles to the same blocks.**

- A candidate is a gate, comparator or `Formula` whose label parses as an expression (or an `And`,
  `Or` or `Not` with a label of its own, read from its wiring). Its sub-blocks are the blocks that
  feed it through one wire each, are used nowhere else, sit on its page and carry their own
  sub-expression as label.
- The candidate is desugared and compared with the config: block types, labels, every binding and
  wire. Only an exact match is lifted. A gate drawn in Loxone Config rarely matches, and then stays a
  declared block.
- **Where it goes.** A chain used once, by a non-gate block or an extern's `<-` on its own page, goes
  back into that argument or wire. Any other chain becomes a named signal under its top block's slug,
  with its label as `.title` (D122) when that is not the expression text.
- **Names.** The labels name the operands. A first pass collects them, and the slugs are assigned
  with those names first where all labels agree, so `sonne.Q` in a label finds the extern `sonne`.
  A constant a label names (`schwelle`) comes back as `let` if the name is free and means one value.
- **Backstop.** The lifted view must check (validate, desugar, ports) as well as the unlifted one. A
  view of a whole config may fail already, for a device it does not lift; then the lifted one must
  fail the same way. Otherwise nothing is lifted.
- Adopt marks the absorbed blocks expression-owned in the lock. `adopt --uuid` (one block) never
  lifts, so it declares the block it was asked for.

Expressions compiled by lox now come back as written. The adoption of each shipped example and of
the r50 house config compiles back unchanged.

## Rejected

- **Lifting by structure alone.** Any `And` would turn into `a.Q and b.Q`, and the next compile
  would relabel the block, a change in Loxone Config the user did not make.
- **Lifting what compiles almost the same** (other defaults, another label). The source would claim
  something the config does not hold, and the next push would change it.
- **Recovering the signal's name.** A signal with the default label leaves only the expression text
  in the config, so its slug comes from that text (`aussentemp_q_30`). Rename it after adopting.
