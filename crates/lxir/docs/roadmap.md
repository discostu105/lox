# Roadmap

Ordered roughly by dependency, not date. The Stufen numbering comes from
the original design sketch (preserved in the git history); everything from
it that is still relevant and not yet implemented is embedded below.

## Stufe −1 — Connector database consolidation

The two pre-existing databases (lox-cli `connector-map.json`, 195 types,
missing e.g. *all* AutoJalousie inputs; lox-sim `block_signature`, 237
types) contradict each other. Results and methodology:
[connector-db.md](connector-db.md).

- [x] Aggregator: `lxir observe <cfg>...` merges evidence across a corpus;
      the merged database is committed as
      [data/connectors-observed.json](data/connectors-observed.json).
- [x] Cross-check tooling: `lxir observe --crosscheck <legacy.json>`
      (plus [../tools/extract-sim-signature.py](../tools/extract-sim-signature.py)
      for the lox-sim table). First run: legacy dbs have 3 corrupt/wrong
      direction entries and are missing whole connectors (`Remanence`,
      `Reset`) that every real instance materializes.
- [x] First verified batch fed into `connectors::builtin`: `Formula`,
      `Monoflop`, `PulseGen`, `AnalogThresholdTrigger`, and the comparator
      family (`NotEqual`, `Greater`, `Less`, `LessEqual`).
- [x] Second admission batch via the mint oracle (2026-08-25): `Memory`,
      `PushButton`, `PButtonT` — minted instances survived a Loxone
      Config save; `Memory.Q` proven an output by a surviving sourced
      wire ([connector-db.md](connector-db.md), [oracle-wine.md](oracle-wine.md)).
- [x] AutoJalousie admitted (2026-08-25): 49 connectors in the uniform
      house element order; `OutputAPI` bidirectionally evidenced →
      new `PortDir::Api`. All 16 house instances adopt — the 4 carrying
      Miniserver-created `FLG=` wire flags were unblocked the same day by
      the oracle probe that proved the flag inert (carried as D19 wire
      residue). Details: [connector-db.md](connector-db.md).
- [x] Corpus grown beyond one installation (2026-08-25): ~110 public
      configs downloaded via [`tools/fetch-corpus.py`](../tools/fetch-corpus.py)
      (official Loxone samples, LoxWiki, GitHub; local/gitignored —
      unclear licenses), V74–V273, 214 types observed. Admitted
      `PulseAt` and `DayTimer`; flipped `PushButton.OutputAPI` to `Api`
      on new counterexample evidence. Real config: 56 of 64
      managed-type blocks adopt; all 8 refusals are `Inv=`.
      Remaining leads: loxforum.com and the Loxone Library (both need
      free-account logins).
- [x] Second 2026-08-25 batch: `LightController2` (75 connectors, the
      house's flagship type), `Switch2Button`, `CentralShade`,
      `CentralLight`, `Code16` — modern house-V273 element orders, zero
      direction conflicts ([connector-db.md](connector-db.md)). Together
      with D20 (GUI-owned `Inv=` connectors, [design.md](design.md)) the
      house config now adopts **100 of 100** managed-type blocks. The
      corpus doubles as a mechanized counterexample hunt:
      `LXIR_CORPUS=corpus/web cargo test --release --test corpus`
      re-checks every admitted classification against every corpus
      config.
- [x] Demand-ranked batch (2026-10-29): `OnPulseDelay`, `OffDelay`,
      `OnDelay`, `StairwayLS`, `EdgeDetection`, `AnalogMultiplexer2`,
      `SRFlipFlop`, `State` — the most-needed unmintable types by
      lox-cli's 473-task eval set; descriptors decided by the save
      oracle (sessions 16–17; StairwayLS took the saved order), and
      the Central* `Out*` ports flipped to Output on surviving wires.
      Documented port facts (TechDoc) joined as the weakest evidence
      tier (D52).
- [x] DayTimer schedules from source (2026-10-29, D53): `schedule`
      sets claimed onto extern or managed DayTimers, the ten built-in
      operating modes, analog values, `activate`; all seven weekday
      columns always written (oracle session 18: a save fills missing
      ones with "on all day"). Open: user-defined modes (resolve the
      config's mode objects by title), and `lxir diff` does not show
      schedule changes yet (GUI-owned content).
- [x] Minting virtual inputs (2026-10-29, D54): `VirtualIn` blocks
      land in the "Virtual Inputs" folder with a fresh `VI<n>`, the
      input range as claimed attribute parameters, and the defaults a
      save writes (oracle session 18). Open: adopting existing ones
      (they stay externs); a room/category for minted ones (the D39
      claims work on externs only).
- [x] Eval harness over lox-cli's task set (2026-10-29,
      `tools/eval/`): 260 of 475 tasks use only mintable types; a
      10-case sample passes 9, and the failure is lox-sim's DayTimer model
      (it ignores `From=`/`Modes=`, so it is worth reporting upstream).
      Open: a larger sample; `test` blocks generated from a case's
      scenarios so solutions carry their own checks.
- [x] `lxir scan` (2026-10-29): personal data in a config, located but
      never quoted; CI fails on any finding in a committed `.Loxone`.
- [x] Device-narrowed extern matching (2026-10-29, D55):
      `device: "<serial>"` beside `room:`/`category:`; decompile uses it
      before falling back to `uuid:`. Open: Air devices (their id is in
      `IP=`, not `Serial=`); decompile narrowing by room, which would
      resolve more repeated Tree titles than the serial does.
- [ ] LightController2 moods from source. Surveyed 2026-10-29: a mood
      is a `<LightsceneC>` in `<LightscenesC Num= Outputs= FC=>` with a
      minted `UUID=`, a scene id `SID=` that `<LSConfig OpM= ScMv=>`
      references (777/778 are the built-in "Viel Licht"/"Aus"), a `CID=`
      of unknown meaning, `ReadOnly`/`Move` flags, and one `Q<n>=` per
      circuit whose encoding follows the circuit type (dimmer 0–100,
      switch 0/1, packed colour). Needs an oracle-driven design before
      it can be claimed; values on *existing* moods by name are the
      likely first step.
- [ ] Block `V=` on Config 17.2: the save writes `V="178"`; the
      compiler emits `175` and `adopt` refuses any other value, so
      blocks saved by 17.2 cannot be adopted yet. Needs a per-release
      table of descriptor versions (oracle per release), not a bump.

## Stufe 0 — Hardening the v0 pipeline

- [x] **Verify the I3+ index assumption** (design decision D8).
      **Answered 2026-08-25, negatively**: Loxone Config 17 gates are
      fixed two-input — the GUI cannot grow them, and a compiled `I3`
      (index 3) is silently deleted on save together with its wire.
      The compiler now refuses `I3`+ (hint: cascade 2-input gates).
      Method and evidence: [oracle-wine.md](oracle-wine.md).
- [x] Round-trip a compiled config through a real Loxone Config save and
      assert `lxir diff` semantic-emptiness (the ultimate oracle test).
      **Passed 2026-08-24**: Loxone Config 17.1 under Wine opened a
      compiled config (real base + minted `Monoflop`/`And`/wire), saved it,
      and the semantic diff came back empty — every minted UUID, wire, and
      param survived. Method and the full save-fingerprint findings:
      [oracle-wine.md](oracle-wine.md). The manual rig became a
      repeatable one-command script 2026-08-26: `scripts/oracle.sh run
      compiled.Loxone` does open → save → semantic diff → teardown
      (Xvfb+xdotool, isolated wineprefix, recovery-dialog and
      news-overlay handling built in); first scripted run blessed the
      full r50 rebuild with an empty diff.
- [ ] More verified block types: timers (`TimerDelay`…), flip-flops,
      `Switch` — prioritized by what real modules need. (The 2026-08-25
      batches covered everything the house needs; see Stufe −1.)
- [ ] Minting ports for extern types with observed (not just builtin)
      connector indexes, lifting the "port must exist in base" limitation.
      (Deprioritized 2026-08-25 on corpus evidence: of ~49k corpus
      objects that have same-type siblings, exactly **one** lacks a
      `<Co>` a sibling carries — Loxone writes every descriptor
      connector, so the limitation essentially never bites and the
      "wire it once in Loxone Config" remedy stands. Revisit only if a
      real config refuses.)
- [x] Preserve trailing comments and comments inside block bodies (D10).
      Trailing comments attach to their statement/parameter, body comments
      are body items. (Since the 2026-08-25 revision, `} # text` stays on
      the `}` line instead of being detached.)
- [x] Language revision 2026-08-25 (from a syntax review; decisions
      D13–D15): in-language lifecycle statements (`removed <slug>`,
      `moved <old> -> <new>`); `let` named constants; `set` restricted to
      extern ports; typed value tokens (fixes a `fmt` fixpoint bug with
      string values like `"5+"`); strict number literals; static
      type/port/direction validation in `lxir check` (no base needed);
      "did you mean" suggestions; `lxir check --json` for structured
      diagnostics.
- [x] Language v1 2026-08-25 (D16): constructor syntax. A block's
      parameters *and* input wires move into its declaration
      (`slug = Type("Label", Input1: sonne.Q, Input2: 28)`); wires onto
      extern ports become `target.Port <- source.Port`; `set` becomes
      plain port assignment (`target.Port = value`). v0 keywords are
      reserved with migration errors; the v1 example compiles
      byte-identically to the v0 output ([ir-spec.md](ir-spec.md),
      design decision D16).
- [x] Full-view decompile, grouped per page 2026-08-25 (D17):
      `lxir decompile` lifts every page block as an `extern` and every
      wire between lifted objects into the view (`--managed-only` for the
      adoption subset); `--out-dir` writes one self-contained module per
      logic page, foreign references annotated with their origin page.
      Groundwork for the Stufe-4 `externals.lxir` / multi-module layout.
- [x] Full-view readability 2026-08-25 (D29): `InputRef`/`OutputRef`
      plumbing wires fold behind `# mirrors <Type> "<name>"` notes on
      the ref externs (periphery objects connected only by plumbing stay
      out entirely), the `<-` pile is sorted by sink then source, and a
      block label the slug already encodes is dropped. All three are
      full-view only — the adoptable `--managed-only` view keeps
      document order and exact labels, because those are compiled
      bytes. Real config: 2667 → 1818 lines, 486 wires folded, 674 →
      396 externs.
- [x] Observed-default parameter elision 2026-08-25 (D30): the full
      view hides a lifted parameter whose value equals the GUI default,
      defined statistically over the corpus (modal `Def=` at ≥90% share,
      ≥10 occurrences; `tools/extract-defaults.py` →
      docs/data/param-defaults.json evidence + generated
      src/observed_defaults.rs). `--all-params` shows everything, the
      report counts elisions, `--managed-only` never elides. With D29
      the real config's full view halved: 2667 → 1332 lines.
- [x] Removal tombstones 2026-08-25 (D31, lockfile v2): every withdrawal
      (block, extern wire, Def write) leaves a tombstone that keeps
      re-applying it against pre-deployment bases and retires when a
      base without it is witnessed. Closes the compile → push window
      hole found on the real config (recompile passed the removed
      cascade through unmanaged, double-driving the alarm input); the
      window is now a committable lock fixpoint. `lxir drift` names
      pending removals instead of blaming another writer.
- [x] `mirrors:` ref matcher 2026-08-25 (D32): `extern x =
      InputRef(mirrors: status_alarm)` resolves a ref through its
      `Ref=` attribute (format fact: it names the mirrored object's
      UUID) instead of a uuid pin; page statements narrow duplicates,
      ambiguity is refused, pin conversions are verified, and a pinned
      `mirrors:` re-confirms every compile. Decompile/adopt emit it
      where the target has a slug and the match is unique.
- [x] Composite extern matching 2026-08-25: `extern x = Type(title:
      "Deckenlicht", room: "Büro")` — `room:`/`category:` narrow an
      iname/title match via the object's `<IoData Pr=/Cr=>` reference
      to a Place/Category title. Format fact validated corpus-wide
      first (~36 900 occurrences, zero counterexamples in real
      configs; [loxone-format.md](loxone-format.md)).
- [x] Unit-suffixed values 2026-08-25 (D27): `Time: 90min`, `1.5h`,
      `250ms`, `2700K`, `70%` — the suffix scales exactly into the base
      unit at compile (byte-identical to the plain number) and is the
      value's canonical spelling. Per-port units followed 2026-10-29
      (D52): suffixes scale into the documented unit (minutes ports
      too), `Time: 70%` is refused, ranges are linted, `lxir types`
      shows the table.
- [x] Identity hardening 2026-10-18 (D38): a base with repeated
      `<C>`/`<Co>` UUIDs is refused by `compile`/`adopt`, the compile
      output is self-checked, `diff` warns and `drift` notes repeats;
      an edited `uuid:` value on a pinned extern re-points it instead of
      being ignored. Found by the r50 drift forensics (a push tool had
      given all mirrors of one target the same port UUIDs).

## Stufe 1 — Templates

- [x] Templates 2026-08-25 (D23): `template fassade(jalousie:
      AutoJalousie, schwelle = 28, pos = 70) … end` declares a reusable
      body; `sued = fassade(jalousie: jal_sued, pos: 55)` instantiates it
      (lowercase callee = template, PascalCase = block type). Pure macro
      expansion before compile: body slug `hoch` becomes `sued_hoch`, and
      the **expanded** slug keys the lockfile — re-instantiation never
      re-mints, body edits mint only the additions per instance,
      `removed`/`moved` apply per expanded slug. Object params pass slugs
      (annotation checked against the declared type when known); value
      params default and are overridable with literals or `let` refs; free
      body identifiers capture module externs/lets. Body limited to
      blocks/wires/sets/comments — nesting and template-local
      `let`/`extern` deferred. Spec: [ir-spec.md](ir-spec.md), rationale:
      design.md D23.

## Stufe 2 — Expression sugar

- [x] Discrete backend 2026-08-25 (D24): `jal_sued.AutoShade <- sonne.Q
      and aussentemp.Q >= schwelle` desugars — before compile, like
      template expansion — into the verified gate/comparator blocks,
      each labeled with its sub-expression so the rule stays readable on
      the Config canvas. Precedence `or` < `and` < `not` < comparison;
      operand order preserved (lhs → Input1); constants become `Def=`,
      ports become wires; `and`/`or`/`not` reserved. Synthetic slugs
      `<sink>_<port>__<op><n>` key the lockfile like hand-written blocks
      but are marked `expr_owned`: an unchanged expression never
      re-mints, and editing one auto-removes its orphaned blocks — no
      `removed` statement, the expression is their single source of
      truth. Composes with templates (desugar runs after expansion).
      Also fixed en route: the minter now seeds past the lock's
      `next_mint` high-water mark and every locked UUID, so a block
      minted in a later compile session can never collide with an
      earlier one at an identical mint time. Spec:
      [ir-spec.md](ir-spec.md), rationale: design.md D24.
- [x] `formula` backend — shipped as the *arithmetic* backend
      (2026-08-25): `+ - * /` in an expression desugar to ONE `Formula`
      block per maximal arithmetic tree (`verbrauch.AQ / 1000` →
      `Formula: "I1/1000"`), inputs `I1..I4` in first-appearance order
      with repeated ports deduplicated, constants (numbers, numeric
      `let` refs) inlined, slug operator `f`, result `AQ`. Standalone
      only — under gates/comparisons arithmetic errors at parse time
      (unverified analog-into-gate wiring), and boolean logic keeps the
      discrete backend (readable on the canvas); a per-expression
      backend switch is rejected. The full Formula grammar (`IF`,
      functions) stays on the explicit block. Rationale: design.md D35.

## Stufe 3 — Verification loop

- [x] `lxir test` shipped 2026-08-26 (D36): `test "Name" … end` blocks
      in the module itself — injections (`slug.Port = value`),
      `tick <n> [dt <s>]`, `expect slug.Port <cmp> <value>` (`==`, `>`,
      `>=`, `<`, `<=`, `~=`), `clock "HH:MM"` / `"YYYY-MM-DD HH:MM"`.
      The command compiles in memory against the lockfile, maps slugs to
      simulator addresses ("Title.Port", room-qualified on duplicate
      titles), generates SimSpec JSON and shells out to `lox sim run`
      (discovered via `--lox`, `$LOX`, or PATH), then attributes each
      flattened check back to its `expect` line. Tests survive `fmt`,
      `rename`, decompile-adopt; references validate on the flattened
      (expanded + desugared) form so expects can name template-expanded
      and desugared slugs. The sketch's `given`/`after` braces became
      line-oriented statements to match the rest of the DSL. Simulated
      clock: `lox sim`'s ClockSpec already existed — the "known gap"
      note was stale. Along the way lox-sim (local fork) learned to
      honor wires into parameter connectors (Formula Input1–4 arrive as
      params); the pool example's PV test exposed the bug.
- [x] CI recipe: `examples/ci.sh` runs `lxir test` when a `lox` binary
      is available (`LOX=` or PATH), skips with a note otherwise.
- [x] CI recipe 2026-08-25: `examples/ci.sh`, a tree-untouched check path
      for a repo holding lxir sources — `check`, `fmt --check`, lock
      currency (compile against a lock copy must change nothing),
      byte-determinism (second compile identical), optional
      byte-comparison against a committed `EXPECTED` output; `--sync`
      additionally requires empty semantic diff and green drift against
      the base (the post-push/download state). Exercised by
      `.github/workflows/ci.yml` on the shipped example, alongside cargo
      test / clippy / fmt.

## Stufe 4 — Multi-module projects

The sketch's target shape for a whole installation:

```text
haus/
  lox.toml            # target Miniserver, paths, options
  haus.lock.json      # identities — generated, but committed
  externals.lxir      # externs for everything Loxone Config owns
  rooms/…  systems/…  templates/…
```

- [x] Multi-module projects, complete 2026-08-25 (D25) — in three steps,
      and with the considered `import` statement **rejected, not
      deferred**: fragments share one namespace and one lockfile, and
      the compiler merges everything into the one `.Loxone` document, so
      an import would declare a dependency with no semantic consequence.
      First step: **module directories** — `check`, `fmt`, and
      `compile --module` accept a directory of `*.lxir` fragments,
      merged in path order (since the third step: recursively, so
      `rooms/…` nests); fragments parse individually (errors name the
      file) and may reference sibling-file slugs, with name resolution
      running once on the whole. One file per page is the convention
      (the house repo's `pages/` layout). Second step: `adopt --out-dir`
      writes the adoption directly in that layout — one fragment per
      page plus `_periphery.lxir` (externs; sorts first), concatenation
      identical to the `--out-module` single file, same lockfile, dir
      compile byte-identical. Third step: **`lox.toml` project files** —
      base, module, lock, out, serial, page in flat `key = "value"`
      lines (strict TOML subset, hand-parsed, no new dependency);
      `lxir compile` inside the directory needs no flags, flags override
      the file, and `check`/`fmt`/`drift` default to the project's
      module and lock. The sketch's ConfigVersion pin lives in the
      lockfile instead (D22), where qualification state belongs.
- [x] `page "Title"` placement statements 2026-08-25 (D28): which page
      a block is drawn on is source, not just a lock pin. Positional
      (governs the block declarations that follow; expression blocks
      land under their expression), authoritative on every compile (a
      pin still matching the declared title is kept — adopted blocks
      never move behind your back — any other moves to the first
      matching page, a missing title errors), and emitted by
      decompile/adopt as real section headers in place of the old
      `# page:` comments. Corpus adoption fidelity unchanged.
- [x] `adopt` (whole-config form) 2026-08-25 (D18): `lxir adopt <cfg>`
      moves every managed-type block under source control — the
      managed-only module plus a lockfile pinning existing object/port
      UUIDs, layout, and page, so the first compile rebuilds in place
      instead of minting duplicates. Verified per block (unfaithful
      rebuilds are skipped with a reason, e.g. GUI input inversion
      `Inv=`); acceptance on the real house config: adopt → compile →
      semantically empty diff, recompile byte-identical. Brought page
      pinning (`page_uuid` in the lock) and attribute parameters
      (`Formula:`) with it.
- [x] GUI-owned residue carried forward (D19, 2026-08-25): rebuilds
      re-emit display attributes (`Cl`/`LtE`/`WF`, `Tp=`, `Sun=`,
      `SpStates=`, `NDOC=`, `Stats*=`), visualization children
      (`IoData`/`Display`/`PSD`/`COHist`), and `FLG=` wire flags
      verbatim from the base, and adoption accepts them. Real-config
      coverage went from 22 to 53 of 59 managed-type blocks (the rest
      are genuine `Inv=` inversions); the rebuild is a semantic no-op
      with **zero** changed lines (position-only diff), oracle-blessed
      by a Loxone Config open+save.
- [x] `adopt` (incremental form) 2026-08-25: `lxir adopt <cfg> --uuid
      <uuid> --as <slug> --module <m> --lock <l>` adopts one existing
      block (e.g. freshly drawn in Loxone Config) into an existing
      module/lock pair. Appends the declaration to the module (in a
      directory, to its page's fragment), pins externs for wired
      neighbors the lock does not already know, references
      already-pinned objects by their existing slugs, and re-baselines
      the drift fingerprint. Verified in memory before writing: the
      updated pair must rebuild the config as a semantic no-op. A wire
      into a *managed* sink is refused unless the sink's declaration
      already states it (the compiler would tear it down otherwise) —
      the error names the exact argument-list line to add, and the
      module loads leniently so that line may reference the new slug
      before it is declared. Promoting an existing extern to a managed
      block stays unsupported for now.
- [x] Drift fingerprint 2026-08-25 (D21): `semantic_fingerprint` hashes
      exactly the projection the semantic diff compares (locale-suspect
      titles excluded), recorded in the lock at adopt/compile; `lxir
      drift <cfg> --lock <lock>` detects "another writer changed
      something" from one parse — no reference config, no full diff.
      Save noise, position moves, and locale renames don't fire it.
- [x] ConfigVersion pin policy 2026-08-25 (D22): compile refuses a base
      whose ConfigVersion differs from the lock's pin; qualifying a new
      Loxone release = one oracle open+save run, then
      `--accept-version <v>` re-pins explicitly (the value must match
      the base exactly — no accidental double-bump acceptance).
- [x] A complete end-to-end showcase module 2026-08-25: the sketch's
      `pool` idea — water temperature below target, cover interlock,
      PV-surplus enable for the heat pump — as
      `examples/ir/pool.lxir` against `examples/configs/pool.Loxone`,
      demonstrating lets, iname externs, composite `room:` matching,
      the fixed-two-input gate cascade (D8), and an extern wire.
      Kept compiling by `tests/ir.rs` (fixpoint + canonical form).

## Devices: assignment from source

Devices and their inputs/outputs are created in Loxone Config (pairing,
serials, channel layout); what a source file can own is how existing
ones are *assigned*.

- [x] Attribute claims 2026-10-18 (D39): `x.title`, `x.room`,
      `x.category` on externs, with followers (the device's children
      that shared its room/category move along), D31 tombstones on
      withdrawal, room/category bindings in the semantic diff and
      fingerprint (v2), lockfile v3. Accepted against a real device
      repurposing on the house config: the compiled claims equal the
      hand patch they replace.
- [x] Ref titles follow title claims 2026-10-29 (D43): a `.title` claim
      rewrites the refs whose title the GUI derives from the object (all
      but an alarm block's collected inputs), and minted mirrors take
      the claimed title — the next GUI save no longer shows the old
      title on refs as drift.
- [ ] Membership lists: blocks that list devices they act on (the smoke
      alarm's detector list and similar) keep that list inside the
      block, not on the device. Needs a corpus survey of the list
      formats and an oracle proof before a `members:`-style claim is
      admitted.

## Program blocks: PicoC from source

- [x] Program code from source 2026-10-29 (D40): `Code: file("prog.c")`
      on Code16 claims the program text; an unclaimed program is carried
      verbatim. Numeric character references are read as text, as Loxone
      reads them, and `lxir drift` lists every attribute that holds one.
      Program text joins the semantic diff (as a line diff) and the
      fingerprint (v3). Accepted on the house config: it restored the
      Hue program that an `&#xA;`-encoding push had collapsed to a
      single comment line, byte for byte.
- [x] Repair 2026-10-29 (D44): `lxir repair <cfg> --out <out>` writes a
      copy with numeric-reference text (`&amp;#xA;`, `&#xA;`) replaced
      by the raw characters, byte-exact elsewhere, listing each
      attribute it rewrote. Covers the damage `Code:` cannot claim:
      `APC=` rules, `NDOC=` texts, programs left with the GUI.
- [x] Oracle probe: does a Loxone Config open+save keep a compiled
      program byte-identical? Yes (session 13, D44): the house's
      compiled program and a corpus program both came back unchanged.
- [ ] `Task=` (the program's run schedule) stays GUI-owned until a
      corpus survey shows its format.

## Automatic Designer (Automatik Designer)

The app's rule engine; each rule is an `AutopilotRule` block whose whole
logic sits in its `APC=` JSON attribute — format facts in
[loxone-format.md](loxone-format.md), scope decision D37 in
[design.md](design.md). No public spec exists (checked the official API
docs 2026-08-26); the schema is corpus-derived from 10 real rules.

- [x] Read side 2026-08-26 (D37): `APC=` joins the semantic projection —
      an app-edited rule shows up in `lxir diff` (as JSON field paths),
      moves the fingerprint, and trips `lxir drift`; the decompile full
      view renders each rule as `# when … / # then …` comment lines with
      control UUIDs resolved to titles.
- [ ] Oracle probe: does a Loxone Config open+save round-trip `APC=`
      byte-identically (key order/whitespace), and does it preserve a
      foreign-minted rule at all? Cheap to answer with
      `scripts/oracle.sh`; prerequisite for any write-side work.
- [ ] Authoring (`rule` statements compiling to `AutopilotRule` blocks)
      is gated on evidence that does not exist yet: the `descID`/
      event-type tables (they ship only inside the Loxone app bundle),
      the oracle answer above, and a live-Miniserver check that a
      compiled rule actually *executes* (silent non-execution is the
      D8-class risk). Until then: refuse.

## Tooling & ecosystem

- **Refactoring**
  - [x] `lxir rename <old> <new>` 2026-08-25: renames a module-level name
        (extern, block, constant, template, instance) across every module
        file — comments included — and rekeys the lockfile so every pinned
        identity survives. Synthetic slugs are covered by expanding and
        desugaring the module before and after the rename and pairing the
        item lists positionally (template bodies `<instance>_<body>`,
        expression blocks `<sink>_<port>__<op><n>`). Verified before
        writing: the baseline lock must be current, and the recompiled
        output must be byte-identical except for `Title=` labels the slug
        itself feeds (auto-labeled blocks, D24 expression labels); the
        project's out file is refreshed alongside. Born from the r50
        Slug-Kur, which needed a hand-written script plus `moved`
        statements for 170 renames.
  - [x] `lxir lint` 2026-08-25: advisory findings a compile has no
        business rejecting. Source layer: unused externs/constants (after
        expansion + desugaring, so template captures and expression
        operands count as uses), uninstantiated templates. Project layer:
        managed blocks whose outputs feed nothing in the *compiled*
        config — GUI-drawn wires count as consumers, a block reaching
        only dead ref plumbing is still dead, and side-channel actors are
        exempt (central blocks command through `rec=` uuid lists, `Code16`
        acts through its program). Findings name their declaring
        fragment; exit 1 on findings, but deliberately not part of the
        compile path (reference externs kept as documentation are a
        legitimate pattern). First run on the house config surfaced one
        real dead block and three suspicious app-visible tasters.
- **Layout**
  - [x] Placement in the GUI's geometry, 2026-10-29 (D42). New blocks
        go beside what they wire to, per page, as left-to-right layered
        clusters, and minted ref tags go beside their port. A drag in
        Loxone Config is taken over into the lock. `--relayout <page>`
        places a page afresh, except blocks wired to refs drawn in the
        GUI. `lxir render` draws a page as SVG with layout metrics. On
        a kitchen page of the house, backward wires went from 15 to 5.
  - [x] Placement keeps to the paper sheet, 2026-10-29 (D50). A3
        landscape from the Document's `PaperFormat`. A cluster short of
        room is packed tighter, and goes past the edge only if it fits
        nowhere. `compile` warns about managed objects past the edge.
  - [ ] Oracle check: a relayouted page survives a Loxone Config save
        unchanged (footprints are from corpus statistics; the snap on
        save is observed, but no relayouted page has been through the
        oracle yet).
  - [ ] Geometry claims: let the source move ref tags and extern blocks
        that the GUI owns. Today a relayout moves only managed blocks.
- **Editor**
  - [x] VS Code: syntax highlighting + snippets (`editor/vscode/`).
  - [ ] LSP server (`lxir lsp`): diagnostics from `Module::parse`/`validate`
        (already line-precise), completion for port names sourced from the
        base config + builtin table, go-to-definition for slugs, hover with
        resolved extern identity. The library API was shaped so this is a
        thin layer.
  - [ ] tree-sitter grammar (enables highlighting beyond VS Code and
        structural editing for agents).
- **Distribution**
  - [x] License: dual GPL-3.0-or-later / commercial, the same scheme as
        `lox` — see [../LICENSE](../LICENSE). Caveat before accepting
        outside PRs: selling commercial licenses requires unified
        copyright, so external contributions need a CLA (or equivalent) —
        undecided until it becomes relevant.
  - [x] crates.io publication: `lxir` 0.1.0 published 2026-08-26
        (full package metadata, dev-trees excluded, 264 KiB).
  - [x] Release workflow 2026-08-26: a `v*` tag builds and attaches
        binaries for Linux (musl), Windows, and macOS (both arches) —
        `.github/workflows/release.yml`; v0.1.0 is the first tag.
  - [x] Stranger onboarding 2026-08-26: README "Try it on your own
        config" — install, download/extract via lox-cli, decompile,
        adopt one block, compile, read the diff, push; plus the
        "admit a type yourself" recipe in
        [connector-db.md](connector-db.md) (evidence → rules → table
        entry → one-command oracle proof).
  - [ ] `lox-cli` adopting the crate for its config model (the intended end
        state), wiring transport to `lxir compile` output.

## Explicit non-goals (unchanged)

Transport, LoxCC, credentials (stay in `lox`/`lox-cli`); simulating block
semantics (stays in `lox-cli sim`); replacing Loxone Config for hardware/
visualization authoring.
