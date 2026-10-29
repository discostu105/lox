# The save oracle: Loxone Config under Wine

Loxone Config saves projects offline, without a Miniserver. Running it
under Wine on Linux turns it into a scriptable **oracle**: open a
compiled config, save it, and diff — every divergence is either a bug in
the compiler or a new fact about the format. First run: 2026-08-24,
Loxone Config 17.1 (`ConfigVersion 17010727`) under Wine on CachyOS/KDE
Wayland.

## Headline result

**Loxone Config accepts lxir-compiled output.** A real 1.3 MB config with
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

Second oracle run (2026-08-25), probing design decision D8 on the
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

Then the compiler-side test: `lxir compile` minted `I3` at connector
index 3 (after `Q`'s 2 — the D8 assumption) wired from an `InputRef`,
onto the previously saved base. Loxone Config **loaded the file without
complaint** — and on `Ctrl+S` **silently deleted the `I3` connector and
its wire**, reverting the gate to `Nio="3"`. Everything else survived
byte-for-byte (semantic diff: exactly the one wire removed, zero object /
param / rename changes; the `NextObj` +2 burn and the `WF`
147456 → 16384 normalization appeared again).

Conclusions, now encoded in the compiler and docs (design.md D8,
loxone-format.md):

- **Gates are fixed two-input in Loxone Config 17.** `lxir` refuses
  `I3`+ with a cascade hint instead of minting it.
- **A save writes exactly the descriptor's connector set** —
  off-descriptor `<Co>`s are dropped silently, wires included. Never
  invent connectors.
- Positive side-finding: a compiler-drawn wire from an existing
  `InputRef.AQ` into a managed block survives the save untouched, and
  re-emitting a GUI-drawn wire from source (after teardown) reproduces it
  byte-identically.

## The adopt round-trip (D18) — passed

Third oracle session (2026-08-25), directly after `lxir adopt` shipped:
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
resolved the same day by design decision D19 (GUI-owned residue is
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
  layer. `lxir drift` (D21) stayed green across the GUI save: the save
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
  with zero refusals, compile is a semantic no-op, `lxir drift` green.

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

Rig note for next time: the canvas vertical scroll caps at the page
extent computed on load — blocks compiled below the last used page
row exist and save fine but cannot be scrolled into view without
interaction that recomputes the extent. Read the XML instead of
fighting the viewport.

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

Ran 2026-10-29. Question: does Loxone Config keep what `lxir repair`
writes (raw line breaks where a foreign writer had left `&#xA;` text)
in `APC=`, `NDOC=` and `Code=`? Each probe took a config, encoded the
line breaks of one attribute kind as `&amp;#xD;&amp;#xA;` (the
GUI-saved damage), ran `lxir repair`, then one open+save:

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
AnalogThresholdTrigger connectors are reordered, and minted ApiActor
refs gain `Text="true"`, which GUI-created ApiActor refs carry under
17.1 too and D33 does not emit yet.

## The rig, one command

Everything below is wrapped in [`scripts/oracle.sh`](../scripts/oracle.sh):

```sh
scripts/oracle.sh run compiled.Loxone --out saved.Loxone   # open → save → diff
```

starts Xvfb, seeds the prefix's ProgramData from the installed package
(as the packaged launcher does for its own prefix), opens a **copy** in
the isolated wineprefix, answers the recovery dialog with No, unmaps the
news overlay, saves via Ctrl+S (md5-polled), runs `lxir diff` compiled →
GUI-saved, and tears the rig
down (`--keep` to leave it warm; `up`/`open`/`save`/`shot`/`status`/
`down` subcommands drive the pieces for interactive sessions).
Paths are env-configurable (`ORACLE_PREFIX`, `ORACLE_EXE`,
`ORACLE_DISPLAY`, `ORACLE_DIR`, `LXIR`); the script refuses to launch
into a prefix whose wineserver already serves another display — that
would open the file as a tab in the desktop instance.

First scripted run 2026-08-26: the full r50 rebuild (1929 objects,
templates + expressions + D36-era compiler) open+saved with an empty
semantic diff.

## The rig

- Prefix: `~/.local/share/loxone-config/wine`, exe under
  `drive_c/Program Files (x86)/Loxone/LoxoneConfig/`. `LoxoneFormat.exe`
  is an SD-card formatter, not a CLI — there is no headless mode; the GUI
  must be driven.
- Open a file directly (spawns/reuses the GUI):
  `wine LoxoneConfig.exe 'Z:\path\to\copy.Loxone'` — always on a **copy**;
  never open the live project file. Opening never contacts the
  Miniserver; the connect action is a separate explicit button that
  automation must never touch.
- An auto-backup recovery dialog ("Soll diese wiederhergestellt
  werden?") may appear for a project identity that was open before —
  answer **No** (`Alt+N`) so the file opens as compiled.
- **ProgramData must match the installed version.** Loxone Config
  reads its icon library, block images and templates from
  `ProgramData/Loxone/Loxone Config <version>`. The packaged launcher
  (`/usr/bin/loxone-config`) copies them from
  `/opt/loxone-config-bin/programdata` into *its* prefix whenever the
  package version changes, stamped in `.loxone-config-bin-version`. The
  oracle runs the exe directly, so after the update from 17.1.7.27 to
  17.2.8.28 its prefix had no ProgramData for the new version.
  - Seen 2026-10-29: loading took 10+ minutes at full CPU and ended in a
    message box, "Die Icon Library konnte nicht geladen werden". The
    `Ctrl+S` sent meanwhile never saved.
  - The 18.10. runs happened to save in time.
  - `oracle.sh up` now seeds the prefix the same way and with the same
    stamp. A full r50 open+save then takes 17 s.
- Loxone Config's message boxes carry the main window's caption
  (`Loxone Config`, ~84 px high). `oracle.sh` tells the main window
  apart by height, and a failed save says when a message box is up.
- **Keyboard injection works** on KDE Wayland: activate the window via a
  KWin script (`workspace.activeWindow = w` over qdbus, matching caption
  `Loxone Config`), then send X11 XTEST key events (python-xlib) —
  XWayland delivers them to the focused Wine window. `Ctrl+S` saves;
  a freshly opened file is already dirty (schema migration), so the save
  always writes.
- **Pointer injection does not work** via XTEST under KWin: fake motion
  is ignored and clicks land at the physical cursor. The fix is the
  **headless rig** (used for the grown-gate experiment): run
  `Xvfb :5 -screen 0 2560x1440x24 -fbdir <dir> +extension GLX +render
  -noreset`, launch with `DISPLAY=:5` and `LIBGL_ALWAYS_SOFTWARE=1` —
  there `xdotool` has full native pointer *and* keyboard control
  (block selection, wire drags with mid-drag screenshots, menus).
  Screenshots come straight off the framebuffer:
  `magick xwd:<dir>/Xvfb_screen0 shot.png`.
- Xvfb quirks: the QtWebEngine news panel renders as a white overlay
  child window (~630×500) — find it via `xdotool search --name ".*"` +
  geometry and remove it with `xdotool windowunmap <id>`. Wine popup
  menus/pickers ignore Escape and outside clicks under Xvfb; unmap those
  the same way. Trust `xdotool search --onlyvisible` over screenshots
  (stale repaints), and trust the saved XML over the canvas.
- Project bookkeeping: opening a file rewrites
  `drive_c/users/<user>/Documents/Loxone/Loxone Config/Projects/Projects.json`
  (maps project → file path + Miniserver address) — useful to verify
  *which* file an instance actually has open, since window titles don't
  show paths.
