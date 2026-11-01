# Implementation guide

How the crate is built, for people working on it. Rust edition 2024, MSRV 1.88. Dependencies stay light:
`serde`/`serde_json` (lockfile, JSON output), `sha2` (slug entities, config hashes), `thiserror`, and for
the `archive` feature `zip`, `lz4_flex` and `crc32fast`.

## Module map

### The program file

| Path | Contents | Invariant |
|---|---|---|
| `src/xml.rs` | Lossless syntax tree (`XmlDocument`, `Element`, `Node`, `Attr`), hand-written parser and writer, `escape`/`unescape` | `parse(bytes).to_bytes() == bytes` for real Loxone output |
| `src/uuid.rs` | `LoxUuid`, `TailKind`, `Minter`, `parse_serial`, `entity_for_slug` | no clock, no randomness: the caller supplies everything |
| `src/doc.rs` | `LoxoneDoc` and its read views: `objects()`, `ports()`, `wires()`, `index()`, counters, `page_path`, `remove_by_uuid`, `duplicate_uuids()` | views are derived on demand; nothing is cached |
| `src/connectors.rs` | the verified `builtin()` table, evidence from `observe()` | only live-verified types in `builtin` |
| `src/observed_defaults.rs` | GENERATED: the defaults Loxone Config writes, from the corpus (`tools/extract-defaults.py`) | view only: hides defaults in decompile, never affects compile |
| `src/port_meta.rs` | GENERATED: documented port facts (unit, canvas label, range, default) from lox-cli's TechDoc scrape (`tools/extract-port-meta.py`, read from a path, never vendored) | units scale suffixes and refuse mismatches; labels only feed error hints; ranges only feed lint |
| `src/canvas_labels.rs` | canvas labels of input keys that `port_meta` lacks, read off Loxone Config's canvas | hand-written; only entries checked in Loxone Config |
| `src/blockdoc.rs` | one line per builtin type on what it is for (`loxl types`, LSP hover) | written for Lox Logic, not copied from Loxone |
| `src/port_doc.rs` | one line per port from the type's knowledge-base page, with the abbreviation it maps to (`loxl types`, LSP hover and completion) | hand-written; `tools/check-port-docs.py` checks each abbreviation against the page |
| `src/lock.rs` | `Lockfile` ([lockfile-spec.md](lockfile-spec.md)): load, save, `remove_object`/`rename_object`/`absorb_counters` | serialization is deterministic (BTreeMaps) |
| `src/diff.rs` | UUID-keyed semantic diff with the `locale_suspect` heuristic; compares attribute parameters (`Formula=`) and canonical `APC=` rule payloads | `diff(a, a).is_empty()` |
| `src/access.rs` | users, groups and access rights as a read model, for the users/access section of the diff | read only |
| `src/moods.rs` | LightController2 moods: read model and the circuit value codec, shared by `moods` claims and the diff | |
| `src/autopilot.rs` | Automatic Designer `APC=` payload: parse, canonicalize, summary, field-path changes | untyped JSON, so unknown fields still diff |
| `src/autopilot_caps.rs` | GENERATED (`scripts/autopilot_caps.py`): the Miniserver's condition and action codes per app control type | codes only, no localized texts |
| `src/settings.rs` | settings of physical inputs and outputs the source may claim (description, unit, correction, timing) | only attributes the corpus shows on these types; the original is kept in the lock |
| `src/periphery.rs` | virtual devices and Modbus: the object-tree objects a source can create, and their I/Os | |
| `src/hardware.rs` | the Tree and Air devices and extensions a declaration can take over, the I/O types below each, the statistics values | matches existing hardware only |
| `src/tree_kinds.rs` | the Tree device kinds a declaration can create, each element as a save writes it | only verified kinds ([connector-db.md](connector-db.md#tree-devices)) |
| `src/archive.rs` | feature `archive`: backup ZIPs, the LoxCC codec, `stage_upload_zip` | bytes only, no transport; other entries copied raw |
| `src/catalogue.rs` | `LoxAPP3.json`: carries a compile's renames, rooms, categories, deletions and new entries into the app catalogue | new entries only for types in `KNOWN` |
| `src/permissions.rs` | `permissions.bin`: which principals may see and use which object | |
| `src/layout.rs` | page geometry as Loxone Config draws it (`PageGeometry`) and `place`: layered clusters, ref tags beside their port, on the paper sheet | deterministic; moves only the objects it is handed |
| `src/render.rs` | `loxl render`: one page as SVG, and `PageMetrics` (overlaps, backward and long wires, crossings) | read only |
| `src/scan.rs` | `loxl scan`: personal data in a config by rule | reports element, attribute and length, never a value; CI runs it over every committed `.Loxone` |
| `src/coverage.rs` | `loxl coverage`: every object by type against the lockfile (managed, extern, rule, untouched), and the attributes still carried from the base | read only |

### Lox Logic (`src/logic/`)

| Path | Contents | Invariant |
|---|---|---|
| `parser.rs` | line-oriented lexer and parser; only a block's `( … )` argument list spans lines | errors carry 1-based line numbers; v0 keywords give migration errors |
| `ast.rs`, `ast/check.rs` | `Module`, `Item`, declarations, `Binding`s, typed `Value`; canonical `to_text()`; `Module::validate` (names, references, per-statement checks) | `parse(to_text(m)) == m` |
| `validate.rs` | builtin-table checks (`validate_ports`): types, port names, wire directions, "did you mean" | needs no base config; shared by `check` and `compile` |
| `hardware.rs` | hardware declarations lowered into externs, port statements and owned records | runs last in expansion |
| `template.rs` | template expansion into plain statements | the lockfile and compiler only see the expanded form |
| `defaults.rs` | `defaults <Type>(…)`: applied during expansion; factoring for decompile and adopt; fitting a block `adopt --uuid` brings in | compiles like the values written on each block |
| `desugar.rs` | expressions into gate and comparator blocks; arithmetic into one `Formula` | runs after expansion, before port validation |
| `compile.rs`, `compile/` | the compiler ([design.md](design.md)): `plan.rs` (identity, page, container, values), `teardown.rs` (remove the previous output), `emit.rs` (managed `<C>` elements), `wires.rs`, `claims.rs` (extern assignments, settings, attribute claims), `owned.rs` (what hardware declarations own), `create.rs` (Tree devices the source creates, the guard against deleting hardware) | teardown and rebuild converge; deterministic |
| `decompile.rs` | config → Lox Logic view (`decompile`, `decompile_pages`), `slugify`, the shared `Lift` | full view by default; `ManagedOnly` lifts only wires touching managed blocks |
| `adopt.rs`, `adopt_tree.rs`, `adopt_hardware.rs` | `adopt`: a managed module and a lockfile that pins existing identities; a virtual or Modbus device with all its I/Os; a Tree or Air device with its I/Os | adopt then compile changes nothing; refused blocks stay untouched |
| `rename.rs` | `loxl rename`: slug rename across module and lockfile, synthetic slugs included | output unchanged except titles the slug feeds |
| `lint.rs` | advisory lints: unused names, values out of range, bare durations (`unit-suffix`, fixable), blocks whose outputs feed nothing in the compiled config, `# lint: allow` comments | never part of compile; `lint --fix` keeps the compile output byte-identical |
| `tiles.rs` | Touch Pure Flex tiles: tile arguments → functions, rewrite of claimed `<btns>`, tile lints | unlisted tiles and layout kept byte for byte |
| `access.rs` | access rights on NFC Code Touch blocks, and push notification recipients | an `access` declaration owns every `TIM=` on its block |
| `schedule.rs` | DayTimer schedules from source | rewrites `Modes=`, the entries and their counts only |
| `calendar.rs`, `calendar/date.rs` | the mode a calendar entry switches on (`switches:` → `CalType=`) and its date (`on:`, `easter:`, `from:`/`to:` → `Mode=` and the attributes of its kind), the refusal of the stored attributes, fmt and adopt | writes `CalType=` after every mode has its number; the date through `collect_params`, a date attribute an adopted entry gains in the save's order |
| `rule.rs` | Automatic Designer rules from source | writes only when the payload differs; edits the way the app does |
| `moods.rs` | LightController2 mood claims | moods keep their identity by name |
| `simtest.rs` | `loxl test`: plan the test bodies against the compiled config, run them in `sim`, map results back to `expect` lines | the lockfile is never written |

### Simulator (`src/sim/`)

| Path | Contents |
|---|---|
| `mod.rs` | runs a compiled document cycle by cycle; a port whose logic has no model is an error that names the object |
| `model.rs` | one model per simulated type, written from Loxone's documentation ([simulator.md](simulator.md)) |
| `formula.rs` | the `Formula` expression language as Loxone documents it |
| `clock.rs` | the simulated local wall clock |

### Tools and front-ends

| Path | Contents |
|---|---|
| `src/lsp/` | `loxl lsp`: JSON-RPC framing and handlers (`mod.rs`), diagnostics, hover, completion, references and rename (`analysis.rs`), UTF-16 columns and URIs (`text.rs`). Writes nothing; the deep check compiles on a copy of the lock |
| `src/project.rs` | `Project`: `lox.toml` (a strict flat TOML subset) and path resolution; anything outside the subset is refused |
| `src/library.rs` | the template library (`templates/`, `loxl templates`) |
| `src/cli.rs` | the CLI, run by `loxl` (`src/main.rs`) and `lox logic`; thin wrappers over the public API |
| `src/error.rs` | the crate's error type |

## Tests

`cargo test -p loxone-config` runs:

- **Unit tests** in each module. Format details are pinned to live observations, e.g. real UUIDs in the
  `uuid` tests.
- **Integration tests** in `tests/`, one file per feature. `tests/logic.rs` covers the pipeline:
  determinism, lockfile identity, teardown and restore, refusals, and the decompile views.
  `tests/skill.rs` keeps the agent skill's example compiling, `tests/agent_cases.rs` the reference
  solutions in `tools/agent/`.
- **`tests/roundtrip.rs`**: byte fidelity on the example configs, plus an opt-in corpus:

  ```sh
  LOXL_CORPUS=~/loxone-backups cargo test --test roundtrip
  ```

Real configs contain personal data (addresses, key hashes). **Never commit them.** `.gitignore` blocks
`/corpus`, and CI fails on any `loxl scan` finding in a committed `.Loxone`; run it before adding a
fixture.

The public corpus (about 150 configs from Loxone's KB samples, LoxWiki and GitHub) is pinned in
[`tools/corpus-manifest.json`](../tools/corpus-manifest.json) with each file's source, sha256 and
license:

```sh
tools/fetch-corpus.py              # download into corpus/web, verify hashes
LOXL_CORPUS=corpus/web cargo test --release --test corpus --test roundtrip
tools/fetch-corpus.py --discover   # search the sources again, rewrite the manifest
```

The *Corpus* workflow runs this weekly and on demand. Configs whose license allows it also go into the
[example library](../examples/library/README.md).

Before a change: `cargo test && cargo clippy --all-targets && cargo fmt --check`, all clean.

Outside `cargo test`, [`tools/eval/`](../tools/eval/README.md) grades Lox Logic solutions to lox-cli's
automation tasks, and [`tools/agent/`](../tools/agent/README.md) runs Claude Code on our own requests.

## Adding a block type to the builtin table

The table (`connectors::builtin`) decides which types the compiler may create. Grow it from evidence:

1. Run `loxl observe <cfg.Loxone>...` over the corpus (several configs merge). Per port key it reports
   the connector index (from the port UUIDs) and the sink, source and default counts.
2. Cross-check with `--crosscheck` against the legacy databases: lox-cli's
   `docs/schemas/connector-map.json`, and lox-sim's `block_signature` via
   `tools/extract-sim-signature.py`. They contain wrong entries. A disagreement means more evidence is
   needed, not a vote. The admission rules are in [connector-db.md](connector-db.md).
3. Check the full connector list and order against a real instance in the XML (all `<Co>`s, in order).
4. Add the `const` slice in index order, with directions. Write the descriptor's complete connector set
   and nothing more: Loxone Config deletes unknown connectors on save.
5. Add a test that pins the shape, and ideally a compile whose output you have opened and saved in
   Loxone Config.
6. Regenerate `src/port_meta.rs` (`tools/extract-port-meta.py`, the command is in
   [connector-db.md](connector-db.md)) so the new ports get their units, labels and ranges, regenerate
   the library views if `decompile` now writes suffixes, and add the type's port descriptions to
   `src/port_doc.rs`. `tools/check-port-docs.py` checks them against the knowledge-base page.

## Changing the language

A grammar change touches, in order: `logic/parser.rs`, `logic/ast.rs` (AST, `to_text`, `validate`),
`logic/validate.rs` if the builtin-table checks change, [language.md](language.md), the VS Code grammar
and snippets (`editor/vscode/`), and [agents.md](agents.md) if agent guidance changes. The
`text_roundtrip` test keeps `parse ∘ to_text = id`.

## Gotchas

- `Element.attrs` order is the serialized order. Where Loxone has a canonical position (`Co`:
  `K, Nc, Def, U`), insert in order like `set_attr_ordered` in `logic/compile.rs`.
- Attribute values are stored escaped. Read with `attr_decoded`, write with `set_attr` (it escapes).
- Paths from `objects()` are child indexes over all nodes and go stale after a structural change.
  Derive them again, or find the element by UUID (`find_c_mut`).
- After a compile error the lockfile may be partly advanced. Reload it; the CLI saves only on success.
- An element the compiler empties must be written self-closing (see `remove_object` in `doc.rs`).
  Loxone never writes `<C ...></C>` for an emptied element.
