# Connector database consolidation

The roadmap's Stufe −1: the two pre-existing connector databases contradict
each other, so lxir builds its own from **evidence** — every real config is
a test case. This doc records the methodology, the first consolidation run,
and the admission rules that gate `connectors::builtin` (the table of
mintable types).

## The three knowledge sources

| Source | Shape | Trust |
|---|---|---|
| **Corpus evidence** (`lxir observe`) | per type/key: connector index from the port UUID, wired-as-sink / wired-as-source / `Def=` counts | ground truth for whatever it covers |
| lox-cli `docs/schemas/connector-map.json` (195 types) | `type → {c: [order], t: {key: I/O/P}}` | good ordering info; several corrupt entries (see findings) |
| lox-sim `block_signature` (237 types, extracted via [`tools/extract-sim-signature.py`](../tools/extract-sim-signature.py)) | (inputs, outputs, params) per type | best direction coverage; some simulator-internal key names diverge from XML keys; no index order |

## Reproducing the run

```sh
tools/extract-sim-signature.py ~/repos/my/lox-cli > sim-signature.json
lxir observe corpus/*.Loxone \
    --crosscheck ~/repos/my/lox-cli/docs/schemas/connector-map.json \
    --crosscheck sim-signature.json > corpus-report.json
```

The merged evidence (no identity data — types, keys, indexes, counts only)
is committed as [`data/connectors-observed.json`](data/connectors-observed.json)
and regenerated whenever the corpus grows.

## The 2026-08-24 run (6 real configs, 102 types observed)

- **connector-map.json**: 33 types comparable; after normalizing its `P`
  (param) to input-like, 3 direction conflicts remain — two of them corrupt
  legacy entries (`InputRef`/`OutputRef` store the *key name* as the
  direction), one real disagreement (below).
- **sim-signature**: 61 types comparable, exactly 1 direction conflict.
- **`Intercom.API`**: both legacy dbs call it an output; the corpus has
  wires *into* it. The corpus wins — legacy-db bug (or a mixed-direction
  API connector; either way not trustworthy).
- **Both legacy dbs are incomplete.** e.g. `Monoflop` materializes five
  connectors in every real instance (`InputTrigger`@0, `Reset`@1,
  `Remanence`@2, `Time`@3, `Q`@4); both dbs list only three
  (`InputTrigger`, `Q`, `Time`). Loxone Config emits a `<Co>` for *every*
  connector (design D4), so corpus instances reveal the complete table
  including index order — which no legacy db has at all.
- **`OutputAPI` is genuinely mixed** across types (20 sinks, 50 sources in
  the corpus) — it must be resolved per type, never by name.

## Admission rules for `connectors::builtin`

A type becomes mintable only if **all** hold:

1. **Complete, stable shape**: every instance in the corpus materializes
   the same key set, with identical connector indexes, contiguous from 0.
   (≥3 instances. There is no variadic growth: Loxone Config deletes
   off-descriptor connectors on save — design decision D8.)
2. **Every port's direction resolved**, in order of preference:
   - corpus wire evidence (sink→Input, source→Output, `Def`-only→Param);
   - agreement of a legacy db, with zero corpus contradiction;
   - the **inert-flag rule**: a key that is never wired and never carries
     `Def` *anywhere in the corpus* (e.g. `Remanence`: 201 occurrences over
     11 types, zero evidence in any direction) is classified `Input`. This
     is negative evidence, not a guess-by-name: wiring *into* such a port
     stays possible, wiring *from* it is refused, and one observed
     counterexample evicts the classification.
3. **No unresolved key remains.** One undetermined port keeps the whole
   type out (that is what blocks `PulseAt` and `DayTimer` today).

A fourth path can resolve what the corpus cannot: the **save oracle**
([oracle-wine.md](oracle-wine.md)). Loxone Config deletes anything
off-descriptor on save (D8), so a minted minimal instance surviving an
open+save proves the shape, and a compiled wire sourced from a port
surviving the save proves that port is an output.

Hardware/IO reference types (sensors, actors, `InputRef`/`OutputRef`,
Tree/Air/Modbus devices) are **never admitted** regardless of evidence —
they carry device bindings; lxir references hardware, it does not own it
(see [vision.md](vision.md)).

## Block versions

Every `<C>` carries a block version, `V=`, one per document: Loxone
Config writes all objects at its release's version on save. The table
is verified for the versions in `connectors::VERIFIED_BLOCK_VERSIONS`:
175 (17.0, 17.1) and 178 (17.2, oracle session 14). A new release's
version joins the list after the mint oracle has open+saved every
builtin type at that version, and a real rebuild, under that release
(design decision D46). Until then compile writes its blocks at 175 and
adopt refuses blocks of that version.

## Admitted in the first batch

| Type | Ports (index order) | Basis |
|---|---|---|
| `Formula` | Input1‑4 → AQ, TQ | 39 corpus instances; TQ direction from lox-sim agreement |
| `Monoflop` | InputTrigger, Reset, Remanence, Time(P), Q | 20 instances; Reset sink-evidenced; Remanence inert-flag |
| `PulseGen` | InputEnable, InputInvert, Remanence, TimeHigh(P), TimeLow(P), Q | 5 instances; InputInvert agreed by both dbs |
| `AnalogThresholdTrigger` | Input, Remanence, On(P), Off(P), PulseTime(P), Q, RisingEdge, FallingEdge | 4 instances; edge outputs from lox-sim agreement |
| `NotEqual`, `Greater`, `Less`, `LessEqual` | Input1, Input2, Q | comparator family: both dbs agree on the shape shared with live-verified `Equal` |

Kept out of the first batch, with the blocking key: `Memory` (`Q` —
absent from both dbs, no corpus wires), `PulseAt` / `PushButton` /
`DayTimer` (`OutputAPI`, plus `RtD`/`AQm`/`AQmt` on DayTimer).

## Admitted 2026-08-25 via the mint oracle

Minted minimal instances of three of the blocked types survived a Loxone
Config open+save on the Xvfb rig (evidence and healing details in
[oracle-wine.md](oracle-wine.md)); `Memory.Q` was proven an output by a
compiled wire sourced from it surviving the save.

| Type | Ports (index order) | Basis |
|---|---|---|
| `Memory` | Input, AQ, Q | 7 corpus instances; `Q` direction from the oracle probe |
| `PushButton` | InputTrigger, On, Reset, InputDisable, Remanence, Q, Qon, Qoff, OutputAPI | 5 instances; whole shape oracle-survived, `OutputAPI` resolved by the inert-flag rule for this type |
| `PButtonT` | InputTrigger, Reset, InputDisable, Remanence, Time(P), Q, OutputAPI | 8 instances; same basis as PushButton |

The GUI heals these types with visualization children (`IoData`,
`Display`, `PSD`) and display attributes (`Tp=`, `SpStates=`) the
compiler never authors. Since design decision D19 that content is
**GUI-owned residue**: rebuilds carry it forward verbatim from the base
and adoption accepts it, so GUI-created instances adopt cleanly (the
only remaining refusals in the real config are genuine `Inv=` input
inversions).

`PulseAt` and `DayTimer` stayed out of this batch (each had unresolved
keys) — both were admitted later the same day when the web corpus
resolved them (below).

## AutoJalousie, admitted 2026-08-25

The strongest evidence base yet: 78 corpus occurrences, 16 of them the
house's Config-17 instances with an **identical 49-key element order**.
The corpus-wide `index_conflict`s are older-generation configs plus
connectors added later by schema migration (their UUIDs, minted on the
house Miniserver, reuse index bytes — `Rdd`, `TargetPos`) — the element
order, not the UUID index byte, is the canonical order.

Directions: `EndUp`/`EndDown` corpus-sink ×70; 18 params carry `Def=` on
every instance with both legacy dbs agreeing; the 9 `Output*`/`TargetPos`
outputs agree across both legacy dbs with zero corpus contradiction; the
19 never-touched keys classify Input by the inert-flag rule.

`OutputAPI` forced a model extension: the house wires **into** it on 4
instances and **from** it on 12 (corpus: 12 sink / 36 source) — the API
connector is genuinely bidirectional. It is classified `PortDir::Api`:
valid as wire source *and* sink, never a `Def=` target. (PushButton/
PButtonT keep `OutputAPI` as Output — zero sink evidence there; one
observed counterexample flips them to Api.)

4 house instances initially refused adoption over `FLG="1"` on their
incoming API wires (Miniserver/app-created wire metadata). The oracle
probe — strip the flag, open + save — showed Loxone Config treats it as
inert stored state: round-tripped verbatim, never regenerated, absence
accepted. Since then `FLG=` is carried forward per (sink, source) pair
as D19 wire residue and **all 16 house instances adopt**
([oracle-wine.md](oracle-wine.md)).

## The web corpus (2026-08-25): 149 configs, 214 types observed

The corpus grew from one installation to ~110 downloaded public configs
plus the house's three generations, spanning `ControlList` Version
74–273: official Loxone KB sample files (25 zips, one per block domain),
LoxWiki attachments (41 community configs — Modbus, irrigation, Sonos,
heat pumps, ...), and GitHub projects (~50 files: university coursework
houses, an empty-project baseline, the ONOKOM template collection,
lox-cli's golden configs). [`tools/fetch-corpus.py`](../tools/fetch-corpus.py)
reproduces the download; the files themselves stay **local and
gitignored** — most carry no license (one file, attribute-reordered by a
third-party tool, is quarantined as non-evidence). Untapped leads:
loxforum.com attachments and the official Loxone Library, both behind
free-account logins.

Type coverage doubled (102 → 214). Consequences applied:

- **`PushButton.OutputAPI` flipped Output → `Api`**: the corpus now
  shows 2 sink + 2 source wires — the predicted counterexample. The
  original Output classification (batch 2) was a name-prior the
  admission rules never supported.
- **`PButtonT.OutputAPI` corrected Output → Input**: still zero evidence
  in any direction (20 occurrences) and absent from both legacy dbs —
  the inert-flag rule classifies exactly this as Input. Same for the
  zero-evidence `OutputAPI` of the two new types below.

## PulseAt and DayTimer, admitted 2026-08-25 (web corpus)

| Type | Ports (element order) | Basis |
|---|---|---|
| `PulseAt` | InputDisable, Remanence, Time(P), Q, OutputAPI | 16 modern instances (V254–V273, incl. the house) with an identical element order; `Q` corpus-source ×23; `Time` carries `Def=` on every instance, connector-map agrees `P` |
| `DayTimer` | InputTrigger, Reset, RtD, PulseTime(P), Remanence, Manual(P), Mode(P), AQ, Qon, Qoff, AQm, AQmt, OutputAPI | 13 modern instances; InputTrigger/Reset corpus-sink, AQ/Qon/Qoff corpus-source; AQm/AQmt both legacy dbs Output, zero contradiction; Manual/Mode/PulseTime connector-map `P`; RtD/Remanence inert-flag |

The DayTimer order is the **V259+ order**: the V259 schema migration
moved `PulseTime` from element position 6 to 3 while keeping its port
UUID (the house instance has index byte `06` at element position 3) —
the strongest confirmation yet that element order, not the UUID index
byte, is canonical. Compiling against a pre-V259 base would need the
roadmap's ConfigVersion pin policy.

Both types keep GUI-authored logic in element attributes and children
that the IR cannot express; per D19 it is carried forward verbatim:
`Sec=`/`Typ=`/`AutP=` (PulseAt fire time and mode), the `<Entry>`
schedule children with their `N=` count, `Analog=`/`DefValue=`/`On=`/
`Off=` output behavior, `Modes=`/`UserModes=` operating-mode gating,
`Desc=`. Decoding `Sec=` (seconds since midnight) far enough to author
it as an attribute parameter is future work. A fresh **mint** of either
type has not been oracle-proven yet — the adopted rebuilds are
byte-identical to GUI-authored content, so only minting needs the
Stufe-0 gate before first use.

With this batch the real config adopts **56 of 64** managed-type blocks;
all 8 remaining refusals are genuine `Inv=` input inversions (five
PushButton, one PulseGen, both DayTimers).

## The second 2026-08-25 batch: LightController2, Switch2Button, CentralShade, CentralLight, Code16

Analysis rerun over the 145-file corpus (web corpus + the post-push house
download): per-instance element orders grouped into variants, directions
from the committed evidence db plus both legacy dbs. Shared result: the
house's Config-17 (V273) element order is uniform across every house
instance and canonical; corpus variants are older generations whose
divergence is pure schema migration — appended keys (`OutputAPI` on
Switch2Button, `OutOpen`/`OutClose` on CentralShade, `OutActive` on
CentralLight), one insertion (`I5`–`I8` into LightController2's V259
order), one param reorder (LightController2 pre-V259, the DayTimer
story), one swap (Switch2Button `On`/`Reset`). Every key resolved with
**zero direction conflicts** across all five types.

| Type | Keys | Basis |
|---|---|---|
| `LightController2` | 75 | The largest admitted type. 23 house instances, one identical order; 86 corpus occurrences. 17 corpus-sink inputs (`Sel1` ×47, `Brightness` ×19, `Move` ×15); 16 Def-on-every-instance params + 5 connector-map-agreed; `AQ1`–`AQ20`/`Scene`/`OutputReset`/`OutputResetAll` connector-map O (`AQ1` corpus-source ×131); `MoveOn` carries Def everywhere *plus* one sink wire — wire evidence precedes, Input still admits the Def; `OutputAPI` zero-evidence → inert-flag Input |
| `Switch2Button` | 9 | 49 occurrences; `InputTrigger` sink on every instance, `Q`/`Qon`/`Qoff` corpus-source ×16/×33/×15, `Time` Def everywhere, rest inert-flag |
| `CentralShade` | 18 | 11 occurrences; `EndUp`/`EndDown`/`AutoShade`/`Safety`/`Gesture` corpus-sink, classic inputs connector-map I, `OutputAPI` connector-map O, appended `OutOpen`/`OutClose` inert-flag Input |
| `CentralLight` | 24 | 9 occurrences; `Reset`/`On`/`Alarm` corpus-sink, rest connector-map I, `OutputAPI` connector-map O, appended `OutActive` inert-flag Input |
| `Code16` | 34 | The most stable shape in the corpus: ONE identical element order across V99–V273 (12 instances, 8 files, house included). `AI1`–`AI13` corpus-sink, `TQ1`/`AQ1`–`AQ13`/`TeQ` connector-map O mostly corpus-source, `Remanence` inert-flag |

The batch surfaced three new **GUI-owned content** classes, all carried
verbatim per D19/D20 (survey: each name appears on exactly one managed
type — no cross-type collisions):

- LightController2 instance attributes (`NameAI<n>`/`CapAI<n>` circuit
  names and capabilities, `PresM<n>` presence-mode membership, `COName`,
  `T5P`, `uuidSeqencing`/`uuidSeqenceIx`) and child subtrees
  (`<LightscenesC>` scene definitions, `<LSConfig>` scene behavior,
  `<HCL>` human-centric-lighting curve, `<SeqConf>` RGB sequences — the
  complete child inventory of all 45 corpus instances).
- `rec=` on CentralShade/CentralLight: the UUID list of controlled
  blocks, edited in the GUI's central dialog (the `Modes=` class).
- `Code=`/`Task=` on Code16 — it is the PicoC *program block*; the
  program source is GUI-authored logic like the DayTimer `<Entry>`
  schedule. Since D40 `Code` is also an attribute parameter (the
  `Formula=` precedent): `Code: file("prog.c")` claims it from source,
  and an unclaimed program is still carried verbatim. `Task=` stays
  GUI-owned.

And one rule extension, **D20** ([design.md](design.md)): an
`Inv=`-carrying connector is GUI-owned as a whole — its `<Co>` is
re-emitted verbatim (flag, Def, wires) and the compiler refuses source
wires/values on it. Every house LightController2 carries `Inv="true"` on
unwired `Remanence` (the GUI's encoding of the enabled Remanenz
checkbox), which had made the "genuine inversion" refusal a de-facto
type blocker. With the batch plus D20 the house config adopts
**100 of 100** managed-type blocks — zero refusals.

Since this batch the corpus is also a mechanized counterexample hunt:
`LXIR_CORPUS=corpus/web cargo test --release --test corpus` re-checks
every admitted classification (no Output observed as sink, no
Input/Param as source, no Def on Output/Api, no unlisted key
materialized) and re-verifies that whatever adopts rebuilds as a
semantic no-op, on every config in the corpus.

**Mint-validated (oracle sessions 8–9, 2026-08-25):** bare-minted
instances of all five types survive a Loxone Config open+save with
their exact connector sets. The GUI backfills absent GUI-owned content
(LightController2's default scenes, `IoData`, DayTimer's default
entries) rather than rejecting the block; central blocks are legal
with no `rec=`, Code16 with no `Code=`. Details in
[oracle-wine.md](oracle-wine.md).

## Growing the table

Add configs to the corpus (any installation helps — foreign corpora
especially, since one house exercises few types), re-run the command above,
and re-apply the rules. The final gate stays [implementation.md's
workflow](implementation.md): a new entry should eventually be confirmed by
compiling it and passing the config through Loxone Config (the Stufe 0
oracle).

## Admit a type yourself — the recipe

The builtin table refuses to mint what nobody has verified. If a block
type you need is missing, this is the path — your own configs are the
corpus:

1. **Gather evidence.** Run `lxir observe your.Loxone` on every config
   you have (older downloads and backups count). Each instance of the
   type contributes: its connector key set with indexes, wire
   directions (a `<Co>` receiving an `<In>` is a sink; a wire's source
   side is an output), and `Def=`-only keys (parameters).

2. **Apply the admission rules** (above): identical, contiguous key
   sets across instances; every key's direction resolved by wires, by
   legacy-db agreement, or by the inert-flag rule; no key left
   undetermined. One unresolved key keeps the type out — that is the
   rule, not a formality: a guessed direction becomes an invented wire
   in someone's house.

3. **Add the entry** to `connectors::builtin` in `src/connectors.rs`
   (key set in index order, direction per key), mirroring an existing
   entry of the same family.

4. **Close the loop with the save oracle.** Mint a minimal instance
   (`t = YourType("Oracle Probe")` on a test page of a copy of a real
   config), compile, and run
   `scripts/oracle.sh run compiled.Loxone` — Loxone Config deletes
   anything off-descriptor on save (D8), so the block surviving
   open+save with its exact connector set is the proof. An undecided
   port direction is settled the same way: compile a wire *sourced*
   from it; survival proves output.

5. **PR both**: the table entry and a sentence of evidence (instance
   count, which rule resolved each contested key, oracle result). The
   evidence sentence is what makes the next person trust the entry.

Types that stay out regardless of evidence: hardware/IO references
(sensors, actors, refs, Tree/Air/Modbus devices) — lxir references
hardware, it never owns it.
