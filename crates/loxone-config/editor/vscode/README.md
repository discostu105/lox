# Lox Logic for VS Code

VS Code extension for `.loxl` modules: syntax highlighting
(TextMate grammar), comment/bracket configuration, and snippets for every
statement form — `page`, `let`, `extern`, blocks, `template`/`end` and
instances, wires (plain and expression), named signals, `defaults`, `set`,
attribute assignments (`x.room = …`), the named sets (`tiles`, `schedule`,
`moods`, `access`, `recipients`) with their claims, `rule`, `test`,
`removed`/`moved` — plus
unit-suffixed values (`30min`, `2700K`) and the expression operators
(`and`/`or`/`not`, comparisons, arithmetic).

It also starts the language server (`loxl lsp`) for `.loxl` files:
diagnostics as you type and on save, go to definition (template instance
blocks too: `inst.block`), hover with the base object an extern resolves to,
completion of ports, types, argument keys and signal names, formatting
(*Format Document*), the outline, *Find All References*, *Rename Symbol*
(fragments and lockfile together) and quick fixes for "did you mean", old
spellings, bare durations and a file without its `page` line.

Install by linking the folder into your extensions directory, and install
the language client once (without it the extension still highlights, and
says how to enable the server):

```sh
ln -s "$(pwd)/editor/vscode" ~/.vscode/extensions/loxl.loxl-0.3.0
cd editor/vscode && npm install --omit=dev
```

(then reload VS Code), or package it properly with
[`vsce`](https://github.com/microsoft/vscode-vsce):

```sh
cd editor/vscode && npm install && npx @vscode/vsce package && code --install-extension loxl-0.3.0.vsix
```

Settings:

Settings `loxl.server.path` and `loxl.server.args` are read from the user settings only: a workspace cannot choose the program.

| Setting | Default | |
|---|---|---|
| `loxl.server.enable` | `true` | start the language server |
| `loxl.server.path` | `loxl` | the program: `loxl`, or `lox` |
| `loxl.server.args` | `["lsp"]` | `["logic", "lsp"]` for `lox` |
