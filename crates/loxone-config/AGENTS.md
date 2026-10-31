# Agent notes for the loxone-config crate

`loxl` is a Rust library + CLI that treats Loxone Miniserver configs
(`.Loxone` XML) as source code: a text language, Lox Logic (`.loxl` files) compiles against a
base config with a lockfile pinning all UUIDs. Start with
[docs/agents.md](docs/agents.md) — it is the operational guide for using the
toolchain (the loop, hard rules, error remedies).

## Build & test

```sh
cargo test -p loxone-config               # unit, pipeline, roundtrip
cargo clippy -p loxone-config --all-targets   # must be warning-free
cargo fmt --check
tools/fetch-corpus.py && LOXL_CORPUS=corpus/web cargo test --release -p loxone-config --test corpus --test roundtrip   # opt-in: the public corpus
cargo run --bin loxl -- help     # the CLI (also: lox logic help)
```

## Rules that matter here

- **Never edit `.Loxone` files by hand** — only through `loxl compile`.
  Never hand-edit `*.lock.json` beyond the documented operations.
- **Never commit real Miniserver configs** (personal data). Synthetic
  fixtures live in `examples/configs/`; real ones go in an untracked corpus
  dir referenced via `LOXL_CORPUS` (the public one: `tools/fetch-corpus.py`,
  pinned by `tools/corpus-manifest.json`). The only real configs in the
  repository are the redistributable, redacted ones of `examples/library/`,
  each with its `SOURCE.md` and `LICENSE`.
- The builtin connector table (`src/connectors.rs`) only accepts
  **live-verified** entries — the growth workflow is in
  [docs/implementation.md](docs/implementation.md).
- Format knowledge is documented, not guessed: check
  [docs/loxone-format.md](docs/loxone-format.md) before assuming anything
  about the XML.
- Keep `cargo test`, clippy, and fmt clean; when compiler output changes,
  regenerate `examples/out/` via `cargo run --example compile` and the
  project artifacts via `loxl compile --time 1767225600` inside
  `examples/project/` ([examples/README.md](examples/README.md)).
- Apache-2.0 (see [LICENSE](LICENSE)), same as lox. Published on crates.io as `loxl`; lox itself uses
  it by path.
- This crate lives in the lox workspace (`crates/loxone-config`). It stays
  network-free: transport and credentials belong to lox (`src/`). Backup
  ZIPs and the LoxCC codec are file formats and live here (`src/archive.rs`,
  feature `archive`).
  The CLI is `src/cli.rs`, run by the `loxl` binary and by `lox logic`.

## Doc map

| File | Purpose |
|---|---|
| [README.md](README.md) | overview, quickstart |
| [docs/vision.md](docs/vision.md) | why this exists |
| [docs/design.md](docs/design.md) | architecture, decisions D1–D12 |
| [docs/language.md](docs/language.md) | normative `.loxl` language spec |
| [docs/lockfile-spec.md](docs/lockfile-spec.md) | lockfile v1 spec |
| [docs/loxone-format.md](docs/loxone-format.md) | validated `.Loxone` format facts |
| [docs/implementation.md](docs/implementation.md) | module map, testing, gotchas |
| [docs/connector-db.md](docs/connector-db.md) | connector database: evidence, admission rules |
| [docs/oracle-wine.md](docs/oracle-wine.md) | save oracle via Loxone Config under Wine: rig + findings |
| [docs/agents.md](docs/agents.md) | **how agents use the toolchain** |
| [skills/lox-logic/SKILL.md](skills/lox-logic/SKILL.md) | condensed agents.md as a Claude Code skill; keep in sync |
| [docs/roadmap.md](docs/roadmap.md) | Stufen, open questions |
