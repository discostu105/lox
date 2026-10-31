# Agent notes for the loxone-config crate

This crate reads and writes Loxone Miniserver configs (`.Loxone` XML, backup ZIPs) and compiles Lox
Logic (`.loxl`) against a base config, with a lockfile that pins every UUID. The CLI is `loxl`, also
run as `lox logic`.

- To use the toolchain, read [docs/agents.md](docs/agents.md): the loop, hard rules, error remedies.
- To change the crate, read [docs/implementation.md](docs/implementation.md): module map, tests, how
  to add a block type.
- Write help, errors, comments and docs by [WRITING_GUIDELINES.md](../../WRITING_GUIDELINES.md).

## Build and test

```sh
cargo test -p loxone-config                    # unit, pipeline, roundtrip
cargo clippy -p loxone-config --all-targets    # must be warning-free
cargo fmt --check
cargo run --bin loxl -- help                   # the CLI
tools/fetch-corpus.py && LOXL_CORPUS=corpus/web cargo test --release -p loxone-config --test corpus --test roundtrip   # opt-in: the public corpus
```

## Rules

- **Never edit `.Loxone` files by hand.** Only `loxl compile` writes them. Edit `*.lock.json` only
  through the documented operations.
- **Never commit real Miniserver configs.** They hold personal data. Synthetic fixtures live in
  `examples/configs/`. Real ones go in an untracked directory named by `LOXL_CORPUS`
  (`tools/fetch-corpus.py` fetches the public corpus pinned in `tools/corpus-manifest.json`). The only
  real configs in the repo are the redacted, redistributable ones in `examples/library/`, each with its
  `SOURCE.md` and `LICENSE`.
- **Format facts are documented, never guessed.** Check [docs/loxone-format.md](docs/loxone-format.md)
  before assuming anything about the XML.
- **The builtin connector table** (`src/connectors.rs`) takes only live-verified entries. The
  workflow is in [docs/implementation.md](docs/implementation.md), the admission rules in
  [docs/connector-db.md](docs/connector-db.md).
- **No decision IDs** (`D42`) in help, errors, `///` docs or the changelog. Name the rule instead.
- **Network-free.** Transport and credentials belong to lox (`src/` at the repo root). Backup ZIPs and
  the LoxCC codec are file formats and live here (`src/archive.rs`, feature `archive`).
- When compiler output changes, regenerate `examples/out/` with `cargo run --example compile`, and the
  project artifacts with `loxl compile --time 1767225600` in `examples/project/`
  ([examples/README.md](examples/README.md)).
- Apache-2.0, published on crates.io as `loxone-config`; lox uses it by path.

## Docs

| File | Purpose |
|---|---|
| [README.md](README.md) | overview and quickstart (the crates.io page) |
| [docs/agents.md](docs/agents.md) | how agents use the toolchain; the one full guide |
| [skills/lox-logic/SKILL.md](skills/lox-logic/SKILL.md) | the hard rules as a Claude Code skill, linking to agents.md |
| [docs/language.md](docs/language.md) | the `.loxl` language spec |
| [docs/lockfile-spec.md](docs/lockfile-spec.md) | the lockfile format |
| [docs/loxone-format.md](docs/loxone-format.md) | verified `.Loxone` format facts |
| [docs/design.md](docs/design.md) | architecture |
| [docs/decisions/](docs/decisions/) | design decisions |
| [docs/implementation.md](docs/implementation.md) | module map, tests, gotchas |
| [docs/connector-db.md](docs/connector-db.md) | connector table: evidence and admission rules |
| [docs/simulator.md](docs/simulator.md) | what `loxl test` simulates |
| [docs/oracle-wine.md](docs/oracle-wine.md) | Loxone Config under Wine, to check what a save keeps |
| [docs/vision.md](docs/vision.md) | why this exists |
| [docs/roadmap.md](docs/roadmap.md) | direction; open work is in GitHub issues |
