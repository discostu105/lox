# Design

How the crate is put together, and why. The companion references are
[ir-spec.md](ir-spec.md) (the language), [lockfile-spec.md](lockfile-spec.md)
(the state file), and [loxone-format.md](loxone-format.md) (the target
format's validated facts).

## Layering

```
            ┌───────────────────────────────────────────┐
            │  ir: Module ── compile ──► LoxoneDoc      │  semantics
            │            ◄─ decompile ─                 │
            │  lock: Lockfile (identity between runs)   │
            ├───────────────────────────────────────────┤
            │  doc: objects / ports / wires / counters  │  read views
            │  connectors: builtin table + observe()    │
            │  diff: semantic comparison                 │
            ├───────────────────────────────────────────┤
            │  xml: lossless CST, byte-faithful writer  │  syntax
            └───────────────────────────────────────────┘
```

- **`xml`** stores the concrete syntax losslessly (attribute order, raw
  escaping, self-closing flags, BOM, declaration). Everything above mutates
  *through* this tree, so anything the upper layers don't understand
  round-trips untouched.
- **`doc`** is a read-only *view* layer: it never caches, always derives from
  the tree, so views cannot go stale after mutations.
- **`ir` + `lock`** hold all write semantics. `compile` is the only function
  that performs coordinated mutations.

## The ownership model

A config file has **three writers**: Loxone Config (the GUI), the Miniserver
itself (app-created autopilot rules, device registrations), and this
compiler. The compiler therefore claims ownership of exactly four kinds of
edit, and records each in the lockfile:

1. **Managed blocks** (`slug = Type(…)` declarations) — created, rebuilt,
   and deleted by the compiler alone.
2. **Extern wires** (`target.Port <- source.Port`) — `<In>` elements the
   compiler added to ports of objects it does *not* own. Recorded in
   `lock.extern_wires` so they can be removed again without touching wires
   drawn in the GUI.
3. **Extern sets** (`target.Port = value`) — `Def=` values the compiler
   rewrote on extern ports. The pre-set value is recorded in
   `lock.set_originals` and restored when the assignment disappears from
   source.
4. **Attribute claims** (`target.title|room|category = value`) — the
   title, or the room/category binding (`<IoData Pr=/Cr=>`), of an
   *extern* object: devices and their inputs/outputs, which only Loxone
   Config can create. Original, written value and the followers a
   claim carried along (a room/category claim's descendants, a title
   claim's refs, D43) are recorded in `lock.attr_claims` and restored
   when the claim disappears (D39).

Three declarations write GUI content without recording it: the tile
programs of a Touch Pure Flex (D41), the access rights on an NFC Code
Touch (D49) and the recipients of a push notification (D51). They own what they declare while they declare it; removed,
they leave the last written state in place.

Everything else is somebody else's, and passes through unchanged. One
piece of a managed block is shared: its position on the page. The GUI may
drag it, and compile takes the drag over (D42).

## The compile strategy: tear down, rebuild

Each compile:

1. Validates the module; refuses if the lock knows a managed slug the source
   lost (see *Removal trichotomy* below).
2. Resolves externs (lock pin first, then match spec; ambiguity is an error).
3. **Tears down** its previous output from the base document: removes all
   locked managed objects, removes all locked extern wires, restores all
   recorded set-originals and claimed attributes. What remains is exactly
   the state owned by the other two writers.
4. **Rebuilds** from source: plans port lists, mints missing UUIDs, emits
   managed `<C>` elements, applies wires and sets, records everything in the
   lock, and places the blocks that have no position yet (D42).
5. Writes counters (never decreasing) and target metadata.

This makes compilation *convergent*: compiling the compiler's own output
again is a byte-level fixpoint (tested in `tests/ir.rs`), which is what makes
the real workflow — compile → upload → Config saves → download → compile —
safe. Where a save reorders a connector's `<In>`, the rebuild follows the
saved order (D45).

## Identity and determinism

- Object identity is the **UUID**, never the title (titles are
  locale-volatile — see [loxone-format.md](loxone-format.md)). Externs
  resolve by `uuid` > `iname` > `title`, and the winning UUID is pinned in
  the lock; later compiles use the pin even if the title has since changed.
- New UUIDs come from a **deterministic minter**: creation time and machine
  id are caller inputs; per-block port entities are `sha256(slug)[..6]`;
  sequence counters advance per mint. No clock, no RNG anywhere in the
  library. Within a run, determinism comes from the minter; across runs,
  from the lockfile (minted UUIDs are recorded immediately and reused
  forever).
- Result: same base + module + lock + options → **same output bytes**.
  Different mint time with the same lock → still the same bytes.

## The removal trichotomy

A managed slug present in the lock but missing from source is ambiguous
(typo? intentional delete?), so the compiler refuses by default. The
explicit resolutions — the in-language statements are preferred because
they are scoped and show up in the PR diff (D13):

| Intent | Mechanism | Effect on config | Effect on lock |
|---|---|---|---|
| oops, typo | fix the source | — | — |
| delete it | `removed <slug>` in source (or the global `allow_removals` / `--allow-removals`) | block removed | entry dropped |
| stop managing it | `Lockfile::remove_object` | block **stays** (orphan) | entry dropped |
| rename the slug | `moved <old> -> <new>` in source (or `Lockfile::rename_object`) | identity survives | key renamed |

## Refuse, never guess

Anything unverified is an error, not a heuristic:

- Only block types in the **verified builtin table** (`connectors::builtin`)
  can be minted. The two pre-existing connector databases (lox-cli's map,
  lox-sim's signatures) contradict each other for many types; only entries
  confirmed against real configs are admitted. Growth path: `lxir observe`
  gathers evidence → cross-check → verify live → extend the table.
- Wiring or `set`ting an extern port whose `<Co>` is absent from the base
  config is an error (the compiler will not invent port UUIDs for types
  whose connector indexes it cannot verify). The error lists the ports that
  *do* exist.
- Extern resolution with zero or multiple matches is an error listing the
  candidates.

## Decisions and rationale (ADR-style)

- **D1 — Byte-faithful CST, not a DOM.** A conforming XML parser rejects or
  corrupts real `.Loxone` files (digit-leading attribute names, raw newlines
  in attribute values). Hand-rolled parser + canonical writer; verified
  byte-identical on six real configs. This is the crate's foundation — a
  DOM-level writer (like lox-cli's) cannot prove it changed nothing else.
- **D2 — The lockfile pins *port* UUIDs, not just object UUIDs.** Wires
  reference port UUIDs; if ports re-minted on each compile, every compile
  would rewrite every wire.
- **D3 — Ports are minted before their object.** Matches the counter order
  observed in real Loxone Config output (`…0749, 074a, 074b` ports, then
  `…074c` object).
- **D4 — Managed blocks emit a `<Co>` for every connector**, with
  `Nio` = connector count. Verified live: Loxone emits all connectors, even
  unwired outputs.
- **D5 — `Nc` is maintained as the count of `<In>` children**, omitted at
  zero; `<Co>` attribute order is `K, Nc, Def, U`. Verified live.
- **D6 — Removal is a trichotomy** (see above), defaulting to refusal.
- **D7 — `decompile` lifts only wires that touch a managed block.** A wire
  between two externs — even one the compiler itself created — belongs to
  the config, not the IR view. `decompile(compile(m))` is therefore a
  faithful *subset* of `m`, not an inverse. (Since D17 this describes the
  `ManagedOnly` scope; the default full view also shows wires between
  lifted extern page objects.)
- **D8 — Gate inputs are fixed: `And`/`Or` have exactly `I1`, `I2`, `Q`,
  and wiring `I3`+ is a compile error.** The original assumption (grown
  inputs take indexes after the builtin ports) was **refuted by the Wine
  oracle** (2026-08-25): Loxone Config 17 offers no way to grow a gate —
  wiring the last free input does not add one, the wire-drop connector
  picker lists only existing inputs — and a compiled `I3` at index 3
  *loads*, but **saving silently deletes the off-descriptor connector and
  its wire** while preserving everything else. Silent logic loss is the
  worst outcome a compiler can produce, so the compiler refuses `I3`+ with
  a hint to cascade 2-input gates. The corpus concurs: zero gates with
  more than two inputs across six configs
  ([oracle-wine.md](oracle-wine.md)).
- **D9 — Transport is out of scope.** The library is pure (bytes → bytes);
  FTP/LoxCC/credentials live in `lox` / `lox-cli`.
- **D10 — All comments survive the round trip**, so `lxir fmt` is
  non-destructive: whole-line comments are AST items, trailing comments
  attach to their statement, argument line, or closing delimiter (v0: `}`;
  v1: `)`), and comments inside argument lists are argument items.
  (Originally `} # text` was canonicalized onto its own line; it now stays
  attached to the closing delimiter — detaching a comment from its anchor
  lost intent for no gain.)
- **D11 — Counters (`NextObj`) advance by one per minted managed object**
  and never decrease (`Lockfile::absorb_counters` takes the max of lock and
  document). Whether ports also consume `NextObj` is unknown; object-only is
  the conservative reading of observed files. Oracle observation
  (2026-08-24): Loxone Config itself burns **+2** per open+save cycle even
  with zero objects added — counter consumption is not tied to persisted
  objects, and max() absorbs it safely
  ([oracle-wine.md](oracle-wine.md)).
- **D12 — A dedicated grammar, not a YAML/KDL/HCL host syntax.** Weighed in
  the sketch phase: YAML would have been the fastest start, but wiring
  expressed in YAML is exactly the ergonomics this project exists to remove,
  and the planned sugar (expressions, templates — see
  [roadmap.md](roadmap.md)) wants first-class syntax. The grammar stays
  small enough to hand-parse ([ir-spec.md](ir-spec.md)).
- **D13 — Lifecycle lives in the language: `removed` and `moved`
  statements** (2026-08-25). The original design routed deletion through a
  CLI flag and rename through a library call — out-of-band state surgery
  that never appears in a reviewable diff, the exact gap Terraform closed
  by moving `state rm`/`state mv` into `removed`/`moved` blocks. lxir's
  whole pitch is "every change is a one-line diff in a PR", so intent to
  delete or rename must be expressible in source: `removed <slug>` is
  scoped to one block (unlike `--allow-removals`, which authorizes *all*
  removals in a run), `moved <old> -> <new>` keeps object and port UUIDs
  across a slug rename, and both are idempotent no-ops once applied so
  history can keep them. The flag and library calls remain as escape
  hatches.
- **D14 — `set` is for extern ports only; managed parameters live in the
  block body** (2026-08-25). v0 allowed `set` on managed blocks as a body
  synonym with an override rule — two spellings for one thing, and a keyword
  with different ownership semantics per target (tracked-and-reverting on
  externs, plain param on blocks). One meaning per keyword: `set` now
  always means "tracked write to somebody else's port", and the validator
  points managed-block sets at the body. (v1 keeps the semantics and
  renames the spellings: the body became the argument list, `set` became
  plain port assignment — D16.)
- **D15 — Values are typed tokens, not sniffed strings** (2026-08-25). The
  AST records how a value was written (`Number` | `Str` | `Ref`), and the
  formatter emits by variant. The previous content-sniffing emitter printed
  string values like `"5+"` as bare tokens that did not re-parse, breaking
  the advertised `parse(to_text(m)) == m` fixpoint. One canonicalization
  remains, at parse time: a quoted string that reads exactly as a number
  becomes the bare number. Bare identifiers in value position are `let`
  references and must resolve — the previously undefined
  bare-ident-as-string lenience is gone.
- **D16 — Constructor syntax: a block's inputs live at its declaration**
  (2026-08-25, v1). v0 scattered a block's behavior across three statement
  forms: `block` gave the type, a `{ … }` body the parameters, and `wire`
  lines elsewhere the inputs. v1 unifies them into one call-shaped
  declaration — `slug = Type("Label", Port: value, Port: source.Q, …)` —
  where the argument's value decides its meaning (literal/constant →
  `Def=` parameter; port reference → wire), mirroring the fact that both
  target the same `<Co>` in the XML. Wires onto extern ports keep a
  statement form (`target.Port <- source.Port`) because externs are not
  constructed, and `set` collapses into plain port assignment
  (`target.Port = value`). Every fact still has exactly one spelling:
  managed sinks bind only in the argument list (`<-`/`=` on a managed slug
  is an error pointing there), and the canonical formatter emits one
  argument per line, so diff granularity stays one-line-per-fact.
  Alternatives weighed and rejected: an expression language
  (`node x = a >= 28 and sonne`) creates anonymous intermediate blocks
  whose path-based identity breaks the lockfile's rename stability
  (Terraform's `count` lesson), and a general-purpose host language
  (Starlark) belongs *above* the IR as a generator, not in place of it —
  the flat declarative form remains the total representation every
  hand-drawn config can decompile into. Names stay mandatory, so the
  lockfile, `moved`/`removed`, and the compile strategy are untouched; the
  v1 example compiles byte-identically to its v0 counterpart.
- **D17 — Decompile is a view, grouped by page** (2026-08-25). The first
  decompile of a real 19-page house config exposed the gap: only
  builtin-type blocks and their direct neighbors were lifted (23 of ~1900
  objects), in one flat module, with the "1840 raw objects untouched"
  report easy to miss on stderr. `lxir decompile` now defaults to a *full
  view*: every page object with connectors is lifted (managed types as
  block declarations, everything else as `extern`s), every wire between
  lifted objects is shown, and output is grouped by logic page —
  `# page:` sections in the single module, or one module per page via
  `--out-dir`, where objects a page references but does not contain
  become externs annotated with their origin. The view is for reading,
  not compiling: compiling it against the same base would mint duplicates
  of the managed blocks and claim ownership of every wire, so the
  compilable-shaped adoption subset remains available as `--managed-only`
  (`DecompileScope::ManagedOnly`, the D7 behavior). Honest limits, all
  counted in the report: extern `Def=` parameters are never lifted (a
  `target.Port = value` line would claim ownership of the value),
  periphery-to-periphery wiring stays raw (the view covers page logic,
  not the device tree), and objects whose type or port keys are not
  language identifiers (Loxone's numeric type ids) stay raw. Slugs form
  one namespace across the whole document — identical in the single and
  per-page views — and statement keywords are never handed out as slugs.
- **D18 — Adoption is decompile-with-lock, verified per block**
  (2026-08-25). `lxir adopt <cfg>` moves existing managed-type blocks
  under source control by pairing the `--managed-only` view with a fresh
  lockfile that pins each block's *existing* identity — object UUID, port
  UUIDs, layout, and page — so the first compile rebuilds the blocks in
  place instead of minting duplicates (acceptance: adopt → compile →
  semantically empty diff against the real house config; recompiling that
  output is a byte-identical fixpoint). Identity comes from the lift, not
  from matching: title-based adoption is ambiguous in real configs
  (duplicate titles like "O1657"×4 observed). Consequences that fell out
  of making the rebuild faithful:
  - **Page pinning** (`LockedObject.page_uuid`): without it, every
    adopted block would be re-emitted onto the *one* compile-options
    page. Every managed block is now pinned to a page — adopted blocks to
    the page they were drawn on, new blocks to the options' page on first
    compile. Old locks without the field keep working (the next compile
    fills it in).
  - **Attribute parameters**: some block logic lives in element
    attributes, not connectors — `Formula="I1+I2"` on Formula blocks. A
    rebuild that dropped it would silently destroy logic. These are now
    language surface (`Formula: "I1+I2"` binds like a parameter, never a
    wire), declared per type in `connectors::attr_params`, emitted with
    the observed `Valid="false"` companion, lifted by decompile, and
    compared by `lxir diff` (which was blind to element attributes).
  - **Verification, not translation**: adopt whitelists exactly what the
    rebuild emits (plus known normalizations: `WF=` → the value Loxone
    Config itself normalizes to on save, `LtE=`/color are display state,
    `<Co>` element order is cosmetic — the port-UUID index tails prove
    spec order, the GUI just serializes in canvas order sometimes). A
    block failing verification is *skipped, not translated*: it stays
    unmanaged, re-enters as a pinned extern where adopted logic touches
    it, and the reason lands in `AdoptReport::refused` — one bespoke GUI
    flag (the real config's `Inv=` input inversion on a PulseGen) must
    not block adopting the other 22 blocks. All-or-nothing was rejected
    for exactly that reason.
  - The real-config run also falsified "empty elements in Loxone output
    are always self-closing" (`<IoData></IoData>` exists): the compiler's
    blanket empty-element fixup rewrote elements it never touched, and is
    now scoped to elements *our removals* emptied.
  Adopt never modifies the config and refuses to overwrite existing
  outputs. The per-object incremental form (`lxir adopt <uuid> --as
  <slug>`) remains future work.
- **D19 — GUI-owned residue is carried forward from the base, not
  refused and not snapshotted** (2026-08-25). Real GUI-created blocks
  carry content the compiler does not model: display/visualization
  attributes (`Tp=`, `Sun=`, `SpStates=`) and child elements
  (`<IoData>` — which holds the room/category binding `Pr=`/`Cr=` —
  `<Display>`, `<PSD>`, `<COHist>`). Under D18's strict whitelist this
  content blocked adoption of 20 of the house's 43 managed-type blocks
  and caused compile/save churn on freshly minted types (the GUI
  schema-heals the missing children on every save; the next compile
  deleted them again). Decision: when the compiler rebuilds a block that
  already exists in the base config, it **harvests an allowlisted set of
  attributes and child elements from the base element and re-emits them
  verbatim** — same values, same order, same self-closing form. Freshly
  minted blocks get the defaults; after their first GUI save the healed
  content is simply carried forward, so the churn converges to zero.
  - **Carry-forward, not lockfile snapshot** (rejected alternative):
    storing the residue in the lock would silently revert later GUI
    edits to it (stale copy wins). The GUI owns this content, and owner
    state is read from the config — the same principle externs follow.
  - `Cl=`/`LtE=`/`WF=` join the carried set: the fixed defaults the
    compiler previously wrote would have repainted adopted non-green
    blocks (Memory is grey, AutoJalousie blue in the real config — the
    first 22 adopted blocks were all default green, hiding the bug) and
    fought the GUI over `WF` (dropped entirely on some types, rewritten
    on others). Absent-in-base now stays absent in the rebuild.
  - The **allowlist is the boundary of faithfulness**: content is carried
    only if re-emitting it verbatim cannot contradict what the source
    expresses. `Inv=` (input inversion on a `<Co>`) fails that test *as
    plain residue* — it silently inverts a wire the source declares
    un-inverted — and originally refused adoption; D20 later carried it
    by making the whole connector GUI-owned. Unknown attributes/children
    still refuse (refuse, never guess); growing the allowlist takes
    evidence, not pattern-matching.
  - **`FLG=` on `<In>` wires joined the carried set** (2026-08-25, same
    day): Miniserver/app-created wire metadata (113 of the house's 880
    wires, mostly API-connector and central-alarm distributions). The
    oracle probe — strip the flag from two wires, open + save — showed
    Loxone Config treats it as inert stored state: it round-trips the
    flag verbatim, never regenerates it, and accepts its absence without
    repair or wire loss. It passes the faithfulness test because it
    decorates a wire the source *does* declare (unlike `Inv=`, which
    contradicts one); it is keyed by (sink port, source port), so a wire
    whose source changes in the module is emitted plain — exactly the
    state the probe validated. Evidence: [oracle-wine.md](oracle-wine.md).
- **D20 — An `Inv=`-carrying connector is wholly GUI-owned**
  (2026-08-25). The GUI's input-inversion flag looked like D19's
  permanent boundary: carrying it as plain residue could contradict the
  source (a declared wire into an inverted connector silently means its
  negation). But the house evidence reframed it — **every one of the 23
  LightController2 instances** carries `Inv="true"` on its *unwired*
  `Remanence`, i.e. the GUI encodes an enabled Remanenz checkbox by
  inverting the unwired (constant-0) input so the block reads 1;
  dropping the flag on rebuild would silently disable state retention
  across reboots. Decision: a connector carrying
  `Inv=` becomes GUI-owned as a whole. The rebuild re-emits its entire
  `<Co>` element verbatim — flag, `Def=`, and `<In>` wires included —
  and the contradiction is eliminated by construction: the compiler
  **hard-errors** when the source tries to wire or set such a connector
  (both managed and extern sinks), and decompile/adopt keep its `Def=`
  and wires out of the lifted source. The faithfulness rule is
  unchanged; what changed is the granularity of ownership — like an
  `AutopilotRule` the GUI owns whole, an inverted connector is a unit
  the IR references around, never through. This removed the last 31
  refusals in the house config (23 LightController2, 5 PushButton, both
  DayTimers, one PulseGen): **all 100 managed-type blocks adopt**.
  - Rejected alternative — model inversion in the language (`!source.Q`
    or `inv:` markers): it would make lxir a *second writer* of a flag
    the GUI edits via checkbox, exactly the ownership conflict externs
    exist to avoid, for a feature better expressed by an explicit `Not`
    block when the logic is lxir-authored.
- **D21 — Drift baseline = a fingerprint of the diff's own projection**
  (2026-08-25). "Did another writer change something since my last
  adopt/compile?" should not require keeping the last compiled output
  around, and must not false-alarm on what a GUI save touches anyway
  (element positions, visualization residue, the save fingerprint,
  locale renames of built-ins). Decision: `semantic_fingerprint(doc)`
  hashes **exactly the projection `diff` compares** — objects by UUID
  with type and title, `Def=`-carrying ports, attribute parameters, the
  wire set — and lives next to `diff` in the same module so the two
  cannot drift apart; locale-suspect titles are left out, so two
  documents are fingerprint-equal iff their diff is empty apart from
  locale-suspect renames. Adopt and compile record it in the lock
  (`target.semantic_fingerprint`); `lxir drift <cfg> --lock <lock>`
  answers from one parse of one file, with `lxir diff` remaining the
  tool that says *what* changed. Corpus-enforced: every adopted rebuild
  must reproduce the fingerprint recorded at adoption bit-for-bit.
- **D22 — ConfigVersion is a qualification pin, not metadata**
  (2026-08-25). Every Loxone Config release may change descriptors,
  schema migrations, and save behavior — the whole builtin table and
  residue model are *validated against a specific release* (currently
  17.1, oracle sessions 1–9). So the lock's `target.config_version` is
  enforced: compiling against a base written by a different release is
  a hard error until the release is qualified (one oracle open+save
  run of a rebuilt config) and accepted with `--accept-version <v>`,
  which must equal the base's version exactly. Acceptance is
  per-compile and deliberate; the lock then pins the new version.
  Adoption pins whatever release wrote the adopted config — the first
  compile against the same file always passes.
- **D23 — Templates are macro expansion with locked identities**
  (2026-08-25, Stufe 1). The sketch's `template` + `use` (v0 braces)
  becomes, in v1 grammar: `template <name>(<params>)` … `end` declares a
  reusable body of block/wire/assignment statements, and
  `<slug> = <name>(<param>: <arg>, …)` instantiates it — the lowercase
  callee distinguishes instantiation from a block declaration, because a
  template instance *is* a composite block. Parameters are either object
  parameters (`jalousie: AutoJalousie` — the instance passes an extern
  or block slug; the annotation is checked when the slug's declared
  type is known) or value parameters with a default (`pos = 70`; the
  instance may override with a literal or a `let` reference). Expansion
  is a pure source-to-source pass before compilation: body slug `b` in
  instance `sued` becomes `sued_b`, object parameters substitute to the
  passed slugs, value parameters substitute like shadowing `let`s, and
  free identifiers resolve in the module namespace after expansion — a
  template may capture module externs (`aussentemp`, `wind_alarm`).
  Identity: the lockfile keys the *expanded* slugs, so `sued_hoch` is
  pinned exactly like a hand-written block — re-instantiating never
  re-mints, editing a template body mints only what it adds, and the
  existing lifecycle statements apply unchanged (`removed sued_alt` per
  instance when the body drops a block, `moved` to rename). Other
  statements reference an instance's blocks by their expanded names;
  the instance slug itself names no object. Nothing downstream knows
  templates exist: compile, lockfile, diff, drift, and the oracle all
  see plain blocks, so no new format risk and no oracle run is needed.
  - Rejected alternatives: brace-delimited bodies (v0 syntax that v1
    deliberately removed) and indentation-sensitive bodies (the lexer
    is whitespace-agnostic everywhere else; `end` keeps the grammar
    line-oriented). A dedicated `use`/`instance` keyword lost to the
    block-declaration form — one call syntax, distinguished by case.
    Nested templates and template-local `let`/`extern` are deferred,
    not rejected.
  - Port forwarding (2026-08-25): a call-site binding naming no template
    parameter forwards verbatim as a port binding onto the body's single
    block — the instance call reads exactly like a block declaration
    with the shared parameters factored away, which is what "an instance
    *is* a composite block" promises. Needed because per-instance feeds
    are heterogeneous in the corpus (r50's AutoJalousie rows carry 0–2
    `EndUp` sources of mixed types plus reverse-recorded `OutputAPI`
    entries), so neither typed object parameters nor module-level wires
    (`<-` targets extern ports only) can express them. Forwards may
    repeat like block feeds; validation of the refs happens on the
    expanded module (the fragment loader now validates the expanded
    view, matching compile — pre-expansion validation wrongly rejected
    references to expanded names). Multi-block bodies take no forwards;
    a qualified `<body_slug>.<Port>` form is deferred.
  - Title interpolation (2026-08-25): body titles may embed `{param}`
    placeholders that substitute a string value parameter at expansion —
    without this, every instance of a template shares one app-visible
    title, which ruled templates out for real configs (r50's 16
    AutoJalousie blocks all carry distinct titles). Interpolation lives
    entirely in the expansion pass — grammar, AST, fmt, and everything
    downstream are untouched, and a placeholder naming no value
    parameter is an error (typo guard) while non-slug braces pass
    verbatim. Rejected: a `Value`-typed label position in the grammar
    (ripples through parser/fmt/decompile for no added power).

- **D24 — Expressions are sugar over the discrete blocks, owned by their
  statement** (2026-08-25). The sketch's
  `jal.AutoShade = sonne.Q and (aussentemp.AQ >= 28)` becomes, in v1
  grammar, an expression on the wire statement:
  `jal_sued.AutoShade <- sonne.Q and aussentemp.Q >= schwelle`. `<-`
  stays the one wiring operator (`=` on a port remains the `Def=`
  write); a bare `slug.Port` RHS is a plain wire, anything more
  desugars — before compile, like template expansion — into the
  live-verified discrete blocks: `and`/`or` → `And`/`Or` (fixed
  2-input per D8, chains cascade left-associatively), `not` → `Not`,
  comparisons → the comparator family, with operand order preserved
  (lhs → `Input1`, rhs → `Input2`), constants becoming `Def=`
  parameters and ports becoming wires. Each generated block's label is
  its sub-expression text, so the rule stays readable on the Config
  canvas — the point of the discrete backend. Precedence `or` < `and` <
  `not` < comparison; parens group; comparisons take plain operands and
  do not chain. `and`/`or`/`not` join the reserved words.
  Identity: synthetic slugs are `<sink>_<port>__<op><n>` (post-order,
  per-operator counter), keyed in the lockfile like hand-written
  blocks but marked `expr_owned`. An unchanged expression therefore
  never re-mints; an edited one re-derives its slugs, and the compiler
  auto-removes the orphaned `expr_owned` entries — no `removed`
  statement, because no hand ever wrote those blocks and the
  expression is their single source of truth. A hand-written slug
  colliding with a synthetic name is an error naming the expression.
  Templates compose for free: desugaring runs after expansion, so a
  body expression's sink prefix uses the instance's actual extern.
  This work also surfaced (and fixed) a latent mint hazard: the
  minter's per-run counter restarts at 0, so with an identical mint
  time a block added in a *later* compile session could reuse a
  (time, sequence) pair from the first — an object-UUID collision.
  Compiles now seed the minter past the lock's recorded high-water
  mark (`counters.next_mint`) and every locked UUID's sequence.
  - Rejected alternatives: the `formula` backend (one `Formula` block
    per expression — compact but opaque in the canvas and capped at
    four inputs) is deferred, not rejected; expressions in block
    argument lists and parenthesized comparison operands are deferred
    until a use case demands them; content-hash slugs (stable across
    edits, but unreadable and leaking into the canvas) lost to the
    positional scheme, accepting that an edit re-mints the edited
    expression's nodes — a self-contained blast radius.

- **D25 — Projects are a `lox.toml`, and there is no `import` statement**
  (2026-08-25). A directory with a `lox.toml` is a project — one
  deployment target: `base` (the deployed config) and `module` (file or
  fragment directory) required, `lock`/`out` defaulted, `serial`/`page`
  optional, all in flat `key = "value"` lines with `#` comments, paths
  relative to the file. `lxir compile` inside the directory needs no
  flags (flags override the file); `check`/`fmt`/`drift` default to the
  project's module and lock. Module directories now search recursively,
  so the sketch's Stufe-4 layout (`externals.lxir` beside `rooms/`,
  `systems/`, `templates/`) is fully expressible.
  The considered `import` statement is rejected, not deferred: fragments
  of a module share one namespace and one lockfile, and the compiler
  merges everything into the one `.Loxone` document — an import would
  declare a dependency with no semantic consequence, pure ceremony on
  every file. If a future multi-project or namespacing need arises, it
  gets its own decision; nothing in today's format precludes it.
  Implementation choices: the file is parsed by ~60 lines of strict
  subset parser instead of a `toml` crate — six string keys do not
  justify three transitive dependencies, and refusing tables, arrays,
  and unquoted values with pointed errors keeps the format honest
  (better a small language parsed exactly than a big one parsed
  approximately). `module` deliberately has no default: defaulting to
  `.` would sweep stray `.lxir` files (decompile views, backups) into a
  compile. The serial is validated at parse time so a typo fails before
  any file is read.
- **D26 — Expressions bind in argument lists too** (2026-08-25). D24
  allowed a boolean expression only on the RHS of `<-` (extern sinks),
  so `ext.Port <- a.Q and b.Q` worked while
  `x = Monoflop(InputTrigger: a.Q and b.Q)` did not — a rule that
  existed only because D24 shipped first. The original D16 objection to
  expressions (anonymous intermediates break lockfile identity) was
  already answered by D24's deterministic synthetic slugs and
  expression-owned lock entries, and `<sink>_<port>__<op><n>` extends
  naturally when the sink is a managed block's port. An argument
  binding's value may now be an expression; it desugars through exactly
  the D24 machinery, and the binding becomes a plain wire from the
  expression root's `Q`. A bare `slug.Port` (parenthesized or not)
  stays a wire binding and a bare value a parameter binding — one
  spelling per fact. Synthetic-slug counters persist across the
  module's expressions per (sink, operator), so two expressions fanning
  into the same port number on instead of colliding.
- **D27 — Unit-suffixed values, scaled at compile, no port checking**
  (2026-08-25). `Time: 90min`, `TimeHigh: 250ms`, `DayMinTemp: 2700K`,
  `TargetPos: 70%` — a number may carry a unit suffix, written
  immediately adjacent. Time units (`ms`/`s`/`min`/`h`) scale exactly
  (decimal integer arithmetic, no floats) into Loxone's base unit,
  seconds; `K` and `%` are annotations with factor 1. The suffix is the
  value's canonical spelling (`1.5h` stays `1.5h` through `fmt`) and
  the compiled `Def=` is byte-identical to writing the plain number —
  tested. Deliberately *not* included: per-port unit checking (is
  `Time:` really seconds? is `Dir:` degrees?) — that needs the
  connector DB to learn per-port units with evidence, so a wrong unit
  today is accepted like a wrong plain number, and the roadmap keeps
  the checking entry. A quoted string that looks like a unit value
  (`"40s"`) stays a string; decompile keeps lifting plain numbers (it
  cannot know a port's unit without that same evidence).
- **D28 — `page "Title"`: placement is source, and it is authoritative**
  (2026-08-25). Which page a block is drawn on was the one piece of a
  block's fate invisible in the language — a compile option plus a
  lockfile pin, reviewable in neither. A `page` statement names the base
  page (by display title) for the block declarations that follow it,
  until the next `page` statement; blocks above the first one keep the
  `--page`/project default, so existing modules compile unchanged.
  Semantics are positional — expression-desugared blocks land on the
  page their expression is written under, template-expanded blocks on
  the page of their instantiation (`page` itself is not allowed in
  template bodies: placement belongs to the module). The statement is
  authoritative on every compile: a pin that still matches a page with
  the declared title is kept (titles need not be unique — this is what
  keeps adopted output byte-faithful, verified across the corpus), any
  other pin moves to the first matching page in document order, and a
  missing title is an error. Creating pages was left with Loxone Config
  until D56 (`page "Title" (room: …)`).
  The decompiler now emits real `page` statements as section headers
  (per-page modules and fragments open with theirs; the periphery, not
  being a page, keeps its comment), so decompile/adopt output states
  its placement instead of hinting at it. This makes fragment
  concatenation order observable for the first time — documented in the
  spec with the convention (every fragment opens with its `page`
  statement) that neutralizes it.
- **D29 — The full view optimizes for the reader; only the adoptable
  view owes the compiler** (2026-08-25). The two decompile scopes have
  different contracts — `ManagedOnly` output must recompile
  byte-identically, the `Full` view is documentation — so they earn
  different liberties. Three readability changes, all full-view only:
  (1) `InputRef`/`OutputRef` plumbing folds. A ref's `Ref=` attribute
  already names the object it mirrors; the wires between the two are GUI
  routing, not logic. They vanish from the view, the ref's extern
  declaration gains `# mirrors <Type> "<name>"`, and a periphery object
  whose only connection was plumbing is never pulled in as an extern.
  (2) The `<-` pile sorts by (sink slug, sink port, source slug, source
  port) — big fan-ins read as a table instead of canvas order. Not
  possible in the adoptable view: the relative `<In>` order within a
  sink connector is part of the compiled bytes. (3) A block label the
  slug already encodes (`slugify(title) == slug`, e.g. `"Temp über 28"`
  → `temp_ueber_28`) is dropped; the adoptable view keeps every label
  because the rebuild writes `Title=` back exactly. The report counts
  folded wires honestly (`ref_wires_folded`, printed by the CLI). On the
  real house config the view shrank from 2667 to 1818 lines — 486
  plumbing wires folded and 278 of 674 externs turned out to exist only
  to feed refs. Corpus adoption fidelity verified unchanged.
- **D30 — Corpus-observed defaults: elide what the GUI wrote, show what
  a human chose** (2026-08-25). Loxone Config writes a `Def=` for nearly
  every port at block placement — at the GUI default — so a decompiled
  block drowns its two configured values in twenty boilerplate ones. The
  observed default for a (type, port) is defined statistically:
  the modal `Def=` value across the local corpus, accepted at ≥90% share
  and ≥10 occurrences. `tools/extract-defaults.py` writes the full
  evidence (every candidate, share, counts, the rejected tail) to
  docs/data/param-defaults.json and generates src/observed_defaults.rs
  restricted to the builtin types — the only ones whose parameters the
  decompiler lifts. The full view elides a lifted parameter whose value
  equals the observed default; `--all-params` shows everything, and the
  report counts elisions honestly. The adoptable ManagedOnly view never
  elides — rebuilding writes the exact `Def=` back. This is the same
  boundary as D29: corpus statistics clear the bar for *view-only*
  knowledge because a wrong entry can hide a value from a reading human
  but can never change what compiles; the connector DB proper (what the
  compiler may *write*) still demands live verification. Real config:
  486 parameters elided; with D28+D29 the full view halved, 2667 → 1332
  lines. Minted blocks already write no `Def=` for unbound parameters,
  so the two conventions agree: absence means default, on both sides.
- **D31 — Removal tombstones: withdrawals survive until the base catches
  up** (2026-08-25, found in the field). A removal used to *forget*: the
  transition compile deleted the block and dropped its lock entry — and
  from that moment, any recompile against the still-undeployed base
  treated the physical object as foreign and passed it through unmanaged.
  Observed on the real house config: the recompiled artifact carried both
  the old gate cascade and its expression-owned replacement, with the
  alarm input double-driven — an artifact that must never be pushed, in
  exactly the window where CI recompiles it. The same hole existed for
  the other two withdrawal kinds: an extern wire gone from source
  (`extern_wires` forgets it, the old base still has it) and a `set`
  gone from source (`set_originals` restores once, then forgets; the old
  base still carries our written value). The fix is one rule applied to
  all three: **everything the compiler withdraws leaves a lockfile
  tombstone** (`removed` by object UUID, `removed_wires`, `removed_sets`
  with the original *and* the written value as the recognition marker) —
  provided the base actually carried it. Every compile re-applies pending
  withdrawals (delete the object, delete the wire, restore the original)
  and retires a tombstone the moment a base without the withdrawn
  artifact is seen — deployment is *witnessed*, never assumed. A
  `removed_sets` port showing a third value means another writer took the
  port over: the tombstone retires and drift surfaces it. Consequences:
  the compile → push → download window is a lock fixpoint (committable,
  CI-green, byte-reproducible); a lingering `removed` statement flips
  from tolerated to hard error once its tombstone retires — the ratchet
  that gets the transient statement cleaned up; `Lockfile::remove_object`
  keeps its orphan semantics (no tombstone) as the deliberate escape
  hatch. Lockfile format bumped to v2 so pre-tombstone binaries refuse
  the new locks instead of silently dropping the tombstones.
- **D32 — `mirrors:` — a ref matched by what it mirrors** (2026-08-25).
  The r50 slug cure surfaced 113 uuid-pinned `InputRef`/`OutputRef`
  externs whose pins say nothing a reader can check. Resolving every one
  against the live base yielded the format fact that makes a semantic
  matcher possible: a ref's `Ref=` attribute names the mirrored
  *object's* UUID (a periphery subdevice, a flag, or a managed block),
  with the actual signal carried by ordinary plumbing wires on top
  ([loxone-format.md](loxone-format.md)). So `extern status_alarm_ref =
  InputRef(mirrors: status_alarm)` finds the ref through its target —
  where the target is nameable: a managed block with a locked identity
  or a plain-matched extern. Duplicate refs of one target usually sit on
  different pages, so the declaring file's `page` statement (D28)
  narrows candidates; a still-ambiguous match is refused per
  refuse-never-guess (two mirrors of one flag on one page keep their
  `uuid:` pins). Two deliberate asymmetries against the other matchers:
  a matcher-*kind* change on a pinned extern must confirm the pinned
  object before the lock records the new kind (converting a pin to
  `mirrors:` is thereby *verified*, not taken on faith), and a pinned
  `mirrors:` re-confirms on every compile — a title may drift under its
  pin, but "this ref mirrors X" is a source claim that must stay true,
  and a ref the GUI re-pointed elsewhere is drift worth stopping on.
  Decompile/adopt emit `mirrors:` where the target has a slug in the
  module and the ref is its target's only mirror of that type (the
  `# mirrors …` note stays for periphery targets no slug can name).
  lxir still never *mints* refs — that write stays blocked on oracle
  verification.
- **D33 — minted mirrors: `spiegel = InputRef(mirrors: quelle)`**
  (2026-08-25). D32 closed with "lxir still never *mints* refs" — this
  lifts that, as the foundation for eliminating ref plumbing from
  source altogether (auto-routing). A managed block of type
  `InputRef`/`OutputRef` takes a mandatory `mirrors:` binding naming any
  extern or managed block of the module (a block minted the same compile
  included — unlike the D32 *matcher*, which needs the ref to pre-exist
  in the base). The compiler emits the mirror identity the corpus
  shows on every GUI-created ref: `Ref=` (the target's UUID),
  `LinkRefType=` (Loxone Config's type-registry code for the target's
  XML type, learned per type from the house corpus —
  `connectors::ref_link_type`), and `Analog=` where the target type is
  analog. A target type without a verified code refuses the mint
  (refuse-never-guess; extend the table from corpus evidence). The
  three identity attributes are compiler-owned on ref blocks — excluded
  from D19 residue carry, so a retargeted `mirrors:` wins over the
  base's copy. Feed wires (`AI:`/`I:` from the target's outputs) stay
  ordinary explicit wires for now; drawing them automatically is the
  auto-routing step. Port shape verified across 189 InputRef +
  154 OutputRef in the house config: `AI`/`I` in, `AQ`/`Q` out
  (OutputRef: `AI` in, `AQ` out). Oracle-blessed same day (session 11):
  minted refs survive open+save; the GUI *heals* a missing piece rather
  than rejecting it, and each heal is encoded — a ref's `Title=` is
  derived from its target (compiler emits the target's title, a label
  on a ref block is refused), feed wires follow target connector
  index 0 → `AI` / 1 → `I` (the GUI draws them itself if absent — the
  compiler still requires them in source so rebuilds converge), and
  refs draw as flat 2112×192 tags (the mint footprint). The
  OutputRef → actor distribution wire is *not* healed and stays
  source-drawn. Confirmation cycle: recompile onto the GUI-saved file
  is a semantic no-op, second save empty in both directions.
- **D34 — mirror routing: wires reuse the page's refs** (2026-08-25).
  With D33 the pieces exist to drop ref plumbing from consumer source:
  a wire that names the *mirrored object* is drawn through a ref the
  base already carries. Reuse-only and page-local, because that is what
  the corpus proves the GUI does: a consumer reads a signal through the
  ref *on its own page* (all 96 `ref.AQ` and 5 `ref.Q` consumer wires
  in r50 are same-page), and a cross-page read with no local ref is a
  legal direct wire (oracle session 6). Rules, all refuse-never-guess:
  an input-side wire `X: obj.port` routes through a base ref iff the
  ref mirrors `obj`, is *fed from exactly that port* (feed wire
  `obj.idx0 → AI` serves `AQ`, `obj.idx1 → I` serves `Q`), and sits on
  the consuming block's page; an output-side wire `obj.port <- y` whose
  port the base feeds from an `OutputRef.AQ` lands on that ref's `AI`
  (writer's page — the corpus wires all 154 `OutputRef.AQ`
  distributions explicitly, so that leg stays source-side for actors
  but the *write into* the mirror is routed). Several same-page
  candidates: the previously drawn wire pins the choice — read from the
  *base pre-teardown* (the lock's `extern_wires` can't pin input-side
  wires: a wire into a managed sink vanishes with its block and is
  never recorded there); no pin → refuse, naming the candidates and the
  explicit-extern escape hatch. Ref-*typed* endpoints are exempt — a
  wire naming a ref (D32 extern or D33 minted block) is drawn
  literally, which also protects feed wires from re-routing. No
  same-page fed ref → direct wire. The r50 migration surfaced two
  format facts that shaped the mechanism
  ([loxone-format.md](loxone-format.md)): InputRef mirrors of one
  target *share* their output-port uuids — several same-page candidates
  usually collapse to one signal port, no ambiguity in the bytes
  (*corrected by D38: a push-tool artifact, not the format*) — and
  API distribution wires are recorded in reverse (`<In>` on the block's
  output naming the extern input), so the OutputRef redirect also
  applies to a wire's *source* side. Gate: folding all 162 foldable ref
  externs out of the r50 sources (153 target externs; 12 refs stay —
  cross-page consumers and source-fed export mirrors) recompiles
  byte-identical to the deployed config. e2e: reuse through `AQ`/`Q`,
  the `OutputRef.AI` redirect, shared-output twins, cross-page staying
  direct, literal explicit-ref wiring. Decompile/adopt invert the same
  rules: a consumer wire drawn on a ref is emitted against the mirrored
  object when the consumer shares the ref's page (conventional feeds
  only — target idx0 → `AI`, idx1 → `I`, one `OutputRef.AQ` sink), and
  a ref is no longer page-lifted into the full view by itself — it
  appears only while some unfoldable wire still names it, the way its
  plumbing wires have always folded (D29). On the live r50 config the
  full view drops from 174 ref externs to 12.
- **D35 — arithmetic is the `Formula` backend, standalone-only**
  (2026-08-25). `+ - * /` join the expression grammar (tighter than
  comparison; `* /` tighter than `+ -`; parens group), but unlike the
  boolean operators they do NOT map to one block per operator — there
  are no discrete arithmetic gates worth cascading. A maximal
  arithmetic tree becomes ONE `Formula` block: distinct port operands
  bind `Input1`…`Input4` in first-appearance order (a repeated port
  reuses its input — `(a.AQ + b.AQ) * a.AQ` is `(I1+I2)*I1` with two
  inputs), numbers and numeric `let` references are inlined into the
  compact formula text, negative constants parenthesized (`I1*(-2)`),
  the result is `AQ`, the slug operator `f`. The 4-input cap is the
  block's own; exceeding it is an error suggesting a split. Identity,
  labels, and lock ownership are exactly D24's.
  Standalone only: arithmetic may be a whole `<-` RHS or a whole
  argument binding, and nothing else. Under a gate or comparison
  (`a.Q and x.AQ + 1`, `x.AQ + 1 >= 5`) it errors at parse time with a
  pointer at the explicit-`Formula` escape hatch — a `Formula`'s
  analog `AQ` feeding a boolean input is behavior we have not verified
  against the Miniserver, and comparison operands staying plain keeps
  the v1 grammar decision intact. Both compositions are deferred, not
  rejected: the desugarer already composes (a gate operand may be any
  node), so allowing them later is deleting two parse checks after a
  sim/live verification, not building machinery.
  Lexing: `-` after a token that can end an operand (identifier,
  number, `)`) is binary minus; everywhere else it starts a negative
  number, so `SSoff: -30` and `x < -5` read as before — no existing
  program changes meaning.
  - Rejected alternatives: a per-expression backend *switch*
    (`formula(...)` wrapper or pragma choosing Formula vs discrete for
    boolean logic too, as the roadmap once sketched) — arithmetic has
    exactly one sensible backend and boolean logic already has one;
    a switch would be two spellings per rule with nothing choosing
    between them. Folding r50's ten hand-written PV `Formula` blocks
    into expressions — their titles and lock identities are
    established; a migration would re-mint for zero byte-level gain
    (the language now covers new cases; it does not force rewrites).
    The full Formula grammar (`IF`, functions, comparisons *inside*
    the formula string) stays out of the expression language — the
    string parameter remains available verbatim on an explicit block.

- **D36 — tests live in the module and run through `lox sim`**
  (2026-08-26). `test "Name" … end` is a top-level item whose body is
  line-oriented like everything else: injections (`slug.Port = value`),
  `tick <n> [dt <v>]`, `expect slug.Port <cmp> <value>`, and
  `clock "…"`. Statements group into simulator steps at `tick`
  boundaries; `expect`s attach to the preceding `tick`. Compilation
  ignores tests entirely — they change no bytes, so a module with tests
  compiles identically to one without.
  `lxir test` is a *driver*, not a simulator: it compiles in memory
  against the lockfile, maps slugs to the simulator's title-based
  addresses (lock UUID → compiled `Title`, room-qualified
  `"Title [Room].Port"` when titles collide; same-signal
  `InputRef`/`OutputRef` mirrors are tolerated), emits SimSpec JSON,
  shells out to `lox sim run --json`, and attributes the flattened
  check results back to `expect` lines by per-step slicing. Owning a
  simulator is out of scope — eisber/lox-cli already has one with
  injection-as-override semantics and a simulated clock (ClockSpec),
  which the DSL exposes 1:1. The comparator set is the simulator's
  (`== > >= < <= ~=`; no `!=`), surfaced honestly instead of emulated.
  References validate on the flattened form (templates expanded,
  expressions desugared): tests address what the simulator actually
  sees, including expansion-produced and expression-synthetic slugs.
  The unexpanded module skips test object-refs (a `flattened` guard in
  validate) because those slugs do not exist yet; `check`, `compile`,
  and `test` all re-validate the flattened form, so nothing escapes.
  Injections may drive any port, including outputs — driving a
  sensor's `Q` **is** the point (the simulator's persistent-override
  model), so port direction is deliberately unrestricted in tests.
  - Rejected alternatives: the roadmap's `given {…} after N ticks`
    block syntax — braces and keyword soup in a language that is
    otherwise line-oriented statements; the body reads like the rest
    of the DSL instead. Compiling tests to XML test configs and
    round-tripping through files — in-memory compile plus a temp dir
    for the two artifacts the CLI needs is simpler and leaves no
    droppings. Addressing the simulator by UUID — `lox sim` addresses
    by title; deriving titles from the lock keeps lxir's identity
    model authoritative and made the ambiguity rules explicit.
    A `!=` comparator emulated as two checks — dishonest reporting
    when only one leg fails; the simulator can grow `!=` upstream
    instead.
- **D37 — Automatic Designer rules: eyes, not hands** (2026-08-26).
  The app's Automatic Designer stores each rule as an `AutopilotRule`
  block whose entire logic — conditions, actions, activation — lives in
  the element's `APC=` JSON attribute
  ([loxone-format.md](loxone-format.md)). These blocks are the
  Miniserver's writes (D20 already cites them as the canonical
  GUI-owned-whole unit), so lxir does not author them; but a reader
  blind to `APC=` had three real holes, and this decision closes
  exactly those:
  - **`diff`/`drift`/fingerprint compare `APC=`** like an attribute
    parameter — an app-edited rule (a changed schedule time, a
    disabled rule) was previously "semantically empty", the same hole
    D18 closed for `Formula=`. `APC` deliberately does *not* join
    `connectors::attr_params`: that table is language surface
    (validate/compile/decompile bind those names), and the rule
    payload must never become bindable. A `diff`-local attribute set
    (`diff_attrs`) carries the union. Comparison is **structural, not
    textual**: the raw payload is not writer-stable (two house rules
    carry a raw CRLF inside the JSON), so `APC=` compares and
    fingerprints as canonicalized parsed JSON — a reformatted rule is
    not drift, an edited one is. The fingerprint moves only for
    rule-carrying configs (projection extension, no version bump);
    `lxir diff` renders an APC change as JSON field paths
    (`eventGroups[0].events[0].value: 540 -> 600`), not two blobs.
  - **The decompile full view renders each rule** as comment lines
    above its extern (`# when … / # then …`), resolving the payload's
    control UUIDs to titles — factual, every observed reference
    resolves in-document. Unknown codes print raw (`descID 272001`),
    interpreted labels are refused until evidence exists; the
    adoptable view stays bare (D29 boundary: comments are for
    readers, ManagedOnly output owes the compiler byte-fidelity).
  - The payload is handled as **untyped JSON** end to end: a typed
    struct would silently drop fields a newer app adds, and the diff
    must see exactly what changed, schema known or not.
  - Authoring (compiling rules from source) is deferred, not
    rejected, behind the evidence it needs: the `descID`/event-type
    tables (only inside the app bundle), an oracle answer on whether
    Loxone Config preserves foreign-minted rules, and a live
    Miniserver answer on whether it *executes* them — silent
    non-execution being the D8-class failure to rule out. Until all
    three exist, refuse-never-guess keeps the write side closed.
    (Opened by D57 for the documented shapes, 2026-10-29.)
- **D38 — UUIDs are unique; a `uuid:` edit re-points** (2026-10-18).
  The r50 drift forensics of 2026-10-18 traced 38 duplicate UUIDs in
  the live config to the first push on 2026-08-25: the upload tool
  remapped every ref's ports by `Ref=` target plus connector key, so
  all mirrors of one target received the same `AQ`/`Q` UUID; Loxone
  Config healed them on its next open (2026-08-28) by re-minting the
  later twins, which re-pointed their wires and surfaced as drift.
  Every UUID-keyed step in lxir — extern matching, wiring, routing, the
  semantic diff and fingerprint — silently picks one twin of such a
  pair, and D34 had even recorded the sharing as a format fact (its
  dedup branch in route picking is gone; the routing e2e test now
  exercises what the GUI actually writes: twins with their own ports,
  the drawn wire pinning the choice). Refuse-never-guess, applied to
  identity: `compile` and `adopt` refuse a base in which any `<C>`
  object or `<Co>` connector UUID repeats (`<SObj>` entries reuse their
  object's UUID by design and are not counted; zero repeats
  corpus-wide), naming the remedy — one Loxone Config open+save, which
  heals the twins. `compile` also checks its own output before
  returning it: a repeat there can only come from this compile (a mint
  colliding with a base UUID minted in the same second under the same
  serial, or a lock mapping two identities onto one UUID), and such an
  output must never reach a push. `diff` warns instead of refusing —
  diffing a damaged config is how the damage gets investigated — and
  `drift` adds a note, since lxir never emits repeats. The same
  forensics found the second gap: an edited `uuid:` value on an
  already-pinned extern was silently ignored, because the lock pin
  wins while its object exists. That tolerance exists for titles and
  inames, which drift under a pin; a `uuid:` matcher *is* the identity,
  so a value that differs from the pin can only be a source edit — the
  extern now resolves afresh (a typo is a `NoMatch`). A matcher-*kind*
  change keeps D32's rule and must still confirm the pinned object.
- **D39 — attribute claims: externs get title, room and category from
  source** (2026-10-18). Repurposing a device — a sensor moved to
  another room and renamed — touched only objects lxir could match but
  not change: the device, its outputs, their titles and room bindings.
  The edit had to be a byte patch on the base plus an oracle check, the
  exact workflow lxir exists to replace. Creating devices stays with
  Loxone Config (pairing, serials and channel layout are the GUI's and
  the Miniserver's); *assigning* an existing one is a plain fact a
  source file can hold. Three statements, top-level only:
  `x.title = "…"`, `x.room = "…"`, `x.category = "…"`, where `x` is an
  extern. Rooms and categories are named by title and must exist in the
  base exactly once (created in the GUI, like every other taxonomy
  entry); the claim writes the `Place`/`Category` UUID into
  `<IoData Pr=/Cr=>`, and only ever *rewrites* an attribute the object
  already carries — an object without an `<IoData>` binding is refused
  with the remedy (set it once in the GUI), so element shape and
  attribute order are never invented. Managed blocks are refused:
  their title is the block label, their bindings are D19 residue. An
  extern may not claim what it is matched by (`title:` matcher plus
  `.title`, `room:` plus `.room`, …): after the push the match would no
  longer find its object — pin it by `uuid:` or `iname:`. **Followers:**
  a device's inputs and outputs carry their own `Pr=`, and in the house
  config 673 of 687 device children share their device's room. Loxone
  Config has no cascade prompt for this, so lxir defines the rule: a
  room/category claim also rewrites every descendant whose binding
  equals the object's *original* value, skipping managed blocks and
  descendants claimed explicitly for the same attribute (with their
  subtrees). A child bound elsewhere stays where it is. The lock records
  original, written value and the follower list (`attr_claims`);
  teardown restores the object unconditionally and each follower only
  while it still shows the written value — a follower moved in the GUI
  after the claim keeps the GUI's choice and leaves the list on the next
  compile. Withdrawal follows D31: a claim gone from source leaves a
  `removed_attrs` tombstone that keeps restoring while the base still
  shows the written value and retires once it does not. Claims are
  planned against the torn-down document before anything is written, so
  nested claims never record each other's write as their original. The
  semantic diff and fingerprint now include room/category bindings
  (fingerprint v2, `lxir-fingerprint-v2`; a diff blind to them would
  call a moved device empty); `lxir drift` still accepts a v1
  fingerprint recorded by an older lxir and notes it, and the next
  compile records v2. Lockfile format bumped to v3. Validated against
  the real config: the compiled claims equal the hand patch they
  replace, and Loxone Config saves the output without a semantic
  change. Not covered yet: membership lists such as the smoke alarm's
  device list, which live in the alarm block, not in the device.
- **D40 — program code from source; numeric character references are
  text** (2026-10-29). The house's Hue program block (Code16 "Philipps
  Hue Steuerung", PicoC that PUTs light states to the Hue bridge) had
  done nothing since late August. The first push on 2026-08-25 went
  through an XML writer that encoded the program's line breaks as
  `&#xA;`. Loxone Config does not decode numeric character references:
  its next save (2026-08-28) re-escaped the ampersand, `&amp;#xA;`, and
  the 503-line program became one line that starts with `//`, a single
  comment. The same save damaged two `AutopilotRule` `APC=` payloads
  (now invalid JSON) and ten Modbus `NDOC=` texts. lxir could neither
  see nor repair it: `Code=` was D19 residue outside the diff, and
  `xml::unescape` decoded numeric references, so lxir read those
  attributes differently from Loxone. Decision:
  - **Numeric character references are text.** `xml::unescape` decodes
    only the five named entities and keeps `&#…;` verbatim, which is
    what Loxone reads. No config in the 133-file corpus contains one,
    so no GUI-written content reads differently. `lxir drift` now
    starts with a note listing every attribute that holds one
    (`LoxoneDoc::char_ref_attrs`, program code and `APC=` first).
  - **`Code: file("prog.c")`** on Code16 is an attribute parameter, the
    D18 `Formula:` precedent. A PicoC program is hundreds of lines,
    which the line-oriented IR cannot hold inline, so the value comes
    from a file. The path is relative to the `.lxir` file that names
    it and is read when the module loads (`Module::load_files`). CRLF
    is normalized to LF, and the text is written the way Loxone Config
    writes it: raw LF line breaks and the five entities. `file(…)`
    binds only file parameters (`connectors::file_params`, Code16
    `Code` so far); anywhere else it is an error. `Task=` stays
    GUI-owned.
  - **A GUI-owned attribute parameter is a claim.** `Code` stays in
    D19's carried set: unclaimed, it is carried verbatim from the base,
    damage included. Claimed, the source text replaces the carried
    copy. A withdrawn claim hands the program back to the GUI and
    needs no tombstone, because the base holds the last pushed text.
    Decompile never lifts `Code`: a decompiled module writes no files,
    and a program inlined into the view would be unreadable. Adoption
    therefore leaves the program with the GUI until an author moves it
    into a file on purpose. Adopt accepts the `Valid=` companion only
    on `Formula`.
  - **The diff and fingerprint see program text.** A `Code` change is
    a param change, printed as a line diff (`diff::text_changes`:
    common prefix and suffix trimmed, at most eight lines per side).
    The fingerprint moves to v3 (`lxir-fingerprint-v3`) and includes
    program text. `lxir drift` still accepts a v2 fingerprint and notes
    that it is blind to program code; the next compile records v3. The
    lockfile format is unchanged.
  Validated on the real config: compiling the house modules with the
  program in `philipps_hue_steuerung.c` reproduces the Loxone-written
  `Code=` from before 2026-08-25 byte for byte, every byte outside that
  attribute equals the base, and the semantic diff shows exactly one
  change, the program going from 1 line back to 503. The damaged
  `APC=` and `NDOC=` attributes are still not authorable: they need a
  Loxone Config fix, and the drift note keeps them visible until then.
- **D41 — Touch Pure Flex tiles from source** (2026-10-29). The tiles
  of a Touch Pure Flex were GUI-only state: a `<btns>` child of the
  `TreeDevice` with one `<btn>` per tile and three functions each
  ([loxone-format.md](loxone-format.md)). In the house, three Flex
  devices in one room share a tile layout whose shade tiles carry no
  `ctrls=`; such a command reaches every shade linked to the device, so
  a shade tile moved all linked shades. Decision:
  - **Syntax.** A named set `tiles <name>` … `end` lists
    `tile <i>(click:, up:, down:, text:, to:, …)` programs; a claim
    `<device>.tiles = <name>` writes it onto an extern `TreeDevice`, and
    one set may be claimed by several devices. `click`/`up`/`down` are
    `f1`/`f2` (`bT="1"`)/`f3` (`bT="2"`); the corpus shows `Co` (open)
    on `bT="1"` and `Cc` (close) on `bT="2"`. `text:` and `to:` apply to
    every function with a command; `<fn>_text:`/`<fn>_to:` override one
    function. `to:` is repeated rather than a list, like a fan-in. The
    device stays a plain extern — tree devices have free-text titles,
    so it is pinned by `uuid:` — and no new extern kind is needed.
    Commands and texts are quoted strings written as Loxone Config
    shows them; lxir does not parse the command language beyond the
    abbreviations below.
  - **Rewrite existing tiles only.** Tiles are addressed by `i`. A
    named index the device lacks is an error that lists the indices it
    has; unlisted tiles are carried byte for byte. Creating tiles and
    the layout (`x`/`y`/`h`/`w`, `ci`, icons) stay with Loxone Config.
    A written function emits `api`, `cT`, `bT`, `ctrls` in the GUI's
    order, keeps the raw bytes of a value that decodes unchanged,
    carries unknown attributes after them, and writes an empty click as
    `<f1></f1>`. A `bT=` that is not the slot's is refused.
  - **`to:` targets must be API-linked** to the device in the compiled
    output: a wire, in either direction, between the target's
    `OutputAPI` and the device's `ApiActor.I`, or the `AI` of an
    `OutputRef` mirroring that actor whose `AQ` distributes into
    `ApiActor.I`. Both directions count because API distribution wires
    are recorded in reverse. The check runs after all wires of the
    compile are drawn, so a link written in the same source counts.
    A target is resolved like any wire endpoint (extern, managed block,
    template body block by flattened name).
  - **New API links through an explicit mirror.** A page that already
    holds several mirrors of the actor leaves D34 no choice for
    `OutputAPI: flex_api.I`. The source then mints the mirror and draws
    the link's three wires itself: the forward
    `OutputRef(mirrors: flex_api, AI: blk.OutputAPI)`, the distribution
    `flex_api.I <- ref.AQ`, and the reverse `OutputAPI: ref.AI`. The
    reverse wire names an input port as its source, which the direction
    check refused. It is now admitted for exactly that shape: the `AI`
    of an `OutputRef` whose `mirrors:` target is an `ApiActor`, read by
    an `OutputAPI` connector that takes wires. Evidence: all 61 links
    through a mirror in the corpus and the house config have these
    three wires, all reverse wires sit on `OutputAPI` (or a connector
    keyed `API`), all mirrors mirror an `ApiActor`. An extern ref's
    `AI` was already accepted as a source (extern ports are
    open-world). `FLG="1"` marks the house's 16 reverse wires and none
    of the corpus's 45; the oracle showed the flag inert (D19), so a
    minted reverse wire carries none. A flag already present is carried
    as residue.
  - **Type check from abbreviations.** The first argument of a
    multi-argument call (`SET(As;Co;pulse)`) names the addressed block
    type. lxir knows `As` = `AutoJalousie` and `Lico` =
    `LightController2`, both observed in the house config. When every
    abbreviation in a command is known, a target of another type is
    refused; unknown abbreviations are written unchecked. The corpus
    fan tile's `Sts` sits on a Flex linked to a `StepSel` — a single
    observation, not added.
  - **Warnings are lint.** Compile has no warning channel, so the
    advisory checks are `lxir lint` kinds over the compiled config,
    for every Flex, claimed or not: `tile-broadcast` (a command without
    `ctrls` while two or more linked blocks of its type receive it) and
    `tile-dead-command` (no linked block of its type, among `ctrls`
    when given). Both need a known abbreviation.
  - **No lock state, no restore.** Like `Code:` (D40), a withdrawn
    claim leaves the last written programs in the device, and the base
    holds them; there is nothing to tombstone.
  - **Diff and fingerprint see tile programs.** Each non-empty function
    projects to a param keyed `tile <i> click|up|down` (attributes
    except `bT`). The fingerprint moves to v4 and includes them; drift
    still accepts v3 and notes that it is blind to tile programs. The
    layout stays outside the projection, as GUI state.
  - **Decompile and adopt do not lift tiles.** A set is per-device data
    whose `ctrls` name blocks that need slugs, and the device itself
    needs an extern; lifting would add externs to every decompiled
    view for state that works unclaimed. As with D40, programs move into
    source on purpose; an unclaimed `<btns>` is carried verbatim.
  Not covered: creating or moving tiles, display-only tiles (a text
  without a command; no corpus evidence), and checking abbreviations
  beyond `As`/`Lico` (the house also has `TpfController` blocks on
  Flex API connectors, whose commands lxir writes unchecked).
- **D42 — Placement: the GUI's geometry, shared with the GUI**
  (2026-10-29). A block without a position went into one column at
  x 7392, below the lowest managed block of the whole lock, on every
  page. Compile wrote positions from the lock only, so the next compile
  reverted a GUI drag. On a kitchen page of the house config the 11
  minted objects sat in one column at y 18552–29040, below everything
  else, and 15 of the page's 76 wires ran right to left. On
  "Automatik-Regeln" the content ends near y 6000 and two managed
  blocks sat at y 15192. Decision:
  - **Positions are shared state.** Teardown harvests each managed
    block's rectangle from the base, like other GUI residue (D19),
    together with the page it is drawn on. When lock and base disagree,
    the base wins: a drag in the GUI goes into the lock. Positions stay
    outside the diff projection and the fingerprint (D21), so a drag is
    not drift. Two limits:
    - Only a move of a grid unit (96) or more is a drag. The GUI
      recomputes `Px2`/`Py2` on every save; a block on the same spot
      takes the GUI's rectangle, so a save moves nothing.
    - A rectangle on another page than the module's (a GUI cut and
      paste) is ignored: the block is rebuilt on its own page, where it
      was.
  - **The GUI's geometry** (`src/layout.rs`, from corpus statistics
    and GUI refs placed next to block ports). Grid 96, port row 192.
    Small blocks are 1344 wide and 312 + 192·rows tall, with the first
    port row at 288. Function blocks are 2688 wide and 504 + 192·rows
    tall, first row at 480. Ref tags are 2112 × 192 and sit 960 off the
    block edge. Rows are what the GUI shows:
    - Types that show at least a set number of rows, from the corpus
      mode: LC2 6; AutoJalousie, CentralShade, CentralLight and Code16
      4; DayTimer 3; PButtonT and PulseAt 2. Wired connectors beyond
      those add rows.
    - Other built-ins show their inputs and outputs, and show params
      and `Remanence` only when they are wired.
    - Unknown types show only wired ports. Keys starting with `out` go
      on the right: the API links of `OutputAPI` are stored on the
      output, so a wired connector there is not an input.

    `Memory` draws as a 2112 × 192 tag but is a block and sits in its
    chain. Objects of unknown type keep the size they are drawn with.

    The old formula `504 + 192·(ports−2)` was wrong for function blocks:
    a minted PushButton came out 1344 × 1848, where the GUI draws
    2688 × 1272. The GUI snaps `Py` to the grid on save and recomputes
    `Px2`/`Py2`. Compile writes grid positions with the GUI's
    footprint, so a save moves nothing.
  - **Placement runs after wiring**, per page, on the page as it then
    stands. Free blocks joined by wires form a cluster, laid out as a
    layered graph:
    - feedback is broken in source order by a depth-first walk;
    - columns come from the longest path, and order within a column
      from barycenter sweeps;
    - each block sits level with the ports that feed it;
    - columns are spaced for the labels and tags beside them.

    The cluster goes to the free spot, cleared by a margin, that keeps
    its wires to fixed objects shortest and running left to right. A
    cluster without such a wire goes below the page's content. A minted
    ref tag goes beside the port it serves, 960 off and stacked by row,
    as the GUI places them. Integer arithmetic and sorted candidates
    make placement deterministic.
  - **`--relayout <page>`** places the page's managed blocks afresh, as
    if new. A block stays where it is if it is wired to a ref tag the
    compile does not own (a GUI-drawn ref) and drawn beside it, level
    and at most two gaps off: someone arranged those two together in
    the GUI. A wire routed through a GUI ref elsewhere on the page
    (D34) pins nothing. A cluster's old position (that of its first
    block) is worth one row in the cost, so a spot about as good keeps
    it. A title that names no page is an error.
  - **A stale base must not revert a placement.** After a relayout,
    the lock holds the new position, and the base keeps the old one
    until it is uploaded and downloaded again. Harvest would read the
    old position as a GUI drag. So the lock records it as
    `superseded_layout`:
    - a base position on the same spot is ignored;
    - any other base position is taken over (the new one once it
      arrives, or a drag) and clears the field;
    - every placement writes it anew: the base position it replaced,
      or nothing.

    This is the same lifecycle as D31's tombstones. The field is new in
    lockfile v4, so an older binary refuses the lock instead of dropping
    it. A block that changes pages is placed afresh; its old page's
    position is not on the new page, so nothing is superseded.
  - **`lxir render`** draws one page as SVG: managed objects blue, GUI
    objects grey, wires orthogonal with backward ones red, labels for
    other pages purple. It prints the metrics that placement is judged
    by: overlaps, backward wires (listed), long wires (over 8000), wire
    crossings and total wire length. This shows a layout without
    opening Loxone Config.
  - **Results on the house.** Without `--relayout`, compiling the house
    gives the same bytes and the same lock as before. With
    `--relayout`:
    - The kitchen page: backward wires 15 → 5, long wires 11 → 4,
      crossings 34 → 18, total wire length 333k → 210k. Four of the
      remaining backward wires are real feedback, from a scene block
      back into the two light controllers it follows; the fifth runs
      between two GUI objects.
    - "Automatik-Regeln": the page's extent shrinks from y 768–16584 to
      768–7416.

  Not covered:
  - Moving objects the compile does not own (GUI refs, extern blocks).
  - Wire routing: the GUI routes wires itself.
  - Negative coordinates: never observed, so x and y stay ≥ 0, and a
    chain that feeds a block at the page's left edge runs backward.
  - Lifting positions into source: they live in the lock.
- **D43 — a title claim carries the refs that copy the title**
  (2026-10-29). Every `InputRef`/`OutputRef` holds its own `Title=`,
  and Loxone Config re-derives it from the mirrored object on every
  save (oracle session 11 found this for minted refs; session 12
  confirms it for the GUI's own, stale ones included). A D39 `.title`
  claim rewrote the object and left its refs on the old title, so the
  next GUI save showed up as drift, and the house config carried two
  stale relay refs from such a claim. Now a title claim has followers
  like a room/category claim: every ref whose `Ref=` is the claimed
  object, whatever its current title (the GUI would rewrite a stale one
  too). **Except collected inputs:** the `InputRef`s an alarm block
  gathers into its sensor list are written with `WF` bit 0x1000, often
  under a composite title ("<input> <device>"), and the GUI keeps their
  titles — composite or plain copy, through a rename of the target or
  of its device. The bit is GUI-derived (session 12: flipped by hand,
  the save restores it; a position added or removed changes nothing),
  so it marks exactly the refs whose title the GUI does not own. The
  `OutputRef`s such a block uses carry 0x1000 too and *do* follow.
  Managed refs and refs claimed explicitly are skipped, as for D39
  followers. Lock, teardown and tombstones are D39's unchanged: each
  follower is restored to the claim's original while it still shows
  the written value, which is also what the GUI derives once the
  object is back. Minted `mirrors:` refs (D33) take the title this
  compile leaves on their target — the claimed one, or after a
  withdrawal the restored one — not the base's, which may still show
  a withdrawn claim. Validated with the oracle on the house config:
  the existing relay claims now fix their stale refs, and claims on a
  target with visible, hidden and collected refs, and their
  withdrawal, both save with an empty semantic diff.
- **D44 — `lxir repair`: numeric character references back to
  characters** (2026-10-29). D40 made lxir read `&#xA;` the way Loxone
  does, as text, and `lxir drift` lists the attributes that hold it,
  but repairing them stayed manual: the house's two damaged `APC=`
  payloads were byte-patched, and ten Modbus `NDOC=` texts still show
  the literal `&#xD;&#xA;`. Decision: a command that writes a repaired
  copy, `lxir repair <cfg> --out <out>`, over
  `LoxoneDoc::repair_char_refs`:
  - **What it writes is what Loxone wrote before the damage.** Each
    reference becomes the character it names, written the way Loxone
    writes it (loxone-format, deviation 2): line breaks and every other
    character raw, `& < > " '` as the five named entities. A foreign
    writer encodes characters one by one, so decoding them one by one
    restores the original: a `&#xD;&#xA;` pair becomes CRLF, a lone
    `&#xA;` a bare LF.
  - **Both damaged forms.** `&amp;#xA;` (re-escaped by a later GUI save,
    the house case) and `&#xA;` (straight from the foreign writer, not
    yet saved by the GUI) both read as the text `&#xA;`, and both are
    repaired. A reference naming nothing an attribute can hold (`&#0;`,
    a surrogate) stays text, and the command names what it left.
  - **Byte-exact elsewhere.** The rewrite works on the raw attribute
    value and touches only the references; every other byte of the
    document, the rest of each repaired value included, is copied.
    Same scope as `char_ref_attrs`: attributes of `<C>` elements.
  - **A copy, not in place, and no compile step.** The input stays as
    evidence; each rewritten attribute is listed with its object, UUID
    and reference count. The repaired file is a base like a download:
    compile against it, push the output. The repaired attributes are
    GUI content (`NDOC`, `APC`, an unclaimed `Code`), so they reach the
    Miniserver through the compiled output, and drift sees the push.
  Validated with the oracle (session 13): repaired corpus configs equal
  their undamaged originals byte for byte, and Loxone Config keeps the
  repaired `APC=` (2 rules, raw CRLF in the JSON) and `Code=` (a
  program with 436 line breaks) byte-identical on open and save. On the house
  config the ten `NDOC=` texts, plus a `Code=` damaged on purpose, come
  back with raw CRLF, and the save keeps them, with no semantic change
  beyond two known stale ref titles (D43).
- **D45 — a rebuilt wire keeps its place among the sink's `<In>`**
  (2026-10-29). Loxone Config writes a connector's `<In>` children in
  an order of its own when it saves. The order means nothing, but the
  teardown removes the compiler's wires and the rebuild appended them,
  so recompiling a GUI-saved config moved every compiler wire that the
  save had sorted in front of another `<In>` to the end of its
  connector. The bytes changed with the logic unchanged, and `lox
  config push` offered an upload, with its reboot, for nothing.
  Decision: before the teardown, the compile records each sink's
  `<In>` sources in base order. A wire added back goes in before the
  first `<In>` that the base listed after it, or that the base did not
  have, and so lands where the base had it:
  - **Managed and extern sinks alike.** A managed block is rebuilt
    empty and gets its wires back in base order; on an extern sink the
    GUI's wires stay put and the compiler's wires return between them.
  - **New wires still append.** A wire the base did not carry has no
    place to keep and goes last, as before.
  - **An `<In>` already present stays.** The first compile after adopt
    finds the declared wire in the base; it used to move it to the end
    so that the next compile, which appended, matched. It now stays
    where it is, which is where every later compile puts it back.
  Compiling the compiler's own output is still a fixpoint, since that
  output is a base like any other. Not covered: a save also rewrites
  what the rebuild emits for a managed block. Loxone Config 17.2
  stamps every block `V="178"` where the rebuild writes the verified
  `V="175"` (settled by D46), writes `Desc=` right after `Type=` and
  moves blocks within a page (both settled by D47), and adds
  `Text="true"` to some minted refs. A recompile of a config saved with
  17.2 still differs in those bytes, though not in logic.
- **D46 — managed blocks carry the document's block version**
  (2026-10-29). Every `<C>` carries `V=`, a block-format version, and
  across the corpus each file has exactly one: Loxone Config writes
  every object at its release's version on save (16.1 writes 174, 17.0
  and 17.1 write 175, 17.2 writes 178). The rebuild wrote a fixed
  `V="175"`, the version the builtin table was verified for. In a house
  on 17.2 every push set the compiler's blocks back to 175 and every
  save stamped them 178 again. That is harmless, since Loxone Config
  migrates an older block on load, but no recompile of a saved config
  could reproduce it, and adopt refused every block of a 17.2 file.
  Decision:
  - **The document's version, when verified.** Compile emits the block
    version the base's objects carry after the teardown
    (`LoxoneDoc::block_version`: the most common, a tie the higher)
    when `connectors::VERIFIED_BLOCK_VERSIONS` lists it. Adopt accepts
    a block of any listed version.
  - **Otherwise the first verified version.** In a document of a
    release nobody has verified, managed blocks stay at 175 and Loxone
    Config migrates them on load. A block stamped with a version whose
    layout nobody checked could be read as something it is not.
  - **178 is verified** (oracle session 14). Under 17.2 all 24 builtin
    types, minted at `V="178"`, survive open+save at 178 with their
    exact connector keys and indexes, and the house rebuild at 178
    saves with an empty semantic diff. One layout fact changed: 17.2
    writes AnalogThresholdTrigger's `Remanence` connector after `Off`
    instead of second, at the same index. A save reorders a rebuilt
    one; the logic is the same.
  - **The next release** is admitted the same way: both oracle runs
    under it, then its version joins the list.
  The release pin (D22) is unchanged: `--accept-version` still accepts
  a release, and D46 only decides which `V` the managed blocks get.
- **D47 — a rebuilt block keeps the base's order** (2026-10-29). D45
  for the block itself. A Loxone Config save also orders what the
  rebuild writes for a managed block, and the rebuild wrote its own
  order:
  - objects within their page: the rebuild appended its blocks at the
    end of the page;
  - attributes: Loxone Config writes `Desc=` right after `Type=`, the
    rebuild wrote it behind the position attributes;
  - connectors: 17.2 writes AnalogThresholdTrigger's `Remanence` after
    `Off` (D46), the rebuild in table order.
  None of it means anything, but each kept a recompile of a saved
  config from reproducing it. Decision: the teardown records each
  block's attribute names and connector keys in base order with its
  D19 residue, and before that each page's objects in base order. The
  rebuild follows them:
  - **Attributes and connectors** go into base order. One the base did
    not have, such as a parameter or a connector new to the block,
    stays right behind the item it followed as emitted.
  - **A block goes back to its place in the page**: before the first
    object the base listed after it, or did not have, and otherwise
    behind the page's last object. A page's `<IoData>`, which Loxone
    Config writes before the objects on some pages and after them on
    others, stays where it is. A block new to the page, minted or moved
    there from another page, is appended as before.
  Compiling the compiler's own output is still a fixpoint. With D45
  and D46, compiling a config that Loxone Config 17.2 saved reproduces
  it byte for byte, apart from `Text="true"` on minted ApiActor refs,
  which D33 does not emit (settled by D48).
- **D48 — a minted ref of a text value carries `Text="true"`**
  (2026-10-29). D33 emits a ref's mirror identity: `Ref=`,
  `LinkRefType=`, and `Analog=` where the target is analog. Refs of
  text-valued objects carry a third flag in `Analog=`'s slot,
  `Text="true"`. Every ref of an `ApiActor` (36) and of a
  `VirtualTextIn` (12) in the corpus has it, from Loxone Config 12.0
  to 17.2, and no ref has both flags. A minted ref without it gained
  it on the next GUI save, so no recompile of a saved config could
  reproduce it. Decision:
  - `connectors::ref_link_type` returns the value kind with the code
    (`RefValue`: digital, analog or text) instead of an analog flag.
    `ApiActor` and `VirtualTextIn` are text.
  - `Text=` is compiler-owned on refs like the other identity
    attributes: a ref retargeted to a non-text object loses it.
  - `Memory` and `VirtualOutCmd` refs carry `Text=` on a few instances
    only (4 of 806, 3 of 30). There the flag depends on the target's
    configuration, not its type, so it stays out of the table, as
    `Analog=` on `Memory` does.
  With this and D45 to D47, compiling a config saved by Loxone Config
  17.2 reproduces it byte for byte.
- **D49 — access rights on NFC Code Touch blocks from source**
  (2026-10-29). Who may open a door is part of the program: a user or
  group may open a Code Touch when one of its `<PRM>` rights names the
  block with a `TIM=` access schedule (loxone-format, *Users, groups and
  access rights*). lxir could show these rights in the diff, and `lox
  config push` kept them as they were live, but no source could set
  them. Decision: an `access <code_touch>` declaration lists the users
  and groups that may open the block, each with its schedule:
  - **The declaration owns the block's access, completely.** Every
    `TIM=` on the block, across all users and groups, is the
    declaration's: compiling grants what it lists, moves a right to
    another schedule, and revokes what it leaves out. A grant-only form
    could never take a right away, and taking rights away is half of
    the point. Blocks without a declaration are not touched, as with
    D39's claims.
  - **Users, groups and schedules stay GUI-owned, referenced by
    title.** They are created in Loxone Config or the app together with
    passwords, tags and codes the source must never hold. A title that
    names nothing, or several objects of its kind, is a compile error
    that names the candidates: a user renamed in the app stops the
    compile instead of silently losing access. Schedules are the
    `SwitchingTimer` objects of the access schedule folder.
  - **Access only.** A right's `ACT=` (operating the block in the app)
    stays as it is. A revoke removes `TIM=`; a `<PRM>` left with nothing
    but `REF=` goes, and a list left empty goes with it, since no
    config in the corpus carries an empty `<PH>`.
  - **Config's own edit.** A grant appends `<PRM REF="…" TIM="…"/>`
    to the user's or group's list (creating the list if there is none),
    a reschedule rewrites `TIM=` in place, and `NUM=` follows the count.
    Nothing else changes, not even `LastEdit=`, so a declaration that
    matches the config writes nothing and compiling reproduces it byte
    for byte.
  - **No lock state.** Like tile programs (D41) the rights are the
    source's while it declares them; removing the declaration leaves
    the last written rights in place.
  - **`NfcCodeTouch` only.** Config 16 and 17 put `TIM=` only on those
    blocks; Config 12 rights on modes, actors and right groups are out
    of scope, and the compile refuses an `access` on any other type.
  - **A contextual keyword.** `access` starts a declaration only in
    front of a slug at the start of a line; reserving the word would
    turn every existing `access` slug into an error.
  - **Decompile and adopt lift the rights** of every Code Touch that
    has any, so adopting a config puts its access under source control
    unchanged. A right whose user or group has no unique title cannot
    be referenced; the block gets a comment instead of a declaration
    that would revoke it.
  - **Lint `no-access-right`**: a user with an NFC tag or access code
    but no access right on any Code Touch, directly or through a group.
    The tag opens nothing; *Full access* does not help (live-verified).
  - **`permissions.bin` follows when it can be trusted.** Loxone Config
    writes it into the program ZIP: per object, the users and groups
    with any right on it (loxone-format, *permissions.bin*). A grant to
    someone who had no right on the block changes it, a reschedule or
    a revoke that keeps `ACT=` does not. `lxir::permissions` generates
    it from the program and the ZIP's `LoxAPP3.json`, byte for byte on
    every ZIP we have. `lox config push` uses it only where it is
    proven: when the generator reproduces the live file exactly, the
    staged ZIP carries the file generated for the compiled config;
    otherwise the live file stays, and the report flags it as stale
    (`permissions_stale`) if the compiled config assigns users and
    groups differently.

  Validated: compiling the declaration lifted from a Loxone Config 17.2
  save that granted a user access, onto the config from before that
  save, reproduces Config's users, groups and schedules byte for byte,
  and the `permissions.bin` generated for it with the `LoxAPP3.json`
  from before the save is the one Config wrote.
  Every declaration lifted from the corpus (3 in the public configs, 16
  in the house's history) compiles to the same bytes; revoking one
  changes only access rights, and granting it again reverts that
  exactly. Not covered: the revoke edit is inferred, not observed in a
  Config save; the Miniserver stores an uploaded `permissions.bin`
  unchanged, but whether it enforces that file or derives the rights
  from the program is a live test.
- **D50 — placement keeps to the paper sheet** (2026-10-29). Loxone
  Config draws every page on a paper sheet. D42 placed clusters on an
  unbounded canvas: on a full page the best clear spot was past the
  sheet's right edge. After a relayout of a kitchen page of the house
  config, a chain of nine blocks sat at x 24192–35904; the sheet ends at
  23808, so in the GUI the chain hung off the paper. Decision:
  - **The sheet comes from the Document's `PaperFormat`** (one for all
    pages). A3 landscape (`2`) is x 0..23808 × y 0..16032: the paper in
    twips on the grid, less the title block along the bottom edge.
    Other formats are not known, so their pages stay unbounded as
    before.
  - **A cluster goes to the best clear spot on the sheet**, with the
    room its labels and tags need. The sheet's edges are candidate
    positions next to the obstacle edges. Without room at the GUI's
    spacing, the cluster is laid out tighter and tried again: columns
    960 apart, then 576, then 384, with stacking 384 → 192 at the last
    step. The GUI's spacing comes first because a far spot with room
    to breathe reads better than a cramped one close by.
  - **Past the edge only when nothing fits.** A cluster that fits
    nowhere on the sheet even at the tightest spacing goes where D42
    put it, at the GUI's spacing. A ref tag goes beside its port if it
    stays on the sheet there; otherwise it takes the best clear spot on
    the sheet like a cluster of one. A tag whose block is past the edge
    stays beside the block.
  - **Compile names what is past the edge.** After writing, `compile`
    warns per page about managed objects past the sheet (by slug) and
    names the `--relayout` that lays the page out afresh. An object is
    past the edge if its footprint or one of its off-page labels is:
    the same test as placement's, and the one `render` reports along
    with the sheet's outline. Less than a grid unit over is on the
    sheet: the GUI draws a block as tall as its rows wherever it was
    dropped, and a GUI-placed block on the kitchen page ends 24 below
    the title-block line. `lox config push` deploys the placement the
    lock holds and places nothing, so it does not warn.

  The A3 page is the limit: Loxone Config offers no larger format. A page
  that is full stays full. Moving logic to a page of its own is a
  module change, and moving GUI-owned objects to make room is not
  placement's job (geometry claims, roadmap). On the kitchen page the
  chain now sits in the top right, the only free region left, at the
  tightest spacing, and the whole page is on the sheet. The price is
  wires: the chain runs in a loop from two light controllers back into
  them. Past the edge, right of the controllers, only the four wires
  back into them ran right to left (5 backward wires on the page). Above
  them, the four out of them do too (9).
- **D51 — push notifications from source** (2026-10-29). A push
  notification is three pieces in the config (loxone-format, *Push
  notifications*): the `Notification` object under *Messages*, an
  `OutputRefLM` on a page whose `<LoggerMailer>` child holds the texts
  and the minimum time, and the recipients as `ACT=` rights on the
  notification. Logic that should warn someone needed the GUI for all
  three. Decision:
  - **The notification stays periphery.** Like a device, Loxone Config
    creates it; the module names it as an extern `Notification(…)`.
    Minting it would put objects into the *Messages* tree, which no
    compile writes.
  - **The page object is a minted ref** (D33): `OutputRefLM(mirrors:
    <notification>, AI: …)`. It gets the shape Config gives a fresh
    one — `Cl="0,0,0"`, no `WF=`, no `LinkRefType=` — and its
    `mirrors:` must name a `Notification`. Its message is parameters on
    the block: `On`, `Off` and `MinimumTime` (a duration, compiled to
    milliseconds; default the GUI's 5 s). An empty `Off` is not
    written, as Config does. The parameters are compiler-owned like the
    rest of the block, so the `<LoggerMailer>` is rebuilt each compile.
  - **Messages are semantics.** The diff compares the attributes of any
    `<LoggerMailer>` (`message On`, …), and the fingerprint includes
    them: v5. `lxir drift` accepts a v4 fingerprint recorded before.
  - **Recipients are a `recipients <notification>` declaration**, D49's
    machinery with `ACT=` instead of `TIM=`: it owns every `ACT=` on the
    notification, grants with `ACT="3"` (Config 17.2), keeps an
    existing `ACT=` of any value (older configs write `1`), revokes by
    removing `ACT=`, and keeps no lock state. Users and groups are named
    by title. The *Full access* group is not listed: Config shows its
    members as inherited recipients and writes nothing for them.
  - **Not lifted.** Decompile leaves `OutputRefLM` blocks and
    recipients with the GUI; a module takes them over by declaring
    them.
  Validated: oracle session 15 — a house compile with a notification,
  a recipient and an `OutputRefLM` open+saves with an empty semantic
  diff. Not covered: whether the Miniserver delivers to a recipient
  that the uploaded `permissions.bin` does not list.
- **D52 — documented port facts: units, labels, ranges** (2026-10-29).
  D27 read every time suffix as seconds, which is wrong on the ports
  Loxone keeps in minutes: `AutoJalousie(AutoShadeTime: 2h)` compiled
  to `7200`, i.e. 120 hours. The corpus cannot say which unit a port is
  in (it holds numbers, not units); Loxone's block documentation
  (TechDoc) does. lox-cli scraped it into
  `loxone-block-types-full.json`; `tools/extract-port-meta.py` reads
  that file from a path at generation time (it is never vendored) and
  keeps, for the ports of the builtin table only, the documented unit,
  canvas label, range and default in the generated `src/port_meta.rs`.
  Decision:
  - **Documentation is a fourth, weakest evidence tier.** It never
    admits a type or decides a direction; it supplies what corpus and
    oracle cannot. Every generation cross-checks it: documented keys
    must exist on the table, documented outputs must be outputs, and the
    documented default is compared with the corpus GUI default (D30).
    On the 34 builtin types every comparable default agrees except
    `AutoJalousie.Dir` (documented −1, corpus 180), and the
    minute-documented ports' defaults (`SRoff` 30, `SSoff` −30,
    `AlarmClockPeriod` 3) agree with the corpus, which corroborates the
    unit.
  - **Suffixes scale into the port's unit.** On a port documented in
    seconds or minutes a time suffix scales exactly into that unit (`2h`
    → `120` on a minutes port, `90s` → `1.5`); a value that is no exact
    decimal number of minutes (`10s`) is refused rather than rounded.
    `%` binds only where the port is documented in percent, `K` only in
    Kelvin, and a time suffix on a `%`/`K`/`°` port is refused. A `let`
    carries its unit to every port that uses it, so the same constant
    can mean `90` on one port and `1.5` on another. Undocumented ports
    keep D27 (seconds; `%`/`K` as annotations). Extern ports are
    checked against the extern's declared type.
  - **Labels are hints only.** The canvas shows `Tr`, the XML says
    `InputTrigger`; an unknown port that equals a documented label gets
    pointed at its key. Labels never resolve a port — several are
    ambiguous or noisy in the documentation.
  - **Ranges are lint, not errors.** `lxir lint` reports
    `out-of-range` when a parameter (read in the port's unit) lies
    outside the documented range. Loxone Config clamps or accepts such
    values itself, and documentation is the weakest tier, so a compile
    never refuses one.
  - **`lxir types`** prints the table with all of this (direction,
    unit, label, range, documented and observed default, attribute
    parameters), `--json` for agents; `lxir observe --crosscheck` also
    reads the documented shape, expanding `%d` names against the
    observed keys (corpus-wide: 736 direction agreements, one conflict,
    `Intercom.API`, which is API-like).
- **D53 — DayTimer schedules from source** (2026-10-29). A DayTimer's
  switching times were GUI-owned content (D19): the `<Entry>` list, the
  `Modes=` columns, the `N=` count. Automations an agent writes need
  them ("lights on at 17:00 on weekdays"), and lox-cli writes
  schedules too. Decision: a `schedule` set and a
  `<timer>.schedule = <set>` claim, the D41 tiles shape:
  - **Format facts first** (corpus, 26 DayTimers in 145 configs, zero
    counterexamples): `Modes=` lists operating-mode UUIDs; an entry
    names its mode by index into that list (`Ix=`, absent = 0), spans
    `From=`…`To=` in minutes of the day (`From` absent = 0, `To=1440`
    = midnight), carries `V=` and an optional `NeedActivate="true"`;
    entries are ordered by `Ix`; `N=` equals the entry count. The ten
    built-in modes have fixed UUIDs `00000000-0000-000N-15…` in every
    config (holiday 1, vacation 2, day off 3, Monday 4 … Sunday 10).
  - **Built-in modes only.** User modes are objects of each config,
    with their own UUIDs; a quoted mode title is refused until
    resolving them is built. The two seasonal modes (11, 12) never
    appear in a corpus `Modes=` list and are not offered.
  - **All seven weekdays, always.** Every corpus `Modes=` list has
    them, and a save fills a missing weekday column with an all-day
    "on" entry (oracle session 18) — so a weekday the schedule does not
    name is written as an empty column (off). `UserModes=` is the
    count of the remaining modes, absent at zero (corpus-wide; the
    save computes the same).
  - **The claim owns the whole schedule** — `Modes=`, `UserModes=`,
    `N=` and every
    `<Entry>` — and nothing else: `Analog=`, `DefValue=`, `IoData`
    stay GUI-owned. Values are checked against the timer: an analog
    timer needs one per interval, a digital one takes none. Intervals
    must not overlap per day, the GUI's editor has no representation
    for that.
  - **Extern or managed.** Unlike tiles (a device only), a DayTimer is
    also mintable, so the claim applies to both; it runs as a post-pass
    over the compiled document by UUID. No lock state, no restore
    (D40/D41).
  - **Schedules are semantics** (lox#162). The diff compares every
    DayTimer's schedule per mode column, in the source's terms
    (intervals, `activate`, `= value` on an analog timer), and groups
    the columns that changed alike under the day words; `N=` and
    `UserModes=` are counts of the columns and add nothing. A column
    keys on its mode, not on its index, so a reordered `Modes=` list is
    the same schedule. The fingerprint includes the columns: v6.
    `lxir drift` accepts a v5 fingerprint recorded before.
  Validated with the oracle (session 18): see oracle-wine.md.
- **D54 — minting virtual inputs** (2026-10-29). Agent-written
  automations often need a switch the user flips in the app ("party
  mode", a setpoint), and until now the IR could only reference one the
  GUI had created. A VirtualIn is not a page block: it is an
  object of the "Virtual Inputs" folder (`<C Type="VirtualInCaption">`)
  wired across to the pages, which D34 established as legal. Decision:
  `VirtualIn` is a mintable builtin type whose element lives in that
  folder:
  - **Shape** (corpus, 175 instances): connectors `[Q, Qm]`, both
    sources; attribute order `Type, IName, V, U, Title, Cl, Nio, WF, …`;
    no `Px=`/`Py=`; fresh defaults as an oracle save writes them
    (session 18): `Cl="238,238,238"`, `WF="147456"`, `Analog="false"`
    with `MaxVal="1" MinChange="0.5" MinTime="10"` (an analog one gets
    `EnVal="true"` instead), and `<IoData>`/`<Display>` children.
  - **Placement.** Appended to the base's first `VirtualInCaption` —
    never pinned in the lock (`page_uuid`/`layout` stay empty), never
    laid out, not governed by `page` statements. A base without the
    folder is refused.
  - **`IName=`** (`VI<n>`, the designation the GUI shows) is minted once
    as one past the highest number in the base and in carried residue,
    then carried verbatim like the other input-dialog attributes
    (`connectors::carried_attrs`).
  - **Parameters.** `Analog:` (`"true"`/`"false"`), `MinVal:`,
    `MaxVal:`, `DefVal:` are attribute parameters with claim semantics
    (D40): written while bound, the GUI's otherwise. The `<IoData>`
    room/category binding and `<Display>` stay GUI-owned.
  - **Not adopted.** Existing VirtualIns stay externs (`iname:`
    matching); `adopt` refuses them. Adopting would add nothing an
    extern cannot do.
  Validated with the oracle (session 18): see oracle-wine.md.
- **D55 — device-narrowed extern matching** (2026-10-29). Identical
  hardware repeats its I/O titles: every Tree push-button interface has
  a "Taster 1", every Extension the same input names. `title:` is then
  ambiguous and decompile falls back to `uuid:`, which tells a reader
  nothing. Where a room tells the objects apart, `room:` already
  narrows; where it does not — the I/Os of an Extension in the
  distribution board share its room or have none — the device's
  **serial number** does: printed on the hardware, scanned at
  installation, never changed by the GUI. Decision: a
  `device: "<serial>"` constraint, like `room:`/`category:`, narrowing an
  `iname`/`title` match to objects whose nearest enclosing element with
  a `Serial=` attribute carries that serial (compared
  case-insensitively). Nearest, because devices nest — a Tree device
  sits in a Tree Extension inside a Miniserver, each with its own serial
  — and the I/O belongs to the innermost; the Miniserver's own I/Os and
  virtual folders narrow by the Miniserver's serial, which tells the
  Miniservers of a client/gateway project apart. Not with `uuid:` (pins
  already) or `mirrors:` (page-narrowed).
  - **Evidence** (web corpus, titled I/Os below a device whose
    type+title repeats in their config): Extensions 34, all told apart
    by device and none by room; Tree devices 204, 45 by device, 151 by
    room; below the Miniserver itself 4017, mostly repeats within one
    Miniserver (call and virtual commands), which neither resolves.
    Air devices carry no `Serial=` (their `IP=` holds a 32-digit hex id
    not verified to be the printed serial), so their I/Os narrow only
    to the Air base Extension; they are out for now.
  - **Decompile** emits `title:` plus `device:` where the title is
    unique within its device but not overall, ahead of the `uuid:`
    fallback (27 externs across the corpus's wired views).
  - A serial is personal data (`lxir scan` reports it): modules of a
    real house belong in a private repository anyway.
- **D56 — creating pages from source** (2026-10-29). D28 made a page
  title authoritative but left creating the page to Loxone Config, so an
  agent that needed room for a block chain — a nine-block feedback loop
  needs 17 088 twips in one row, the full page it belonged on had
  10 560 free on the A3 sheet (D50) — had to stop and ask a human to click "Neue Seite". Decision:
  `page "Title" (room: "…"[, category: "…"])` creates the page when no
  page carries the title.
  - **Opt-in by `room:`.** Without it a missing title stays the D28
    error, so a typo never creates a page; the error names the syntax.
    `room:` is required because the GUI requires it: its dialog keeps
    Save disabled while the room is "Nicht zugeordnet", and lxir refuses
    the unassigned `Place` (`First="true"`) likewise. The category
    defaults to the unassigned one, as the dialog does.
  - **The element** is what the GUI writes (oracle, Loxone Config
    17.2): `<C Type="Page" V=… U=… Title=… WF="16384">` with
    `<IoData Cr="<category>" Pr="<place>"/>`, `V=` the document's block
    version (D46). The GUI also writes `LtE=` (seconds since 2009-01-01,
    the last edit); the compiler leaves it out, and a GUI save of a
    compiled page does not add it.
  - **Position:** the program's pages in case-insensitive title order,
    which is where the GUI filed its new page. The GUI does not re-sort
    on load, and real lists are not sorted (`EG/OG Treppe` before
    `EG Büro`), so the page goes behind the last page sorting at or
    before it — next to its namesake — never ahead of the special pages
    (`SpNo`).
  - **Identity in the lock** (v5 `pages`, title → UUID), minted like a
    block (NextObj + 1). It makes the compile → push → download window a
    fixpoint: a recompile against the old base re-creates the same page,
    against the downloaded one creates nothing. A base titling the UUID
    differently means the page was renamed in Loxone Config: an error,
    not a second page.
  - **Ownership ends at creation.** Once the base has the page, it is
    Loxone Config's like any other: room, category, title and position
    are not enforced (a claim on the page's attributes would be D39's
    job, and nothing needs it yet), and a vanished statement releases
    the lock entry without deleting the page — deleting a page deletes
    whatever else the GUI drew on it.
  Validated with the oracle (session 19): see oracle-wine.md.
- **D57 — Automatic Designer rules from source** (2026-10-29). D37
  kept the write side closed behind three pieces of evidence. For the
  rules the house actually has — a time of day, optionally weekdays,
  and `on`/`off`/`pulse` to a button — they now exist, so a `rule`
  declaration owns what such a rule does:
  `rule <slug>` (an `AutopilotRule` extern) with exactly one
  `active`/`inactive`, `when time HH:MM`, `when weekday <days>` and
  `then <slug> "<command>"` lines.
  - **Evidence, not tables.** The `descID` of an action is a code per
    target type and command that ships only in the app. The compile
    copies it from an action in the same document with the same target
    type and command, else from the corpus-observed triples (PushButton
    `on` 342101 / `off` 342102, PButtonT `pulse` 493103), and refuses
    anything else with a message that says how to give it evidence
    (create one such action in the app). Event shapes are the two
    corpus-documented ones: time (5/272001/272, minutes) and weekday
    (3/270001/270, either a single `value` 0–6 with Monday = 0, or the
    `9999999999` sentinel with `arrValues`).
  - **Read as payload, write as edit.** The compile interprets the
    rule *as read* and writes nothing when the declaration says the
    same — the raw payload is not writer-stable (two house rules carry
    a CRLF inside the JSON), so a lifted declaration must not touch it.
    A changed rule is the app's edit: unchanged events and actions keep
    their JSON objects, a changed one keeps every field the source does
    not state (a single-day `value` stays that form, a sentinel stays
    a sentinel), only new ones are built; the text is written compact in
    the app's key order.
  - **Active means two flags.** The engine runs a rule only when
    `active` *and* `activeIfValid` are true: the house's rules sat at
    `active: true, activeIfValid: false` for three months and never ran,
    a compile that set `active` alone did not run either, and the app's
    own activation (saved as a custom change) set both and ran within
    the minute. `active`/`inactive` therefore writes both, and a rule
    reads as active only with both. `STA="1"` follows: set right after
    `APC=`, removed when inactive (10/10 corpus rules, the app's save).
  - **No creation, no deletion.** The rule block, its name, its
    `ref=` and its position stay the app's (creation came with D60); a
    declaration addresses an existing rule by extern, and removing it releases the rule
    unchanged (no lock state, like D41/D53 claims). Rules whose payload
    has any other shape (control-state conditions, operating modes, OR
    groups, several event groups) are neither lifted nor writable — the
    decompile leaves them as D37 comments.
  - **Lift.** Decompile and adopt lift every interpretable rule into a
    declaration, pulling the rule and its action targets in as
    externs; adopt + compile is byte-identical (house config: all five
    rules).
  Validated live (Miniserver firmware 17.2.8.28, house config,
  2026-10-29/30): compiles with `active` alone were pushed, verified
  byte-exact, and never ran; the app's activation of the same rule ran;
  a compile setting both flags ran at its compiled minute (00:37:00 in
  the rule log). Compiling the original declaration
  back restored the original bytes. See loxone-format.md.
- **D58 — Lighting moods from source** (2026-10-30). Asked for by a
  user: "four moods in each room, with the same colors, with all the
  buttons on the switches… Loxone Config won't let me for loop it".
  A `moods <set>` declaration lists moods by name with circuit values;
  `<controller>.moods = <set>` writes the set onto a LightController2's
  `<LightscenesC>`, also from a template body — the loop the GUI lacks.
  - **Name is identity.** Buttons, the app and `<LSConfig>` address a
    mood by `SID=`; matching by name keeps an existing mood's
    `UUID=`/`SID=`/`CID=`/flags and rewrites only its `Q<n>=`, so an
    adopted controller compiles byte-identical and a recoloured mood
    stays on its button. New moods take the lowest free `SID` from 1
    (776–778 built-in) and the lowest free `CID` above the off mood's —
    what the house's GUI-made moods show. The off mood (`SID` 778) is
    implicit; the motion mood (777) and any `<LSConfig>`-named mood
    cannot be dropped (the controller depends on them), only re-valued.
  - **Values per circuit kind, read off the moods.** The top four bits
    of a `Q` value are a property of the circuit, never of the mood (0
    inconsistencies over 648 corpus circuit columns), so the compile
    reads each circuit's kind from the existing moods and encodes
    `on`/`N%`/`rgb()`/`temp()` for it, refusing what does not fit. A
    value outside the decoded packings round-trips as a raw number.
  - **Seeding a minted controller.** Loxone Config backfills a bare
    controller's moods on its first save (oracle session 9); session 21
    recorded exactly what it writes, and a claim on a controller with
    no `<LightscenesC>` writes the same (stock moods, `<LSConfig>`,
    `<IoData>`, `<PSD>`, `<HCL>`, `<COHist>`) before applying the set.
    Its circuits then read as switch/dimmer circuits — the kind changes
    only when Loxone Config assigns an actuator.
  - **Lock state for minted moods only.** A minted mood's UUID is pinned
    (`moods`, lock v6) until a base carries it, so compiles before the
    push are a lock fixpoint; afterwards the base's `UUID=` is the
    identity and the entry retires. No other lock state: removing a
    claim leaves the moods as written (like D41/D53).
  - **Semantics.** Diff and fingerprint (v7) carry every controller's
    moods as rendered source values; drift accepts a lock's v6
    fingerprint.
  - **Lift** (2026-10-30, #170). Decompile and adopt write every
    controller's own moods as a set (one per distinct mood list, shared
    by name of the first controller) plus a claim, pulling the
    controller in as an extern — also one adopt refuses to manage, whose
    moods a claim owns all the same. The stock "Viel Licht" alone is not
    lifted. Rendering factors the majority value (`all on, AQ3 off`) and
    compiles back to the same `Q` values; adopt + compile is
    byte-identical (house config: 9 controllers; corpus: 20 controllers
    in 9 configs, each also surviving a reorder-and-restore). Lifting
    showed the off mood is not always first or last (4 of 22 corpus
    controllers keep it in the middle), so a rewrite now keeps its index.
  Validated: oracle sessions 21–22 (Loxone Config 17.2.8.28
  keeps minted and seeded moods byte-identical) and live on the house
  Miniserver.
- **D59 — the language server is a lexical layer over the pipeline**
  (2026-10-30). `lxir lsp` (`src/lsp/`) speaks JSON-RPC over stdio by
  hand, with no LSP crate: the protocol surface is fifteen methods, and
  the crate keeps its four dependencies.
  - **Positions.** The AST carries no spans, and giving it spans would
    touch every node and the `parse(to_text(m)) == m` fixpoint. The
    language is line-oriented, and a declaration always opens its line,
    so declarations are found lexically (`analysis::scan`). A parse error
    has its line; a semantic error names what it is about in backticks.
    The diagnostic goes to a `slug.port` use of two named names, else to
    a named key inside a named declaration, else to the declaration,
    else to the first use.
  - **What runs when.** On each change: what `lxir check` runs, plus the
    source lints (10 ms on the house module). On open and save, in a
    `lox.toml` project: also the compile against the base in memory, on
    a copy of the lock, plus the lints that need it, exactly what
    `lxir lint` reports (0.8 s for the house: 21 files, the same 20
    findings). Nothing is written.
  - **Identity on hover.** `resolve_extern_uuids` resolves the externs
    the way compile pins them. The last result that resolved is kept,
    so hover still shows the base object while the text does not parse.
  - **References and rename** are lexical too: the name outside
    strings and comments, not after a `.` (a port) and not before a `:`
    (an argument key), and not inside a template body that declares the
    name itself (a parameter or a block). Only module-level names are
    renamed. In a project the rename is `lxir rename` (`cli::plan_rename`,
    on the editor's buffers): the lock must be current, the output may
    change only in titles, and the lockfile is re-keyed. The server
    returns one workspace edit for the fragments and the lockfile and
    writes nothing; the next compile writes the output. A name the
    editor's buffers already hold, or a plan that fails, is a JSON-RPC
    error, so nothing is half applied.
  - **Quick fixes** come from the diagnostics: a "did you mean `X`?" on
    a single name replaces that name with `X`.
  Validated: protocol tests (`tests/lsp.rs`); diagnostics, definition,
  hover, completion, formatting and rename also in Neovim 0.12; the first four in VS Code
  (headless, through the bundled client).
- **D60 — Creating Automatic Designer rules** (2026-10-30). D57 left
  creation to the app; the oracle now answers the question it waited on
  (session 24: Loxone Config keeps a compiler-created rule). `rule
  <slug> "Name"` declares a rule by name, with the D57 body.
  - **Name finds, lock keeps.** The compile takes the base's one rule
    of that name (two of one name are refused) and pins its UUID in the
    lock (v7 `rules`, slug → UUID), like pages (D56): a base without
    the UUID gets the rule again under it, and from then on the source
    owns the name — changing it rewrites `Title=` and the payload's
    `name` instead of creating a second rule. Keyed by slug, not name,
    so that a rename in source is a rename; `lxir rename` rekeys it.
  - **Built from the base's own rules.** A new rule copies `V`, `Cl`,
    `Nio`, `WF` and `ref=` (the `AutoPilot` container) from the
    document's last rule, goes on that rule's page below the page's
    rules (gap 168, the house's), and mints its seven connectors right
    before itself, as the Miniserver does, so their UUIDs follow from
    the rule's. It starts inactive and empty; the D57 edit then writes
    the body, so a created rule takes the same evidence rules (action
    `descID`s) as an edited one. A config with no rule is refused:
    where rules live (a special page, logic pages) is the app's choice,
    and one rule made in the app settles it.
  - **No deletion.** Removing the declaration drops the lock entry and
    leaves the rule; a slug already declared otherwise, or a named rule
    that some `rule <extern>` also declares, is refused.
  Validated: oracle session 24 (byte-identical but for `WF=`, which the
  save drops from every rule). Not yet run live on a Miniserver.
- **D61 — The app catalogue follows the compile** (2026-10-30). The
  Miniserver serves the `LoxAPP3.json` of the upload as it is, so a push
  that only replaced `sps0.LoxCC` left the app on the last save from
  Loxone Config (#155). `lxir::catalogue::update` carries a compile into
  the catalogue of the config it compiled against.
  - **Text edits, not a rewrite.** Only the values that change are
    replaced; the rest stays as Loxone Config wrote it (its key order,
    its number formatting). `lastModified` becomes the push time, which
    makes the app reload.
  - **What the compile changes, nothing else.** An entry changes where
    the live and compiled objects differ in name, room, category or
    favourite flag; one whose object the compile deletes or hides goes,
    unless another entry names it (a central block's list). Staleness
    from before (an older push) stays: fixing it would mean trusting the
    rules for every entry.
  - **New entries only for evidenced types** (push buttons, digital
    virtual inputs; *LoxAPP3.json* in loxone-format.md), and only where
    Loxone Config lists them (a room and a category the catalogue knows;
    a virtual input a page uses). Like `permissions.bin`, the generator
    proves itself on the live file first: a type with a listed entry it
    does not reproduce gets no new ones.
  - **The rest is named**: a room the catalogue does not list, an icon,
    other block types; the push reports them for a save from Loxone
    Config.
  Validated: three Config saves (every entry of the three types back
  byte for byte); a push to the house and back (the served catalogue
  and `LoxAPPversion3` followed).
