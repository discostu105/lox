# D81: Named signals

Status: accepted (2026-10-31), amends [D24](0024-expressions.md)

## Context

An inline expression's blocks are named after their sink (`wp_freigabe_on__and2`). A test, a second
consumer or a tile had to spell that synthetic slug, and it moved whenever the expression was edited.

## Decision

`<slug> = <expr>` names an expression's result.

- It desugars exactly like an inline expression: the same blocks, labels and errors. The sub-blocks take
  the prefix `<slug>__`, and the top block is `<slug>` itself.
- `<slug>.Q` (gate, comparator) or `<slug>.AQ` (Formula) is the signal. No wire or alias block is added,
  so the compile output is the inline expression's under other slugs.
- It is a statement of its own (`Item::Signal`), not a block form, because its inputs are its expression.
  `<-`, port-`=` and attributes on a signal are refused. A body's only signal takes no template forwards.

**Ownership.** The top slug is written by hand, so it is hand-owned like a declared block.

- Dropping the statement needs `removed`; renaming needs `moved` or `loxl rename`.
- It keeps its UUID while the expression below it changes.
- The sub-blocks stay expression-owned and are removed automatically, since their names are derived.
- `moved old -> new` on a signal also moves the expression-owned `old__*` entries to `new__*`, so a rename
  keeps every UUID.
- A changed top operator changes the top block's type, which a pinned block never does. The compile
  refuses it and names the remedy: a new slug and `removed <old>`.

**No bare operands.** `x = a.Q` would be an alias, and Loxone has no block for one. It is refused with
"an alias is not a block". `x = 5` is a `let`.

**Labels.** The label stays the expression text. A block that needs a title of its own is declared by
hand.

**Templates and the language server.** In a template body a signal is body-local like a block
(`<instance>_<slug>`, `<instance>_<slug>__<op><n>`). The language server lists signals as declarations
(kind `signal`) typed by their top block, so port completion and hover work on `<slug>.`.

## Rejected

- **The whole signal expression-owned.** A typo in the slug would silently create a new block that tests,
  tiles and rules reference by name. The lifecycle statements exist to catch exactly that.
- **A pass-through gate for `x = a.Q`.** It adds a block and a cycle of latency.
- **A leading label string** (`x = "Label" a.Q and …`). It reads poorly on one line.
