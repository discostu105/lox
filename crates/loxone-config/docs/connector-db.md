# The connector database

`connectors::builtin` is the table of block types the compiler may create, with each port's index and
direction. The two older connector databases contradict each other, so Lox Logic builds its own from
evidence in real configs. This page has the sources, the admission rules, and the evidence behind
each admitted type.

## The knowledge sources

| Source | Shape | Trust |
|---|---|---|
| **Corpus evidence** (`loxl observe`) | per type/key: connector index from the port UUID, wired-as-sink / wired-as-source / `Def=` counts | ground truth for whatever it covers |
| lox-cli `docs/schemas/connector-map.json` (195 types) | `type → {c: [order], t: {key: I/O/P}}` | good ordering info; several corrupt entries (see findings) |
| lox-sim `block_signature` (237 types, extracted via [`tools/extract-sim-signature.py`](../tools/extract-sim-signature.py)) | (inputs, outputs, params) per type | best direction coverage; some simulator-internal key names diverge from XML keys; no index order |
| Loxone TechDoc, as scraped by lox-cli into `docs/schemas/loxone-block-types-full.json` | per type: connectors with direction, unit, canvas label, range, default (`%d` patterns for numbered ports) | weakest tier: never admits or decides a direction; the only source for **units**, generated into `src/port_meta.rs` by [`tools/extract-port-meta.py`](../tools/extract-port-meta.py), cross-checked against table and corpus on every run |
| Loxone knowledge-base pages (reference `kb-block-pages`) | per type: inputs, outputs, parameters by abbreviation, with summary and description | the source of the one-line **port descriptions** in `src/port_doc.rs`, checked by [`tools/check-port-docs.py`](../tools/check-port-docs.py); never admits or decides anything |

## Reproducing the run

```sh
tools/extract-sim-signature.py ~/repos/my/lox-cli > sim-signature.json
loxl observe corpus/*.Loxone \
    --crosscheck ~/repos/my/lox-cli/docs/schemas/connector-map.json \
    --crosscheck sim-signature.json \
    --crosscheck ~/repos/my/lox-cli/docs/schemas/loxone-block-types-full.json \
    > corpus-report.json
tools/extract-port-meta.py ~/repos/my/lox-cli/docs/schemas/loxone-block-types-full.json
```

The merged evidence (no identity data — types, keys, indexes, counts only)
is committed as [`data/connectors-observed.json`](data/connectors-observed.json)
and regenerated whenever the corpus grows.

## What the first cross-check found (6 real configs, 102 types)

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
  (`InputTrigger`, `Q`, `Time`). Loxone Config writes a `<Co>` for every
  connector, so corpus instances reveal the complete table
  including index order — which no legacy db has at all.
- **`OutputAPI` is genuinely mixed** across types (20 sinks, 50 sources in
  the corpus) — it must be resolved per type, never by name.

## Admission rules for `connectors::builtin`

A type becomes mintable only if **all** hold:

1. **Complete, stable shape**: every instance in the corpus materializes
   the same key set, with identical connector indexes, contiguous from 0.
   (≥3 instances. There is no variadic growth: Loxone Config deletes
   unknown connectors on save.)
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
   type out.

A fourth path can resolve what the corpus cannot: a **save in Loxone Config**
([oracle-wine.md](oracle-wine.md)). Loxone Config deletes unknown connectors on save, so a new minimal instance surviving an
open+save proves the shape, and a compiled wire sourced from a port
surviving the save proves that port is an output.

Physical hardware types (sensors, actors, Tree, Air and extension devices) are not admitted to this
table. They carry device bindings and live in the device tree, not on pages. Devices get their own
table of verified device types, with the same rule: the corpus shows one child shape per type and a
Loxone Config save keeps a minted instance (#320). Pairing stays in Loxone Config (see
[vision.md](vision.md)).

## Block versions

Every `<C>` carries a block version, `V=`, one per document: Loxone
Config writes all objects at its release's version on save. The table
is verified for the versions in `connectors::VERIFIED_BLOCK_VERSIONS`:
175 (17.0, 17.1) and 178 (17.2). A new release's version joins the list once Loxone Config of that
release has opened and saved every builtin type, and a real rebuild. Until then compile writes its
blocks at 175 and adopt refuses blocks of that version.

## Admitted types

Each table gives the ports and the evidence that admitted them.

### Gates, comparators, timers

| Type | Ports (index order) | Basis |
|---|---|---|
| `Formula` | Input1‑4 → AQ, TQ | 39 corpus instances; TQ direction from lox-sim agreement |
| `Monoflop` | InputTrigger, Reset, Remanence, Time(P), Q | 20 instances; Reset sink-evidenced; Remanence inert-flag |
| `PulseGen` | InputEnable, InputInvert, Remanence, TimeHigh(P), TimeLow(P), Q | 5 instances; InputInvert agreed by both dbs |
| `AnalogThresholdTrigger` | Input, Remanence, On(P), Off(P), PulseTime(P), Q, RisingEdge, FallingEdge | 4 instances; edge outputs from lox-sim agreement |
| `NotEqual`, `Greater`, `Less`, `LessEqual` | Input1, Input2, Q | comparator family: both dbs agree on the shape shared with live-verified `Equal` |

### Memory and push-buttons

New minimal instances survived a Loxone Config open and save (details in
[oracle-sessions.md](oracle-sessions.md)). `Memory.Q` is proven an output by a compiled wire from it that
survived the save.

| Type | Ports (index order) | Basis |
|---|---|---|
| `Memory` | Input, AQ, Q | 7 corpus instances; `Q` direction from a Loxone Config save |
| `PushButton` | InputTrigger, On, Reset, InputDisable, Remanence, Q, Qon, Qoff, OutputAPI | 5 instances; whole shape survives a save; `OutputAPI` is `Api` (see the public corpus below) |
| `PButtonT` | InputTrigger, Reset, InputDisable, Remanence, Time(P), Q, OutputAPI | 8 instances; same basis as PushButton |

Loxone Config adds visualization children (`IoData`, `Display`, `PSD`) and display attributes
(`Tp=`, `SpStates=`) to these types that the compiler never writes. That content is **owned by Loxone
Config**: rebuilds carry it verbatim from the base and adopt accepts it, so instances made in Loxone
Config adopt cleanly.

### AutoJalousie

78 corpus occurrences, 16 of them the
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

`FLG="1"` on incoming API wires (wire metadata written by the Miniserver or app) is inert stored
state: Loxone Config keeps it verbatim, never regenerates it and accepts its absence. It is carried
per (sink, source) pair like other content Loxone Config owns
([loxone-format.md](loxone-format.md); the probe is in [oracle-sessions.md](oracle-sessions.md)).

### The public corpus and `OutputAPI`

The public corpus ([`tools/fetch-corpus.py`](../tools/fetch-corpus.py), pinned in
`tools/corpus-manifest.json`) holds about 150 configs from `ControlList` Version 74 to 273: Loxone KB
sample files, LoxWiki attachments and GitHub projects. The files stay local and gitignored; most carry
no license. It covers 214 types. Its counterexamples set these `OutputAPI` directions:

- **`PushButton.OutputAPI` is `Api`**: 2 sink and 2 source wires.
- **`PButtonT.OutputAPI` is Input**: no evidence in any direction (20 occurrences) and absent from
  both legacy dbs, so the inert-flag rule applies.
- **`LightController2.OutputAPI` is `Api`**: six V161 instances in one config
  (`benjaminellmer/mc-hba`, `Loxone/HB_SS23.Loxone`) are API-linked: a wire from `OutputAPI` into the
  `AI` of an `OutputRef` titled "API Connector", plus the reverse `<In … Hidden="true"/>` into
  `OutputAPI` (the API-link shape of loxone-format.md).
- **`MeterAbsSt.OutputAPI` is `Api`**: one V172 instance (`eisber/lox-cli`,
  `UseCase-wallbox-charging-modes`) has the same API-link pair. MeterAbsUni and MeterAbsBi are `Api`
  on the same evidence.

### PulseAt and DayTimer

| Type | Ports (element order) | Basis |
|---|---|---|
| `PulseAt` | InputDisable, Remanence, Time(P), Q, OutputAPI | 16 modern instances (V254–V273, incl. the house) with an identical element order; `Q` corpus-source ×23; `Time` carries `Def=` on every instance, connector-map agrees `P` |
| `DayTimer` | InputTrigger, Reset, RtD, PulseTime(P), Remanence, Manual(P), Mode(P), AQ, Qon, Qoff, AQm, AQmt, OutputAPI | 13 modern instances; InputTrigger/Reset corpus-sink, AQ/Qon/Qoff corpus-source; AQm/AQmt both legacy dbs Output, zero contradiction; Manual/Mode/PulseTime connector-map `P`; RtD/Remanence inert-flag |

The DayTimer order is the **V259+ order**: the V259 schema migration
moved `PulseTime` from element position 6 to 3 while keeping its port
UUID (the house instance has index byte `06` at element position 3) —
which confirms that element order, not the UUID index byte, is canonical. A pre-V259 base needs the
ConfigVersion pin.

Both types keep logic from Loxone Config in element attributes and children, carried verbatim:
`Sec=`/`Typ=`/`AutP=` (PulseAt fire time and mode), the `<Entry>`
schedule children with their `N=` count, `Analog=`/`DefValue=`/`On=`/
`Off=` output behavior, `Modes=`/`UserModes=` operating-mode gating,
`Desc=`. Source writes `Sec=` as `At: HH:MM` on a block with `Typ="0"`; `Typ=` and the rest stay
carried. DayTimer schedules come from source with `schedule` claims ([language.md](language.md)).

### LightController2, Switch2Button, CentralShade, CentralLight, Code16

Over the 145-file corpus: per-instance element orders grouped into variants, directions
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
| `LightController2` | 75 | The largest admitted type. 23 house instances, one identical order; 86 corpus occurrences. 17 corpus-sink inputs (`Sel1` ×47, `Brightness` ×19, `Move` ×15); 16 Def-on-every-instance params + 5 connector-map-agreed; `AQ1`–`AQ20`/`Scene`/`OutputReset`/`OutputResetAll` connector-map O (`AQ1` corpus-source ×131); `MoveOn` carries Def everywhere *plus* one sink wire — wire evidence precedes, Input still admits the Def; `OutputAPI` `Api` (see above) |
| `Switch2Button` | 9 | 49 occurrences; `InputTrigger` sink on every instance, `Q`/`Qon`/`Qoff` corpus-source ×16/×33/×15, `Time` Def everywhere, rest inert-flag |
| `CentralShade` | 18 | 11 occurrences; `EndUp`/`EndDown`/`AutoShade`/`Safety`/`Gesture` corpus-sink, classic inputs connector-map I, `OutputAPI` connector-map O, appended `OutOpen`/`OutClose` Output (wires from them survive a save; documented as outputs) |
| `CentralLight` | 24 | 9 occurrences; `Reset`/`On`/`Alarm` corpus-sink, rest connector-map I, `OutputAPI` connector-map O, appended `OutActive` Output (a wire from it survives a save) |
| `Code16` | 34 | The most stable shape in the corpus: one element order across V99–V273 (12 instances, 8 files, house included). `AI1`–`AI13` corpus-sink, `TQ1`/`AQ1`–`AQ13`/`TeQ` connector-map O mostly corpus-source, `Remanence` inert-flag |

Three kinds of content on these types are owned by Loxone Config and carried verbatim. Each name
appears on exactly one managed type:

- LightController2 instance attributes (`NameAI<n>`/`CapAI<n>` circuit
  names and capabilities, `PresM<n>` presence-mode membership, `COName`,
  `T5P`, `uuidSeqencing`/`uuidSeqenceIx`) and child subtrees
  (`<LightscenesC>` scene definitions, `<LSConfig>` scene behavior,
  `<HCL>` human-centric-lighting curve, `<SeqConf>` RGB sequences — the
  complete child inventory of all 45 corpus instances).
- `rec=` on CentralShade/CentralLight: the UUID list of controlled
  blocks, edited in the GUI's central dialog (the `Modes=` class).
- `Code=`/`Task=` on Code16, the PicoC program block. `Code` is also an attribute parameter:
  `Code: file("prog.c")` claims it from source, and an unclaimed program is carried verbatim. `Task=`
  stays with Loxone Config.

A connector with `Inv=` is owned by Loxone Config as a whole: its `<Co>` is written back verbatim
(flag, Def, wires), and the compiler refuses source wires and values on it
([decision 0020](decisions/0020-inv-connector-gui-owned.md)). Every LightController2 in
the house config has `Inv="true"` on an unwired `Remanence`, which is how Loxone Config stores the
ticked Remanenz checkbox.

The corpus also runs as a counterexample hunt:
`LOXL_CORPUS=corpus/web cargo test --release --test corpus` re-checks
every admitted classification (no Output observed as sink, no
Input/Param as source, no Def on Output/Api, no unlisted key
materialized) and re-verifies that whatever adopts rebuilds as a
semantic no-op, on every config in the corpus.

**Checked in Loxone Config:** new bare instances of all five types survive an open and save with
their exact connector sets. Loxone Config fills in the content it owns
(LightController2's default scenes, `IoData`, DayTimer's default
entries) rather than rejecting the block; central blocks are legal
with no `rec=`, Code16 with no `Code=`. Details in
[oracle-sessions.md](oracle-sessions.md).

### Timers, edges, multiplexer, flip-flop, State

These were chosen by demand: how many of lox-cli's 473 automation tasks need each type (counted, never
copied).

| Type | Keys | Basis |
|---|---|---|
| `OnPulseDelay` | 6 | 14 corpus instances, one order (V70–V172). `InputTrigger` sink ×14, `Q` source ×14; `Delay`/`Time` carry Def on every instance *and* a sink wire → Input; `Reset`/`Remanence` inert-flag |
| `OffDelay`, `OnDelay` | 5 | one shared shape, 11 + 9 instances, one order each. `InputTrigger` sink on every instance, `Q` source; `Time` Def-only → Param |
| `StairwayLS` | 10 | 30 instances (V126–V151), one order — **not** the Config-17 order: a Loxone Config save reorders it and appends `OutputAPI`; the table is the saved order. `InputTrigger`/`Reset`/`TimeHigh` sink ×30, `InputDisable` sink ×8, `Q` source ×30; `On`/`TimeWarn`/`Remanence`/`WarnTime` Def-only → Param; `OutputAPI` connector-map O, confirmed by a wire from it that survives a save |
| `EdgeDetection` | 5 | 6 instances (V172), one order. `Input` sink ×6; `RisingEdge`/`FallingEdge` source ×4; `Edge` connector-map O; `PulseTime` Def-only → Param |
| `AnalogMultiplexer2` | 5 | 67 instances in two orders: V70–V172 `Input1, Input2, InputDisable, Select, AQ` and V174 `Select, Input1, Input2, InputDisable, AQ` (6 instances, identical key set). The table takes V174; a save keeps it. `Select` sink ×67, `AQ` source ×66. Behavior (lox-cli live report, [loxone-format.md](loxone-format.md)): `Select` is 1-based — 0 gives 0, 1 `Input1`, 2 `Input2` |
| `SRFlipFlop` | 5 | 3 instances (V126/V141), exactly the admission minimum. `InputS`/`InputR` sink, `Q` source; `InputTrigger`/`Remanence` inert-flag. The eval set's "flip-flop" maps here; `RSFlipFlop` has no corpus instance and stays out |
| `State` | 11 | 251 instances in four generations; the V175 shape (7 instances) adds `I5`–`I8` to the V161+ order. `I1`–`I4` sink, `AQ`/`TQ` source; `I5`–`I8` and `OutputAPI` inert-flag Input. The status texts (`<StateTexts>`) are GUI-owned, carried verbatim |

**Checked in Loxone Config:** all eight types survive an open and save with their exact connector
sets, order and UUIDs, in a wired chain with parameters on every block
([oracle-sessions.md](oracle-sessions.md)).

### The simple-logic batch (#316)

The most frequent page blocks of the public corpus that Lox Logic could only reference: virtual status,
wiping relay, arithmetic, averaging, rounding, counters, text pulses and analog memory. Each type has one
connector order across all its corpus instances, but the corpus holds them only in pre-17 generations (V70
to V174). Oracle session 38 minted all thirteen in that order and saved them in Loxone Config 17.2; session
39 minted them in the saved order and they came back unchanged.

| Type | Ports (element order) | Basis |
|---|---|---|
| `StateV` | I, AQ | 239 instances in 22 files, one order. `I` sink ×236; `AQ` never wired, all three legacy dbs Output |
| `WipingRelay` | InputTrigger, Remanence, TimeOn, Q | 68 instances, one order. `InputTrigger` sink ×68, `Q` source ×68; `TimeOn` Def on every instance *and* sink ×2 → Input; `Remanence` inert-flag. In no legacy db |
| `Mult`, `Add`, `Sub`, `Div` | Input1, Input2, AQ | 59 + 12 + 18 + 7 instances, one shared order. Inputs sink on every instance, `AQ` source |
| `Avg` | Input, Reset, Remanence, Time(P), Max, AQ | 18 instances (V109–V145) lack `Reset`; the newest (V151) has it, and the save keeps it. `Reset` both legacy dbs Input; `Max` Def everywhere plus sink ×2; `Time` Def-only; `Remanence` inert-flag |
| `Int` | Input, Mode, AQ | 5 instances. `Mode` never wired and never `Def` → inert-flag Input |
| `UpDownCounter` | Trigger, InputDir, Reset, OnValue, OffValue, Remanence(P), StartValue, Q, AQ | 9 instances (V126, V141) in another order: **the save reorders it**, keeping every UUID. `Trigger`/`Reset`/`OnValue` sink ×9, `Q` source; `Remanence` Def-only; `InputDir`/`OffValue`/`StartValue` inert-flag; `AQ` both legacy dbs Output |
| `HourCounter` | InputEnable, Reset, InputResetAll, Unit(P), Maintenance, Q, AQ, AQl, AQr, AQs, OutputAPI | 3 instances (V137, one file). **The save appends `OutputAPI`.** `InputEnable`/`InputResetAll` sink, `Maintenance` Def plus sink, `Unit` Def-only, `Q`/`AQ`/`AQr` source, `AQl` both legacy dbs Output; `Reset` both dbs Input; `AQs` and `OutputAPI` are in no db and never wired → inert-flag Input |
| `Counter` | Trigger, Reset, Remanence, EndValue, Mode, Q, AQ | 6 instances (V99). `Trigger`/`Reset` sink, `EndValue` Def plus sink, `Q`/`AQ` source; `Remanence`/`Mode` inert-flag |
| `PulseBy` | Text, Time(P), Pulse | 15 instances (V172, V174). `Text` sink, `Pulse` source, `Time` Def-only |
| `AMemory` | Input, Trigger, Reset, Remanence, AQ | 38 instances. `Input`/`Trigger` sink ×38, `Reset` sink ×7, `AQ` source; `Remanence` inert-flag |

Content Loxone Config owns on these types, carried verbatim: `Tx=`, `NTXT=` and `linkC=` on `StateV`
(`Analog=`, `StatsType=` and the display children are carried for every type), and `WTA=` on `PulseBy`, which holds
a search pattern (corpus values such as `*10001*`). The block takes up to four patterns, and where the
others are stored is unknown, so the search patterns stay with Loxone Config.

### VirtualIn

`Q`, `Qm` — both Output. 175 corpus instances from Config 3.x to 17;
the 113 that carry connectors all have exactly `[Q, Qm]` in that order,
and both are wire sources wherever wired. VirtualIn is the first
admitted type that is not a page block: a minted one goes into the
`VirtualInCaption` folder (`connectors::container_type`), with its
input-dialog attributes carried per type (`connectors::carried_attrs`)
and `Analog`/`MinVal`/`MaxVal`/`DefVal` as attribute parameters.
Checked in Loxone Config (digital and analog, wired to page blocks): connectors, UUIDs and wires come
back identical.

### Alarms, irrigation, room controller, energy, meters, access

Alarm, SmokeAlarm, Irrigation, HeatIRoomController2, EFM, MeterDig,
MeterAbsUni, MeterAbsBi, MeterAbsSt, NfcCodeTouch, Intercom,
TpfController. Each connector's direction comes from the corpus where a
wire proves it; otherwise from the legacy map, provided no corpus wire
contradicts it; an input never wired and never given a `Def` stays
Input. A save in Loxone Config does not count as evidence here, since 17.2.8 keeps a wire drawn the wrong
way. The internals the
GUI writes (zones, timers, `statGrp`, meter attributes such as `PwrN`,
`StatsType`) are carried per type, not modelled. NfcCodeTouch, Intercom
and TpfController are adopt-only: Loxone Config creates them when it
pairs the device.

### Virtual and Modbus devices

The devices (VirtualHttpIn, VirtualUdpIn, VirtualOut, ModbusServer)
have no connectors. ModbusDev has one, `AQ` (Input, never wired in the
corpus). The I/O: VirtualHttpInCmd and VirtualUdpInCmd `[AQ, Q]`,
VirtualTextIn `[TQ]`, ModbusASensor and ModbusSensor `[Q, Qe]`, all
Output. VirtualOutCmd and ModbusAActor `[I]`, Input. These are the
connector sets of every corpus instance that carries connectors, and
corpus wires prove the directions. The table lives in
`src/periphery.rs` together with the settings each type binds and the
folder or device it goes into. Checked in Loxone Config.

## Growing the table

Add configs to the corpus (any installation helps, since one house uses few types), run the command
above again, and apply the rules. The final gate is [implementation.md's workflow](implementation.md):
compile the new entry and pass the config through Loxone Config.

## Admit a type yourself

The builtin table refuses to mint what nobody has verified. If a block
type you need is missing, this is the path — your own configs are the
corpus:

1. **Gather evidence.** Run `loxl observe your.Loxone` on every config
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

4. **Close the loop with a Loxone Config save.** Mint a minimal instance
   (`t = YourType("Oracle Probe")` on a test page of a copy of a real
   config), compile, and run
   `scripts/oracle.sh run compiled.Loxone`. Loxone Config deletes unknown connectors on save, so the block surviving
   open+save with its exact connector set is the proof. An undecided
   port direction is settled the same way: compile a wire *sourced*
   from it; survival proves output.

5. **PR both**: the table entry and a sentence of evidence (instance
   count, which rule resolved each contested key, save result). The
   evidence sentence is what makes the next person trust the entry.

Physical hardware types go to the device type table instead (see the admission rules).
