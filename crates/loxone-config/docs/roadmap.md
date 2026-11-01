# Direction

Open work is tracked in [GitHub issues](https://github.com/discostu105/lox/issues). What is done is in
git and the [changelog](../../../CHANGELOG.md). This page says where the crate is heading.

## Goals

- **More of the config in source.** Each block type, attribute and app setting that Loxone Config or the
  app owns today can move into Lox Logic once its format is documented and a Loxone Config save keeps
  it. `loxl coverage` shows what is left.
- **More types the compiler can create.** A type joins the builtin table only with live evidence (see
  [connector-db.md](connector-db.md)).
- **Tests that need no Miniserver.** The simulator models the blocks Loxone documents
  ([simulator.md](simulator.md)), so more modules can carry their own `test … end` blocks.
- **Agents as authors.** The language, `loxl types`, the template library and [agents.md](agents.md) are
  written so an agent can write a module and prove it with `loxl test` and `loxl diff`.
- **Hardware in source.** Take over existing Air, Tree and extension devices with their I/Os and
  settings (#317), and create Tree devices and extensions from a verified type table (#320). Pairing
  stays in Loxone Config.
- **Editors.** `loxl lsp` and the VS Code extension; a tree-sitter grammar would reach other editors.

## Non-goals

- Transport and credentials. They stay in `lox`.
- Simulating block behavior that Loxone does not document. Such a test is refused, never guessed.
- Pairing hardware, and replacing Loxone Config for visualization.
