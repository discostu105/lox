# Design

How the crate is put together, and why. The references are [language.md](language.md) (the language),
[lockfile-spec.md](lockfile-spec.md) (the state file) and [loxone-format.md](loxone-format.md) (the target
format). Each design decision has its own file in [decisions/](#decisions).

## Layering

```
            ┌───────────────────────────────────────────┐
            │  ir: Module ── compile ──► LoxoneDoc      │  semantics
            │            ◄─ decompile ─                 │
            │  lock: Lockfile (identity between runs)   │
            ├───────────────────────────────────────────┤
            │  doc: objects / ports / wires / counters  │  read views
            │  connectors: builtin table + observe()    │
            │  diff: semantic comparison                │
            ├───────────────────────────────────────────┤
            │  xml: lossless CST, byte-faithful writer  │  syntax
            └───────────────────────────────────────────┘
```

- **`xml`** stores the concrete syntax losslessly: attribute order, raw escaping, self-closing flags, BOM and
  declaration. The upper layers mutate through this tree, so anything they don't understand round-trips
  untouched ([D1](decisions/0001-byte-faithful-cst.md)).
- **`doc`** is a read-only view layer. It never caches and always derives from the tree, so a view cannot go
  stale after a mutation.
- **`ir` + `lock`** hold all write semantics. `compile` is the only function that makes coordinated
  mutations.

## The ownership model

A config file has three writers: Loxone Config, the Miniserver itself (rules created in the app, device
registrations) and this compiler. The compiler owns four kinds of edit and records each in the lockfile:

1. **Managed blocks** (`slug = Type(…)`). The compiler alone creates, rebuilds and deletes them.
2. **Extern wires** (`target.Port <- source.Port`). These are `<In>` elements the compiler added to ports of
   objects it does not own. `lock.extern_wires` records them, so they can be removed without touching wires
   drawn in Loxone Config.
3. **Extern sets** (`target.Port = value`). These are `Def=` values the compiler rewrote on extern ports.
   `lock.set_originals` records the old value, and the compile restores it when the assignment leaves the
   source.
4. **Attribute claims** (`target.title|room|category = value`). These set the title, room or category of an
   extern object: devices and their inputs and outputs, which only Loxone Config can create.
   `lock.attr_claims` records the original, the written value and the objects the claim carried along. The
   compile restores them when the claim leaves the source ([D39](decisions/0039-attribute-claims.md),
   [D43](decisions/0043-title-claim-carries-refs.md)).

Some declarations write content of Loxone Config without recording that content in the lockfile:

- the named sets and their claims: the tiles of a Touch Pure Flex
  ([D41](decisions/0041-touch-pure-flex-tiles.md)), the access rights on an NFC Code Touch
  ([D49](decisions/0049-nfc-access-rights.md)), the recipients of a push notification
  ([D51](decisions/0051-push-notifications.md)), the schedule of a DayTimer
  ([D53](decisions/0053-daytimer-schedules.md)) and the moods of a lighting controller
  ([D58](decisions/0058-lighting-moods.md)), each a set and its claims
  ([D83](decisions/0083-access-recipients-sets.md)),
- the rules of the Automatic Designer ([D57](decisions/0057-automatic-designer-rules.md)).

They own what they declare while they declare it. Removed, they leave the last written state in place.

Everything else belongs to the other writers and passes through unchanged. One part of a managed block is
shared: its position on the page. Loxone Config may move it, and the compile takes the move over
([D42](decisions/0042-placement-gui-geometry.md)).

## The compile strategy: tear down, rebuild

Each compile:

1. Validates the module. It refuses if the lock knows a managed slug that the source lost (see
   [The removal trichotomy](#the-removal-trichotomy)).
2. Resolves externs: the lock's pin first, then the match spec. Ambiguity is an error.
3. **Tears down** its previous output from the base: removes all locked managed objects and extern wires,
   and restores all recorded set originals and claimed attributes. What remains is the state the other two
   writers own.
4. **Rebuilds** from source: plans port lists, creates missing UUIDs, writes managed `<C>` elements, applies
   wires and sets, records everything in the lock, and places blocks that have no position yet
   ([D42](decisions/0042-placement-gui-geometry.md)).
5. Writes counters, which never decrease, and the target metadata.

Compilation converges: compiling the compiler's own output again gives the same bytes (tested in
`tests/ir.rs`). This makes the real workflow safe: compile, upload, save in Loxone Config, download, compile.
Where a save reorders wires or blocks, the rebuild keeps the saved order
([D45](decisions/0045-wire-order-kept.md), [D47](decisions/0047-block-order-kept.md)).

## Identity and determinism

- An object's identity is its **UUID**, never its title: titles change with the locale (see
  [loxone-format.md](loxone-format.md#objects-c)). Externs resolve by `uuid`, then `iname`, then `title`.
  The lock pins the UUID that matched, and later compiles use the pin even if the title changes.
- New UUIDs come from a **deterministic minter**. The caller passes the creation time and machine id. Port
  entities are `sha256(slug)[..6]` per block, and sequence counters advance per UUID. The library uses no
  clock and no RNG. Within a run, the minter makes UUIDs deterministic. Across runs, the lockfile does: it
  records each new UUID at once and reuses it.
- So the same base, module, lock and options give the **same output bytes**. A different mint time with the
  same lock gives the same bytes too.

## The removal trichotomy

A managed slug that is in the lock but missing from the source may be a typo or a deletion. The compiler
refuses by default. The statements in the language are the preferred resolutions, because they are scoped
and show up in the PR diff ([D13](decisions/0013-removed-moved-statements.md)):

| Intent | Mechanism | Effect on config | Effect on lock |
|---|---|---|---|
| a typo | fix the source | — | — |
| delete it | `removed <slug>` in source (or `allow_removals` / `--allow-removals` for all) | block removed | entry dropped |
| stop managing it | `Lockfile::remove_object` | block **stays** (orphan) | entry dropped |
| rename the slug | `moved <old> -> <new>` in source (or `Lockfile::rename_object`) | identity kept | key renamed |

A removal leaves a tombstone in the lock until a downloaded base no longer has the block. After that, a
leftover `removed` statement is an error, so it gets cleaned up
([D31](decisions/0031-removal-tombstones.md)).

## Refuse, never guess

Anything unverified is an error, not a heuristic:

- Only block types in the verified builtin table (`connectors::builtin`) can be created. The two older
  connector databases (lox-cli's map and lox-sim's signatures) contradict each other for many types, so the
  table admits only entries confirmed against real configs. To grow it, `loxl observe` gathers evidence, the
  evidence is cross-checked, and the entry is verified live ([connector-db.md](connector-db.md)).
- Wiring or setting an extern port whose `<Co>` is missing from the base is an error. The compiler does not
  invent port UUIDs for types whose connector indexes it cannot verify. The error lists the ports that do
  exist.
- An extern that matches no object, or several, is an error that lists the candidates.

## Decisions

Each decision records its context, the choice and the rejected alternatives. The number stays with the
decision, so `grep -r D42` finds it. A new decision takes the next number and a file
`decisions/NNNN-short-slug.md`. A changed decision gets a new one, and the old file's status links to it.

| # | Decision | Status |
|---|---|---|
| D1 | [A byte-faithful syntax tree, not a DOM](decisions/0001-byte-faithful-cst.md) | accepted |
| D2 | [The lockfile pins port UUIDs](decisions/0002-lock-pins-port-uuids.md) | accepted |
| D3 | [Ports are minted before their object](decisions/0003-ports-minted-before-object.md) | accepted |
| D4 | [Managed blocks write a `<Co>` for every connector](decisions/0004-co-for-every-connector.md) | accepted |
| D5 | [`Nc` counts the `<In>` children](decisions/0005-nc-counts-in-children.md) | accepted |
| D6 | [Removal is a trichotomy](decisions/0006-removal-trichotomy.md) | accepted, amended by [D13](decisions/0013-removed-moved-statements.md) and [D31](decisions/0031-removal-tombstones.md) |
| D7 | [The managed-only view lifts only wires that touch a managed block](decisions/0007-decompile-managed-wires.md) | accepted, amended by [D17](decisions/0017-decompile-view-by-page.md) |
| D8 | [Gate inputs are fixed](decisions/0008-fixed-gate-inputs.md) | accepted |
| D9 | [Transport is out of scope](decisions/0009-no-transport.md) | accepted |
| D10 | [All comments survive the round trip](decisions/0010-comments-survive.md) | accepted |
| D11 | [`NextObj` advances by one per minted object](decisions/0011-nextobj-counter.md) | accepted |
| D12 | [A dedicated grammar](decisions/0012-dedicated-grammar.md) | accepted |
| D13 | [`removed` and `moved` statements](decisions/0013-removed-moved-statements.md) | accepted, amended by [D31](decisions/0031-removal-tombstones.md) |
| D14 | [Port assignment is for extern ports only](decisions/0014-set-for-extern-ports.md) | accepted, amended by [D16](decisions/0016-constructor-syntax.md) |
| D15 | [Values are typed tokens](decisions/0015-typed-value-tokens.md) | accepted, amended by [D80](decisions/0080-v2-one-spelling.md) |
| D16 | [A block's inputs live at its declaration](decisions/0016-constructor-syntax.md) | accepted, amended by [D24](decisions/0024-expressions.md) and [D26](decisions/0026-expressions-in-arguments.md) |
| D17 | [Decompile is a view, grouped by page](decisions/0017-decompile-view-by-page.md) | accepted, amended by [D28](decisions/0028-page-statement.md), [D29](decisions/0029-full-and-adoptable-view.md) and [D30](decisions/0030-observed-defaults.md) |
| D18 | [Adoption is decompile with a lock, verified per block](decisions/0018-adoption.md) | accepted, amended by [D19](decisions/0019-gui-residue-carried-forward.md), [D20](decisions/0020-inv-connector-gui-owned.md) and [D62](decisions/0062-house-control-types.md) |
| D19 | [GUI-owned residue is carried forward from the base](decisions/0019-gui-residue-carried-forward.md) | accepted, amended by [D20](decisions/0020-inv-connector-gui-owned.md) |
| D20 | [A connector with `Inv=` is wholly GUI-owned](decisions/0020-inv-connector-gui-owned.md) | accepted |
| D21 | [The drift baseline is a fingerprint of what the diff compares](decisions/0021-drift-baseline.md) | accepted |
| D22 | [The Loxone Config release is a qualification pin](decisions/0022-config-version-pin.md) | accepted, amended by [D46](decisions/0046-block-version.md) |
| D23 | [Templates are macro expansion with locked identities](decisions/0023-templates.md) | accepted, amended by [D82](decisions/0082-template-instance-names.md) |
| D24 | [Expressions are sugar over discrete blocks, owned by their statement](decisions/0024-expressions.md) | accepted, amended by [D26](decisions/0026-expressions-in-arguments.md), [D35](decisions/0035-arithmetic-formula.md) and [D81](decisions/0081-named-signals.md) |
| D25 | [Projects are a `lox.toml`, and there is no `import`](decisions/0025-projects-lox-toml.md) | accepted |
| D26 | [Expressions bind in argument lists too](decisions/0026-expressions-in-arguments.md) | accepted |
| D27 | [Values with a unit suffix, scaled at compile](decisions/0027-unit-suffixed-values.md) | accepted, amended by [D52](decisions/0052-port-facts.md) |
| D28 | [`page "Title"` places blocks, and it is authoritative](decisions/0028-page-statement.md) | accepted, amended by [D56](decisions/0056-creating-pages.md) and [D84](decisions/0084-page-per-fragment.md) |
| D29 | [The full view is for the reader; only the adoptable view must compile](decisions/0029-full-and-adoptable-view.md) | accepted |
| D30 | [The full view hides what the GUI wrote by default](decisions/0030-observed-defaults.md) | accepted |
| D31 | [Removal tombstones](decisions/0031-removal-tombstones.md) | accepted |
| D32 | [A ref matched by what it mirrors](decisions/0032-mirrors-matching.md) | accepted |
| D33 | [Minted mirrors](decisions/0033-minted-mirrors.md) | accepted, amended by [D48](decisions/0048-text-ref-flag.md) |
| D34 | [Mirror routing](decisions/0034-mirror-routing.md) | accepted, amended by [D38](decisions/0038-unique-uuids.md) |
| D35 | [Arithmetic compiles to a Formula block](decisions/0035-arithmetic-formula.md) | accepted |
| D36 | [Tests live in the module](decisions/0036-tests-in-module.md) | accepted, partly superseded by [D68](decisions/0068-own-simulator.md) (the simulator), amended by [D80](decisions/0080-v2-one-spelling.md) |
| D37 | [Reading Automatic Designer rules](decisions/0037-automatic-designer-read-only.md) | accepted, amended by [D57](decisions/0057-automatic-designer-rules.md) |
| D38 | [UUIDs are unique; a `uuid:` edit re-points](decisions/0038-unique-uuids.md) | accepted |
| D39 | [Attribute claims on externs](decisions/0039-attribute-claims.md) | accepted, amended by [D43](decisions/0043-title-claim-carries-refs.md) and [D63](decisions/0063-managed-room-category.md) |
| D40 | [Program code from source; numeric character references are text](decisions/0040-program-code.md) | accepted |
| D41 | [Touch Pure Flex tiles from source](decisions/0041-touch-pure-flex-tiles.md) | accepted |
| D42 | [Placement in the GUI's geometry, shared with the GUI](decisions/0042-placement-gui-geometry.md) | accepted, amended by [D50](decisions/0050-placement-paper-sheet.md) |
| D43 | [A title claim carries the refs that copy the title](decisions/0043-title-claim-carries-refs.md) | accepted |
| D44 | [`loxl repair` turns numeric character references back into characters](decisions/0044-loxl-repair.md) | accepted |
| D45 | [A rebuilt wire keeps its place among the sink's `<In>`](decisions/0045-wire-order-kept.md) | accepted |
| D46 | [Managed blocks carry the document's block version](decisions/0046-block-version.md) | accepted |
| D47 | [A rebuilt block keeps the base's order](decisions/0047-block-order-kept.md) | accepted |
| D48 | [A minted ref of a text value carries `Text="true"`](decisions/0048-text-ref-flag.md) | accepted |
| D49 | [Access rights on NFC Code Touch blocks from source](decisions/0049-nfc-access-rights.md) | accepted, amended by [D83](decisions/0083-access-recipients-sets.md) |
| D50 | [Placement keeps to the paper sheet](decisions/0050-placement-paper-sheet.md) | accepted |
| D51 | [Push notifications from source](decisions/0051-push-notifications.md) | accepted, amended by [D83](decisions/0083-access-recipients-sets.md) |
| D52 | [Documented port facts: units, labels, ranges](decisions/0052-port-facts.md) | accepted, amends [D27](decisions/0027-unit-suffixed-values.md) |
| D53 | [DayTimer schedules from source](decisions/0053-daytimer-schedules.md) | accepted, amended by [D69](decisions/0069-operating-modes.md) |
| D54 | [Minting virtual inputs](decisions/0054-virtual-inputs.md) | accepted, amended by [D64](decisions/0064-virtual-modbus-devices.md) and [D80](decisions/0080-v2-one-spelling.md) |
| D55 | [Device-narrowed extern matching](decisions/0055-device-narrowed-matching.md) | accepted, amended by [D71](decisions/0071-multi-object-statements.md) |
| D56 | [Creating pages from source](decisions/0056-creating-pages.md) | accepted |
| D57 | [Automatic Designer rules from source](decisions/0057-automatic-designer-rules.md) | accepted, amended by [D60](decisions/0060-creating-rules.md), [D67](decisions/0067-deleting-rules.md), [D73](decisions/0073-rule-conditions.md) and [D76](decisions/0076-weather-date-message-rules.md) |
| D58 | [Lighting moods from source](decisions/0058-lighting-moods.md) | accepted |
| D59 | [The language server is a lexical layer over the pipeline](decisions/0059-language-server.md) | accepted |
| D60 | [Creating Automatic Designer rules](decisions/0060-creating-rules.md) | accepted, amended by [D67](decisions/0067-deleting-rules.md) |
| D61 | [The app catalogue follows the compile](decisions/0061-app-catalogue.md) | accepted, amended by [D66](decisions/0066-app-blocks.md) and [D72](decisions/0072-light-controllers-catalogue.md) |
| D62 | [House control types, adopt-only devices, promoting an extern](decisions/0062-house-control-types.md) | accepted |
| D63 | [A managed block's room and category](decisions/0063-managed-room-category.md) | accepted |
| D64 | [Virtual and Modbus devices from source](decisions/0064-virtual-modbus-devices.md) | accepted |
| D65 | [Settings of physical inputs and outputs from source](decisions/0065-physical-io-settings.md) | accepted |
| D66 | [Blocks minted from source in the app](decisions/0066-app-blocks.md) | accepted |
| D67 | [Deleting a rule from source](decisions/0067-deleting-rules.md) | accepted |
| D68 | [`loxl test` runs in a simulator of its own](decisions/0068-own-simulator.md) | accepted, supersedes the simulator part of [D36](decisions/0036-tests-in-module.md) |
| D69 | [Operating modes from source](decisions/0069-operating-modes.md) | accepted |
| D70 | [Users stay the Miniserver's](decisions/0070-users-stay-miniserver.md) | accepted |
| D71 | [Several objects in one statement, device settings, Air serials](decisions/0071-multi-object-statements.md) | accepted |
| D72 | [Light controllers in the app catalogue](decisions/0072-light-controllers-catalogue.md) | accepted |
| D73 | [Rule conditions on states and modes, OR groups](decisions/0073-rule-conditions.md) | accepted, amended by [D76](decisions/0076-weather-date-message-rules.md) |
| D74 | [Calendar entries](decisions/0074-calendar-entries.md) | accepted |
| D75 | [A redacted copy](decisions/0075-redacted-copy.md) | accepted |
| D76 | [Weather, dates and messages in rules](decisions/0076-weather-date-message-rules.md) | accepted |
| D77 | [A stale constant under a wire](decisions/0077-stale-constant-under-wire.md) | accepted |
| D78 | [A block's own blocks stay with it](decisions/0078-owned-sub-blocks.md) | accepted |
| D79 | [A PulseAt's fire time from source; what the compile carries](decisions/0079-pulseat-fire-time.md) | accepted |
| D80 | [Language v2, one spelling per value](decisions/0080-v2-one-spelling.md) | accepted |
| D81 | [Named signals](decisions/0081-named-signals.md) | accepted |
| D82 | [Template instances are named by the instance](decisions/0082-template-instance-names.md) | accepted |
| D83 | [Access rights and recipients are named sets](decisions/0083-access-recipients-sets.md) | accepted |
| D84 | [A `page` statement governs its own file](decisions/0084-page-per-fragment.md) | accepted |
| D85 | [`defaults <Type>(…)` states a type's shared parameters once](decisions/0085-defaults.md) | accepted |
| D86 | [Contextual words, one-line calls and the v2 polish](decisions/0086-v2-polish.md) | accepted |
| D87 | [Tooling for v2: unit-suffix lint, lint --fix, scoped names in the language server](decisions/0087-v2-tooling.md) | accepted |
| D88 | [One documented line per port](decisions/0088-port-descriptions.md) | accepted |
| D89 | [What dead-outputs counts as used, and `# lint: allow`](decisions/0089-lint-dead-outputs.md) | accepted |
