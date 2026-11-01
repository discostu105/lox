- **loxl lint** groups its findings by kind with counts (`--summary`: the counts only) and reports
  bare time values that read better with their unit (`unit-suffix`: `Delay: 10800` → `Delay: 3h`).
  **`loxl lint --fix`** writes the units; in a project it refuses if the compiled config would change.
- **loxl lsp** hovers and goes to the definition of a template instance's blocks (`inst.block`,
  `inst.block.Port`), completes argument keys (in `defaults` too) and signal names, and offers quick
  fixes for `unit-suffix` and for a file without its `page` line.
- **loxl check** names the file and line of an error in a directory module (`check --json`: `path`
  and `line`).
- **Lox Logic for VS Code** has snippets for every named set (`tiles`, `schedule`, `moods`, `access`,
  `recipients`), rules and tests, and highlights the rule words (`when daylight`, `then wait`, `or`).
