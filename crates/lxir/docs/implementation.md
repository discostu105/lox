# Implementation guide

Orientation for working on the crate itself. Rust edition 2024, MSRV 1.88.
Dependencies are deliberately light: `serde`/`serde_json` (lockfile,
observe output), `sha2` (slug entities, config hashes), `thiserror`.

## Module map

| Path | Contents | Key invariant |
|---|---|---|
| `src/xml.rs` | Lossless CST: `XmlDocument`, `Element`, `Node`, `Attr`; hand-rolled parser; canonical writer; `escape`/`unescape` | `parse(bytes).to_bytes() == bytes` for real Loxone output |
| `src/uuid.rs` | `LoxUuid` parse/format, `TailKind`, `Minter`, `parse_serial`, `entity_for_slug` | no clock, no RNG — fully caller-driven |
| `src/doc.rs` | `LoxoneDoc` + read views: `objects()`, `ports()`, `wires()`, `index()`, counters, `page_path`, `remove_by_uuid`, `duplicate_uuids()` (D38) | views derive from the tree on demand; nothing caches |
| `src/connectors.rs` | verified `builtin()` table, evidence `observe()` | only live-verified types in `builtin` |
| `src/observed_defaults.rs` | GENERATED corpus-observed GUI defaults (D30, `tools/extract-defaults.py`) | view-only: full-view elision, never affects compile |
| `src/lock.rs` | `Lockfile` (spec: [lockfile-spec.md](lockfile-spec.md)); load/save/stable JSON; `remove_object`/`rename_object`/`absorb_counters` | serialization is deterministic (BTreeMaps) |
| `src/ir/parser.rs` | line-oriented lexer + parser (block `( … )` argument lists span lines) | errors carry 1-based line numbers; v0 keywords give migration errors |
| `src/ir/ast.rs` | `Module`, `Item` (incl. `Comment`), decls, argument `Binding`s (param vs. wire), typed `Value`; `validate()`; canonical `to_text()` | `parse(to_text(m)) == m`; `to_text` is a fixpoint |
| `src/ir/validate.rs` | static builtin-table checks (`validate_ports`): types, port names, wire directions; "did you mean" suggestions | needs no base config — shared by `lxir check` and `compile` |
| `src/ir/compile.rs` | the compiler (strategy: [design.md](design.md)) | tear-down/rebuild convergence; determinism |
| `src/ir/tiles.rs` | Touch Pure Flex tiles (D41): `program` (tile args → three functions), `apply` (compile phase: rewrite claimed `<btns>`, API-link and abbreviation checks), `lint_tiles` (`tile-broadcast`/`tile-dead-command`) | rewrites existing `<btn>` functions only; unlisted tiles and layout carried byte for byte |
| `src/ir/decompile.rs` | config → IR view (`decompile` with `page` statement sections (D28), `decompile_pages` per page), `slugify`, `DecompileReport`; the shared `Lift` | full view by default (D17); `ManagedOnly` lifts only managed-touching wires (D7) |
| `src/ir/adopt.rs` | `adopt`: managed-only module + lockfile pinning existing identity (D18); per-block rebuild verification (`AdoptReport::refused`) | adopt → compile is a semantic no-op; skipped blocks stay untouched |
| `src/diff.rs` | UUID-keyed semantic diff, `locale_suspect` heuristic; compares attribute parameters (`Formula=`) and canonicalized `APC=` rule payloads (D37) | `diff(a, a).is_empty()` |
| `src/layout.rs` | page geometry as the GUI draws it (`PageGeometry`: footprints, visible port rows, wires, off-page labels) and `place` (D42: clusters as layered graphs, ref tags beside their port; D49: on the paper sheet, packed tighter where room is short) | deterministic; moves only the objects it is handed |
| `src/render.rs` | `lxir render`: one page as SVG, `PageMetrics` (overlaps, backward and long wires, crossings, wire length) | read-only |
| `src/autopilot.rs` | Automatic Designer `APC=` payload: parse/canonicalize, view summary, field-path change list (D37) | read-only; untyped JSON so unknown fields still diff |
| `src/project.rs` | `Project`: `lox.toml` load/parse (strict flat TOML subset), path resolution against the file's directory | refuses everything outside the subset with pointed errors (D25) |
| `src/bin/lxir.rs` | CLI; thin wrappers over the public API only | no semantics of its own |

## Testing strategy

Three layers, all run by `cargo test`:

1. **Unit tests** in each module — format details pinned to live
   observations (e.g. `uuid` tests contain real UUIDs from the
   investigation).
2. **`tests/ir.rs`** — end-to-end pipeline properties: byte determinism,
   lock pinning across mint times, recompile-own-output fixpoint, counter
   monotonicity, set-restore and wire-teardown, the removal trichotomy,
   refusal paths (unknown type/port, grown gate inputs, no-match,
   direction misuse), decompile views (managed-only subset, full view,
   per-page split).
3. **`tests/roundtrip.rs`** — byte fidelity on the committed example
   configs, plus an opt-in corpus:

   ```sh
   LXC_CORPUS=~/loxone-backups cargo test --test roundtrip
   ```

   Real configs contain personal data (addresses, key hashes) — **never
   commit them**; `.gitignore` blocks `/corpus`. Every new real config is a
   free test case: drop it in the corpus dir and run the suite.

QA bar for changes: `cargo test && cargo clippy --all-targets && cargo fmt
--check`, all clean.

## How to add a block type to the builtin table

The table (`connectors::builtin`) is the compiler's license to mint. Grow it
evidence-first:

1. Run `lxir observe <cfg.Loxone>...` over the whole corpus (multiple
   configs merge). The output gives, per port key: connector index (from
   the port UUIDs), sink/source/def counts.
2. Cross-check with `--crosscheck` against both legacy databases (lox-cli
   `docs/schemas/connector-map.json`; lox-sim `block_signature` via
   `tools/extract-sim-signature.py`) — they disagree and have corrupt
   entries; disagreement means more evidence is needed, not a vote. The
   admission rules and the current state live in
   [connector-db.md](connector-db.md).
3. Confirm the full connector list and index order against a real block
   instance in the XML (all `<Co>`s, in order).
4. Add the `const` slice in index order with directions. Emit the
   descriptor's **complete** connector set and nothing beyond it — Loxone
   Config deletes off-descriptor connectors on save (D8).
5. Add a test pinning the shape, and ideally a compile test whose output
   you have loaded into Loxone Config once (the ultimate oracle).

## Extending the IR

Grammar changes touch, in order: `ir/parser.rs` (lexer/statement),
`ir/ast.rs` (AST + `to_text` + `validate`), `ir/validate.rs` when the
builtin-table checks are affected, [ir-spec.md](ir-spec.md) (normative
text), the VS Code grammar + snippets (`editor/vscode/`), and
`docs/agents.md` if agent guidance changes. Keep `parse ∘ to_text = id` —
the `text_roundtrip` test enforces it.

## Gotchas

- `Element.attrs` order **is** the serialized order — use
  `set_attr_ordered`-style insertion (see `ir/compile.rs`) when Loxone has
  a canonical position (`Co`: `K, Nc, Def, U`).
- Attribute values in the tree are stored **raw/escaped**; use
  `attr_decoded` for semantics and `set_attr` (which escapes) for writing.
- Paths from `objects()` are child indexes counting *all* nodes; they go
  stale after structural mutations — re-derive or locate by UUID
  (`find_c_mut` in the compiler).
- On compile error, the lockfile may be partially advanced — callers must
  reload it (the CLI saves only after success).
- Emptied elements must flip back to self-closing (`fixup_emptied_elements`)
  or the writer would emit `></C>`, which Loxone never does.
