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
compiler. The compiler therefore claims ownership of exactly three kinds of
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

Everything else is somebody else's, and passes through unchanged.

## The compile strategy: tear down, rebuild

Each compile:

1. Validates the module; refuses if the lock knows a managed slug the source
   lost (see *Removal trichotomy* below).
2. Resolves externs (lock pin first, then match spec; ambiguity is an error).
3. **Tears down** its previous output from the base document: removes all
   locked managed objects, removes all locked extern wires, restores all
   recorded set-originals. What remains is exactly the state owned by the
   other two writers.
4. **Rebuilds** from source: plans port lists, mints missing UUIDs, emits
   managed `<C>` elements, applies wires and sets, records everything in the
   lock.
5. Writes counters (never decreasing) and target metadata.

This makes compilation *convergent*: compiling the compiler's own output
again is a byte-level fixpoint (tested in `tests/ir.rs`), which is what makes
the real workflow — compile → upload → Config saves → download → compile —
safe.

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
  missing title is an error. Creating pages stays with Loxone Config.
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
  usually collapse to one signal port, no ambiguity in the bytes — and
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
