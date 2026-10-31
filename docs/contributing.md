---
title: Contributing
layout: default
nav_order: 6
---

# Contributing
{: .no_toc }

## Table of contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Development setup

```bash
git clone https://github.com/discostu105/lox
cd lox
cargo build
cargo test
```

No live Miniserver is needed for development. All unit tests use pure functions or mocked data.

The repository is a Cargo workspace: the `lox` CLI at the root and the network-free
[`loxl`](https://github.com/discostu105/lox/tree/main/crates/loxone-config) crate in `crates/loxone-config`. Read
`crates/loxone-config/AGENTS.md` before changing loxone-config: format facts are documented, never guessed, and real configs are never
committed.

## Build commands

```bash
cargo build              # debug build
cargo build --release    # production binary (~4MB)
cargo test               # run all tests (lox and both library crates)
cargo test -p loxone-config       # only the loxone-config crate
cargo run -- ls          # run lox from source
cargo run -- tui --demo  # the TUI against a demo house
cargo clippy -- -D warnings  # lint (warnings are errors)
cargo fmt --check        # check formatting
```

## Pre-push checklist

All four checks must pass before pushing (mirrors CI):

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo build --release
cargo test
```

## External references

The official Loxone API documents and the community sources are listed in
[references.json](references.json) (links only; the PDFs are Loxone's). Download them
for local reading with `tools/fetch-refs.py` (into `refs/`, gitignored); `--check`
reports any document Loxone has changed since it was pinned.

Real configs for testing: `crates/loxone-config/tools/fetch-corpus.py` downloads the
public corpus (~150 configs) as pinned in its manifest; the redistributable ones are
in the [example library](https://github.com/discostu105/lox/tree/main/crates/loxone-config/examples/library).

## Manual testing

To test against a real Miniserver:

1. Loxone Miniserver (Gen 1 or Gen 2) on your local network
2. Configure credentials: `lox setup set --host ... --user ... --pass ...`
3. Verify: `lox status`

## Pull request process

- One feature or fix per PR
- Tests expected for new logic
- Run `cargo fmt` and `cargo clippy` before submitting
- Add a changelog entry as a new file in `changelog.d/` (see `changelog.d/README.md`); don't edit `CHANGELOG.md`

## Project status

This is an experimental project. Contributions welcome — especially for:

- Testing with different Miniserver configurations
- New control type support
- Bug reports and fixes
- Documentation improvements
