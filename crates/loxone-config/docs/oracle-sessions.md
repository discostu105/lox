# Oracle sessions

A historical log of the Loxone Config save-oracle runs, kept as written at the time. Current format facts live
in [loxone-format.md](loxone-format.md); the rig is described in [oracle-wine.md](oracle-wine.md).

The first run was on 2026-08-24, with Loxone Config 17.1 (`ConfigVersion 17010727`) under Wine on CachyOS/KDE
Wayland.

## Headline result

**Loxone Config accepts Lox Logic-compiled output.** A real 1.3 MB config with
a compiled-in managed island (`Monoflop` + `And` + one wire, `Def=`
param, placed on an existing page) opened without complaint and survived
open → save with an **empty semantic diff**: every minted object UUID,
port UUID, wire, and param came back untouched. Loxone Config's fresh
output also round-trips **byte-identically** through the `xml` layer.

What it adjusted on minted blocks (cosmetic only):

- `Py2` corrected — the builtin height table used for layout is taller
  than Loxone's real block heights (Monoflop: ours 1080, Loxone's 696).
- `Py` grid-snapped to a multiple of 96 where placement was off-grid.
- `WF` (view-flags) rewritten (`147456` → `16384`) — also done to
  pre-existing corpus blocks, so it is a general normalization, not a
  reaction to minted objects.

## The save fingerprint

What a Loxone Config save changes in a file it considers unmodified
(open → save with no edits; confirmed on two independent runs):

| Change | Detail |
|---|---|
| `NextObj` **+2** | Consumed per open+save cycle with zero objects added — counter values are burned on transients, not only on persisted objects. `Lockfile::absorb_counters`' max() strategy handles this; "one per minted object" (D11) describes *our* minting, not Loxone's consumption. |
| `Date` / `DateS` | Document header stamped with the save wall-clock time. |
| `<In>` order rewritten | Wire lists under a `<Co>` are re-emitted in an internal canonical order — **not** the input file's order (observed: exact reversals vs. an older save). Deterministic: two saves of the same state produce the same order. `<In>` order is therefore **not semantically meaningful**; `diff` already treats wires as sets. Byte-stability of wire order across an external Loxone Config save is impossible by design. |
| Schema migrations | Older objects gain missing attributes (observed: `SwitchingTimer M="3"` gained `UserModes="3"`). This alone dirties the document — a freshly opened file already has unsaved changes. |
| `AutopilotRule` re-layout | Autopilot rule blocks get their `Px`/`Py` rewritten with no user input — their canvas positions are not stable across saves. |
| `Nc` dropped at zero | A `<Co>` whose last `<In>` disappears is written self-closing without `Nc`, matching the compiler's own rule. |

One run also dropped a Miniserver-created wire into the `AutoPilot`
block's `Input` connector (`FLG="1"` on the `<In>`); a clean keyboard-only
run did **not** reproduce this, so it is attributed to stray input during
that first run, not to load-time pruning. Re-test before treating it as a
format fact.

## The grown-gate experiment (D8) — answered, negatively

Second oracle run (2026-08-25), probing [D8](decisions/0008-fixed-gate-inputs.md) on the
headless Xvfb rig. Three GUI facts first, established by driving the
canvas:

- Wiring an `And` gate's last free input (`I2`) does **not** make an `I3`
  appear — not immediately, not after save + reload. The saved XML
  confirms: the wire connected, the gate stayed `I1`/`I2`/`Q`, `Nio="3"`.
- Dropping a wire on the gate *body* opens a connector picker
  ("Anschluss auswählen") that enumerates **only the existing inputs** —
  the GUI's own descriptor says a gate has exactly `I1` and `I2`.
- The block's `⊕` expander only toggles *display* of existing descriptor
  connectors (e.g. Monoflop's unwired `Off`/`Remanence`/`D`); there is no
  input-count property, no context-menu entry, no resize handle.

Then the compiler-side test: `loxl compile` minted `I3` at connector
index 3 (after `Q`'s 2 — the D8 assumption) wired from an `InputRef`,
onto the previously saved base. Loxone Config **loaded the file without
complaint** — and on `Ctrl+S` **silently deleted the `I3` connector and
its wire**, reverting the gate to `Nio="3"`. Everything else survived
byte-for-byte (semantic diff: exactly the one wire removed, zero object /
param / rename changes; the `NextObj` +2 burn and the `WF`
147456 → 16384 normalization appeared again).

Conclusions, now encoded in the compiler and docs ([D8](decisions/0008-fixed-gate-inputs.md),
loxone-format.md):

- **Gates are fixed two-input in Loxone Config 17.** `loxl` refuses
  `I3`+ with a cascade hint instead of minting it.
- **A save writes exactly the descriptor's connector set** —
  off-descriptor `<Co>`s are dropped silently, wires included. Never
  invent connectors.
- Positive side-finding: a compiler-drawn wire from an existing
  `InputRef.AQ` into a managed block survives the save untouched, and
  re-emitting a GUI-drawn wire from source (after teardown) reproduces it
  byte-identically.

## The adopt round-trip (D18) — passed

Third oracle session (2026-08-25), directly after `loxl adopt` shipped:
the whole-config rebuild (adopt the real house config → compile the
adopted module back onto it) was opened and saved by Loxone Config. The
adopt rebuild differs from the original in exactly the normalizations the
compiler applies — Co children re-emitted in spec order, `LtE` dropped,
managed blocks re-emitted at the end of their page — and none of them had
ever been GUI-blessed. Result: **semantically empty diff** against the
original; 14 of 22 adopted blocks byte-identical after the save, the rest
differing only by the known save fingerprint (`WF` 147456 → 16384, `<In>`
order rewrite, `NextObj` +2).

New format fact from this run: Loxone writes `<Co>` attributes in the
order **`K`, `Def`, `Nc`, `U`** — the save moved a compiler-emitted
`K,Nc,Def,U` into that order. The compiler's `sync_nc` now inserts `Nc`
after `Def`, before `U`.

## The mint oracle: Memory, PushButton, PButtonT — admitted

Same session, with the rig warm: minted minimal instances of the three
corpus-blocked types (plus wires from a real `VirtualIn`) onto the real
base, opened, saved. **All blocks, wires, and `Def` params survived.**
What the GUI added on save — schema-healing of children the compiler does
not emit:

- `Memory`: `Tp="0"` attribute, `<IoData></IoData>` (note:
  **non-self-closing even when empty** — falsifying the assumption that
  empty elements always serialize as `<X/>`), and a
  `<Display Unit="&lt;v.1&gt;" StateOnly="true"/>`.
- `PushButton`: a GUI-minted `SpStates` attribute (three fresh UUIDs whose
  machine tail matches our serial — the GUI consumes counter space for
  them), `<IoData Visu="true"/>`, `<PSD .../>`, `<Display Type="1" .../>`.
- `PButtonT`: `<IoData Visu="true"/>`.
- `WF` was **dropped entirely** on these types (not rewritten to 16384).
- Geometry corrected as usual (cosmetic).

Consequence at the time of the run: because compile is teardown/rebuild,
every compile deleted these GUI-added children and the next GUI save
re-added them, and adoption of GUI-created instances refused. Both were
resolved the same day by [D19](decisions/0019-gui-residue-carried-forward.md) (GUI-owned residue is
carried forward verbatim from the base on every rebuild) — the churn
converges after the first GUI save, and the instances adopt cleanly.

The last blocker was `Memory.Q` — never wired anywhere in the corpus,
absent from both legacy dbs. Direction probe: compile a wire **sourced**
from a minted `test_mem.Q` into a `Not.I`. Since a save deletes anything
off-descriptor (D8, above), survival is decisive — the wire **survived
the save intact**, so `Q` is an output. All three types are now in
`connectors::builtin` (see [connector-db.md](connector-db.md)).

## The D19 + AutoJalousie rebuild — passed

Fourth session (2026-08-25, same day as D19 and the AutoJalousie
admission): the full adopt-rebuild of the real house config — **49
managed blocks**, including 12 AutoJalousie with carried
`COHist`/`IoData`/`SpStates` residue, grey Memory blocks, `NDOC=`,
`StatsType=` — opened and saved by Loxone Config.

Result: **semantically empty diff in both directions** (vs the original
and vs the rebuild we handed it). Byte-level, the save touched nothing
of ours except one Formula whose `WF` it rewrote 16384 → 147456 — the
*reverse* of the earlier 147456 → 16384 normalization, settling that
`WF` is GUI view-state it freely recomputes (D19 carries whichever
value the base has, so this never fights the GUI). Everything else in
the save fingerprint was unrelated to managed blocks: a schema
migration stamping `TId=` template ids onto 62 hardware-device
elements, the known AutopilotRule re-layout, the `NextObj` +2 burn,
and a one-pixel `Py2` nudge on an unmanaged Text note.

## The FLG wire-flag probe — inert, and now carried

Fifth run (2026-08-25, warm rig). 113 of the house's 880 wires carry
`FLG="1"` (one `FLG="2"`) on their `<In>` — Miniserver/app-created wire
metadata concentrated on API-connector distributions (`OutputRef` blocks
titled "API Connector" with port `K="AI"`, `SmokeAlarm.OutSilent`,
`Alarm.Q1`, the AutoPilot input). It blocked adoption of 4 AutoJalousie
instances: the rebuild could not reproduce the flag.

Probe: strip `FLG="1"` from two of those wires in a copy of the
GUI-saved base, open + save. Result — the GUI treats the flag as
**inert stored state**: the stripped wires survived the save *plain*
(neither deleted nor re-flagged), the 111 remaining flags round-tripped
verbatim, and the only other changes were the known fingerprint
(`NextObj` +2, three more `WF` flips).

Consequence, shipped the same day as a D19 extension: `FLG=` is
GUI-owned residue on wires — harvested at teardown keyed by (sink port,
source port) and re-emitted verbatim; a wire whose source changes in the
module drops the flag, which is exactly the state the probe validated.
Adoption accepts it, taking the real config from 49 to **53 of 59**
managed-type blocks (all 16 AutoJalousie; the 6 remaining refusals are
genuine `Inv=` inversions).

Confirmation run: the full 53-block rebuild — with compiler-emitted
`FLG=` wires — opened and saved. Semantically empty diff in both
directions; all 113 flags intact; the byte delta was exclusively the
fingerprint (`NextObj` +2, ~30 `WF` rewrites *in both directions* on
mostly unmanaged blocks — settling beyond doubt that `WF` is freely
recomputed view-state — plus one unmanaged Text-note geometry nudge and
`<In>` order rewrites).

## The first real-world change — passed

Sixth run (2026-08-25, fresh rig). The first change compiled for actual
deployment: the pool UV lamp, previously running permanently on a latched
`PushButton`, gated to two weekdays by wiring `Or(Montag.Q, Dienstag.Q)`
into the button's `On` and its inversion into `Reset` (module in the
private sibling repo `~/repos/my/r50`).

The probe value beyond the change itself: the `Or` inputs are wired
**directly from the system `Mode` objects' `Q` ports** — objects that
live in the system area with no `Px/Py` and no on-page representation.
The GUI's own pattern is different (dragging a mode onto a page creates
an `InputRef` wired from `Mode.Q`; nine corpus configs show it), so this
save answered whether the visual ref is *required*. It is not: open +
save kept both minted blocks and all 5 wires verbatim, created no
`InputRef`, and the semantic diff was empty in both directions. The
fingerprint was the known set plus a grid-snap of the two new blocks'
`Py` (the compiler's `next_free_py` cursor is not grid-aligned —
cosmetic, picked up by the next adopt).

Handing Loxone Config the crate's synthetic `examples/out` file
SIGSEGVs it on load (minidump in the prefix's
`AppData/Local/Temp/Loxone/Loxone Config/Dumps/`). A real config loaded
the same way is fine. The synthetic corpus is sufficient for the XML/UUID
layers but is **not loadable** by the real tool — it lacks required
structure. Oracle runs must always compile onto a real base config.
Re-learned on 2026-10-30 (#174): every synthetic file crashes in
17.2.8.28 too, right after the document's docking panes are built, even
in a fresh prefix with an empty `Documents` folder and no network, while
a real config opens in the same prefix minutes later. Its crash box is
also captioned `LoxoneConfig`, like the recovery dialog, so
`oracle.sh` now watches the dumps folder and stops with that diagnosis.

## The second-batch bless + mint oracle (session 8) — passed

Ran 2026-08-25 against the freshest live download (post-UV-push R50,
100 managed-type blocks after the second admission batch and D20).
Two configs, one Loxone Config instance (isolated `wine-oracle`
prefix on Xvfb `:5`), saved via Ctrl+S per tab:

- **Bless the 100-block rebuild.** `adopt` → `compile` output (all 23
  LightController2, Switch2Button, CentralShadeControl,
  CentralLightControl, Code16, and every D20-carried `Inv=` connector
  included) opened and saved with a **completely empty semantic diff**
  — every rebuilt element survived byte-faithfully at the semantic
  layer. `loxl drift` (D21) stayed green across the GUI save: the save
  fingerprint (NextObj burn, Date stamps, WF churn) never touches the
  drift baseline.
- **Mint oracle: PulseAt, DayTimer, Switch2Button.** Three blocks
  minted from bare declarations (`t = PulseAt("Oracle PulseAt")`, no
  args) on page Testing. All three survived the open+save with their
  **exact connector sets intact** (5, 13, 9 — the descriptor-law
  check: an off-descriptor key would have been dropped silently, a
  missing one materialized). The GUI backfilled only known D19 residue:
  recomputed block extents (`Px2`/`Py2`), `<IoData Visu="true"/>`,
  `<Display Unit>`, and DayTimer's ten default `<Entry To="1440"
  V="1">` schedule rows — all invisible to the semantic layer and
  carried verbatim by the next adopt/compile. PulseAt and DayTimer
  were admitted from web-corpus evidence only; this closes their mint
  validation.

## The mint oracle, round 2: the whole second batch (session 9) — passed

Ran 2026-08-25, same rig. Four blocks minted from bare declarations
(`t = LightController2("Oracle LC2")` — no args, no wires) on page
Testing of the real house base, opened and saved once:

- **All four survive with their exact connector sets** —
  LightController2 75, CentralLight 24, CentralShade 18, Code16 34.
  Empty semantic diff.
- **The GUI backfills missing GUI-owned children instead of rejecting
  the block.** The bare LC2 gained its default scene configuration on
  save (`<LightscenesC>` with the stock scenes "Viel Licht"/"Aus",
  `<LSConfig>`, `<PSD>` presence defaults, `<HCL>`, `<IoData>`,
  `<COHist>`) — the DayTimer-default-entries pattern at full scale. So
  a minted LC2 is legal; its scene setup then belongs to the GUI as
  D19 residue.
- **Central blocks need no `rec=`**: controlling zero objects is a
  valid state; the UUID list appears only once objects are assigned in
  the GUI. **Code16 needs no `Code=`**: zero backfill at all, an empty
  program block is legal.
- Cycle closed: re-adopting the GUI-saved file lifts all 104 blocks
  with zero refusals, compile is a semantic no-op, `loxl drift` green.

With session 8 (PulseAt, DayTimer, Switch2Button) this mint-validates
every type of the second admission batch.

## Expression titles (session 10, D24) — passed

Ran 2026-08-25, same rig. D24 left one open note: expression-generated
blocks carry their sub-expression as the `Title=` — text with `>=`,
`(`, `)`, and dotted port references, XML-escaped correctly but never
shown to the GUI. Compiled
`relais_1_treppenlicht.AI <- not (montag.Q and dienstag.Q) or
dienstag.Q >= 1` against a copy of the real house base (four blocks —
And, Not, GreaterEqual, Or — placed on Automatik-Regeln, plus a second
`<In>` on an already-wired extern sink), opened and saved once:

- **Empty semantic diff across the save.** All four blocks, their
  titles (`Title="dienstag.Q &gt;= 1"`, the full parenthesized Or
  title), and all seven wires re-serialized byte-preserved — the GUI
  parses, holds, and re-emits the special-character titles unchanged.
- The canvas renders the titles as plain caption text (the And's
  `montag.Q and dienstag.Q` was on screen); nothing in the title
  pipeline treats `>=` or parens specially.
- Second `<In>` children on an extern connector survive alongside the
  pre-existing wire — additive wiring onto an already-wired sink is
  legal.

## Minted mirrors (session 11, D33) — passed, with three heals

Ran 2026-08-25, fresh rig (isolated `wine-oracle` prefix on Xvfb `:5`).
The gate for eliminating ref plumbing from source: does a
compiler-*minted* `InputRef`/`OutputRef` survive open+save? Three probes
compiled onto a copy of the freshest live download, on page Testing: a
full InputRef mirror of a real `VirtualIn` (feed wires `AI:`/`I:` from
the target's ports, `And` consumer), a *naked* InputRef mirror (no feed
wires at all), and an OutputRef mirror of a real `Actor` fed by the
gate.

- **All three survived** — `Ref=`, `LinkRefType=`, and every connector
  byte-preserved. Minted refs are legal.
- **The GUI healed, not rejected, what was missing.** Three heals, each
  now encoded in the compiler: (1) every ref's `Title=` was rewritten to
  the *mirrored object's* title — a ref's title is derived state, so the
  compiler now emits the target's title and `validate` refuses a label
  on ref blocks; (2) the naked ref got both feed wires drawn by the GUI
  itself (target connector index 0 → `AI`, index 1 → `I` — matching the
  corpus pattern across all device types); (3) ref geometry was redrawn
  to the flat 2112×192 tag footprint, now the mint default.
- **The OutputRef → actor distribution wire was *not* healed** — the
  corpus wires all 154 `OutputRef.AQ` ports into their actors
  explicitly, and the GUI leaves a dangling mirror dangling. That wire
  stays source-drawn.
- Confirmation cycle: recompile (fixed compiler, feed wires everywhere)
  against the GUI-saved file → **semantic no-op**; second open+save →
  **empty semantic diff in both directions**, refs byte-identical except
  the known geometry snap on identities minted before the footprint fix.

Corpus fact from the same session: `LinkRefType=` is a deterministic
type-registry code of the mirrored object's XML type (VirtualIn 71,
Actor 63, Memory 320, …) with `Analog=` following the target type —
learned into `connectors::ref_link_type`, unknown types refuse the mint.

## Ref titles under a title change (session 12, D43) — passed

Ran 2026-10-29 on copies of the freshest house download (local only).
Question: what does Loxone Config write into a ref's `Title=` when its
target's title changed — and does the composite form "<input> <device>"
some refs carry survive? Five probes, each a hand-renamed target (and
in two of them a hand-edited ref) through one open+save:

- **Every ref follows its target** — visible `InputRef`s and
  `OutputRef`s, position-less `OutputRef`s (`WF` bit 0x1000), and two
  refs that were already stale before the probe (left so by a D39 title
  claim; they were rewritten without any rename).
- **Except an alarm block's collected inputs** (`InputRef`, `WF` bits
  0x1000|0x0800, wired into the alarm's sensor inputs): neither
  composite nor plain-copy titles change — not on a target rename, not
  on a rename of the target's device.
- `WF` is derived: hand-moving 0x0800 between a following and a
  keeping ref is undone by the save and changes nothing. Removing a
  following ref's `Px`/`Py`, or giving a collected one a position,
  changes neither its bit nor its behaviour.
- Confirmation: the house project compiled with D43 (the existing relay
  claims now carry their refs) saves with an **empty semantic diff**;
  so do title claims on three targets with visible, hidden and
  collected refs, and the compile that withdraws them again.

## Repaired numeric references (session 13, D44) — passed

Ran 2026-10-29. Question: does Loxone Config keep what `loxl repair`
writes (raw line breaks where a foreign writer had left `&#xA;` text)
in `APC=`, `NDOC=` and `Code=`? Each probe took a config, encoded the
line breaks of one attribute kind as `&amp;#xD;&amp;#xA;` (the
GUI-saved damage), ran `loxl repair`, then one open+save:

- **Corpus, `APC=`** (a public use-case config with two rules whose
  JSON holds a raw CRLF): the repair reproduced the original file byte
  for byte; the save kept both payloads byte-identical.
- **Corpus, `Code=`** (a public Modbus example whose program has 436
  line breaks): repair equal to the original; the save kept the program
  byte-identical. The semantic diffs of both corpus files show only
  the release upgrade of an older config (added system objects,
  locale renames).
- **House config, local only:** the ten damaged Modbus `NDOC=` texts
  as downloaded, plus its compiled program (D40) damaged on purpose. All
  eleven came back with raw CRLF, everything else unchanged; the save
  kept every `NDOC=` (18/18) and the program byte-identical, and the
  semantic diff shows only two ref titles known to be stale (D43).

## Block version 178 (session 14, D46) — passed

Ran 2026-10-29 with Loxone Config 17.2.8.28 (`ConfigVersion 17020828`),
whose saves write every object at `V="178"`. Two runs, both with the
compiler emitting 178:

- **Mint oracle.** One bare instance of each of the 24 builtin types,
  on page Testing of the house config as last saved by 17.2, opened
  and saved once. All 24 come back at `V="178"` with their exact
  connector keys and UUIDs. The semantic diff holds two backfills on
  the bare probes: `Formula=""` on the Formula, `Code=""` on the Code16
  (under 17.1 the Code16 got none). One layout change:
  AnalogThresholdTrigger's connectors are written `Input, On, Off,
  Remanence, …`, where 17.1 wrote `Input, Remanence, On, Off, …`, with
  unchanged indexes. The house's own 17.2 save shows the same order.
- **House rebuild.** The whole house config (1909 objects) compiled at
  178 open+saves with an empty semantic diff, every object still at 178.

A save of that rebuild still differs from it in bytes: blocks move
within their page, `Desc=` moves right after `Type=`, the
AnalogThresholdTrigger connectors are reordered (the next compile keeps
all three, D47), and minted ApiActor refs gain `Text="true"`, which
GUI-created ApiActor refs carry under 17.1 too and D33 did not emit
(D48 does).

## Push notifications (session 15, D51) — passed

Ran 2026-10-29 with Loxone Config 17.2.8. Two parts:

- **Probe.** On a copy of the house config: *Messages › Notification*
  created and renamed, dragged onto a page, a user ticked as recipient
  in its permissions dialog, saved. The save is the format reference in
  loxone-format.md (*Push notifications*): the `Notification` element,
  the `OutputRefLM` with its `<LoggerMailer>`, the recipient's
  `<PRM ACT="3"/>`.
- **Compile.** The notification spliced into the base (a byte patch,
  since it is periphery), then the house module plus an `InputRef` of a
  temperature sensor, a `LessEqual`, an `OutputRefLM` with `On:` and
  `MinimumTime: 12h`, and `recipients` for one user, compiled, opened
  and saved. The first run diffed in two places, both source errors:
  the comparator's `Input2: 0` came back without `Def=` (Config writes
  no zero there; leave the input open), and the sensor ref gained the
  wire `I ← sensor.Q` (loxone-format, *Refs*). With both fixed the
  semantic diff is empty — message texts, `MinimumTime=` and the
  recipient included. The save added the known `WF` fingerprint
  (`147456` on the `OutputRefLM`, `16384` on the comparator, `16400` on
  the notification and the mirrored sensor).

The GUI-driving lessons of this run are in [oracle-wine.md](oracle-wine.md#the-rig).

## The timer/edge/state batch (sessions 16–17) — passed

Ran 2026-10-29 against a copy of the house config (Loxone Config
17.2.8.28). Question: what connector descriptor does Config 17 write
for eight types the corpus holds almost only in pre-17 generations
(OnPulseDelay, OffDelay, OnDelay, StairwayLS, EdgeDetection,
AnalogMultiplexer2, SRFlipFlop, State)? One page minted all eight as a
wired chain (VirtualIn → OnPulseDelay → OffDelay → OnDelay → StairwayLS
→ EdgeDetection → SRFlipFlop via `RisingEdge`/`FallingEdge`/`Edge` →
AnalogMultiplexer2.Select → State → Not), with parameters on each.

- **Session 16:** seven types came back with the identical connector
  keys, order and UUIDs; State gained the GUI-owned `IoData`,
  `Display` and `<StateTexts>` children (StateTexts is now in
  `GUI_OWNED_CHILDREN`). StairwayLS minted in the newest corpus order
  (V151) was **rewritten**: the save reordered the connectors to
  `InputTrigger, On, Reset, InputDisable, TimeHigh, TimeWarn,
  Remanence, WarnTime, Q` and appended `OutputAPI` with a GUI-minted
  UUID, keeping every existing UUID, and added `IoData`/`PSD`/
  `SpStates`. That is the D8 law seen from the other side: the save
  writes the type descriptor's connector set, in descriptor order. The
  semantic diff was still empty. The table took the saved order.
- **Session 17:** StairwayLS minted in that order, plus a wire drawn
  *from* its `OutputAPI`, and wires drawn from `CentralShade.OutOpen`/
  `OutClose` and `CentralLight.OutActive` (admitted as inert-flag
  inputs; documented as outputs). Everything came back identical,
  connector order and UUIDs included, all four wires kept; the semantic
  diff was empty. The three Central* ports are outputs since.

These runs predate D46 here: the compiler still emitted `V="175"` and
the save re-stamped the blocks `V="178"`, which session 14 settled.

## Schedules and virtual inputs (session 18, D53/D54) — passed, with one heal

Ran 2026-10-29 against a copy of the house config (Loxone Config
17.2.8.28), in a reflink clone of the oracle prefix on its own display
because another session held the shared one. One probe carried schedule
claims on an extern DayTimer and on fresh ones, and two minted
VirtualIns (digital, analog with range and default) wired to page
blocks.

- **Pass a:** the extern timer's twelve entries came back unchanged,
  with `UserModes="2"` added before `N=`. The fresh timer, whose
  schedule named only Monday and Sunday (`Modes=` with two columns),
  came back with **all ten** columns: every missing one filled with an
  all-day `<Entry To="1440" V="1"/>`, i.e. switched on — a silent
  change of meaning. Corpus check: the seven weekdays are in every
  `Modes=` list, and `UserModes=` is always the count of modes beyond
  them (absent at zero). **Heal:** the compiler now writes all seven
  weekday columns (a weekday the schedule does not name is an empty
  column, off) and `UserModes=`. The VirtualIns kept their identity,
  wires and claimed values; the save set `WF="147456"`, added
  `MaxVal="1" MinChange="0.5" MinTime="10"` to the digital one and
  `EnVal="true"` to the analog one (corpus: `EnVal` goes with
  `Analog="true"`, never without), wrote the analog range in the order
  `EnVal, DefVal, MinVal, MaxVal`, and appended
  `<IoData Visu="true" St="2"/>` and
  `<Display Type="1" Unit="&lt;v.1&gt;" StateOnly="true"/>`. A fresh
  mint now writes exactly this.
- **Pass b:** with the heal, plus a fresh timer with a holiday column
  and `daily`: every `<Entry>`, `Modes=`, `UserModes=` and `N=` came
  back byte-identical, and both VirtualIns came back identical except
  the version stamp. The semantic diff was empty. The fresh timers
  gained the GUI-owned `On`/`Off`/`SpStates`, `IoData` and `Display`,
  as every minted DayTimer does.

## Creating pages (session 19, D56) — passed

Ran 2026-10-29 against a copy of the house config (Loxone Config 17.2).
First the GUI as reference: "Neue Seite" with a title and a room wrote
`<C Type="Page" V="178" U=… Title=… LtE=… WF="16384">` with
`<IoData Cr=… Pr=…/>` — `Cr` the unassigned category, `Pr` the chosen
room — filed by title between its neighbours; cancelling the dialog
discards the page, and Save stays disabled until a real room is chosen.
Then the probe: a compile that moved a nine-block chain onto a page
created from source (`page "…" (room: "…")`), no `LtE=`.

- **Pass:** the page element came back byte-identical (no `LtE=` added,
  room and category kept, position kept), the chain with all its wires
  and cross-page connections stayed on it, and the semantic diff was
  empty. NextObj rose by the usual two of the save. A recompile against
  the saved file created nothing and left the lock's `pages` entry as
  it was.

## Lighting moods (sessions 21–22, D58) — passed

Ran 2026-10-30 with `scripts/oracle.sh` against a copy of the house
config (Loxone Config 17.2.8.28).

- **Session 21:** two probes in one file. One was a mood minted by a
  claim onto an existing controller ("Weiß", next free `SID` 3 / `CID`
  14). The other was a bare `LightController2` with no claim.
  - The minted mood came back byte-identical.
  - The bare controller gained the stock first-save content:
    `<LightscenesC FC="0040FFFF,008080FF,00FFFF80,0040FF40" Outputs="18"
    Num="2">` with "Viel Licht" (`Move="true"`, `SID` 777, `CID` 22,
    every `Q` 1) and "Aus" (`ReadOnly="true"`, `SID` 778, `CID` 11,
    every `Q` 0).
  - It also gained `<LSConfig OpM="778" ScMv="777" ScAc="1"/>`,
    `<IoData Visu="true" Rating="1"/>`, `<PSD As="-2" Ae="1380" Amin="1"
    Amax="4" Dmin="30" Dmax="180" V="20"/>`, `<HCL T="2" M="0"
    TMIN="2000" TMAX="6500" FROM="0" TO="0"/>` and `<COHist TRK_NE="20"
    TRK_MAX="100"/>`.
  - Separately, the attributes `uuidSeqencing`/`uuidSeqenceIx`/
    `SpStates` were added, and `WF` changed.
- **Session 22:** Lox Logic wrote that content itself on a claim (seeded
  stock moods plus a claimed "Lesen").
  - The children came back byte-identical.
  - The semantic diff was empty.

## Decimal points in a Formula (session 23, D35) — passed

Ran 2026-10-30 against a copy of the house config: one `Memory` fed by
`montag.Q * 0.5 + 2.25`, folded into `Formula="I1*0.5+2.25"`. Every
corpus formula writes a decimal comma. Loxone Config opened the file
without a message, and the save kept the formula byte for byte; the
semantic diff was empty. Only `Py2` (block height) and `WF` changed,
both known. With the live check of the same day (the Miniserver
evaluates either separator) the point is settled.

## Automatic Designer rules (session 24, D57/D60) — passed

Ran 2026-10-30 against copies of the house config (Loxone Config
17.2.8.28). First the round trip, the old roadmap question: an
open+save of the base kept all five rules' `APC=` byte-identical — key
order, compact spelling and the two raw CRLFs inside the JSON
included. Then the probe: a compile adding a named rule (`rule
regel_probe "Lox Logic Probe-Regel"`: a weekday sentinel event, a time event,
`off` to a PushButton) below the five on the special page.

- **Pass:** the created rule came back byte-identical — payload,
  position, colour, the seven connectors and `ref=` — except `WF=`,
  which the save drops from all six rules alike (view state, as on page
  blocks). The semantic diff was empty; NextObj rose by the usual two,
  `NumO=` was recounted and `TId=` stamps appeared as on every save.
- A recompile against the saved file created nothing (six rules), left
  the lock's `rules` entry as it was, and diffed empty against it.

## House control types (session 25, D62/D63) — passed

Ran 2026-10-30 against copies of the house config (Loxone Config
17.2.8.28). One minted block each of Alarm, SmokeAlarm, Irrigation,
HeatIRoomController2, EFM, MeterDig, MeterAbsUni, MeterAbsBi and
MeterAbsSt on a probe page, wired to virtual inputs.

- **Pass:** the semantic diff was empty. The save added what the GUI
  keeps inside these blocks — zones, `Timer`, `statGrp`, the meters'
  root, `IoData`, `SpStates` — and a recompile against the saved file
  was byte-identical (the carried children converge).
- With `.room`/`.category` on the nine blocks: empty diff again; the
  save kept `Cr=`/`Pr=` as compiled.
- A direction finding: the save keeps a wire drawn in the wrong
  direction, so a save never proves a connector's direction; the
  tables rest on corpus wires (connector-db.md).

## Virtual and Modbus devices (session 26, D64) — passed

Ran 2026-10-31 against a copy of the house config (Loxone Config
17.2.8.28). One minted object of each of the twelve types: an HTTP and
a UDP virtual input with a command each, a virtual text input, a
virtual output with a command, a Modbus server with a device and an
analog sensor, a digital sensor and an actuator.

- **Pass:** the semantic diff was empty. Loxone Config takes devices
  with no captions, no `Online` children and no display settings; the
  save adds them (`Cl` on the HTTP command, an empty `<IoData></IoData>`,
  `Display` defaults, `Tx="false"`, `ModbusCoilQuantity="1"`, `WF` on
  the output command, `WF="16384"` on the text input). A recompile
  against the saved file was byte-identical: what the save added is
  carried.

## Settings of physical inputs and outputs (session 27, D65) — passed

Ran 2026-10-31 against a copy of the house config (Loxone Config
17.2.8.28), on top of session 26's devices. On an Air temperature
sensor: an added `Desc`, a rewritten `Unit` on its `<Display>` and
`MinTime`, an added `DestValLow` and `AverageTime`; on an Air digital
input an added `Desc`.

- **Pass:** the semantic diff was empty. The save kept every attribute
  where Lox Logic wrote it (`Desc` right after `Type`, `DestValLow` before
  `SourceValHigh`, `AverageTime` after `MinTime`). It set `WF` bit 0x10
  on the analog sensor (16384 → 16400; the digital input had it
  already) — GUI bookkeeping, carried. A recompile against the saved
  file was byte-identical.

## Blocks minted for the app (session 28, D66) — passed

Ran 2026-10-31 against a copy of the house config (Loxone Config
17.2.8.28). First thirteen blocks minted bare (light controller,
jalousie, the four meters, central light and shading, alarm, smoke
alarm, irrigation, room controller, an analog virtual input) to see what
a save adds: `WF` 147456 → 16384 on all; the special states on all but
the meters; `<IoData Visu="true">` (with `Rating="1"` on the light
controller, the central blocks and the irrigation); a jalousie's and a
light controller's `<COHist>`; the light controller's stock moods,
`<LSConfig>`, `<PSD>`, `<HCL>` and its `uuidSeqencing`/`uuidSeqenceIx`
lists; a meter's `Rec=""` and `<statGrp>` groups; `<Display>` on the
central blocks and the smoke alarm; `Entries="40"` on the alarms; the
irrigation's zones and the room controller's schedule.

Then a jalousie minted with seven special states, `<IoData>` and
`<COHist>`, a room and a category:

- **Pass:** the semantic diff was empty. The save kept the states,
  `<IoData Visu Cr Pr>` and `<COHist>` as Lox Logic wrote them and set `WF`
  to 16384. A recompile against the saved file was byte-identical.

## Operating modes (session 29, D69) — passed, with two heals

Ran 2026-10-31 against a copy of the house config (Loxone Config
17.2.8.28): a minted user-defined mode wired into an `And`, and a fresh
DayTimer whose schedule named the minted mode, the GUI's user mode
"Haus im Tiefschlaf" (`off`), the weekdays and `heating`.

- **Pass a:** empty semantic diff. Two changes on save: the minted
  mode's `WF="16384"` became `147456`, and the timer's `UserModes="3"`
  became `2` — `UserModes=` counts the columns before Monday, not every
  column beyond the weekdays (the heating column comes after Sunday).
  **Heals:** a fresh mode is minted with `WF="147456"`; `UserModes=`
  counts the columns with `Mode=` below Monday's 3.
- **Pass b:** the mode element and the timer's `Modes=`, `UserModes=`,
  `N=` and entries came back byte-identical, with the columns in
  ascending `Mode=` order (minted `-7`, GUI `-3`, Monday … Sunday,
  heating). The timer gained `On`/`Off`/`SpStates` as every minted
  DayTimer does (session 18).

## Settings of Air and Tree devices (session 30, D71) — passed

Ran 2026-10-31 against a copy of the house config (Loxone Config
17.2.8.28). A first run wrote a switch board onto two Tree room devices
that had none (a room sensor, a presence sensor): the save dropped
`SwitchBoard`, `SwitchBoardRow` and `SwitchBoardPos` again, kept
everything else — a Tree device has a board only where its type is
mounted in one, so Lox Logic rewrites it and never adds it. Then: `Desc` and
`Installation` on an Air relay, a Tree dimmer and a Tree room sensor,
the board, row and position on the relay and the dimmer.

- **Pass:** the semantic diff was empty. The save kept `Desc` right
  after `Type` and wrote `TId=` after it (`NanoIO`, `RoomsensorTree`),
  bookkeeping that is carried. A recompile against the saved file was
  byte-identical.

## Rule sources, calendar entries (session 31, D73/D74) — passed, one finding

Ran 2026-10-31 against a copy of the house config (Loxone Config
17.2.8.28): the D73 probe rule (a state condition, a mode condition, an
`or` group, a mode action) and three fresh calendar entries — a yearly
range, a period, an Easter entry — next to the two existing ones.

- **Pass:** the calendar entries came back unchanged, attributes and
  order included.
- **Finding:** the save wired the rule's state (the UV switch's
  `active`) and the mode's `Q` into the AutoPilot container's `Input`,
  before the `DayOfWeek` input already there. Compiles now set that
  list from the rules (D73); a recompile against the saved file is
  byte-identical.

## Weather, day, daylight and date conditions (session 32, D76) — passed, one finding

Ran 2026-10-31 against a copy of the house config (Loxone Config
17.2.8.28): a probe rule with a time, a weather (temperature), a
day-of-month and a daylight condition, an `or` group with a date and a
time, and the actions message center, pause and notification.

- **Pass:** the rule's `APC=` came back unchanged.
- **Finding:** the save wired three sources into the AutoPilot
  container's `Input`: the `Daylight` block's `Q`, the `Day` block's
  `Q` and the `AQ` of the `WeatherData` block with `WType="1"`
  (temperature), before the inputs already there; the date and the
  times wire nothing. Compiles now set these too (D76); a recompile
  against the saved file is byte-identical.

## The scenario library (session 33, #227) — passed, four findings

Ran 2026-10-31 with Loxone Config 17.2.8.28 on a private prefix clone and
display, offline. These were not house configs: the base was Loxone's own
factory default `programdata/SDcard/prog/Default2.Loxone` (Config 11.1,
`V="141"`). It opens and saves cleanly, unlike the synthetic configs
above. The first save migrates it to `V="178"`. It adds the document's
`APPKEY`/`APPID`, a document `<HP H="" Sc="-2" S="<salt>" Alg="1"/>` and
`DeviceTelemetry="true"`, renames *Zeiten* to *Zeitfunktionen* (a locale
rename) and sets the admin's rights from 8191 to -1. Tree devices were
added in the GUI, the logic compiled, and each compile opened and saved.
The five entries of `examples/library/` (lighting, shading, heating,
access, energy) came out of it.

- **Pass:** every last compile came back with an empty semantic diff.
  That covers minted `InputRef`/`OutputRef` pairs on Tree sensors,
  analog sensors, actuators, Miniserver relays and Modbus values of a Tree
  energy meter; `LightController2` moods; `AutoJalousie`,
  `HeatIRoomController2`, the meters, `EFM`, `Formula` and
  `AnalogThresholdTrigger`; `access` on a GUI-made Code Touch; title,
  room and category claims; pages created with `room:`/`category:`. A
  recompile onto the energy entry's last save was byte-identical.
- **Finding — a ref's missing `I` is wired by the save.** An `InputRef`
  on a `DigitalIn` minted with only `AI: x.Q` came back with its `I`
  wired from the input's `Qe`, which the factory default already
  carries. Stating `I: x.Qe` converges.
- **Finding — `AMode` gets `Def="1"`.** A `HeatIRoomController2` minted
  without anything on `AMode` came back with `<Co K="AMode" Def="1">`.
  Stating `AMode: 1` converges. The first save also writes the controller's
  `<Timer>`: one all-day entry per operating mode in its `Modes=`.
- **Finding — light controller circuits come from the mood editor.**
  Neither a compiled wire `AQ2` → RGBW actuator nor an `OutputRef` on it
  made a plain save write the circuit (`NameAI<n>=`, `CapAI<n>=`,
  `PresM<n>=`, `COName="1"`). So `moods` with `rgb(…)` on that output
  did not compile ("does not fit a switch or dimmer circuit").
  Opening the controller's mood editor and pressing OK wrote them, with
  `CapAI1="31"` for the *RGBW 24V Dimmer Tree* and the circuit named after
  the device title, not the actuator's. The confirm also turned the stock
  mood *Viel Licht* on that output from `1` into a colour temperature
  (80 %, 4700 K).
- **Finding — GUI-only pieces.** Adding an *NFC Code Touch Tree* creates
  only the device (`TId="NFCCodeTouchTree"`, its keycode and NFC-tag
  captions). The `NfcCodeTouch` block, with `Ref=` to the device, category
  *Zutritt* and `HSize="10"` (*Anzahl Meldungen*), appears when the
  device is dragged onto a page. Users and their access codes come from
  the user dialog (`CodeArr=`: 32 upper-case hex digits; not the plain
  MD5 or SHA-1 of the code). A new user gets `UsR="16416"`,
  `UnsP="true"`, `LastEdit=` and the UI right *Eigene Kennwörter
  ändern*. An `EFM`'s meters come from *Monitor konfigurieren*, which
  writes `<root t="5" rst="true">` with one
  `<nd t="2|4" tit= ctrlUuid=<meter> liveState= efmState=>` per meter
  (`t` 2 grid, 4 producer; the type picker also offers consumer).
  `liveState`/`efmState` are the monitor's `SpStates` in order. A nested
  `MeterAbsBi` *Rest* (`IName=` its own UUID, which is also the root's
  `u=`, `NOEFM="true"`) is added for the unmetered consumption.
- Offline-added Tree devices carry `Serial="00000000"`. A page renamed
  in the GUI gets `LtE=`.

## Canvas labels of input keys (session 34, D68) — passed

Ran 2026-10-31 with Loxone Config 17.2.8.28 on a private prefix clone and
display, offline, for the simulator's new models. Synthetic configs crash
on open, so the base was `examples/library/shading/shading.Loxone`. One
compiled probe per block put the block on the page with a `Not` wired
into every fourth input (`keys[g::4]`, one file per offset `g`). The
canvas lists wired rows in key order, so each wire pins one key to its
label without ambiguity. The labels are recorded in
[loxone-format.md](loxone-format.md) (*Canvas labels of input keys*).

- **Finding — `MeterDig` is the fixed value meter.** Its block reads
  *Festwertzähler* with the inputs S and R. It is not the pulse meter that
  the generated block description claimed.
- **Finding — `EndUp`/`EndDown` are Co/Cc** (complete open/close), and
  `EnMove` is DisP (disable motion): the keys' names say otherwise.
- `AutoJalousie.WP` has no canvas row. `CentralShade`'s outputs are
  counts (No, Nc), not wires to its shades.

## LightController2 manual inputs in the simulation (session 35, #250) — passed, three findings

Ran 2026-10-31 with Loxone Config 17.2.8.28 under Wine, offline: no
Miniserver, never the connect action. Unlike sessions 1–34 this used
Config's own **simulation** (*Testen* › *Simulation starten*) as a
behavior oracle. It is Loxone's model of the blocks, not a Miniserver:
the docs cite it as such. Method:

- A probe compiled onto `examples/library/lighting` puts a
  LightController2 on a page with the moods "Viel Licht" (777), A (1,
  AQ1 20 %), B (2, 40 %), C (3, 60 %) and off, in that order, and
  virtual inputs on M+, M−, T5/1, T5/2, On, Off, Alarm, Buzzer and Mood.
- The compiler mints no InputRef boxes, so the virtual inputs were
  dragged from the periphery tree onto the canvas and the file saved by
  Config. In the simulation a digital input toggles with the switch icon
  on its box, and every connector shows its live value; hidden outputs
  (M, 2C, 3C) are shown with the block's ⊕ dialog before starting.
- Pulses were two toggles; readings were screenshots of the block.

Results (M is output `Scene`):

- **M**: 0 off, 99 for "Viel Licht" (`SID="777"`), the SID for a user
  mood (1, 2, 3).
- **M+** from off: 99, 1, 2, 3, 0, 99 — list order, the off mood a
  step, around the end. **M−** from 99: 0, 3, 2, 1, 99. After Off, M+
  goes to 99.
- **T5/n** selects the mood `SID="n"`; a further pulse steps on like M+
  while that input made the last change or its mood shows: T5/1 from
  off gives 1, 2, 3, 0, 99; T5/2 after T5/1 gives 2; T5/1 after M+
  gives 1; T5/2 while mood 2 shows (chosen by T5/1) gives 3. The
  canvas labels T5/n with the name of the mood `SID="n"`.
- **On**: its rising edge selects 99, from off and from mood 2; while
  held, M+ did nothing; on release the mood stays.
- **Buzzer** without an alarm-clock mood selects 99, as the KB says.
- **Alarm** held: Lc1 and M flash between on/99 and off/0 with a period
  of about 1.2 s; when it ends the previous mood returns.
- **Finding — no 30 s reset.** The KB says T5 selects its mood again
  after 30 s without a pulse; T5/1 still stepped on after 32 s and 65 s.
  The simulator refuses that case.
- **Finding — no double click.** Two pulses 0.15–0.2 s apart (Tdc
  0.35 s) acted as two single clicks, not off + 2C. The simulator
  refuses a second click within Tdc.
- **Finding — Alarm flashes at about 1.2 s**, not at Afi (4 s); still
  refused. *Corrected in session 36:* the controller had no `Def=` on Afi
  and flashed fast; screenshots aliased that to about 1.2 s.
- A constant on Mood (`Select:` 2, 777, 99, 0, 5, 778, 1, one controller
  each) left M at 0 on all of them: a constant selects nothing. The
  analog virtual input could not be set from the GUI, so Mood stays
  refused. A T5 hold ended with nothing on the falling edge; mixing (Mmd)
  stays unverified.

## LightController2 in the simulation, continued (session 36, #257) — passed, five findings

Ran 2026-10-31, same rig and method as session 35, on three more probes
compiled onto session 35's saved file (its virtual inputs keep their
canvas boxes). New tricks:

- **Mood** takes the result of a `Formula` over digital virtual inputs
  (`I1*2+I2*99+I3*777+I4*1`, then `I1*4+I2*50+I3*98+I4*1`), so toggling
  one input steps the value.
- A `Formula` (`I1`) on an output shows its value as a number. A
  `Memory` shows only Ein/Aus.
- Alarm timing came from about 390 screenshots in 24 s, reading the colour
  of each M pill.

Results:

- **Mood selects by ID when the value changes.** 0 and 778 select off.
  2, 1 and 3 select their moods. 99 and 777 select "Viel Licht", and so
  does every other value: 4, 50 and 51 (no such mood), 98 (no alarm-clock
  mood), 100, 102, 779, 876 and 877. Session 35's constants selected
  nothing.
- **Finding — M+, M− and T5 act when the input falls.** M+ held for 1 s
  and T5/1 held for 2 s changed nothing until release. On and Buzzer act
  when they rise.
- **Finding — no long click.** T5/2 held for 2.6 s (Mmd 1 s) with mood 1
  showing selected mood 2 on release, a plain click; M never showed −3.
  The KB says it mixes in the mood or steps on, so the case stays
  refused.
- **No double click, again.** Four toggles 110–150 ms apart (presses
  220–300 ms apart, Tdc 0.35 s) stepped twice from 99 to 2. Off would
  have shown 0.
- **Finding — manual moods and motion.** With Met 20 s and Moet 10 s:
  - A mood chosen by T5 stayed for 32 s without motion.
  - Motion during that mood left the mood alone; the lights went off
    18–22 s after motion ended (Met, not Moet).
  - A T5 click while the motion mood showed (Mo held) selected its mood,
    which also went off Met after motion ended.
- **Pto counts from the end of motion.** After Off, motion pulses 2 s
  and 10 s later were ignored, and so was one at 19 s (Pto 15 s). After
  Off, motion held from 2 s to 12 s kept the next pulse at 20 s ignored.
  T5 stepping to the off mood held the automatic back too: motion 2 s
  later was ignored, but motion 11 s after motion ended was not. That
  fits neither rule, so it stays refused.
- **Finding — Alarm flashes at Afi**, on first. Afi 10 s gave 5 s on
  and 5 s off, Afi 1 s gave 0.5 s each, and M showed 99 and 0 in step.
  Every wired circuit flashed, including one that no mood uses. The mood
  returned when Alarm ended. Without `Def=` on Afi it flashed much faster
  than the documented 4 s. Unconfigured circuits show only Ein/Aus, so
  the flashing value (MaxAbr) is the KB's.
- **Lc1** toggled its circuit with M staying 0. The KB says −1 for a
  user-defined mood, so Lc stays refused.

## Text-boolean attributes off (session 37, #298) — passed, one finding

Ran 2026-11-01 with Loxone Config 17.2.8.28 under `oracle.sh run`, in a reflink clone of the oracle
prefix on its own display. The base was the redacted library config `examples/library/energy`. The probe
minted four `VirtualIn`s (`Analog: off`, no `Analog:`, `Analog: off` with the attribute then deleted from
a scratch copy of the output, and an analog one with `MinVal: 0`), a `VirtualOut` with
`CloseAfterSend: off`, three `VirtualOutCmd`s (`Analog: off`, none, `on`), and a `VirtualUdpInCmd` and a
`VirtualHttpInCmd` with `Signed: off, Analog: off`.

- The digital `VirtualIn`s came back with `Analog="false"`, the stripped one included: a save writes it,
  and adds it where it is missing.
- On the commands and the output, the save dropped every `"false"` (`Signed`, `Analog`,
  `CloseAfterSend`) and kept `Analog="true"`. **Heal:** `off` on those compiles to no attribute. The
  recompiled probe diffs against the saved file in one line only, the finding below.
- **Finding:** the save dropped `MinVal="0"` on the analog `VirtualIn`, as on physical inputs.

## Hardware settings and statistics (session 51, D101) — passed

Ran 2026-11-01 on a hand-edited scratch copy of the house config (research
only, never compiled from source). The plain open and save of the
unedited copy gave an empty diff first. The save kept each edit:

- `StatsType="11"` with `StatsAutoDel="1"` added to a TreeAsensor, and
  `StatsType="11"` alone on another.
- `StatsType="7"` added to a TreeSensor.
- `StatsType` and `StatsAutoDel` removed from an I/O that had both.
- `Installation`, `SwitchBoard` (also from `""`) rewritten on a
  `LoxTree`, `Installation` on a `LoxAIR`. The save also added `TId=` to
  both, as on every plain save.

## Hardware declarations (session 52, D101) — passed

Ran 2026-11-01. A module took over one Tree device of the house config
by serial, four of its I/Os by designation and its Tree extension by
UUID: titles, `Desc` added on the device and a sensor, `Installation`,
`Unit`, `MinTime`, a room and category, `Statistics: on` and `off`, a
wire into an actor, the extension's place and board. The compile opened
and saved with an empty semantic diff, and a recompile of the save was
byte-identical. Removing the declarations left every value as written.

## Tree devices added offline (sessions 53 and 54, D102) — passed, findings folded in

Ran 2026-11-01 on scratch copies of the house config (research only). Corpus instances of the
candidate Tree device kinds, without wires and with fresh UUIDs, were added by hand to the branch of
one Tree extension, unpaired (`Serial="00000000"`).

- The save kept every device and its I/O.
- **Finding:** it added `TId=` after `Type=`, reordered the device's attributes, wrote `<IoData></IoData>`
  on the device and on actuators, and added a missing `<IoData>` (`St="2"` digital, `St="4"` analog).
- **Finding:** on a Valve Actuator Tree it added `SwitchBoard=""` and `SwitchBoardPos="1"`.
- The online status kept the kind's title.
- Session 54 repeated four kinds in the saved form, with connector UUIDs in the compiler's form: the
  semantic diff held only the Valve Actuator Tree finding. The kind table carries the saved form.

## Tree devices created by the compiler (sessions 55 and 56, D102) — passed

Ran 2026-11-01 on a copy of the house config. A module declared both Tree extensions by serial and
created one device of each kind, three on one extension's branch and one in the other's empty branch,
and an input of one of them by designation.

- Session 55: the compile opened and saved with an empty semantic diff. The created elements came back
  byte for byte, and a recompile of the save was byte-identical.
- Session 56: `removed` on two of them, compiled against that save. The save gave an empty semantic
  diff.

## Numeric settings at 0 (sessions 61 and 62, #312) — passed, three findings

Ran 2026-11-01 with Loxone Config 17.2.8.28 under `oracle.sh run`, in a private copy of the oracle prefix
on its own display. The base was the redacted library config `examples/library/energy`. Two probes set
every numeric setting the source can set to 0: on four fresh `VirtualIn`s (analog and digital), fresh
HTTP and UDP inputs and commands, a virtual output command, a Modbus server, device, analog and digital
sensor and actuator, three calendar entries, and on the base's analog and digital inputs (`AI1`–`AI4`,
`I1`) through extern settings.

- Of the 68 attributes written as 0, the save dropped 64: every range, correction, timing, Modbus and
  calendar number. **Heal:** a bound 0 on those compiles to no attribute. The recompiled probes differ from the
  saved files only in the findings below.
- **Finding:** a Modbus server keeps `Timeout="0"`.
- **Finding:** a virtual HTTP input raises `Poll="0"` and `Timeout="0"` to `"10"`.
- **Finding:** a digital `VirtualIn` turns `MaxVal="0"` into `"1"`.

## Values a save rewrites (sessions 63 and 64, #332) — passed, five findings

Ran 2026-11-01 with Loxone Config 17.2.8.28 under `oracle.sh run`, in a private copy of the oracle prefix
on its own display. The base was the redacted library config `examples/library/energy`. Session 63 minted
eleven virtual HTTP inputs (`Poll` and `Timeout` at 1, 5, 9, 10, 11, 9.5/10.5, -5, 999999999, 3600/60000,
`Allowedtimeouts` -1 and 1000), ten digital `VirtualIn`s (`MaxVal` 0.5, 2, 5, -1; `MinVal` 1, -1;
`DefVal` 1, 2, 0.5; a range 2 to 5 with `DefVal` 3) and three analog ones with a range upside down or a
default outside it. Session 64 minted twelve more HTTP inputs for rounding and the upper ends (`Poll`
10.4 to 12.5, 600.5, 604800, 604801; `Timeout` 10.6 to 4000.5, 2147483647, 2147483648, 4294967296,
99999999999; `Allowedtimeouts` 2.5, 3.5, 99999999999) and two digital inputs (`MaxVal` 1 with `MinVal`
0, `DefVal` -1).

- **Finding:** `Poll`, `Timeout` and `Allowedtimeouts` are 32-bit whole numbers: a fraction is cut off,
  a larger number wraps around.
- **Finding:** `Poll` goes up to 10 and down to 604800. `Timeout` goes up to 10 and has no upper end
  below 2147483647. `Allowedtimeouts` keeps any whole number.
- **Finding:** a digital `VirtualIn` always has `MaxVal="1"` and no `MinVal`. Its `DefVal` stays as
  written, also outside 0 to 1.
- An analog `VirtualIn` keeps its range and default as written.
- **Finding:** a save turned `<` in two `VirtualIn` titles into `_` (`R Ana Max<Min` became
  `R Ana Max_Min`), and kept `>`. Filed as #342.
- **Heal:** compile refuses a value the save rewrites and names the value the save writes. Every value
  the save kept compiles as before.

## Titles a save rewrites (sessions 80 to 83, #342) — passed, findings

Ran 2026-11-01 with Loxone Config 17.2.8.28 under `oracle.sh run`, in a private copy of the oracle prefix
on its own display. The base was the redacted library config `examples/library/energy`. Session 80
minted a digital `VirtualIn` and an `And` per printable ASCII punctuation character and per `ä`, `ß`,
`€`, `°`, `µ`, `²`, `é` and an emoji, and one object with all of them in its title for every virtual,
Modbus, calendar and mode type, a `Formula`, a created page and a `.title` claim on a base `Actor`.
Session 81 did the same for every other block type the compiler creates, and a `Mode` and a
`CalendarEntry` per character. Session 82 checked mood names, `LightController2` per character, tabs,
line breaks, spaces at the ends and empty titles.

- **Finding:** a save drops `{`, `}`, tabs and line breaks, and turns each space or tab at either end
  into `_`. `"{ a}"` became `"_a"`, `"a\t "` became `"a__"`.
- **Finding:** `%`, `<` and `|` become `_` on every block type and on the virtual and Modbus I/Os.
  `LightController2` keeps `<`. `Mode` keeps `<` but turns `,`, `:`, `;` and `=` into `_`.
  `CalendarEntry` also turns `/` into `_`.
- **Finding:** a page, a Modbus server and a mood name keep every character.
- **Finding:** an empty title comes back as `noname <UUID>`; one the save empties (`"{}"`) loses its
  `Title=`.
- **Heal:** compile and check refuse such a title and name the title the save writes.
- Session 83 compiled expressions with `<`, `<=` and `%`, whose block labels now read `≤`, `＜` and
  `％`. The save kept every label. It dropped a `Def="0"` on a `Less` block's `Input2` (`p1.Q < 0`),
  filed separately.

## Receive timeout of a UDP command (sessions 84 to 86, #346) — passed, three findings

Ran 2026-11-01 with Loxone Config 17.2.8.28, in a private copy of the oracle prefix on its own display.
The base was the redacted library config `examples/library/energy`. Session 84 opened a probe with a
virtual UDP input and two commands, and a virtual HTTP input with one command, under `oracle.sh up`
and edited them in the property panel.

- **Finding:** the UDP command's *Validierung* shows *Zeitüberschreitung Empfang [s]* and
  *Standardwert*. A timeout above 0 adds *Wert bei Zeitüberschreitung* (*Standardwert* or *Letzten
  Wert beibehalten*). The save wrote timeout 7 as `MaxTime="7000"`, *Standardwert* 3.5 as
  `DefVal="3.5"` and *Letzten Wert beibehalten* as `ValOT="1"`, each in a fixed place. A timeout set
  back to 0 dropped `MaxTime` and kept `DefVal`. Nothing else changed on the commands.
- **Finding:** the HTTP command's *Validierung* has no receive timeout, only *Validierung überwachen*
  and *Wertebereichsprüfung*.
- **Heal:** `MaxTime` (ms), `DefVal` and `ValOT` are settings of `VirtualUdpInCmd`. Session 85 minted
  five commands with them: the save changed nothing but `ValOT="0"`, which it dropped, so a bound 0
  compiles to no attribute.
- **Finding:** session 86 minted odd values. `MaxTime` and `ValOT` are 32-bit whole numbers:
  `MaxTime` `1500.5` became `1500`, `0.4` and `ValOT` `0.5` went, `3000000000` became `-1294967296`.
  `MaxTime="-3000"`, `ValOT="2"` and `"-1"` and `DefVal="0.001"` stayed. **Heal:** compile refuses a
  value the save rewrites.

## `Def="0"` on block connectors (sessions 90 and 91, #352) — passed, one finding

Ran 2026-11-01 with Loxone Config 17.2.8.28, in a private copy of the oracle prefix on its own display.
The base was the redacted library config `examples/library/energy`.

- Session 90 compiled 22 connectors at 0 on 15 types: the six comparators (one through an expression,
  `v.Q < 0`, one as `0.0`), `And.I1`, `Or.I1`, `Formula.Input1`, `Monoflop.Time`, `PulseGen` `TimeHigh`
  and `TimeLow`, `AnalogThresholdTrigger` `On`, `Off` and `PulseTime`, `OffDelay.Time`, `OnDelay.Time`,
  `PButtonT.Time`, `StairwayLS.TimeHigh`, `EdgeDetection.PulseTime` and `AnalogMultiplexer2.Input1`. The
  save dropped every `Def="0"` and nothing else. It kept `Def="1"`, and a `Monoflop` and an
  `AnalogThresholdTrigger` minted without `Def=` stayed without one.
- **Finding:** session 91 opened a `Monoflop` minted without `Def=` on `Time` (GUI default 2). The property
  panel shows *Duration* 0. A missing `Def=` is 0 on a connector, not the type's default.
- **Heal:** compile writes no `Def=` at 0, the simulator reads a missing one as 0, and decompile states
  `Port: 0` where the GUI default is not 0 ([D128](decisions/0128-no-def-at-zero.md)). A block minted
  without a parameter has it at 0 (#362).

## Light controller circuits (sessions 45 and 46, #284) — passed, three findings

Ran 2026-11-01 with Loxone Config 17.2.8.28 under `oracle.sh`, in a reflink clone of the oracle prefix
on its own display. The base was the redacted library config `examples/library/lighting`.

Session 45: a probe page with two minted controllers, `L1` (`AQ1` → relay Q2, `AQ2` → relay Q3) and
`L2` (`AQ1` → the Tree actuator *LED-Streifen*, `CCap="31"`), opened and saved, then edited in the
mood editor by hand and saved again.

- The plain save wrote no circuits, as in session 33.
- **Finding:** confirming the editor of `L1` wrote `NameAI0="Aktor (Relais) 2" PresM0="65535"
  NameAI1="Aktor (Relais) 3" PresM1="65535"`: the actuators' titles. It dropped `WF="16384"` on both
  controllers and rewrote `<HCL>`. `L2`'s circuit became a *Schalter* named *LED-Streifen* without
  `CapAI`.
- **Finding:** *Typ* lives in the moods. *Dimmer 0-100%* on `Lc1` rewrote `Q1` of every mood to
  `268435456`; renaming `Lc2` and naming the unwired `Lc3` (*Spot*) wrote `COName="6"`.

Session 46: the same page compiled with `Circuit1: "dimmer", Circuit2: "switch"` on `L1` and a mood
set (`AQ1 80%`, `AQ2 on`), and `Circuit1: "dimmer"` on `L2` with relay Q4, opened and saved.

- The semantic diff was empty: names, `PresM` and every mood prefix survived.
- **Finding:** the save added `COName="3"` (`L1`) and `COName="1"` (`L2`), with the first-save lists.
  Carried, like every `COName`.
- The editor showed `Lc1` as *Dimmer 0-100%* at 80 % in *Viel Licht* and `Lc2` as *Schalter*, *Ein*.
  Compiled again onto the save, the file came back byte-identical.

Not tested: whether the Miniserver dims such a circuit. That needs an upload.
## Central block members, alarm members, energy units (sessions 47 to 50, #278, #253) — passed, findings

Ran 2026-11-01 with Loxone Config 17.2 under `oracle.sh`, offline, on a private prefix and display. Each probe was
a public corpus config with `rec=`, `Objects=` or the units edited by hand, opened and saved. The facts are in
[loxone-format.md](loxone-format.md) (*Objects*).

Session 47: a CentralLight (base `gh_eisber_lox-cli_usecase-shorter-night-lighting`) and a CentralShade (base
`Patio_Door_Shading`) with `rec=` reversed, with a block of another type, a duplicate, a UUID of no object, an
empty list, and reordered with one member dropped.

- The order stays as written, and so do the other type and the duplicate.
- **Finding:** a UUID that names no object is dropped, and `rec=""` is removed.
- Nothing else in the file changed.

Session 48: `rec=` written at the end of the element, and the `Objects=` of an Alarm (base *Retrofit Burglar Alarm*)
reordered, with a hidden ref's UUID dropped from the list, and with the ref element removed instead.

- **Finding:** the save moved `rec=` right before `SpStates=`.
- `Objects=` stays in the order written.
- **Finding:** the list does not own its hidden refs. They are `InputRef`s (`WF="22528"`) wired into the alarm's
  hidden inputs. Dropping the UUID left the ref in the file, still wired. Removing the ref kept its UUID in the
  list, dangling, and removed the ref's wire. So `Objects=` stays refused.

Session 49: the units of an EFM (base `Hotwater_Heating_Rod`) changed, one removed, one empty.

- The changed values stay, and no absent unit is added back.
- **Finding:** the empty `Un2=""` is dropped, and the units are written in the order `UnSt Un1 Un2`, before
  `StatsType=`.

Session 50: two compiled files opened and saved: the light base with `.members` reordered with one dropped, plus a
new managed CentralLight with members, and the shade base with `.members = (left, right)`.

- The semantic diff was empty, `rec=` included. The save added `SpStates=` after `rec=` on the new central.
- Compiled again onto the saved files, both came back byte-identical.

## Units of Modbus and UDP settings (session 70, #344) — passed

Ran 2026-11-01 with Loxone Config 17.2.8.28 under `oracle.sh run --keep`, in a private copy of the
oracle prefix on its own display. The base was the redacted library config `examples/library/energy`.
The probe compiled a Modbus server (`Timeout="2345"`), an analog and a digital Modbus sensor
(`ModbusPollingCycle` 37 and 41, `MinTime` 1500 and 15, `MaxTime` 123000 and 45000) and a virtual
UDP command (`MinTime="25"`). The unedited open+save gave an empty semantic diff. The property panel
showed:

| Object | Panel | Shown | Range (tooltip) |
|---|---|---|---|
| Modbus server | Timeout [ms] | 2345 | 10…8000 |
| Modbus sensors | Abfragezyklus [s] | 37, 41 | 0…4095 |
| Modbus sensors | Zeitüberschreitung Empfang [s] | 123, 45 | 0…∞ |
| UDP command | Minimaler Zeitabstand [s] | 0.025 | 0…∞ |

Then each was set in the panel and saved: Timeout 2346, Abfragezyklus 38 and 42, Zeitüberschreitung
Empfang 124 and 46, Minimaler Zeitabstand 0.03. The save wrote `Timeout="2346"`,
`ModbusPollingCycle="38"` and `"42"`, `MaxTime="124000"` and `"46000"`, `MinTime="30"`. So
`Timeout` is in ms, the polling cycle in s, `MaxTime` and the UDP `MinTime` in ms.

- Neither Modbus sensor shows `MinTime` or `MinChange` anywhere in the panel, so their unit stays
  unknown.
- The UDP command also shows *Zeitüberschreitung Empfang [s]*. Setting it to 7 wrote
  `MaxTime="7000"` and switched *Wert bei Zeitüberschreitung* to *Standardwert*. Sessions 84 to 86
  (#346) followed this up.

## LightController2 on a Miniserver, live (2026-10-31)

Ran 2026-10-31 on a Miniserver (firmware 17.2.8.28), not in Loxone
Config. A test page with three controllers was compiled onto the house
config and pushed. The inputs were driven over HTTP (virtual inputs, by
name), and Formula blocks on the outputs were read by UUID. Settings:
Tdc 0.35 s, Mmd 1 s, Met 20 s, Moet 10 s, Pto 15 s. The original
program was restored afterwards (a byte-identical download). The
circuits had no actuator (`NameAI<n>=` is written only by Config's
mood editor). The details are in #261. Results:

- **Circuits without an actuator are switches.** Moods of 20 %, 40 %
  and 60 % all read 1.
- **Double and triple click.** Two T5 or M+ presses within Tdc switched
  off and pulsed 2C, from off and from a mood. Three pulsed 3C too.
  Presses 0.6 s apart were two single clicks.
- **Click timing.** A click acted at the later of press + Tdc and
  release: a 50 ms tap acted about 0.38 s after the press. One 0.6 s
  T5 hold did nothing; a rerun did not reproduce that.
- **Long click** (T5 held for Mmd), acting while held:
  - From off it selected the mood; the release did nothing more.
  - With another mood on it mixed the mood in, and M read −3.
  - On a mood already in the mix it took that mood out (back to the
    other, or off).
  - A short T5 click during a mix selected that mood alone.
  - M+ has no long click: it acts on release.
- **T5 and 30 s.** T5 steps through the list cyclically, off included.
  Within 30 s of its last pulse it steps on. After that it steps on only
  while its own mood shows, otherwise it selects that mood.
- **A manual step to off** (M+, M− or T5) holds motion back like Off:
  motion switched on only after Pto without motion.
- **Lc1–8** toggled their circuit (on 1, off 0). The other circuits
  stayed, and M read 0.
- **MBr** acted only when its value changed, as a manual change with M
  0: 0 switched everything off, 50 and 100 read 1. A mood selected while
  MBr was 0 lit normally. A change while everything was off changed
  nothing.
- **DisP** (`EnMove`) 1 ignored motion; at 0 motion worked again.

## The simple-logic batch (sessions 38–39, #316) — passed

Ran 2026-11-01 with Loxone Config 17.2.8.28 on a private prefix and display, offline. Base:
`examples/library/shading/shading.Loxone`. One page minted StateV, WipingRelay, Mult, Add, Sub, Div, Avg,
Int, UpDownCounter, HourCounter, Counter, PulseBy and AMemory, each in its corpus connector order, with
parameters, inputs wired from virtual inputs, and every output wired into a `Formula`.

- **Session 38:** the semantic diff was empty and every wire survived. Eleven types came back with the
  identical connector keys, order and UUIDs; Avg kept the `Reset` that only the newest corpus generation
  has. Two were rewritten, keeping every existing UUID:
  - UpDownCounter's connectors were reordered to `Trigger, InputDir, Reset, OnValue, OffValue, Remanence,
    StartValue, Q, AQ`.
  - HourCounter gained `OutputAPI` with a GUI-minted UUID, plus two special states and `IoData`.

  The save added `Analog="false"`, `Tx="false"`, `IoData` and `Display` to StateV and `WTA=""` to PulseBy.
  The tables took the saved order.
- **Canvas labels:** the canvas rows of the wired inputs named `InputTrigger` Tr on WipingRelay, `Reset`
  Off on Counter and `InputResetAll` Off on HourCounter, which Loxone's documentation lacks
  ([loxone-format.md](loxone-format.md), *Canvas labels of input keys*). The other rows matched it.
- **Session 39:** the thirteen types minted in the saved order came back with identical connector keys,
  order and UUIDs, and an empty semantic diff. A recompile onto the saved file was byte-identical, and so was
  a compile of the module `loxl adopt` made from it.

## Block settings (sessions 43 and 44, D95) — passed, four findings

Ran 2026-11-01 with Loxone Config 17.2.8.28 on a private prefix and display, offline. The base was
`examples/library/shading/shading.Loxone`. One probe minted a Memory, a PulseAt, a DayTimer, each meter
type, a Code16 and virtual inputs, then set the attributes in question by hand: `Tp=` 1 and 2, `Typ=` 1
to 12, `Desc=` at the end of the element, meter fields, DayTimer texts, `ValOT=` and `Idx=` from 0 to 3.
Each was opened, saved and read back, and the property pane of each block was read. Session 44 ticked
and unticked options in the panes and saved again. The facts are in [loxone-format.md](loxone-format.md)
(*Objects*).

- A save keeps `Tp=`, `Typ=` 0 to 11, the meter fields and the DayTimer texts. It moves `Desc=` right
  after `Type=`. The panes name each field.
- **Finding — `Typ=` 12 is no choice.** The dropdown has twelve entries; 12 shows blank.
- **Finding — one-off is a date.** Ticking *Einmaliger Impuls* adds a date field and writes `Sec=` as an
  absolute date, with `LtE=`.
- **Finding — `Sun=` off is absent.** Unticking *Sonnenschein verwenden* removes `Sun=`.
- **Finding — hidden virtual-input fields.** No pane shows `MinChange=`, `MinTime=`, `ValOT=` or `Idx=`.
  A save drops `ValOT="0"`, `Idx="0"` and `MinVal="0"`, and forces a digital input's `MinChange=` to 0.5.
- A Program block's pane has no field for `Task=`.

## The first scripted run

First scripted run 2026-08-26: the full r50 rebuild (1929 objects,
templates + expressions + D36-era compiler) open+saved with an empty
semantic diff.
