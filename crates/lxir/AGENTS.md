# Agent notes for the lxir crate

`lxir` is a Rust library + CLI that treats Loxone Miniserver configs
(`.Loxone` XML) as source code: a text IR (`.lxir` files) compiles against a
base config with a lockfile pinning all UUIDs. Start with
[docs/agents.md](docs/agents.md) — it is the operational guide for using the
toolchain (the loop, hard rules, error remedies).

## Build & test

```sh
cargo test -p lxir               # unit, pipeline, roundtrip
cargo clippy -p lxir --all-targets   # must be warning-free
cargo fmt --check
LXIR_CORPUS=<dir> cargo test -p lxir --test roundtrip   # opt-in: real configs
cargo run --bin lxir -- help     # the CLI (also: lox ir help)
```

## Rules that matter here

- **Never edit `.Loxone` files by hand** — only through `lxir compile`.
  Never hand-edit `*.lock.json` beyond the documented operations.
- **Never commit real Miniserver configs** (personal data). Synthetic
  fixtures live in `examples/configs/`; real ones go in an untracked corpus
  dir referenced via `LXIR_CORPUS`.
- The builtin connector table (`src/connectors.rs`) only accepts
  **live-verified** entries — the growth workflow is in
  [docs/implementation.md](docs/implementation.md).
- Format knowledge is documented, not guessed: check
  [docs/loxone-format.md](docs/loxone-format.md) before assuming anything
  about the XML.
- Keep `cargo test`, clippy, and fmt clean; when compiler output changes,
  regenerate `examples/out/` via `cargo run --example compile` and the
  project artifacts via `lxir compile --time 1767225600` inside
  `examples/project/` ([examples/README.md](examples/README.md)).
- Apache-2.0 (see [LICENSE](LICENSE)), same as lox. Published on crates.io as `lxir`; lox itself uses
  it by path.
- This crate lives in the lox workspace (`crates/lxir`). It stays
  network-free: transport and credentials belong to lox (`src/`). Backup
  ZIPs and the LoxCC codec are file formats and live here (`src/archive.rs`,
  feature `archive`).
  The CLI is `src/cli.rs`, run by the `lxir` binary and by `lox ir`.

## Doc map

| File | Purpose |
|---|---|
| [README.md](README.md) | overview, quickstart |
| [docs/vision.md](docs/vision.md) | why this exists |
| [docs/design.md](docs/design.md) | architecture, decisions D1–D12 |
| [docs/ir-spec.md](docs/ir-spec.md) | normative `.lxir` language spec |
| [docs/lockfile-spec.md](docs/lockfile-spec.md) | lockfile v1 spec |
| [docs/loxone-format.md](docs/loxone-format.md) | validated `.Loxone` format facts |
| [docs/implementation.md](docs/implementation.md) | module map, testing, gotchas |
| [docs/connector-db.md](docs/connector-db.md) | connector database: evidence, admission rules |
| [docs/oracle-wine.md](docs/oracle-wine.md) | save oracle via Loxone Config under Wine: rig + findings |
| [docs/agents.md](docs/agents.md) | **how agents use the toolchain** |
| [skills/lxir/SKILL.md](skills/lxir/SKILL.md) | condensed agents.md as a Claude Code skill; keep in sync |
| [docs/roadmap.md](docs/roadmap.md) | Stufen, open questions |
