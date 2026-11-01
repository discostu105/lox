# The `.Loxone` format

What the `.Loxone` program file looks like, and what Loxone Config and the Miniserver do with it. There is no
official specification. This is reverse-engineered knowledge, and the reason the crate's parser is
hand-rolled. Facts marked **assumption** or **unverified** have no direct evidence yet.

Evidence: six generations of one installation's config (Aug 2024 – Aug 2026, 117 KB – 1.34 MB, ~1900
objects), the public corpus (`tools/fetch-corpus.py`), a live Miniserver (FW 17.x), and Loxone Config itself
under Wine ([oracle-wine.md](oracle-wine.md); the log of each run is [oracle-sessions.md](oracle-sessions.md)).

## Container

- UTF-8 with **BOM**, `<?xml version="1.0" encoding="utf-8"?>` declaration,
  **CRLF** line endings, **tab** indentation, single space between
  attributes, no space before `/>`. Empty elements are *usually*
  self-closing, but not always: the GUI writes `<IoData></IoData>` even on
  freshly created blocks. Preserve the form per element; never normalize.
- Root: `<ControlList Version="…" NextObj="…" NextConst="…" NextNote="…"
  NextMem="…">`. The `Next*` counters are bumped by Loxone Config on save
  and must never decrease. `NextObj` correlates with auto-titles (`O1415`).
  A save burns two `NextObj` values even with nothing added (see *What a
  save changes*).
- Inside: one `<C Type="Document">` (carrying `ConfigVersion=` among many
  attributes), containing caption/folder `<C>` elements, `<C Type="Page">`
  drawing pages, and the object tree.
- Pages are the direct children of the one `<C Type="Program">` (titled
  after the Miniserver). A page is `<C Type="Page" V=… U=… Title=… LtE=…
  WF="16384">` with `<IoData Cr="<Category>" Pr="<Place>"/>` and its
  objects. The `<IoData>` comes first on most pages and after the objects
  on some.
- `LtE=` is a page's last edit, in seconds since 2009-01-01 UTC. A page
  renamed in the GUI gets one. A page without `LtE=` loads and saves
  unchanged ([D56](decisions/0056-creating-pages.md)).
- The special page "Automatik-Regeln" carries `SpNo="1"`, no `LtE=` and
  the unassigned `Place`.
- "Neue Seite" (Tools ribbon) asks for a title and a room. Save stays
  disabled while the room is the unassigned one, and cancelling discards
  the page. The page gets `Cr` the unassigned category and `Pr` the chosen
  room. The GUI files it among the others in case-insensitive title
  order, but never re-sorts existing pages, so real lists are not sorted.
  Source: oracle session 19 (Config 17.2).
- `PaperFormat=` on the Document is the paper sheet every page is drawn
  on; pages carry no size of their own. `2` is A3 landscape, the format
  of 109 of 112 corpus configs and the largest the GUI offers. The GUI
  draws the sheet at 420 × 297 mm in twips (1/1440 in): x 0..23808,
  y 0..16800, with a title block printed along the bottom from
  y ≈ 16080. Objects past the edge are drawn on
  the grey canvas beyond it. `1` and `3` occur (one and two configs)
  with unknown sizes ([D50](decisions/0050-placement-paper-sheet.md)).
- A config's first save in a newer Loxone Config migrates it: see *An old
  config's first save*.

## Not actually XML

Two deviations break conforming parsers:

1. **Attribute names may start with digits**: `12hTF="true"`.
2. **Attribute values may contain raw, unescaped newlines** — the `Code=`
   attribute of a `Program` block holds multi-line PicoC source, and two
   of the house config's `AutopilotRule` `APC=` payloads carried a raw
   CRLF inside the JSON (until the damage described below). A standard
   parser normalizes these to spaces on re-serialization, corrupting the
   program, and a standard writer's `&#xA;` is no way out either (see
   below).

Entities used: `&amp; &lt; &gt; &quot; &apos;`, and nothing else.

Loxone does **not** decode numeric character references. `&#xA;` in an
attribute is the five characters `&#xA;`, and Loxone Config re-escapes the
ampersand on its next save (`&amp;#xA;`). A writer must therefore emit line
breaks raw (deviation 2) and escape only the five named entities. No config
in the 133-file corpus contains a numeric reference. Evidence: a foreign XML
writer pushed the house config on 2026-08-25 with the PicoC program's line
breaks encoded as `&#xA;`. The GUI save of 2026-08-28 turned them into
`&amp;#xA;`, and the program has been one comment line since. Two `APC=`
payloads and ten Modbus `NDOC=` texts went the same way.

Lox Logic reads `&#…;` as literal text, and `loxl drift` lists every
attribute that holds one ([D40](decisions/0040-program-code.md)). `loxl repair` writes a copy with each
reference replaced by the raw character it names ([D44](decisions/0044-loxl-repair.md)). Loxone Config keeps
repaired `APC=`, `NDOC=` and `Code=` values byte-identical on open+save.
Source: oracle session 13.

Text content appears only in `<Key>hex</Key>`-style elements and is
rendered inline (`<Key>2B35</Key>`).

## Objects: `<C>`

```xml
<C Type="Or" V="175" U="1e96b762-0130-074c-ffffed57184a04d2" Title="O1415"
   Px="5952" Py="11136" Px2="7296" Py2="11832"
   Cl="141,255,112" Nio="3" WF="147456">
```

- `Type` — block type (`Or`, `AutoJalousie`, `VirtualIn`, …).
- `V` — block-format version, one per document: Loxone Config writes
  every object at its release's version on save. 16.1 writes `"174"`,
  17.0 and 17.1 write `"175"`, 17.2 writes `"178"`. Each corpus file has
  exactly one `V` across its objects. Loxone Config migrates a block of an
  older version on load. All 24 builtin types minted at `V="178"` survive
  a 17.2 open+save at 178 with their exact connector keys and indexes
  ([D46](decisions/0046-block-version.md)). Source: oracle session 14.
- `U` — the object UUID (see below).
- `Title` — display name. **Locale-volatile** for built-ins: saving the
  config in a differently-localized Loxone Config renamed 111 built-in
  objects (Modes, weather fields, caption folders) in one observed save.
  Never use titles as identity. Titles are not unique either: one real
  config has four blocks titled "O1657". A title may hold any text: the GUI keeps
  `>=`, parentheses and dotted port names (`Title="dienstag.Q &gt;= 1"`)
  byte for byte and shows them as plain captions. Source: oracle session
  10.
- `IName` — internal name for I/O objects (`VI1`, `AI3`); locale-stable.
- `Px/Py/Px2/Py2` — drawing rectangle. Grid unit 96, port rows 192
  apart. Small logic blocks (gates, Monoflop, …) are 1344 wide
  and `312 + 192·rows` tall: Not 504, And/Or with two inputs 696. Their
  first port row is 288 below the top. Function blocks (PushButton,
  LightController2, AutoJalousie, …) are 2688 wide and `504 + 192·rows`
  tall, with the first row at 480: PushButton 1272 (4 rows), LC2 1656
  (6). `rows` counts the rows the GUI shows, the larger of the two
  sides:
  - Some types show at least a set number of rows: LC2 6; AutoJalousie,
    CentralShade, CentralLight and Code16 4; DayTimer 3; PButtonT and
    PulseAt 2. Wired connectors beyond those add rows.
  - Other built-ins show their inputs and outputs, and show params and
    `Remanence` only when they are wired.

  Ref tags (`InputRef`/`OutputRef`) are 2112 × 192 and sit 960 off the
  block edge, level with the port. `Memory` draws as the same 2112 × 192
  tag but is a block. The GUI snaps `Py` to the grid on save and
  recomputes `Px2`/`Py2` from the type. No corpus object has negative
  coordinates. Source: corpus statistics (the mode per type) and GUI refs
  placed next to block ports; Lox Logic's placement uses these rules
  ([D42](decisions/0042-placement-gui-geometry.md)).
- `Cl` — RGB display color. Defaults differ by type: logic blocks are
  green (`141,255,112`), Memory grey, AutoJalousie blue.
- `Desc=` — a description. Loxone Config writes it right after `Type=`.
- `Nio` — connector count. Matches the number of `<Co>` children: Loxone
  emits **all** connectors, including unwired outputs and inputs without
  values.
- `WF` — view flags, freely recomputed by the GUI and never identity.
  Saves rewrite it in both directions (`147456` ↔ `16384`, plus other
  values) and drop it on some types (see *What a save changes*).
- Complex blocks carry extra attributes (`LtE=`, `SpStates=`, `rec=`,
  UUID-list attributes…). All of it round-trips untouched through the
  lossless tree. Some of it is **block logic stored outside connectors**:
  - `PulseAt` keeps its fire time in `Sec=`, seconds since midnight
    (`At:` authors it, [D79](decisions/0079-pulseat-fire-time.md)). `Typ="0"` is the fixed-time mode: every
    V172+ instance with a `Sec=` has it (11 of 11), and a corpus block
    titled "Pulse at 00:00" has `Sec="0"`. Loxone's documentation offers
    time functions (sunrise, dusk, hour and month pulses) in the same
    dialog. Which `Typ=` value means which is not observed yet, so a
    non-zero `Typ=` stays with the GUI. `AutP=` also appears on `Memory`
    blocks and is not a PulseAt flag.
  - `DayTimer` keeps its schedule as `<Entry>` child elements after the
    `<Co>` list (`N=` holds their count). `Analog=`/`DefValue=`/`On=`/
    `Off=` set the output behavior, `Modes=`/`UserModes=` the
    operating-mode gating (see *Operating modes*). The entry format:
    - `Modes=` lists operating-mode UUIDs, one per column.
    - An `<Entry>` names its mode by index into that list (`Ix=`, absent
      = 0). It spans `From=`…`To=` in minutes of the day (`From` absent =
      0, `To="1440"` = midnight), carries `V=`, and optionally
      `NeedActivate="true"`.
    - Entries are ordered by `Ix`, and `N=` equals the entry count.
    - Every corpus `Modes=` list has all seven weekdays (see *What a first
      save adds*).

    Source: corpus (26 DayTimers in 145 configs); oracle session 18.
  - `Formula` keeps its formula in an attribute, `Formula="I1+I2"`,
    written with a companion `Valid="false"`.
- Blocks created in the GUI carry display attributes (`Tp=`, `Sun=`,
  `SpStates=`) and child elements (`<IoData>`, `<Display>`, `<PSD>`,
  `<COHist>`). Loxone Config adds missing children to a block on every
  save (see *What a first save adds*).
- Some blocks hold `<C>` children that Loxone Config creates with them.
  An energy flow monitor holds a nested `MeterAbsBi` titled *Rest* (see
  *Energy flow monitor*), a Code Touch holds its `Keycode`s. Lox Logic
  treats them as part of their block
  ([D78](decisions/0078-owned-sub-blocks.md)). Source: oracle session 33.
- Element order of `<Co>` children is the **canonical connector order**;
  the UUID index byte is historical. Proof: the V259 schema migration
  moved DayTimer's `PulseTime` from element position 6 to 3 while its
  port UUID kept index byte `06`.

## Connectors and wires: `<Co>` / `<In>`

```xml
<Co K="I1" Nc="1" U="1e96b762-0130-0749-00ff69723a2bac9e">
	<In Input="1e96b5a8-02ec-bf33-02ffcb4248672396"/>
</Co>
<Co K="I2" Def="1" U="…-01ff…"/>
<Co K="Q" U="…-02ff…"/>
```

- `K` — port key (the name the source uses).
- `U` — the **port's own UUID**; wires reference port UUIDs, not object
  UUIDs.
- Wires are stored at the **sink**: each `<In Input="…"/>` names the source
  port's UUID. An output being wired leaves no trace on the output itself.
- A wire source needs **no on-page representation**: the system `Mode`
  objects (operating modes; no `Px/Py`, deterministic UUIDs) source wires
  directly into page blocks. The GUI's own pattern inserts an `InputRef`
  when a mode is dragged onto a page (nine corpus configs). A save keeps
  the direct wire and creates no `InputRef`. Source: oracle, 2026-08-25.
- A connector may hold several `<In>`: a wire added onto an already-wired
  extern connector survives a save next to the old one.
- A save keeps a wire drawn in the wrong direction. A save therefore never
  proves a connector's direction; the direction tables rest on corpus
  wires ([connector-db.md](connector-db.md)). Source: oracle session 25.
- `Nc` — number of incoming wires (= count of `<In>` children); omitted
  when zero.
- `Def` — the port's parameter value; omitted at type default. A wired
  `<Co>` can carry one too (the Loxcall library entry: an `Equal`'s
  `Input1` with `Def="2"` and an `<In>`): Loxone Config keeps the value
  when a wire is drawn onto the port. The wire supplies the input; the
  constant is what the port reads again once the wire is removed. Lox
  Logic carries it ([D77](decisions/0077-stale-constant-under-wire.md)).
- Attribute order: `K, Def, Nc, U`. A save rewrites any other order into
  this one.
- An `<In>` may carry `FLG="1"` (rarely `"2"`): wire metadata the
  Miniserver or the app writes, mostly on API-connector and central-alarm
  wires (113 of the house's 880 wires). Loxone Config treats it as inert
  stored state. It round-trips verbatim and is never regenerated, and a
  wire with the flag stripped survives a save plain, neither deleted nor
  re-flagged. Source: oracle, 2026-08-25.
- A `<Co>` may carry `Inv="true"`, the GUI's input inversion. Wired, it
  negates the incoming signal. Its main use is **unwired**: it turns the
  constant-0 input into a constant 1. That is how the GUI encodes boolean
  checkbox settings like an enabled Remanenz. Every one of the house's 23
  `LightController2`s, its `PushButton`s and `DayTimer`s carry
  `Inv="true"` on an unwired `Remanence`. Lox Logic treats such a
  connector as GUI-owned: carried verbatim, refused as a target of a
  declared wire or value ([D20](decisions/0020-inv-connector-gui-owned.md)).

### Canvas labels of input keys

The KB names a block's inputs by their canvas labels (`Tr`, `Co`, `Mo`);
the XML by keys that often say something else (`EndUp` is *complete
open*, not an end switch; `EnMove` is *DisP*, disable motion). The table
maps each key to its label. Source: oracle session 34, which wired a `Not`
into every fourth input of each block; the canvas lists wired rows in key
order, so each wire pins one key to its label.

| Type | Key → label |
|---|---|
| `Switch2Button` (*Tastschalter*, push switch) | `InputTrigger` Tr, `On` On, `Reset` Off, `InputDisable` DisPc |
| `PulseAt` (*Impuls um*) | `InputDisable` Off, `Remanence` Rem |
| `MeterDig` (*Festwertzähler*, fixed value meter) | `A` S, `R` R |
| `MeterAbsUni` (*Zähler*) | `Pf` Pf, `Mr` Mr, `R` R |
| `MeterAbsBi` (*Zähler Bidirektional*) | `Pf` Pf, `Mr1` Mrc, `Mr2` Mrd, `R` R |
| `AutoJalousie` | `InputTrigger` Tg, `InputTriggerUp` Po, `InputTriggerDown` Pc, `EndUp` Co, `EndDown` Cc, `Shade` So, `AutoShade` Sps, `EnAutoShade` DisSp, `ReactAutoShade` Spr, `Safety` Wa, `Window` Dwc, `Stop` Off, `ManualPosition` Pos, `ManualLamelle` Slat, `Gesture` T5, `InputDisable` DisPc, `Type` Type, `SO` Wap, `Rdd` Rdd, `Deadtime` Bld; `WP` has no row |
| `CentralShade` | as `AutoJalousie` without `Window`; its outputs `OutOpen` No and `OutClose` Nc count open and closed shades, the shades it drives are its `rec=` list |
| `LightController2` | `I1`–`I8` Lc1–Lc8, `InputTriggerUp` M+, `InputTriggerDown` M−, `Select` Mood, `Reset` Off, `Sel1`–`Sel8` T5/1–T5/8, `EnMove` DisP, `Move` Mo, `On` On, `Alarm` Alarm, `AlarmClock` Buzzer, `Brightness` Br, `InputDisable` DisPc, `Presence` P, `RtD` Rtd, `MasterBr` MBr, `Remanence` Rem, `MoveOn` Moet |

## Refs: `<C Type="InputRef"/"OutputRef" Ref=…>`

A ref is the GUI's page-local mirror of an object living elsewhere: a
periphery device port folder, a system flag, or another page's block.
Evidence: the house config (all 113 uuid-pinned ref externs resolved,
2026-08-25), the corpus, and oracle sessions 11, 12 and 15.

- `Ref` names the mirrored **object's UUID** (not a port): a
  `LoxAIRsensor`/`LoxAIRactor` subdevice, a `VirtualIn`, a flag object,
  or a managed block.
- The signal path is ordinary wiring on top: an `InputRef` carries an
  `<In>` from the mirrored object's output port into its own `AI`
  connector (`ref.AI <- source.AQ` in Lox Logic terms), and consumers wire from
  the ref's `Q`/`AQ`. `Ref=` is the GUI's display link; the wires are
  the semantics. Decompile folds these plumbing wires ([D29](decisions/0029-full-and-adoptable-view.md)); the
  `mirrors:` matcher resolves through `Ref=` ([D32](decisions/0032-mirrors-matching.md)).
- `LinkRefType` — a type-registry code of the mirrored object's XML type
  (`VirtualIn` 71, `Actor` 63, `Memory` 320, …; also 172, 173, 174 seen).
  `Analog="true"` marks analog refs and follows the target type. Lox Logic
  mints refs with these from a corpus-learned table
  (`connectors::ref_link_type`) and refuses a type it does not know ([D33](decisions/0033-minted-mirrors.md)).
  On unmanaged refs they are carried verbatim.
- A ref drawn by the save is a flat 2112 × 192 tag (see *Objects*).
- Refs of text values carry `Text="true"` where an analog ref has
  `Analog="true"`; no ref has both. Every corpus ref of an `ApiActor` (36)
  and of a `VirtualTextIn` (12) has it, from Loxone Config 12.0 to 17.2.
  On `Memory` and `VirtualOutCmd` refs it appears on a few instances only
  (4 of 806, 3 of 30): the flag follows the target's configuration, not
  its type. A ref of a text value written without it gains it on the next
  save. Lox Logic writes it on minted refs
  ([D48](decisions/0048-text-ref-flag.md)).
- Consumers read a mirrored signal through a ref on their own page: all 96
  `ref.AQ` and 5 `ref.Q` consumer wires in the house config are
  same-page. A cross-page wire without a ref is legal too, and a save
  keeps it ([D34](decisions/0034-mirror-routing.md)). Source: oracle
  session 6.
- **Every mirror carries its own ports.** `InputRef`s of one target
  have distinct `AQ`/`Q` connector uuids like any other object, so a
  consumer wire names which tag it reads from. `OutputRef` mirrors
  likewise each carry their own `AQ` and draw their own distribution wire
  into the actor. Evidence: house config, 19/19 multi-mirror targets and
  27/27 `OutputRef`s; corpus-wide no connector uuid repeats. A minted mirror's fresh output uuids ([D33](decisions/0033-minted-mirrors.md)) match what the GUI
  does. House snapshots of 2026-08-25 show shared uuids instead: damage
  from an upload tool (see *UUIDs are unique* below).
- **An `InputRef` of an input carries both of its outputs.** The ref's
  `AI` reads the input's `AQ`, and its `I` reads `Q`. Refs of `Memory`,
  `OvertempShutdown` and other block outputs wire `AI` only. A module
  writes both (`AI: x.AQ, I: x.Q`). Evidence: house config, all 143
  `InputRef`s of sensors and virtual inputs (10 types, `LoxAIRsensor` to
  `VirtualIn`).
- **A save draws a ref's missing feed wires.** A ref with no feed wire
  gets both: `AI` from the target's connector at index 0, `I` from index
  1, the corpus pattern on every device type. A ref of a `LoxAIRAsensor`
  with only `AI` wired gains `I ← Q`; one of a `DigitalIn` gains
  `I ← Qe`. Stating the wire converges. Source: oracle sessions 11, 15,
  33.
- **A save does not draw an `OutputRef`'s distribution wire** into its
  actor: a dangling mirror stays dangling. The corpus wires all 154
  `OutputRef.AQ` ports into their actors explicitly. Source: oracle
  session 11.
- **A ref's `Title=` is derived from its target.** Loxone Config
  rewrites it to the mirrored object's title on every save, whatever it
  held: minted refs and the GUI's own alike, visible or not, stale ones
  included.
  - The exception are the inputs an alarm block (`SmokeAlarm`, `Alarm`)
    collected into its sensor list: `InputRef`s with `WF` bit 0x1000. The
    GUI keeps their titles through a rename of the target or of its
    device. In the house config 46 of 189 InputRefs are such inputs, all
    wired into an alarm block; 33 are titled "<input> <device>", the rest
    a plain copy.
  - The `OutputRef`s those blocks use also carry 0x1000 (54 in the house)
    and follow their target like any other ref.
  - `WF` is the GUI's own bookkeeping: a hand-flipped 0x1000/0x0800 is
    restored on save. Adding or removing `Px`/`Py` changes neither bit
    nor title.

  Lox Logic keeps refs in step with title claims ([D43](decisions/0043-title-claim-carries-refs.md)). Source: oracle
  sessions 11 and 12.
- **API distribution wires are recorded in reverse.** For
  `AutoJalousie.OutputAPI → ApiActor`-style links the `<In>` entry sits
  on the *block's output* connector and names the `OutputRef.AI` (or
  `ApiActor.I`) as its `Input`. Source and sink swap roles relative to
  every ordinary wire. A link drawn through a mirror is always three
  wires:
  - the forward `OutputRef.AI ← block.OutputAPI`;
  - the reverse `block.OutputAPI ← OutputRef.AI`;
  - the mirror's distribution `ApiActor.I ← OutputRef.AQ`.

  Every such mirror's `Ref=` is an `ApiActor`. Every reverse wire sits on
  a connector keyed `OutputAPI` (12 block types: `AutoJalousie`,
  `AcControl`, `NfcCodeTouch`, `TpfController`, …); six more sit on
  connectors keyed `API`. `FLG="1"` marks all 16 house reverse wires and
  none of the 45 public ones, so the flag is not part of the link. Lox
  Logic writes the reverse wire as an argument naming the mirror's `AI`
  ([language.md](language.md), *Block declaration*) and mints it without
  `FLG=`. Evidence: 61 links, 45 across 24 public corpus configs and 16
  in the house config (2026-10-29).

## Push notifications: `Notification`, `OutputRefLM`, `<LoggerMailer>`

```xml
<!-- Messages tree, right after the Loxone Cloud Mailer -->
<C Type="Notification" V="178" U="…" Title="Frostwarnung Garten" WF="16384" Entries="10">
	<IoData Cr="<category>"/>
</C>

<!-- on a page -->
<C Type="OutputRefLM" V="178" U="…" Title="Frostwarnung Garten" Px=… Py=… Px2=… Py2=…
   Cl="0,0,0" Nio="2" Ref="<Notification>">
	<Co K="AI" U="…"/>
	<Co K="AQ" U="…"/>
	<LoggerMailer RefNotification="<Notification>" On="text" Off="text" MinimumTime="5000" NL="1"/>
</C>
```

- **The notification is periphery.** A new one goes at the end of the
  *Messages* tree (after the Cloud Mailer) and counts in `NumO=`; its
  UUID is minted like any GUI object. Placing it on a page sets
  `WF="16400"` on it (in the probe also `Cl="238,238,238"`).
- **The page object is an `OutputRefLM`**, a ref (`Ref=` = the
  notification) with an `AI` input and an `AQ`. Freshly placed it has
  `Cl="0,0,0"` and no `WF=` and no `LinkRefType=`; a later save may add
  `WF="147456"` (see *What a save changes*). Its title follows the
  notification's.
- **The message is a `<LoggerMailer>` child** after the connectors:
  - `RefNotification=` repeats the notification;
  - `On=`/`Off=` are the texts sent when `AI` turns on and off;
  - `MinimumTime=` is the least time between two messages in
    milliseconds (GUI default `5000`, also 53 times in the corpus);
  - `NL="1"` always goes with `RefNotification=`.

  An empty `Off` text is not written. The GUI's defaults for `On`/`Off` are `<v>`.
  Inputs and outputs that log, mail or notify carry the same child with
  other `Ref…=` attributes (`RefLogger`, `RefMailer`, …); Lox Logic compares
  all of it (`message <attr>` in the diff). A `Def="0"` is not written on
  a comparator input; Config leaves the input open.
- **Recipients are `ACT=` rights** on the notification: ticking a user
  in Config 17.2 appends `<PRM REF="<Notification>" ACT="3"/>` to the
  user's `<PH>` list and raises `NUM=` — the same edit as an access
  right (see *Users, groups and access rights*). Public corpus configs
  hold `ACT="1"` on notifications (5 entries in 4 configs), two of them
  together with a `TIM=`. Members of the *Full access* group show as
  inherited recipients that cannot be unticked, and get no `<PRM>`.
- **`ACT=` alone delivers.** `permissions.bin` does not list
  notifications, and neither does `LoxAPP3.json`. A recipient by `ACT=`
  alone got the push once their phone accepted it (below). Source: live,
  2026-10-31.
- **Registered phones** are `<C Type="PuDe" Title="Registriertes Gerät">`
  in a `LoxCaption` with `CaptionType="28"`/`SubType="28"` ("Push-
  Benachrichtigungen: Registrierte Geräte"); the Miniserver writes them
  (UUID tail = its serial). Attributes: `pTkn=` (push token), `PCfg=`
  (the phone's push settings, a signed 32-bit mask), `Usr=` (the user),
  `DevT=`/`DevOs=` (model, OS). **`PCfg` bits 0 and 1 gate program
  notifications** (live, 2026-10-31, one phone): with `-388`
  (`0x…FE7C`) a declared recipient got nothing while full-access users
  got the push; after the user changed the app's notification settings,
  `-385` (`0x…FE7F`), it arrived. Observed elsewhere: `-129`, `-130`.
  The other bits are not known.
- A phone's settings change is a custom change: `/sys/custom/<PuDe>.LoxCC`,
  and one for the caption holding just that phone, each with `WF=` the
  program's plus `0x1000000`. `lox config push` folds it into the upload
  (#242).

Evidence: the house config, the 133 corpus configs (113 with at least one
`Notification`), and oracle session 15 (Config 17.2.8: a notification
created in *Messages*, placed on a page, a recipient ticked, saved).

## Automatic Designer rules: `<C Type="AutopilotRule" APC=…>`

The app's *Automatic Designer* (German "Automatik Designer"; the feature
and block family keep the older "Autopilot" name) stores each rule as an
`AutopilotRule` block in the config. Rules are created in the app or web
UI. The **Miniserver** writes them (UUID tail = its serial), and Loxone
Config displays them. The official KB warns to *load from Miniserver*
before saving, so app-created rules are not lost. There is **no public
spec**: the Structure File and Communicating-with-the-Miniserver PDFs
(v17.0) mention the feature only in passing. The rule schema below comes
from 10 rules: 5 in the house config, 5 across three public use-case
configs (V172–V175).

- One **`AutoPilot` container** exists per config (present even with
  zero rules, 100+ corpus configs): locale-volatile title
  ("Automatik-Regeln"/"Automatic rules"/…), `Nio="2"` (`Input`,
  `output`), `SpStates=`, sitting among the system objects. Each rule's
  `ref=` attribute names this container's UUID. Its `Input` lists, as
  `<In Input=… FLG="1"/>`, every source a rule watches:
  - a weekday condition: the `DayOfWeek` block's `Q`;
  - a state condition: the state UUID;
  - a mode condition: the mode's `Q`;
  - a weather condition: the `AQ` of the `WeatherData` block of that
    field (`WType="1"` for temperature);
  - a day-of-month condition: the `Day` block's `Q`;
  - a daylight condition: the `Daylight` block's `Q`;
  - a time or date condition: nothing.

  A save writes them in reverse document order of the rules' conditions,
  each once, before the inputs already there. Older corpus files mark them
  `Hidden="true"`. One corpus config (V254, two rules on states and a
  mode) has none at all; why is not known. Lox Logic leaves such a list
  alone until a compile changes the rules ([D73](decisions/0073-rule-conditions.md), [D76](decisions/0076-weather-date-message-rules.md)). Source: oracle
  sessions 31 and 32.
- **Rule element**: `Nio="7"` with connectors in index order
  `InputTrigger, Reset, On, run, Q, A, OutputAPI` (matching the app's
  documented Tr/R/O inputs and status outputs), an `<IoData>` child,
  and the rule payload in `APC=`. Rules carry `Px/Py` like page blocks;
  observed both on a dedicated special page (`<C Type="Page"
  SpNo="1">`, house config) and on ordinary logic pages (use-case
  corpus). `STA="1"` was present on exactly the rules whose payload
  says `"active": true` (10/10) — observation, not verified semantics.
- **`APC=` payload**: one JSON object (entity-escaped), schema observed
  across all 10 rules:

  ```json
  {"type": 1, "id": "<rule object UUID>",
   "active": true, "activeIfValid": false,
   "name": "<display name, = Title>", "description": "",
   "operator": 3, "invalid": false,
   "eventGroups": [{"operator": 1, "events": [
     {"type": 5, "descID": 272001, "id": 272, "value": 540,
      "operator": 1, "forceEdge": false}]}],
   "actions": [{"type": 0, "descID": 342101,
     "id": "<target control object UUID>", "command": "on"}]}
  ```

  Validated: an **action's `id` and a control event's `controlUUID`
  resolve to `<C>` objects in the same document** (every occurrence);
  `command` is verbatim app command text (`on`, `off`, `pulse`,
  `setMode/4`, …), with `arrValues` duplicating the command's numeric
  argument where one exists.
- **The codes, from the Miniserver** ([D73](decisions/0073-rule-conditions.md)): the app builds
  rules from a capability table the Miniserver serves,
  `jdev/sps/io/<AutoPilot container>/capabilities/getraw` (JSON,
  `version` `20260521DEU` on firmware 17.2; `capabilities/version`
  alone). One entry per app control type (`controlTypes`, e.g.
  `["Switch"]`, `["Pushbutton"]`), with `events` (`descID`, `type`
  digital/analog/text, `sourceState`, a digital one's `sourceValue`)
  and `actions` (`descID`, `action` = the command with `<val0>`…
  placeholders, value kinds). The web app's `AutomaticDesignerEnums`
  name the rest: event `type` 0 control, 1 operating mode, 2 weather,
  3 weekday, 4 date, 5 time of day; action `type` 0 control, 1
  operating mode, 2 cloud mailer, 3 caller, 4 notification, 5 pause;
  `operator` 0 bool, 1 `==`, 2 `!=`, 3 `>`, 4 `<`, 5 `>=`, 6 `<=`, 7–12
  text comparisons. A rule's `operator` is 3, each event group's 1:
  the events of a group must all hold, the groups are alternatives (the
  editor's sections). A control condition is `{"type": 0, "descID",
  "id": <UUID of the state named by sourceState>, "controlUUID":
  <the control's uuidAction>, "value": <sourceValue or the user's
  number>, "operator", "forceEdge": false}`; an operating-mode
  condition `{"type": 1, "descID": 21001, "id": <the mode's number>,
  "value": 1|0, "operator": 1, "forceEdge": false}` (the number is the
  `Mode=` attribute: 0 … 11 built in, negatives user modes); a mode
  action `{"type": 1, "descID": 21101, "id": <number>, "command":
  "1"|"0", "arrValues": [1|0]}`.
- **Weather, clock, date and message shapes** ([D76](decisions/0076-weather-date-message-rules.md)):
  - A weather condition is `{"type": 2, "descID": 37001…37010, "id":
    <weatherFieldTypes id>, "value", "operator"}`. The ids: 1
    temperature, 2 dewpoint, 4 wind, 5 wind direction, 6 gusts, 7
    irradiance, 8 relative irradiance, 9 precipitation, 10 weather type,
    11 pressure, 26 felt temperature, 27 solar irradiance, 34 sunshine
    (digital).
  - Clock conditions are `type` 3 with `id` = `descID / 1000`: 269001 day
    of month (analog), 281001 daylight (digital), and the moments 294001
    sunrise, 295001 sunset, 298001 dawn, 299001 dusk (`value` 1).
  - A date is `{"type": 4, "descID": 233001, "id": 233, "value": <Unix
    time of the local midnight>}`.
  - Actions carry `arrValues` and the command the table's `action`
    template gives: 2 mailer (92001, `[to, subject, text]`), 3 caller
    (93001), 4 notification (96001, `[subject, text]`), 5 pause (341801,
    milliseconds, `command` the number), 6 speech output (223101), 7
    message center (98101, `autopilotMessage/<severity>/<subject>/<text>`,
    `arrValues` `[severity, subject, text, 2 everyone | 4 admins]`).

  Source: the capability table's `sections` and the web app's
  `_applyEventToRule`/`_applyActionToRule`.
- The raw payload text is **not writer-stable**: two of the house
  rules carry a raw CRLF *inside* the JSON (between tokens — the
  parsed value is unaffected), the other eight are single-line. Any
  comparison of `APC=` must therefore be structural, not textual.
- **Lox Logic's stance.** A rule the source does not declare is carried
  verbatim, compared by `loxl diff`/`drift` (`APC=` as parsed JSON, per
  the point above), and shown as comment lines in the decompile full view
  ([D37](decisions/0037-automatic-designer-read-only.md)). A `rule` declaration edits rules of the documented shapes: time,
  weekday, control states, operating modes, weather, dates, OR groups,
  and any action of the capability table ([D57](decisions/0057-automatic-designer-rules.md), [D73](decisions/0073-rule-conditions.md), [D76](decisions/0076-weather-date-message-rules.md)). A named one
  creates a rule the config lacks ([D60](decisions/0060-creating-rules.md), below). `removed <slug>` deletes
  a rule the lock pins ([D67](decisions/0067-deleting-rules.md)).
- **Activation is two flags** (live, 2026-10-30): the engine runs a
  rule only with `"active": true` *and* `"activeIfValid": true`. Rules
  with `active` alone (the house's for three months, and compiled
  ones) never ran; the app's activation set both and the rule ran at
  its time. The app saves an edited rule as a custom change, not into
  the program: `/sys/custom/<rule-uuid>.LoxCC`, a one-element LoxCC
  (`<C Type="AutopilotRule" V="178" …>` with a `<TPC UL="" GL="…"/>`
  child) — the raw CRLF inside the JSON comes from this writer. Runs
  are logged to `/log/<AutoPilot-container-uuid>.txt` and the
  container's `history` state (`YYYY-MM-DD HH:MM:SS <uuid> <name>`).
- **A rule deleted in the app** stays a custom change under the same
  name: the whole rule element, `STA` and payload as before, without the
  `<TPC>` child, and `WF="33570816"` (the program's `16384` plus
  `0x2000000`). The program file still has the rule, so the next upload
  brings it back; loading an upload removes the custom change. Source:
  live, 2026-10-31, one sample.
- **Edits, as the app makes them.** Deactivating a rule in the app
  changes exactly `"active": true → false` and removes `STA`. The app's
  key order is `type, descID, id, controlUUID, active, activeIfValid,
  name, description, value, operator, invalid, forceEdge, command,
  arrValues, eventGroups, events, actions`, compact. A compiled payload
  in that order, with `STA="1"` right after `APC=`, was accepted by the
  Miniserver and came back byte-identical in the verifying download.
  Weekday indices count from Monday = 0 (`arrValues` `[3, 4]` =
  Thursday, Friday). Source: house config history and a live push,
  2026-10-29.
- **A rule created outside the app** survives a Loxone Config open+save
  byte-identically, with an empty semantic diff. The save drops `WF=` from
  **every** rule, the Miniserver-written ones too, and keeps everything
  else, the raw CRLFs inside the JSON included. The created rule is
  shaped like the house's rules ([D60](decisions/0060-creating-rules.md)):
  - `V`, `Cl`, `Nio="7"`, `WF` and `ref=` copied from an existing rule;
  - the seven connectors minted right before the rule: port UUIDs are the
    rule's counter − 7 + index, with the index byte in segment 4, as 5/5
    house rules show;
  - `<IoData></IoData>`;
  - placed in the column below the last rule (the house's gap: 168
    between `Py2` and the next `Py`, four of five);
  - a payload in the app's key order, `id` = its UUID, `name` = `Title`.

  Source: oracle session 24.

## Touch Pure Flex tiles: `<btns>` on a `TreeDevice`

A Touch Pure Flex is a Tree device (`<C Type="TreeDevice">` with
`SubType`/`TreeType="32799"`, a `cfgId=`, an `Artn=` article number and
an `img=` PNG of the face as a hex blob). Its tiles are not `<C>`
objects: they sit in a `<btns>` child next to the device's API
connector:

```xml
<C Type="TreeDevice" … TreeType="32799" cfgId="…" Artn="…" img="89504E47…">
	<C Type="ApiActor" … Title="API Connector">
		<Co K="I" U="…"/>
	</C>
	<btns>
		<btn i="2" x="6" y="7" h="13" w="15">
			<f1></f1>
			<f2 bT="1"/>
			<f3 bT="2"/>
		</btn>
		<btn ci="1" i="3" x="37" y="7" h="13" w="15">
			<f1 api="SET(As;Tg;pulse)" cT="=ECHO(As)" ctrls="…,…"/>
			<f2 api="SET(As;Co;pulse)" cT="=ECHO(As)" bT="1" ctrls="…"/>
			<f3 api="SET(As;Cc;pulse)" cT="=ECHO(As)" bT="2" ctrls="…"/>
		</btn>
	</btns>
</C>
```

- **`<btn>`** — one tile. `i` is its index (Lox Logic addresses tiles by it);
  `x`/`y`/`h`/`w` are its place and size on the face, and `ci` a
  running number the first tile lacks. The layout is Loxone Config's.
- **`f1`, `f2`, `f3`** — the tile's three functions, always all three,
  in this order. `f1` is the click; `f2` carries `bT="1"` and `f3`
  `bT="2"`.
- **Attributes, in this order:** `api` (the command), `cT` (the status
  text), `bT` (on `f2`/`f3`), `ctrls` (comma-separated UUIDs of the
  blocks the command addresses). No function has `cT` without `api`,
  and no `ctrls` is empty.
- **An empty function** is `<f1></f1>` for the click (3 of 16 tiles) and
  `<f2 bT="1"/>` / `<f3 bT="2"/>` for the arrows (13 of 16).
- **Commands** are the app's command language: `NAME(arg;…)` calls
  chained with `&` (`SET(Lico;M+;pulse)&WAIT(500)&MENU(Lico;Mood)`),
  escaped with `&amp;`/`&quot;`; a status text is an expression such as
  `=ECHO(As)` or `=WAIT(500) & GETOUTPUT(Sw;O;1:On;0:Off)`. Unicode
  and trailing spaces occur. The first argument of a multi-argument call
  is a block-type abbreviation: in the house config `As` addresses
  `AutoJalousie` blocks and `Lico` a `LightController2`.
- **Arrows are up and down:** `f2` is the arrow up, `f3` the arrow down.
  The shading tile of the no-cleaning-room config sends
  `SET(As;Co;pulse)` (complete open) on `bT="1"`, `SET(As;Cc;pulse)`
  (complete close) on `bT="2"`, and `SET(As;Tg;pulse)` on a click. The
  fan tile of the fan-control config opens a menu on click and puts the
  same `SET(Sts;+;Pulse)` step on both arrows.
- **`ctrls`** narrows a command to those blocks. Without it a command
  reaches every block of its type that is API-linked to the device —
  the house config showed shade tiles without `ctrls` moving every
  linked shade. The corpus's `ctrls` UUIDs point at blocks the published
  configs no longer contain.
- **API link**: a block is linked to the device when its `OutputAPI`
  is wired to the device's `ApiActor.I`, directly or through an
  `OutputRef` mirroring the actor (*API distribution wires are recorded
  in reverse*, above). Two corpus Flex devices have a linked block, both
  through an `OutputRef`: a `PushButton` (no-cleaning-room) and a
  `StepSel` (fan control — the block behind the fan tile's `Sts`).
- `TPFC="true"` on the device occurs only in the house config; the
  corpus devices lack it.

Lox Logic rewrites the functions of existing tiles and carries the rest of
`<btns>` byte for byte ([D41](decisions/0041-touch-pure-flex-tiles.md)).

Evidence: three public corpus configs with 16 tiles in all
(`gh_eisber_lox-cli_no-cleaning-room-email`, 9;
`…_UseCase-AC-TouchPureFlex`, 6; `…_Fan_Control_-_Presence_and_Temperature_Support`,
1), plus the house config.

## Rooms and categories: `<IoData Cr=… Pr=…>`

Rooms are `<C Type="Place">` elements under the `PlaceCaption` folder,
categories `<C Type="Category">` under `CategoryCaption` — plain
objects with user-given titles. A block's room/category assignment
lives in its `<IoData>` child: `Pr=` holds the Place UUID, `Cr=` the
Category UUID. A resolver must treat an unresolvable `Pr`/`Cr` as "no
room", not an error: template and fragment exports hold dangling ones.
Evidence: ~36 900 `Cr`/`Pr` occurrences across the house config and all
132 web configs sit on `IoData` and resolve to a Place/Category element.
The only exceptions are one hand-written non-Config file carrying them on
`<C>`, and the exports.

On managed blocks `IoData` is GUI-owned residue, carried forward unchanged
([D19](decisions/0019-gui-residue-carried-forward.md)), unless the source sets the room or category ([D63](decisions/0063-managed-room-category.md)). On externs Lox
Logic reads it for composite matching (`room:` / `category:`). It writes it
only for an attribute claim (`x.room = "…"` / `x.category = "…"`, [D39](decisions/0039-attribute-claims.md)),
which rewrites an existing `Pr=`/`Cr=` value in place and never adds the
attribute or the element.

Devices nest their inputs and outputs as child `<C>` elements, each with
its own `<IoData Pr=…>`. In the house config 673 of 687 device children
carry their device's room; the rest were moved individually. Loxone
Config does not cascade a device's room change to its children (its UI
strings have no such prompt) — the follower rule of [D39](decisions/0039-attribute-claims.md) is Lox Logic's own.

## Virtual and Modbus devices

Virtual inputs, virtual and Modbus devices are not page blocks. They live in fixed folders and have no
`Px=`/`Py=`; pages reach them by wires or refs.

| Object | Folder or parent | `IName=` prefix |
|---|---|---|
| `VirtualIn` | `VirtualInCaption` ("Virtual Inputs") | `VI` |
| `VirtualHttpIn` | `VirtualInCaption` | `VHI` |
| `VirtualUdpIn` | `VirtualInCaption` | `VUI` |
| `VirtualTextIn` | `VirtualInCaption` | `VTI` |
| `VirtualHttpInCmd` | its `VirtualHttpIn` | `VCI` (shared with `VirtualUdpInCmd`) |
| `VirtualUdpInCmd` | its `VirtualUdpIn` | `VCI` |
| `VirtualOut` | `VirtualOutCaption` | `VQ` |
| `VirtualOutCmd` | its `VirtualOut` | `VQC` |
| `ModbusServer` | `WeatherCaption` (shown as "Netzwerkperipherie") | none |
| `ModbusDev` | its `ModbusServer` | none |
| `ModbusASensor`, `ModbusAActor` | their `ModbusDev` | `AMQ` (shared) |
| `ModbusSensor` | its `ModbusDev` | `IM` |

- `IName=` is the designation the GUI shows. It is numbered per prefix, past the highest in use.
- A device's I/O may be grouped in captions inside the device.
- Device and I/O settings (`Address`, `Poll`, `Check`, `ModbusAddress`, …) are attributes.
- A `VirtualIn` has the connectors `[Q, Qm]`, both sources, and the attribute order `Type, IName, V, U, Title,
  Cl, Nio, WF, …`. A fresh one is `Cl="238,238,238"`, `Analog="false"`, with the children and defaults listed
  in *What a first save adds*.
- The ticked boxes `Signed=`, `Analog=` (on `VirtualHttpInCmd`, `VirtualUdpInCmd`, `VirtualOutCmd`) and
  `CloseAfterSend=` (on `VirtualOut`) are `"true"` or absent. An unticked box has no attribute, and a save
  drops an explicit `"false"`. A `VirtualIn`'s `Analog=` is always there: a digital one is
  `Analog="false"`, and a save adds it where it is missing. Corpus (131 `.Loxone` files, `ControlList
  Version` 74 to 267): all 167 `VirtualIn`s have `Analog=`, 53 of them `"false"`, in every version. Two
  saves of the house config made before Lox Logic minted anything (Version 273) have 16 digital ones, all
  `Analog="false"`. The commands and outputs have 1,501 such attribute slots: none is `"false"`, 505 are
  absent. Oracle session 37 checked both directions. A fresh object from Lox Logic ticks `Signed` and
  `Analog` on the HTTP and UDP commands and `CloseAfterSend`, so adopt writes `off` for a missing one
  ([D113](decisions/0113-adopt-writes-off.md)).
- A save writes no numeric setting at 0, see [*Numbers at 0*](#numbers-at-0).

Lox Logic creates these objects ([D54](decisions/0054-virtual-inputs.md),
[D64](decisions/0064-virtual-modbus-devices.md)). Source: corpus (175 `VirtualIn`s); oracle sessions 18, 26
and 37.

### Numbers at 0

A save writes a numeric setting only when it is not 0: an absent attribute is 0, and a save drops an
explicit `"0"`. This holds for these settings, on every type that has them:

- range and default: `MinVal`, `MaxVal`, `DefVal`
- analog correction: `SourceValLow`, `DestValLow`, `SourceValHigh`, `DestValHigh`
- timing: `MinChange`, `MinTime`, `MaxTime`, `ResetTime`, `AverageTime`, `Repeat`, `RepeatRate`
- `CmdOnMethod`, `Allowedtimeouts`, `Port`, `Channel`, `DimmType`
- Modbus: `ModbusAddress`, `ModbusDataType`, `ModbusCmd`, `ModbusPollingCycle`, `ModbusCoilQuantity`
- calendar entries: `CalType`, `Mode`, `From`, `To`, `EasterOffset`, `WeekDayInMonth`

Exceptions:

- A Modbus server keeps `Timeout="0"`.
- A virtual HTTP input raises `Poll="0"` and `Timeout="0"` to `"10"`, see [*Values a save
  rewrites*](#values-a-save-rewrites).
- A digital `VirtualIn` turns `MaxVal="0"` into `"1"`.

Elsewhere `"0"` is a value like any other: real configs hold `Typ="0"` and `Sec="0"` on `PulseAt`, `Tp="0"`
on `Memory`.

Lox Logic writes a bound 0 as no attribute ([D112](decisions/0112-zero-is-no-attribute.md)). Source: corpus
(131 files: none of these settings holds `"0"` on `VirtualIn`, the virtual commands, the Modbus objects,
calendar entries or the physical inputs and outputs; `MinVal` is absent on 112 of 171 `VirtualIn`s,
`ModbusDataType` on 149 of 339 Modbus sensors); oracle sessions 37, 61 and 62 (Loxone Config 17.2.8.28).

### Values a save rewrites

A save puts some settings into a fixed form:

- A virtual HTTP input's `Poll`, `Timeout` and `Allowedtimeouts` are 32-bit whole numbers. A fraction
  is cut off (`10.6` and `10.5` become `10`, `600.5` becomes `600`), and a larger number wraps
  around: `99999999999` becomes `1215752191`, and `2147483648` (negative) and `4294967296` (0) end
  as `Timeout="10"`.
- Then `Poll` goes up to 10 and down to 604800 (one week): `0`, `5`, `9`, `-5` become `10`, `604801`
  and `999999999` become `604800`.
- `Timeout` goes up to 10, and is kept above the documented 8000: `60000` and `2147483647` stay.
- `Allowedtimeouts` keeps any whole number, `-1` and `1000` included.
- A digital `VirtualIn` has `MaxVal="1"` and no `MinVal` (0), whatever it was: `0`, `0.5`, `2`, `5`
  and `-1` become `MaxVal="1"`, a `MinVal` of `1`, `-1` or `2` goes. Its `DefVal` stays as written,
  also outside 0 to 1 (`2`, `-1`, `0.5`). Thirteen corpus files of `ControlList Version` 74 and 75
  hold digital ones with `MinVal="-10000"` and `MaxVal="10000"`; none of a later version does.
- An analog `VirtualIn` keeps its range and default as written, `MaxVal` below `MinVal` and a
  `DefVal` outside the range included.

Lox Logic refuses a value a save would rewrite, and names the value the save writes
([D114](decisions/0114-refuse-what-a-save-rewrites.md)). Source: oracle sessions 61 to 64 (Loxone
Config 17.2.8.28); corpus.

### Units of the settings

The unit a setting's attribute is in, where a Loxone knowledge-base page states it. The table is
`src/settings_meta.rs`; value suffixes scale into these units.

| Object | Attribute | Property | Unit | Range | Source |
|---|---|---|---|---|---|
| `VirtualHttpIn` | `Poll` | Polling cycle | s | 10…604800 | `kb-virtual-http-input`, *Properties* |
| `VirtualHttpIn` | `Timeout` | Timeout | ms | 10…8000 | `kb-virtual-http-input`, *Properties* |
| `VirtualHttpIn` | `Allowedtimeouts` | Number of permitted timeouts | none | 0…100 | `kb-virtual-http-input`, *Properties* |

The attributes match the properties by name. The corpus agrees: all 19 `VirtualHttpIn`s have
`Timeout="4000"` (the mint default), and every `Poll` is 10 or more (10 to 3600).

No Loxone page states the unit of these settings, so they stay bare numbers:

- `ModbusServer` `Timeout`. Reference `kb-modbus-tcp` (German; there is no English page) names the
  server's timeout and the sensors' polling cycle, without units. The Modbus Air and Modbus
  Extension pages document only RS-485 timing ("Timeout: 1000ms" in Auto mode), not these
  attributes.
- `ModbusASensor` and `ModbusSensor` `ModbusPollingCycle`, `MinTime`, `MaxTime`.
- `VirtualUdpInCmd` `MinTime`.

## Settings of physical inputs and outputs

Inputs and outputs of extensions, Tree and Air devices and 1-Wire sensors carry their settings as attributes:

| Setting | Attributes |
|---|---|
| Description (the app shows it as the control's name) | `Desc` |
| Analog correction | `SourceValLow`, `SourceValHigh`, `DestValLow`, `DestValHigh` |
| Range | `MinVal`, `MaxVal` |
| Transmission timing | `MinChange`, `MinTime`, `ResetTime`, `AverageTime`, `MaxTime` |
| Display unit | `Unit=` on the object's `<Display>` child |

- A save omits `Desc`, the low ends of the correction, `MinVal`, `ResetTime` and `AverageTime` at their
  defaults. It writes `Desc` right after `Type`, the others in a fixed order.
- A save drops every numeric setting at 0, the high ends, `MaxVal`, `MinChange` and `MinTime` included
  ([*Numbers at 0*](#numbers-at-0)).
- Digital inputs have no analog correction.
- With a unit set, the Miniserver formats the value (`8.2 °C`), and the app catalogue's format becomes
  `%.1f °C`.

Lox Logic writes these settings ([D65](decisions/0065-physical-io-settings.md)). Source: oracle session 27 (a
save kept description, unit, low end, averaging and time on two Air sensors); live check on the house
Miniserver.

### Statistics: `StatsType=`

An input or output with statistics carries `StatsType=`, often with
`StatsAutoDel="1"`; without statistics it has neither. The value is the
recording kind. Every hardware I/O with statistics in the corpus and the
house config uses one per type:

| Type | `StatsType` |
|---|---|
| `TreeSensor`, `TreeActor`, `LoxAIRsensor`, `LoxAIRactor`, `DigitalIn` | `7` |
| `TreeAsensor`, `LoxAIRAsensor` | `11` |
| `Lox1wireAsensor` | `10` or `11` |
| `VoltageIn` | `6` |

The structure file's statistic `frequency` gives the meaning: 1 is every
change, 2 to 6 averages (deprecated), 8 to 11 intervals. The app
catalogue maps `7` to frequency 1 and `11` to frequency 11. A save keeps
`StatsType` added or removed, with or without `StatsAutoDel` (oracle
session 51). Lox Logic writes `7` and `11` only
([D101](decisions/0101-hardware-declarations.md)).

## Hardware identity: `Serial=`

Hardware elements carry their serial number in `Serial=`, written as
upper-case hex digits (four corpus Miniservers end in a placeholder `X`
instead):

- 12 digits on the Miniserver (`LoxLIVE`). The same serial forms the
  tail of the UUIDs it mints (see *UUID anatomy*).
- 8 digits on everything else: Tree Extensions (`LoxTree`) and Tree
  devices (`TreeDevice`), the Air base (`LoxAIR`), Extensions (`LoxMORE`,
  `LoxDIMM`, `LoxAout`, `LoxOCEAN`), and the bus interfaces (`Comm485`,
  `Comm1wire`, `CommDMX`, `CommIR`, `EIBline`).
- An empty `Serial=` occurs (`LoxDMXdevice`) and means none. A Tree
  device added offline in Loxone Config has `Serial="00000000"`.

They nest the way the hardware is cabled: a Tree device inside its Tree
Extension inside the Miniserver, each with its own serial, the I/O
objects as children of the innermost.

Air devices (`LoxAIRDevice`) have no `Serial=`. A 32-digit hex `IP=`
identifies them: the link-local IPv6 address formed from the device's MAC
(EUI-64), and that MAC is its serial. `/data/status` on the Miniserver
lists `IP="FE80000000000000524F94FFFEBA1F0E"` as
`Serial="50:4F:94:FF:FE:BA:1F:0E"`: the universal/local bit of the first
byte flipped back, `52` → `50` (house, 38 of 38). Lox Logic reads
`Serial=`, and an Air device's serial from its `IP=`, for `device:`
extern matching ([D55](decisions/0055-device-narrowed-matching.md), [D71](decisions/0071-multi-object-statements.md)); `loxl scan` reports both as personal data.

A device's own attributes: an Air device always carries `Installation`,
`SwitchBoard`, `SwitchBoardRow`, `SwitchBoardPos` (103 of 103 in the
corpus); a Tree device `Installation`, the board only where it is
mounted in one. A save drops a board written onto a room device (oracle
session 30). Every Air device in the corpus has
`PWMFrequency="123"`, `UseDimmer="true"` and (99 of 103)
`UseRGBDimmer="true"`. A save writes `TId=` (`NanoIO`,
`RoomsensorTree`) right after a `Desc` it finds on a device.

The Tree and Air extensions (`LoxTree`, `LoxAIR`) carry `Installation`
and `SwitchBoard` too. A save keeps both rewritten, a board also from
`""` (oracle session 51).

## Time functions: per-project singletons

The GUI's *Zeitfunktionen* periphery folder is a `TimeCaption` element
holding one instance each of the time-source types: `Day2009`, `Year`,
`Month`, `Day`, `DayOfWeek`, `Calendar`, `Time` (minutes since
midnight), `Hour`, `Minute`, `Second`, `SecondsBoot`, `DateTime`,
`NightTime`, the impulse family (`ImpulseSecond` … `ImpulseYear`,
`ImpulseSunrise`, `ImpulseSunset`, `ImpulseMorningtwilight`,
`ImpulseEveningtwilight`), and `StartPulse`. Each type appears exactly
once per project — the type *is* the identity. Unlike `SysVar`,
`WeatherData`, or I/O objects they carry **no `IName=`**, and their
titles are locale-volatile like every built-in's; their UUIDs are
minted at project creation by the Config PC (machine id in segment 4),
so they differ between projects. The operating modes (`Mode` under
`ModeCaption`) share the singleton nature but use deterministic
`00000000-…` system UUIDs, identical across projects.

## Operating modes

The *Betriebsmodi* folder is a `ModeCaption` holding one `Mode` per
operating mode, each with an `I` and a `Q` connector (`Nio="2"`) and
`<Display StateOnly="true"/>`. Observed on the house config (Loxone
Config 17.2) and the default template:

- Twelve built-in modes with fixed UUIDs
  `00000000-0000-000N-1500000000000000`, N = 1 … 12: holiday, vacation,
  day off, Monday … Sunday, heating period, cooling period. Their
  `Mode=` attribute is `N - 1` (absent on the holiday mode: 0).
- User-defined modes have UUIDs minted like any object and negative
  numbers, `Mode="-1"`, `"-2"`, … in creation order.
- A timer's `Modes=` (DayTimer, and the `<Timer>` of other blocks) lists
  its columns in ascending `Mode=` order; `UserModes=` counts the
  columns before Monday, so a heating column does not count (oracle
  session 29).
- A fresh mode saved by Loxone Config: `Nio="2" WF="147456"`,
  `<IoData></IoData>`. Modes made in the GUI earlier carry
  `WF="16384"` and an `<IoData Cr=…>` category.
- Calendar entries (*Betriebszeiten*): see below.

### Calendar entries

The *Betriebszeiten* folder is a `CalendarCaption` holding one
`CalendarEntry` per entry, `Nio="2" WF="16384"`, with an `I` (`-00ff`)
and a `Q` (`-01ff`) connector that none of the corpus's 397 entries
wires. Attributes, in the save's order:

- `CalType` — the `Mode=` number of the operating mode the entry
  switches on; absent for 0, the holiday mode. User modes are negative.
- `Mode` — the kind of date. No other value occurs:
  - absent: a fixed date every year (`Month`, `Day`);
  - `1`: relative to Easter Sunday (`EasterOffset` in days, which may be
    negative);
  - `3`: one period (`From`, `To`: days since 2009-01-01; `2375` is 4
    July 2015);
  - `4`: a yearly range (`Month`, `Day` to `Month2`, `Day2`).
- `Month`, `Day`, `Month2`, `Day2` — always present; `1` where the kind
  does not use them. Some Easter entries carry a date there (the day
  of some year the offset gave); it is carried as written.
- `From`, `To`, `EasterOffset` — only with their kind.
- `WeekDayInMonth` — always `1`.

A save of fresh period, Easter and yearly-range entries changed nothing
(oracle session 31); a fixed-date entry for the day switched its mode on
on the house (firmware 17.2.8.28).

## Users, groups and access rights

The GUI's *Users and Permissions* folder is a `LoxCaption
CaptionType="20"`. It holds the users (`User`, under a `UserCaption`),
the user groups (`UserGroup`, under a `UserGroupCaption`) and, in a
`LoxCaption CaptionType="11"`, the access schedules: `SwitchingTimer`
objects (196 in the corpus, among them the built-in *Always* and
*Never*). Loxone Config, the app and the Miniserver's user management
API all write this tree. Lox Logic reads it (`loxone_config::access`, the users/access
section of `loxl diff`) and writes only access rights on NFC Code
Touches, for an `access` declaration ([D49](decisions/0049-nfc-access-rights.md)), and the recipients of push
notifications, for a `recipients` declaration ([D51](decisions/0051-push-notifications.md)).

```xml
<C Type="User" … Title="alice" UsR="-1" LastEdit="562500000"
   NFCArr="<64 hex>" NFCNArr="Key fob" CodeArr="<32 hex>" iBtnArr="" EAIdArr="">
	<HP H="<hash>" Sc="2" S="<salt>" Alg="1"/>
	<HP H="" Sc="-2" S="" Alg="1"/>
	<PH NUM="2">
		<PRM REF="<Code Touch block>" TIM="<SwitchingTimer>" ACT="3"/>
		<PRM REF="<some block>" ACT="3"/>
	</PH>
</C>
<C Type="UserGroup" … Title="Residents" Rights="129" User="<user>,<user>" UserGroups="" LEd="…" TMR="…">
	<PH NUM="1">…</PH>
</C>
```

- **`LastEdit=`** (126 of 154 users) is a Loxone time, seconds since
  2009-01-01 00:00 local time — the `lastEdit` the user management API
  reports. Groups carry the same as `LEd=`.
- **Credentials are attributes.** `NFCArr=` holds the hashed NFC tag
  IDs (64 hex digits each), `NFCNArr=` their names at the same
  positions; `CodeArr=` the hashed access codes (32 hex digits, 6
  users), `iBtnArr=` iButtons, `EAIdArr=` external access IDs. Several
  tags are joined by `;` — one corpus user has two
  (`gh_eisber_lox-cli_UseCase_Wallbox-Billing-Exosphere-Database-Connector`:
  `NFCNArr="…;…"` and two 64-digit hashes in `NFCArr=`). **Unverified:**
  the separator of several access codes (one config of the old `V="126"`
  generation has a single 96-digit `CodeArr=`, three codes' worth without
  a separator) and what `iBtnArr="0"` (9 users) means. Lox Logic splits on
  `;` and reads a value without one as one entry.
- **Access codes** come from the user dialog. Loxone Config 17.2 writes
  one as 32 upper-case hex digits that are neither the plain MD5 nor the
  SHA-1 of the code; the algorithm is unknown. A user created there gets
  `UsR="16416"`, `UnsP="true"`, `LastEdit=` and the UI right *Eigene
  Kennwörter ändern*. Source: oracle session 33.
- **Passwords are `<HP>` children**, two per user: the first is the
  password, the second the visualization password — the `Sc=` score of
  each matched `scorePWD`/`scoreVisuPWD` of the user management API on
  a live Miniserver (FW 17.x, two users). `H=` is the hash, `S=` its
  salt, `Alg=` the algorithm; an empty `H=` means no password.
  Attribute orders: `H Sc S Alg` (218), `H Sc S` (60), `H S Sc` (16),
  `H S` (10). Old configs carry `Pwd=`/`Pwd2=` attributes instead.
- **Rights are a `<PH NUM=n>` list of `<PRM>`** on users (92 lists) and
  groups (96), never self-closing and never empty. `NUM=` is the number
  of `PRM` children in all 188 lists, and no list names the same `REF=`
  twice.
- `REF=` is the object the right is on. `ACT=` is a UI right (`3` in
  642 entries, `1` in 5); `TIM=` makes the entry an access right
  (*Zutritt*) under the access schedule it names — always a
  `SwitchingTimer`. One entry may carry both. Attribute orders: `REF
  ACT` (547), `REF TIM ACT` (100), `REF TIM` (6). In Config 16 and 17
  `TIM=` sits only on `NfcCodeTouch` blocks (8 entries); Config 12
  configs also carry it on modes, actors, inputs and right groups.
- **Group membership** is the group's `User=` (comma-separated user
  UUIDs; 9 groups with several members) and `UserGroups=` (nested
  groups; always empty in the corpus). `Rights=` and the user's `UsR=`
  are bit masks (`-1` = all). `UgType="3"` marks the *Nobody* group
  (131), `UgType="4"` *Full access* (80); `1` and `2` occur 51 times
  each, meaning unverified.
- **Granting access in Loxone Config** appends `<PRM REF="<Code
  Touch>" TIM="<schedule>"/>` to the user's list and raises `NUM=`.
  Nothing else in the users tree changes, the user's `LastEdit=`
  included. The same grant compiled by Lox Logic onto the config from
  before the save reproduces Config's users, groups and schedules byte for
  byte (Config 17.2, one save, 2026-10-29). **Unverified:** what a revoke
  writes. Lox Logic removes `TIM=`, then a `<PRM>` left with only `REF=`,
  then a list left empty.
- **Full access is no access right.** A user in the full access group
  without a `TIM=` right on a Code Touch is refused there (live
  Miniserver, FW 17.x).
- **Named credentials** of a Code Touch are objects: `Keycode` (access
  codes) and `NFCID` (tags), in a `KeycodeCaption`/`NFCIDCaption` of the
  device or as children of the `NfcCodeTouch` block, with the hashed
  value in `Cde=` and an optional validity window in `StartT=`/`EndT=`.
- **A Code Touch takes two steps in the GUI.** Adding an *NFC Code Touch
  Tree* creates only the device (`TId="NFCCodeTouchTree"`, its keycode
  and NFC-tag captions). The `NfcCodeTouch` block, with `Ref=` to the
  device, category *Zutritt* and `HSize="10"` (*Anzahl Meldungen*),
  appears when the device is dragged onto a page. Source: oracle session
  33.

Lox Logic treats every hash and salt as secret: the diff shows a credential
as a name, a count or an eight-digit fingerprint (a SHA-256 over the
stored value), never the value itself.

Evidence: the 132 public web configs (154 users in 131 configs, 402
groups) and the house config, Config 12 – 17.

## The access table: `permissions.bin`

Loxone Config writes `permissions.bin` into the program ZIP, next to
`sps0.LoxCC` and `LoxAPP3.json`: for each object the app shows, the
users and groups that may see and use it. `loxone_config::permissions` parses it
and generates it from the program and the ZIP's `LoxAPP3.json`. The
generator reproduces all 19 files of the evidence byte for byte
(`LOX_CORPUS=<dir> cargo test corpus_permissions`).

- **Records.** No header; a flat list of 16-byte records. A UUID record
  is the Loxone UUID in binary: the first three fields little-endian
  (u32, u16, u16), the last eight bytes as written. A marker record is
  one byte sixteen times: `FE`, `FC`, `FB`, `FD` or `FF`.
- **Groups.** One per object:
  `object, FE, state…, FC, principal…, [FB, principal…]…, [FD], FF`.
  The states are UUIDs the app reads for the object, the principals
  who may use it.
- **Principals** of an object, in document order: every `User` and
  `UserGroup` with a `<PRM REF=…>` on it, whatever the right (`ACT=`,
  `TIM=` or both), and every `RightGroup` that is the `RGR=` of the
  object's room or category (`<IoData Pr=/Cr=>`). An `FB` section
  follows for the object its `Rec=` attribute names, with that object's
  principals, and so on along the chain. Neither the schedule (`TIM=`
  value) nor the kind of right is in the file: granting a right to
  someone who had none on the object changes it, rescheduling does not.
- **Order and content.** The file follows `LoxAPP3.json`, which the
  program alone does not determine:
  1. Six global groups, each with `FD` and no principals:
     `globalStates.operatingMode`, `sunrise`, `sunset` (no states); the
     program's `TaskCaption` with `pastTasks` and `plannedTasks`;
     `modifications` with eleven document states (`notifications`,
     `modifications`, `favColorSequences`, `favColors`,
     `miniserverTime`, `liveSearch`, `userSettings`, `userSettingsTs`,
     `cloudservice`, `trustVersion`, `propsVersion`); `hasInternet`
     with itself.
  2. The operating modes, in the key order of `modes`, without states.
  3. The controls, in the key order of `controls`. States: the
     control's `states` in key order, arrays flattened; for an
     `InfoOnlyAnalog`/`InfoOnlyDigital` with `StatsType=`, its first
     `<Co>`; for `Alarm` and `SmokeAlarm`, a fixed list of connectors
     (`loxone_config::permissions`); then per subcontrol its `uuidAction` (when
     keyed `U:…`) and its states. Repeats are dropped. `FD` when the
     control has neither `room` nor `cat`.
  4. The weather server (its first state as the object, `FD`); every
     `autopilot` rule (`uuidAction` and states), whose principals are
     the users with `UsR=` bit `0x40`; every `messageCenter` entry
     (`FD`, nobody).
- **A grant, regenerated.** Take the Config 17.2 save that granted a
  user access (*Users, groups and access rights* above). The file
  generated from Lox Logic's compile of that grant and the `LoxAPP3.json`
  from *before* the save equals the `permissions.bin` Config wrote, byte
  for byte. That is the push's situation, which keeps the live
  `LoxAPP3.json`.
- **What `FD` means** is unknown; it is written exactly where listed.
- **The Miniserver stores it as uploaded.** After an upload that
  replaced only `sps0.LoxCC` (2026-10-29), the next download was the
  uploaded ZIP byte for byte, `permissions.bin` and `LoxAPP3.json`
  included. **Unverified:** whether the Miniserver enforces this file
  or derives the rights from the program, so whether a stale file
  matters.

Evidence: 19 program ZIPs of one installation, program versions 243 to
274 (Aug 2024 – Oct 2026), 2.8 to 37.7 KB of `permissions.bin` each:
Loxone Config saves, and ZIPs in which an upload replaced `sps0.LoxCC`
and kept Config's `permissions.bin` and `LoxAPP3.json`.

## The app catalogue: `LoxAPP3.json`

Loxone Config writes `LoxAPP3.json` into the program ZIP when it saves
to the Miniserver: the structure the app reads. The Miniserver serves it
as `/data/LoxApp3.json` exactly as uploaded and reports its
`lastModified` as `/jdev/sps/LoxAPPversion3`; it never rebuilds it
(#155). `loxone_config::catalogue` edits it for a compile ([D61](decisions/0061-app-catalogue.md)). Its
`lastModified` equals the program's `Date=`.

Evidence: three Loxone Config 17.2 saves of one installation
(2026-10-29, 148 controls each) and two pushes to the same Miniserver
(2026-10-30).

- **Text.** UTF-8 with a BOM, one line, compact, no escapes beyond JSON's
  own (`/` and non-ASCII unescaped). The keys of `controls`, `modes`,
  `autopilot` and `messageCenter` are `U:<uuid>`; those of `rooms` and
  `cats` are the bare UUID. The order of `controls` is not document
  order (unexplained).
- **Every control.** `name` is the object's `Desc=` when set, else its
  `Title=`; `room`/`cat` are `<IoData Pr=/Cr=>`; `isFavorite` is
  `UseInFav="true"`. 148 of 148 in each save.
- **Which objects are listed.** Not every `Visu="true"` object: a
  `VirtualIn` no `InputRef` uses (2 in each save), a `PushButton` with
  neither room nor category (1), the weather server, the device monitor,
  a `TreeActor`, the meters an energy flow monitor holds (nested
  `MeterAbsBi`) are not.
- **Entries by block type.** Key order `name, type, uuidAction, room,
  cat, defaultRating, isFavorite, isSecured (false),
  [hasControlNotes], defaultIcon, restrictions (0), details, states,
  [statistic | statisticV2]`. `defaultRating` is `<IoData Rating=>`
  (0 without). A jalousie's and a central block's entry have no
  `defaultIcon`. `hasControlNotes` (`true`) is there when the object has
  notes (`NTXT=`, one object; the documentation `NDOC=` adds nothing).
  `defaultIcon` is `null` without `IconV=`; with one it is a file name
  (`IconsFilled/login-key.svg`) the program does not carry.
  - `PButtonT` → `Pushbutton`, details `{"jLockable":true}`, states
    `jLocked` (the `Reset` connector with the top bit of its first byte
    set: `1de6…` → `9de6…`), `active` (`Q`).
  - `PushButton` → `Switch` when its last `<Display>` has `Type="1"`,
    else `Pushbutton`; details `{"jLockable":true}`, states `jLocked` (as
    above), `active` (`Q`), `lockedOn` (the third `SpStates=` UUID).
  - `VirtualIn` with `Analog="false"` → `InfoOnlyDigital` when its
    `<Display>` has `StateOnly="true"` (details `jLockable` false, texts
    `Aus`/`Ein`, colours `#FF0054`/`#8DFF70`), else `Switch` (details
    `{"jLockable":false,"type":71}`); states `active`: the input's own
    UUID.
  - `Memory`, `DigitalIn`, `LoxAIRsensor`, `LoxAIRactor` with a
    `<Display StateOnly="true">` (no attributes beyond `Type`, `Unit`,
    `StateOnly`) → `InfoOnlyDigital`, details and `active` as the
    virtual input's.
  - `TreeAsensor`, `LoxAIRAsensor`, `Lox1wireAsensor`, `ModbusASensor`,
    `ModbusAActor`, likewise → `InfoOnlyAnalog`, details
    `{"jLockable":false,"format":F}`: the display unit with `<v>` as
    `%.0f`, `<v.N>` as `%.Nf` and `%` doubled (`<v>%` → `%.0f%%`,
    `<v.1> °C` → `%.1f °C`); states `value` (the object's UUID) and
    `error`: the `Qe` connector, or `Q` beside `AQ` (none on the
    actor).
  - `statistic`, after `states`, when the object has `StatsType=`:
    `{"frequency":1,"outputs":[{"id":0,"name":N,"uuid":U,"visuType":1}]}`
    for an `InfoOnlyDigital` with `StatsType="7"`, and
    `{"frequency":11,"outputs":[{"id":0,"name":N,"format":F,"uuid":U,"visuType":0}]}`
    for an `InfoOnlyAnalog` with `StatsType="11"` (`F` with `%` as it
    is); `N` the entry's name, `U` the first `<Co>`. No other
    `StatsType` was seen.
  - `PulseAt` → `PulseAt`, details
    `{"jLockable":true,"pulseDurationLocked":false}`, states `jLocked`
    (the `InputDisable` connector, top bit set), `isActive` (`Q`),
    `pulseDuration` (`Time`), `startTime`, `oneTimePulseDate`, `type`
    (the three `SpStates=` UUIDs in order).
  - `DayTimer` with `On=""`, `Off=""` and no `Analog=` → `Daytimer`,
    details `{"jLockable":true,"analog":false,"text":{"on":"Aktiv","off":"Inaktiv"}}`,
    states `jLocked` (`Reset`, top bit set), `mode` (`AQm`), `override`
    (`AQmt`), `value` (`AQ`), `needsActivation` and `modeList` (the two
    `SpStates=`), `entriesAndDefaultValue` (the object's UUID),
    `resetActive` (`Reset`).
  - `AutoJalousie` → `Jalousie`, details
    `{"jLockable":true,"controlHistory":true,"hasHistory":H,"isAutomatic":true,"animation":0,"type":348}`
    (`H` the `<COHist TRK_NE>`; 16 of 16 alike), states `jLocked`
    (`Stop`, top bit set), `infoText` (`SpStates[0]`), `up`, `down`,
    `position`, `shadePosition`, `safetyActive` (`OutputUp`,
    `OutputDown`, `OutputPos`, `OutputLPos`, `OutputSafety`),
    `autoAllowed` (`EnAutoShade`), `autoActive` (`OutputAutoShade`),
    `locked` (`OutputLock`), then `autoInfoText`, `autoState`,
    `deviceState`, `targetPosition`, `targetPositionLamelle`,
    `adjustingEndPos` (`SpStates[1..7]`).
  - `CentralLight` → `CentralLightController`, `CentralShade` →
    `CentralJalousie`: details
    `{"jLockable":true,"controls":[{"id":i,"uuid":U}…],"parameterConnections":0}`,
    the members in the order of `rec=`; states `jLocked` (`Reset`
    resp. `Stop`, top bit set), `events` (`SpStates[0]`), the shading's
    `safetyActive` (`SpStates[1]`).
  - `MeterDig`, `MeterAbsUni` (`unidirectional`), `MeterAbsBi`
    (`bidirectional`), `MeterAbsSt` (`storage`) → `Meter`, details
    `{"jLockable":false,"type":T,"displayType":0,"actualFormat":F,"totalFormat":"","powerName":P}`
    (`F` from `Un1=` as above, `P` the `PwrN=`; the storage adds
    `storageFormat` from `UnSt=` and `storageMax`, `MaxLvl=` as a
    float); states `jLocked` (`R`, top bit set), `actual` (`OPf`),
    `total` (`OMr`) or `total`/`totalNeg` (`OMr1`/`OMr2`), the storage's
    `storage` (`Slvl`), then the day/week/month/year readings (`ORd`,
    `ORw`, `ORm`, `ORy`; `…1` and `…2` for the bidirectional ones as
    `total…` and `totalNeg…`). `statisticV2` after `states`: one group
    per `<statGrp id m [acc]>` (`{"id":"1","mode":m,[ "accumulated":true,]"dataPoints":[…]}`):
    group 1 the power (title `P`, format `0.000kW` for `<v.3>kW`),
    group 2 the reading(s) (title `Zählerstand`, format `""`), group 3
    the storage level (`Speicherstand`, `0%`). Seen only with
    `StatsType="8"` and no `Un2=`.
  - `LightController2` → `LightControllerV2`, no `defaultIcon`. Details
    `{"jLockable":true,"controlHistory":true,"hasHistory":N,"movementScene":M[,"masterValue":"U/masterValue"][,"masterColor":"U/masterColor"]}`
    (`N` the `<COHist TRK_NE=>`, `M` the place of the `Move="true"` mood
    in `<LightscenesC>`, from 1); states `jLocked` (`Reset`, top bit
    set), `activeMoods`, `moodList`, `activeMoodsNum`, `favoriteMoods`,
    `additionalMoods`, `circuitNames`, `daylightConfig`, `presence`
    (`SpStates[2, 3, 10, 4, 5, 9, 11, 12]`). After the states
    `subControls`: the masters, then a circuit `U/AI<n>` per
    `NameAI<n-1>=` (output `AQ<n>`), each
    `{"name","type","uuidAction","defaultRating":0,"isFavorite":false,"isSecured":false,"restrictions":0[,"details"],"states"}`.
    The circuit's kind is the actuator's capability Loxone Config copies
    onto the block (`CapAI<i>=`, the actuator's `CCap=`): `31` a
    `ColorPickerV2` `Rgb`, `80` one `TunableWhite`, details
    `{"pickerType":K,"TWMin":a,"TWMax":b}` from the `TWMIN=`/`TWMAX=` of
    the actuator an `OutputRef` on the output names (2200/6500 without),
    states `color` (`AQ<n>`), `sequence`/`sequenceColorIdx`
    (`uuidSeqencing[n-1]`/`uuidSeqenceIx[n-1]`); without a capability a
    `Switch` (`active`: `AQ<n>`). With two circuits or more a master:
    `Master-Schalter` (`Switch`, `AQ20`) over switches only, else
    `Master-Helligkeit` (`Dimmer`: `position` `AQ20`, `min`/`max`
    `SpStates[7]`/`[8]`, `step` `Step`); `Master-Farbe` (`ColorPickerV2`
    `Rgb` over the circuits' widest range, `AQ19`, the sequence lists'
    19th) when every circuit is a colour. Not derived: a circuit
    without a capability whose moods carry values other than `0`/`1` (a
    Lumitech output, one of 23 in the house), one an analog `OutputRef`
    takes, other capabilities.
  Evidence: hiding these objects per save and showing them again gives
  back Loxone Config's entries byte for byte (120 of 140), except the 20
  with an icon. Of the 140, 51 are named by a central block or the energy
  flow monitor and were dropped by hand
  (`LOXL_CATALOGUE_CORPUS=<dir> cargo test -p loxone-config --test catalogue`).
  The corpus holds catalogues Loxone Config wrote; a catalogue from an
  older push is stale against its program and no evidence.
- **A new display unit** (`<Display Unit=>`) is the entry's
  `details.format` and its statistic outputs' `format`; the push
  rewrites them (2026-10-31: the Miniserver served `%.1f °C` after a
  unit change from `<v.1>°`). A unit with no printf form stays for a
  save.
- **The app reloads on `lastModified`.** A push that renamed a virtual
  input and set `lastModified` to the push time (2026-10-30): the
  Miniserver served the new name, and `LoxAPPversion3` answered the new
  time. `permissions.bin` follows the catalogue's control order (above).
- **A minted block lacks what a save adds.** Loxone Config gives a new
  block its special states on the first save (see *What a first save
  adds*); the catalogue entry names them. Lox Logic mints a jalousie's
  ([D66](decisions/0066-app-blocks.md)), which the save keeps.
- **Unknown:**
  - the entries of the other block types, taken over as they are: the
    energy flow monitor, intercom, alarms, irrigation, room controllers.
    The corpus has one each, with details and states no setting or
    connector explains (the irrigation's `maxExpectedPrecipitation` names
    a connector the block does not write);
  - a light controller minted from source: it has no `NameAI<n>=` and no
    circuits until its mood editor is confirmed (see *LightController2
    moods*);
  - analog day timers, display texts of an object's own, and other
    `StatsType` values;
  - whether Loxone Config drops an entry when the room is removed, and
    where it puts a new entry (Lox Logic appends it).

## UUID anatomy

Format `{8}-{4}-{4}-{16}` hex — the last segment is 8 bytes, so these are
**not** RFC-4122 UUIDs.

| Segment | Meaning |
|---|---|
| 1 (u32) | creation time, seconds since **2009-01-01 UTC** (`1230768000`) |
| 2+3 (u16,u16) | mint-sequence counters |
| 4 (8 bytes) | identity tail, see below |

Tail shapes:

- `ff ff` + 6 bytes — an **object**. The 6 bytes identify the minting
  machine: a Config-PC id (`ed57184a04d2` observed) for GUI-created objects,
  the **Miniserver serial** (`504f94a26236`) for objects the Miniserver
  created itself (app-defined autopilot rules, device registrations). Block
  *states* share the `ffff` shape with a per-object random suffix.
- `<index> ff` + 6 bytes — a **connector**. Byte 0 is the connector's index
  within its block (`I1`→`00`, `I2`→`01`, `Q`→`02` on a 2-input gate);
  the remaining 6 bytes are a per-block entity shared by that block's
  connectors.
- anything else — reserved/system space. Built-ins like operating Modes
  have fully deterministic UUIDs (`00000000-0000-0001-1500000000000000`).

Mint order: Loxone Config mints a block's **ports first, then the object**
(consecutive counters `…0749, 074a, 074b` ports, `…074c` object). The
crate's minter mimics this.

Late-added connectors keep sequential indexes: a LightController2 whose
`I5`–`I8` were added years later (different time segment, different entity
bytes) still carries indexes `04`–`07` — those came from a schema
migration extending the type's descriptor, not from user action.

**A save writes exactly the type's connector set, in the type's order.**
Off-descriptor `<Co>`s are dropped without warning, wires included, and
missing ones are added. Any tool inventing connectors silently loses logic
on the next GUI save.

- **Gates do not grow.** `And`/`Or` in Loxone Config 17 are fixed
  two-input blocks. Wiring the last free input does not add one, and the
  connector picker shown when a wire is dropped on the block lists only
  the existing inputs. The block's ⊕ expander only toggles the display of
  existing connectors. A hand-grown `I3 <Co>` survives *loading* but is
  deleted on save, together with its wires ([D8](decisions/0008-fixed-gate-inputs.md)). No corpus gate has more
  than two inputs. Source: oracle, 2026-08-25.
- **Order follows the type.** A `StairwayLS` written in the newest corpus
  order (V151) came back from a 17.2 save reordered to `InputTrigger, On,
  Reset, InputDisable, TimeHigh, TimeWarn, Remanence, WarnTime, Q`, with
  `OutputAPI` appended under a GUI-minted UUID; every existing UUID was
  kept. 17.2 also writes `AnalogThresholdTrigger` as `Input, On, Off,
  Remanence, …` where 17.1 wrote `Input, Remanence, On, Off, …`, with
  unchanged indexes. Source: oracle sessions 14 and 16.

**UUIDs are unique, and the GUI enforces it.** No two `<C>` objects or
`<Co>` connectors share a `U=` anywhere in the corpus (~60 700 objects,
~76 600 connectors). The only repeats are `<SObj>` entries, which point
back at their object's UUID by design.

Loxone Config treats a repeat as damage and heals it on open. It keeps the
first occurrence and re-mints every later one, with an object-shaped
`ffff…` tail rather than the connector shape. The next GUI save therefore
silently re-points whatever was wired to a later twin. Evidence: the house
config on 2026-08-28, 38 UUIDs carried 80 times, 38 kept and 42
re-minted.

The known producer is upload tooling that maps a ref's ports by its `Ref=`
target plus connector key alone (lox-cli `config push`, 2026-08-25; fix
proposed in [eisber/lox-cli#29](https://github.com/eisber/lox-cli/pull/29)).
Mirrors of one target share that key, so all of them got the same `AQ`/`Q`
UUID. Lox Logic refuses a base with repeats (`compile`, `adopt`) and
checks its own output ([D38](decisions/0038-unique-uuids.md)); `diff` warns, `drift` notes them.

## Energy flow monitor: `<root>`

An `EFM`'s meters come from its *Monitor konfigurieren* dialog. It writes `<root t="5" rst="true">` with one
`<nd t="2|4" tit= ctrlUuid=<meter> liveState= efmState=>` per meter:

- `t` is 2 for the grid and 4 for a producer; the type picker also offers consumer.
- `liveState`/`efmState` are the monitor's `SpStates` in order.
- A nested `MeterAbsBi` *Rest* holds the unmetered consumption. Its `IName=` is its own UUID, which is also the
  root's `u=`, and it carries `NOEFM="true"`.

Source: oracle session 33.

## What a save changes

Loxone Config rewrites the whole file on save. Its own output round-trips byte for byte through the `xml`
layer. Opened and saved with no edits, a config changes in these ways:

| Change | Detail |
|---|---|
| `NextObj` +2 | Burned per open+save even with no objects added: counter values go to transients, not only to persisted objects. A compile takes the max of lock and document ([D11](decisions/0011-nextobj-counter.md)). |
| `Date` / `DateS` | The document header gets the save's wall-clock time. |
| `<In>` order | Wire lists under a `<Co>` are written in an internal canonical order, not the file's (exact reversals observed). Two saves of one state give the same order. The order carries no meaning: `diff` compares wires as sets, and a compile keeps the saved order ([D45](decisions/0045-wire-order-kept.md)). |
| Schema migrations | Older objects gain attributes (`SwitchingTimer M="3"` gained `UserModes="3"`; `TId=` template ids on 62 hardware devices). A freshly opened file therefore already has unsaved changes. |
| `WF` | View flags, rewritten in both directions (`147456` ↔ `16384`; bit `0x10` set on an analog sensor) or dropped: on `Memory`, `PushButton` and `PButtonT`, and on every `AutopilotRule`. |
| Geometry | `Py` is snapped to the grid of 96, `Px2`/`Py2` recomputed from the type (see *Objects*). Config 17.1 saves also rewrote `AutopilotRule` positions; 17.2 saves kept them. |
| `Nc` at zero | A `<Co>` that lost its last `<In>` is written self-closing, without `Nc`. |
| Order within a page | Config 17.2 moves blocks within their page, and writes `Desc=` right after `Type=` (with `TId=` after it on devices). A compile keeps the base's order ([D47](decisions/0047-block-order-kept.md)). |
| Version | Config 17.2 writes every object at `V="178"` ([D46](decisions/0046-block-version.md)). |
| Counts | `NumO=` is recounted. |

None of these touch the semantic diff or the drift baseline ([D21](decisions/0021-drift-baseline.md)). Source:
oracle sessions 1–36, the fingerprint first measured on two no-edit runs with Config 17.1.

### An old config's first save

Loxone's factory default `programdata/SDcard/prog/Default2.Loxone` (Config 11.1, `V="141"`) opens and saves
cleanly. The first save in Config 17.2:

- migrates every object to `V="178"`;
- adds the document's `APPKEY`/`APPID`, a document `<HP H="" Sc="-2" S="<salt>" Alg="1"/>` and
  `DeviceTelemetry="true"`;
- renames *Zeiten* to *Zeitfunktionen* (a locale rename);
- sets the admin's rights from 8191 to -1.

Source: oracle session 33.

## What a first save adds

Loxone Config takes a minted block without its GUI-owned children, and adds them on the first save. It never
rejects a block for their absence. Lox Logic carries what the save added on the next compile, so the churn
stops after one save ([D19](decisions/0019-gui-residue-carried-forward.md)). Where a decision says so, Lox
Logic mints these children itself, and the first save changes nothing.

- **Most function blocks:** `<IoData Visu="true"/>` (with `Rating="1"` on a light controller, the central
  blocks and the irrigation), `<Display …>`, and special states in `SpStates=`. The GUI mints those UUIDs with
  its own machine tail and consumes counter space for them.
- **`Memory`:** `Tp="0"`, `<IoData></IoData>`, `<Display Unit="&lt;v.1&gt;" StateOnly="true"/>`.
- **`PushButton`:** `SpStates=`, `<IoData Visu="true"/>`, `<PSD …/>`, `<Display Type="1" …/>`.
  **`PButtonT`:** `<IoData Visu="true"/>`.
- **`DayTimer`:** `On`/`Off`/`SpStates`, `IoData`, `Display`, and, for a timer with no entries, ten default
  `<Entry To="1440" V="1">` rows. A `Modes=` list without a weekday column gets that column as an all-day
  entry, i.e. switched on. Every `Modes=` list in the corpus holds the seven weekdays.
- **`LightController2`:** the stock moods (see *LightController2 moods*), `<LSConfig OpM="778" ScMv="777"
  ScAc="1"/>`, `<IoData Visu="true" Rating="1"/>`, `<PSD As="-2" Ae="1380" Amin="1" Amax="4" Dmin="30"
  Dmax="180" V="20"/>`, `<HCL T="2" M="0" TMIN="2000" TMAX="6500" FROM="0" TO="0"/>`, `<COHist TRK_NE="20"
  TRK_MAX="100"/>`, the `uuidSeqencing`/`uuidSeqenceIx` lists and fourteen special states.
- **`AutoJalousie`:** `<COHist>`, `IoData` and seven special states.
- **`State`:** `IoData`, `Display` and `<StateTexts>`. **`StairwayLS`:** `IoData`, `PSD`, `SpStates`, and its
  connectors in the type's order (see *UUID anatomy*).
- **`Formula`:** `Formula=""`. **`Code16`:** `Code=""` under 17.2, nothing under 17.1. Central blocks need no
  `rec=`: controlling nothing is valid, and the list appears once objects are assigned.
- **Meters:** `Rec=""`, `<statGrp>` groups, the meters' root. **`Alarm`/`SmokeAlarm`:** `Entries="40"`,
  zones; `Display` on the smoke alarm and the central blocks. **Irrigation:** its zones.
- **`HeatIRoomController2`:** a `<Timer>` with one all-day entry per mode in its `Modes=`, and `<Co K="AMode"
  Def="1">` when nothing was on `AMode`.
- **`VirtualIn`:** `WF="147456"`, `<IoData Visu="true" St="2"/>`, `<Display Type="1" Unit="&lt;v.1&gt;"
  StateOnly="true"/>`. A digital one gains `MaxVal="1" MinChange="0.5" MinTime="10"`. An analog one gains
  `EnVal="true"`, which always goes with `Analog="true"`, and its range is written in the order `EnVal,
  DefVal, MinVal, MaxVal`.
- **Virtual and Modbus devices** need no captions, `Online` children or display settings. The save adds `Cl`
  on an HTTP command, an empty `<IoData></IoData>`, `Display` defaults, `Tx="false"`,
  `ModbusCoilQuantity="1"`, `WF` on an output command and `WF="16384"` on a text input.
- **`Mode`:** a fresh one is saved with `WF="147456"` (see *Operating modes*).

Source: oracle sessions 3, 8, 9, 14, 16, 18, 21, 25, 26, 28, 29 and 33.

## The three writers

1. **Loxone Config** — the GUI; rewrites the whole file on save, including
   locale-dependent built-in titles (see *What a save changes*).
2. **The Miniserver** — creates objects at runtime (autopilots from the app,
   device registrations), minting UUIDs with its serial in the tail.
3. **This compiler** — see the ownership model in [design.md](design.md).

Any tooling that assumes it is the only writer, or that titles are stable,
breaks against 1 and 2.

## Reported by lox-cli, checked against the corpus

lox-cli's backlog files (`REQUIREMENTS-*.md` in eisber/lox-cli, read
2026-10-29) record what its authors saw on a live Miniserver. The
claims below are restated in our words with the corpus check run here
(web corpus, 132 configs). A fact this document relies on elsewhere has
its own evidence; these are leads, marked by how far they hold.

- **The GUI repairs malformed blocks by re-minting connectors.** A
  block written without layout attributes and with a partial connector
  set (a `Formula` with `Nio="4"` and only two inputs) came back from a
  GUI save with `Nio="6"` and all connectors in canonical order. The
  regenerated ones had *fresh UUIDs*, so the wires on them were lost, and
  a live light stuck off. This fits the connector facts above. Lox Logic
  always writes the full set in canonical order plus layout, so it never
  hits this. Not reproduced with the oracle.
- **`Formula=` grammar.** Inputs `I1`–`I4`; `+ - * / ^`; comparisons
  `== != > >= < <=`; `IF(cond;a;b)` with `;` as argument separator;
  functions such as `ABS SQRT INT SIGN MIN MAX LN LOG EXP`, the trig
  family, `PI`; no boolean `&&`/`||`/`!` (nest `IF` or use arithmetic).
  Corpus: `ABS`, `SQRT`, `INT`, `SIN`, `COS`, `ARCTAN`, `IF` occur, `IF`
  with `;`.
- **Either decimal separator works in `Formula=`.** The Miniserver reads
  `0.5` and `0,5` alike, and Loxone Config keeps a point verbatim on save
  without a message. Lox Logic writes a point (`I1*0.5`, [D35](decisions/0035-arithmetic-formula.md)). Six corpus
  formulas use a decimal comma and none a point; lox-cli reports the
  comma in Loxone's own examples. Evidence: live, firmware 17.2.8.28, four
  pushed Formula blocks (`0.5*4` and `0,5*4` both gave 2, `2.5` and
  `2,5` both 2.5); oracle session 23 (`Formula="I1*0.5+2.25"`).
- **`AnalogMultiplexer2` selects 1-based.** Live: `Select=0` drove the
  output to 0, `1` passed `Input1`, `2` `Input2` — like the four-way
  `AnalogMultiplexer`, and unlike lox-sim's model (`0`→`Input1`,
  `1`→`Input2`). A digital flag on `Select` therefore switches between
  off and `Input1`. Not checkable from configs; recorded in
  [connector-db.md](connector-db.md).
- **A `VirtualIn` outside its range reads 0.** An analog virtual input
  with `MaxVal="1000"` fed air pressure (~1024 hPa) read 0 until
  `MaxVal` was raised. `MinVal=`/`MaxVal=` are attribute parameters
  ([D54](decisions/0054-virtual-inputs.md)); set them to cover the values the input will receive.
- **LightController2 moods** ([D58](decisions/0058-lighting-moods.md)).
  - **Shape.** A `<LightscenesC Num= Outputs= FC=>` holds one
    `<LightsceneC Name= UUID= [Move="true"|ReadOnly="true"] Outputs= SID=
    CID= Q1=…Q<Outputs>>` per mood, in the app's list order. `Outputs` is
    18 on every observed controller. `<LSConfig OpM= ScMv= ScAc=>` names
    moods by `SID`.
  - **`Q<n>` belongs to circuit `AQ<n>`**, whose name is `NameAI<n-1>`.
    A circuit without `NameAI<n-1>=` (no actuator, as on a controller
    created from source) is a switch on the Miniserver: moods of 20 %,
    40 % and 60 % all read 1 on its output (a live test on a Miniserver,
    [oracle-sessions.md](oracle-sessions.md)).
    Its top four bits are a property of the circuit, never of the mood:
    132 corpus files, 648 circuit columns, 0 disagreements. The prefixes:
    - 0: switch 0/1, or a plain dimmer 0–100.
    - 1: dimmer 0–100.
    - 4: RGB, percent channels `R + 1000·G + 1000000·B`.
    - 5 and 6: that RGB packing, or tunable white
      `200000000 + 10000·brightness% + kelvin` (house: `0x5000_0000 +
      201002700` is 100 % at 2700 K).
  - **Built-in moods.**
    - `SID="778"` is the off mood ("Aus"/"Off", `ReadOnly`, house
      `CID` 11). Its values are the circuit prefixes with a zero payload.
    - `SID="777"` is the motion mood ("Viel Licht"/"Bright", `Move`,
      `CID` 22).
    - `SID="776"` is the alarm-clock mood.
  - **T5/n is the mood `SID="n"`.** No other attribute stores the
    assignment. Evidence:
    - The canvas labels T5/n with the name of the mood `SID="n"`: the
      lighting example's T5/1 "Lesen" is SID 1, a probe's T5/1 "A" and
      T5/2 "B" are SID 1 and 2.
    - The Structure File says "moodIds < 8 are linked to the appropriate
      T5 Input".
    - The live `moodList` sets `used` bit 2 on mood 1 exactly where
      `Sel1` is wired.
  - **Mood IDs are SIDs.** The live `moodList` `id` of every mood is its
    `SID` (3 controllers read). Output M shows 0 for the off mood, 99 for
    `SID="777"` and n for a user mood `SID="n"` (Loxone Config's
    simulation, [oracle-sessions.md](oracle-sessions.md) session 35). `used` bit 1
    is set exactly on the motion mood where Mo is wired (3/3), and
    `favoriteMoods` follows list order.
  - **A switch circuit's `on` reads 1.** With a mood `Q=1` active, plain
    switch circuits report `active: 1` (live, 3 controllers).
  - **New moods.** The house's GUI-made moods take `SID` 1, 2, 3… and
    `CID` 12, 13…, i.e. the lowest free above the off mood's. lox-cli's
    "from `SID` 2 / `CID` 9" does not match that; the corpus also has
    `CID` 7 and 8. What `CID` means is unknown; Lox Logic only keeps it unique.
  - **Order.** The off mood's place varies: of 43 corpus controllers
    with moods, 16 keep it last, 2 first, 4 in the middle, and 21 have
    none at all (older configs).
  - **First save.** A controller's first Loxone Config save writes the
    stock moods: `<LightscenesC FC="0040FFFF,008080FF,00FFFF80,0040FF40"
    Outputs="18" Num="2">` with "Viel Licht" (`Move="true"`, `SID` 777,
    `CID` 22, every `Q` 1) and "Aus" (`ReadOnly="true"`, `SID` 778, `CID`
    11, every `Q` 0). The rest of that save is in *What a first save
    adds*. A mood minted onto an existing controller comes back
    byte-identical. Source: oracle sessions 21 and 22.
  - **Circuits come from the mood editor.** A plain save writes no
    circuit (`NameAI<n>=`, `CapAI<n>=`, `PresM<n>=`, `COName="1"`), not
    even for a wired output with an `OutputRef` on it. Opening the
    controller's mood editor and pressing OK writes them. The circuit is
    named after the device title, not the actuator's; an *RGBW 24V Dimmer
    Tree* gets `CapAI1="31"`. The confirm also turned the stock mood *Viel
    Licht* on that output from `1` into a colour temperature (80 %,
    4700 K). Source: oracle session 33.
  - **Activation.** The Miniserver recalls a mood live with
    `changeTo/<SID>`.
- **Only eight Program blocks run.** Reported: the Miniserver executes
  the first eight `Code1`/`Code4`/`Code8`/`Code16` blocks and ignores
  the rest; Loxone Config warns on save. No corpus config has more than
  five, so unconfirmed here; a lint candidate.
- **Operating-mode UUIDs are short system forms**
  (`00000000-0000-000N-…`), not minted with a serial — as documented
  under UUID anatomy; a checker must not flag them.

## Related, out of crate scope

`.LoxCC` is the compressed container used for transport (magic
`0xaabbccee`); `lox config compress`/`extract` round-trips it byte-
identically. Handling lives in `lox`/`lox-cli`.
