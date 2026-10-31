# D59: The language server is a lexical layer over the pipeline

Status: accepted (2026-10-30)

## Context

`loxl lsp` (`src/lsp/`) needs positions for diagnostics, hover, references and rename. The AST carries no
spans. Adding spans would touch every node and the `parse(to_text(m)) == m` fixpoint.

## Decision

The server speaks JSON-RPC over stdio by hand, with no LSP crate. The protocol surface is fifteen methods, and
the crate keeps its four dependencies.

- **Positions are found lexically.** The language is line-oriented, and a declaration always opens its line,
  so `analysis::scan` finds declarations by scanning. A parse error has its line. A semantic error names its
  subject in backticks. The diagnostic goes to the first match of:
  1. a `slug.port` use of two named names;
  2. a named key inside a named declaration;
  3. the declaration;
  4. the first use.
- **What runs when.**
  - On each change: what `loxl check` runs, plus the source lints (10 ms on the house module).
  - On open and save, in a `lox.toml` project: also the compile against the base, in memory, on a copy of the
    lock, plus the lints that need it. This is exactly what `loxl lint` reports (0.8 s for the house: 21
    files, the same 20 findings).
  - Nothing is written.
- **Identity on hover.** `resolve_extern_uuids` resolves the externs the way compile pins them. The last
  result that resolved is kept, so hover still shows the base object while the text does not parse.
- **References and rename are lexical too.** A reference is the name outside strings and comments. It does not
  count after a `.` (a port), before a `:` (an argument key), or inside a template body that declares the
  name itself (a parameter or a block). Only module-level names are renamed.
- **Rename in a project is `loxl rename`** (`cli::plan_rename`, on the editor's buffers). The lock must be
  current, the output may change only in titles, and the lockfile is re-keyed. The server returns one
  workspace edit for the fragments and the lockfile, and writes nothing; the next compile writes the output.
  A name the buffers already hold, or a plan that fails, is a JSON-RPC error, so nothing is half applied.
- **Quick fixes come from the diagnostics.** A "did you mean `X`?" on a single name replaces that name with
  `X`.

## Rejected

- Spans in the AST: they would touch every node and the parse/print fixpoint.
- An LSP crate: the surface is small, and it would add dependencies.

Validated: protocol tests (`tests/lsp.rs`). Diagnostics, definition, hover, completion, formatting and rename
also in Neovim 0.12; the first four in VS Code (headless, through the bundled client).
