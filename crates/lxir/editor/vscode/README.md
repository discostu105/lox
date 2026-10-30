# lxir for VS Code

VS Code extension for `.lxir` modules: syntax highlighting
(TextMate grammar), comment/bracket configuration, and snippets for every
statement form — `page`, `let`, `extern`, blocks, `template`/`end` and
instances, wires (plain and expression), `set`, attribute assignments
(`x.room = …`), `removed`/`moved` — plus
unit-suffixed values (`30min`, `2700K`) and the expression operators
(`and`/`or`/`not`, comparisons, arithmetic).

It also starts the language server (`lxir lsp`) for `.lxir` files:
diagnostics as you type and on save, go to definition, hover with the base
object an extern resolves to, port and type completion, formatting
(*Format Document*), and the outline.

Install by linking the folder into your extensions directory, and install
the language client once (without it the extension still highlights, and
says how to enable the server):

```sh
ln -s "$(pwd)/editor/vscode" ~/.vscode/extensions/lxir.lxir-0.2.0
cd editor/vscode && npm install --omit=dev
```

(then reload VS Code), or package it properly with
[`vsce`](https://github.com/microsoft/vscode-vsce):

```sh
cd editor/vscode && npm install && npx @vscode/vsce package && code --install-extension lxir-0.2.0.vsix
```

Settings:

| Setting | Default | |
|---|---|---|
| `lxir.server.enable` | `true` | start the language server |
| `lxir.server.path` | `lxir` | the program: `lxir`, or `lox` |
| `lxir.server.args` | `["lsp"]` | `["ir", "lsp"]` for `lox` |
