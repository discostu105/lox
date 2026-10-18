# lxir for VS Code

Declarative VS Code extension for `.lxir` modules: syntax highlighting
(TextMate grammar), comment/bracket configuration, and snippets for every
statement form — `page`, `let`, `extern`, blocks, `template`/`end` and
instances, wires (plain and expression), `set`, attribute assignments
(`x.room = …`), `removed`/`moved` — plus
unit-suffixed values (`30min`, `2700K`) and the expression operators
(`and`/`or`/`not`, comparisons, arithmetic).

No build step and no extension host code — install by copying or linking the
folder into your extensions directory:

```sh
ln -s "$(pwd)/editor/vscode" ~/.vscode/extensions/lxir.lxir-0.1.0
```

(then reload VS Code), or package it properly with
[`vsce`](https://github.com/microsoft/vscode-vsce):

```sh
cd editor/vscode && npx @vscode/vsce package && code --install-extension lxir-0.1.0.vsix
```

Real autocompletion (port names from the base config, slug references,
diagnostics-as-you-type) needs a language server; that is scoped in
[docs/roadmap.md](../../docs/roadmap.md) — the library's line-precise parse
errors and `observe` API were shaped so an `lxir lsp` subcommand can be a
thin layer. Until then, `lxir check` and `lxir fmt` in a save hook cover the
validation loop.
