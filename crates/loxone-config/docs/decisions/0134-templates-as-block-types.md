# D134: Templates are block types

Status: accepted (2026-11-04), language v3 part 3 (#374)

## Context

v2 told a template call from a block declaration by the callee's case (`sued = fassade(…)` against
`hoch = GreaterEqual(…)`), the one place the language made case carry meaning. What an instance
exposed was implicit: its *primary block*, the body's only block or the one `-> block` named, whose
every port `sued.Port` read and onto which a call-site binding that named no parameter was forwarded.
A value parameter was typed by its default (`pos = 70`), so a required value had no spelling, a
duration and a count looked alike, and a wrong argument surfaced after expansion as a port error of a
block the caller never wrote. Templates could not nest, and a body could not hold a `let`.

## Decision

**A template is a block type.** It is named like one (`template Fassade(…)`), an instance is declared
like a block (`sued = Fassade("Süd", …)`), and the callee decides which it is: a call of a template
the module declares, in any of its files, is an instance (`Module::classify_instances`). A template
may not take a block type's name. A v2 lowercase name parses and is refused with the type name;
`loxl fmt --write` renames the template and its calls across the module.

**The interface is explicit.**

- The header declares the **outputs**: `-> (Q: beschatten.Q, Hoch: hoch.Q)`. `sued.Q` reads one, a
  bare `sued` the first listed, and the expansion rewrites each to the body port before the scoped
  names resolve. The primary block, the "only block" rule and `-> block` are gone; `fmt --write`
  turns `-> block` and a one-block body into the block's default output.
- An **input parameter** (`sonne: input`) is the one way a call feeds a port of the body: a source, a
  list of sources or an expression, bound in the body (`In2: sonne`) or read in an expression. Not
  given, it leaves the port open, as a forward left it. A call-site binding that names no parameter
  is an error naming the rule.
- A **value parameter declares its type**: `number`, `duration`, `string`, `bool`, with or without a
  default. The check at the call judges the literal or the `let` constant; `n = 28` is an error that
  `fmt --write` rewrites. The unit-suffix fix that suffixes a default makes the parameter a duration.
- The **label string** of a call binds the parameter named `label` (`label: string`), interpolated in
  a title like any string parameter; a template without it takes no label.

**Nesting and body constants.** A body may instantiate another template (`hitze = Waechter(…)`,
expanded to `sued_hitze_alarm`, spelled `sued.hitze.alarm`, its outputs `sued.hitze.Q`) and declare
`let` constants and aliases local to the body. Body names shadow module names.

**What stays.** Lock keys are the expanded slugs (`inst_block`), so no lock migrates. `inst.block`
and `inst.block.Port` remain the spelling of a body block; a bare instance where an object stands is
an error naming the block. The library, the examples and the test fixtures were migrated; the compile
output of each is byte-identical.

## Rejected

- **Keeping the lowercase callee as the instance marker.** It made case meaningful once and left
  `check` unable to tell a template call from an unknown type until expansion.
- **Reading every port of a named main block through the instance** (`-> beschatten`, `sued.Q`,
  `sued.On`). It hides which ports are the interface and made forwards possible; declared outputs
  say what an instance is for.
- **Untyped value parameters with a required marker.** A type names what the caller passes and lets
  the check catch a count where a duration is due.
- **Call-site port bindings next to inputs.** Two spellings for one fact.
