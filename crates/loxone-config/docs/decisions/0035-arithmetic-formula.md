# D35: Arithmetic compiles to a Formula block

Status: accepted (2026-08-25)

## Context

Boolean operators in expressions map to one gate per operator ([D24](0024-expressions.md)). There are no discrete
arithmetic gates worth cascading. Loxone's `Formula` block computes up to four inputs into `AQ`.

## Decision

`+ - * /` join the expression grammar. They bind tighter than comparisons, `* /` tighter than `+ -`, and
parentheses group.

- **One block per tree.** A maximal arithmetic tree becomes one `Formula` block. Its result is `AQ`, its slug
  operator `f`. Identity, labels and lock ownership follow [D24](0024-expressions.md).
- **Inputs.** Distinct port operands bind `Input1`…`Input4` in order of first appearance. A repeated port reuses
  its input: `(a.AQ + b.AQ) * a.AQ` becomes `(I1+I2)*I1` with two inputs.
- **Constants.** Numbers and numeric `let` references are inlined into the compact formula text. Negative
  constants are parenthesized: `I1*(-2)`.
- **Four inputs at most**, the block's own limit. More is an error that suggests splitting.
- **Standalone only.** Arithmetic may be a whole `<-` right-hand side or a whole argument binding. Under a gate
  or a comparison (`a.Q and x.AQ + 1`, `x.AQ + 1 >= 5`) it is a parse error that points to an explicit `Formula`.
  A `Formula`'s analog `AQ` feeding a boolean input is not verified against the Miniserver. Comparison operands
  stay plain.
- **Lexing.** `-` after a token that can end an operand (identifier, number, `)`) is binary minus. Anywhere else
  it starts a negative number, so `SSoff: -30` and `x < -5` keep their meaning.

Both compositions under gates and comparisons are deferred, not rejected. The desugarer already composes them;
allowing them means removing two parse checks after a simulator or live check.

The `Formula=` text format is in
[loxone-format.md](../loxone-format.md#reported-by-lox-cli-checked-against-the-corpus).

## Rejected

- A per-expression backend switch (a `formula(...)` wrapper or a pragma choosing Formula or discrete blocks for
  boolean logic too). Arithmetic has one sensible backend and boolean logic has one. A switch would be two
  spellings per rule with nothing to choose between them.
- Folding the house's ten hand-written PV `Formula` blocks into expressions. Their titles and lock identities are
  established. A migration would create new identities for no byte-level gain.
- The full Formula grammar (`IF`, functions, comparisons inside the formula string) in the expression language.
  An explicit `Formula` block takes the string verbatim.
