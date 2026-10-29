# The `.Loxone` format — validated field notes

Everything in this document was established by examining real Miniserver
configs (six generations, Aug 2024 – Aug 2026, 117 KB – 1.34 MB, ~1900
objects) and a live Loxone Config / Miniserver installation (FW 17.x).
Facts marked **assumption** have no direct evidence yet.

There is no official specification; this is reverse-engineered knowledge and
the reason the crate's parser is hand-rolled.

## Container

- UTF-8 with **BOM**, `<?xml version="1.0" encoding="utf-8"?>` declaration,
  **CRLF** line endings, **tab** indentation, single space between
  attributes, no space before `/>`. Empty elements are *usually*
  self-closing, but not always: the GUI writes `<IoData></IoData>` even on
  freshly created blocks (oracle save 2026-08-25) — preserve the form per
  element, never normalize.
- Root: `<ControlList Version="…" NextObj="…" NextConst="…" NextNote="…"
  NextMem="…">`. The `Next*` counters are bumped by Loxone Config on save
  and must never decrease. `NextObj` correlates with auto-titles (`O1415`).
- Inside: one `<C Type="Document">` (carrying `ConfigVersion=` among many
  attributes), containing caption/folder `<C>` elements, `<C Type="Page">`
  drawing pages, and the object tree.

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

Entities used: `&amp; &lt; &gt; &quot; &apos;`, and nothing else. Loxone
does **not** decode numeric character references: `&#xA;` in an attribute is
the five characters `&#xA;`, and Loxone Config re-escapes the ampersand on
its next save (`&amp;#xA;`). Evidence: a foreign XML writer pushed the house
config on 2026-08-25 with the PicoC program's line breaks encoded as `&#xA;`.
The GUI save on 2026-08-28 turned them into `&amp;#xA;`, and the program has
been one comment line since; two `APC=` payloads and ten Modbus `NDOC=` texts
went the same way. No config in the 133-file corpus contains a numeric
reference. A writer must therefore emit line breaks raw (deviation 2) and
escape only the five named entities. lxir reads `&#…;` as literal text, and
`lxir drift` lists every attribute that holds one (design decision D40).
`lxir repair` writes a copy with each reference replaced by the raw
character it names (D44). Loxone Config keeps such repaired `APC=`,
`NDOC=` and `Code=` values byte-identical on open+save (oracle session 13).
Text
content appears only in `<Key>hex</Key>`-style elements and is rendered
inline (`<Key>2B35</Key>`).

## Objects: `<C>`

```xml
<C Type="Or" V="175" U="1e96b762-0130-074c-ffffed57184a04d2" Title="O1415"
   Px="5952" Py="11136" Px2="7296" Py2="11832"
   Cl="141,255,112" Nio="3" WF="147456">
```

- `Type` — block type (`Or`, `AutoJalousie`, `VirtualIn`, …).
- `V` — format/version field, `"175"` on current blocks.
- `U` — the object UUID (see below).
- `Title` — display name. **Locale-volatile** for built-ins: saving the
  config in a differently-localized Loxone Config renamed 111 built-in
  objects (Modes, weather fields, caption folders) in one observed save.
  Never use titles as identity.
- `IName` — internal name for I/O objects (`VI1`, `AI3`); locale-stable.
- `Px/Py/Px2/Py2` — drawing rectangle. Grid unit 96, port rows 192
  apart. Small logic blocks (gates, Monoflop, …) are 1344 wide
  and `312 + 192·rows` tall: Not 504, And/Or with two inputs 696. Their
  first port row is 288 below the top. Function blocks (PushButton,
  LightController2, AutoJalousie, …) are 2688 wide and `504 + 192·rows`
  tall, with the first row at 480: PushButton 1272 (4 rows), LC2 1656
  (6). `rows` counts the rows the GUI shows, the larger of the two
  sides. Some types show at least a set number of rows; on others a
  param appears only once it is wired (D42 has the rules). Ref tags
  (`InputRef`/`OutputRef`) are 2112 × 192 and sit 960 off the block
  edge, level with the port. `Memory` draws as the same 2112 × 192 tag. The GUI snaps `Py` to the grid on save and
  recomputes `Px2`/`Py2` from the type.
- `Cl` — RGB display color (logic green: `141,255,112`).
- `Nio` — connector count. Matches the number of `<Co>` children: Loxone
  emits **all** connectors, including unwired outputs and inputs without
  values.
- `WF` — view flags. Freely recomputed by the GUI: oracle saves rewrote
  it in *both* directions (`147456` ↔ `16384`, plus other values) and
  drop it entirely on some types — pure view-state, never identity.
- Complex blocks carry extra attributes (`LtE=`, `SpStates=`, `rec=`,
  UUID-list attributes…). All of it round-trips untouched through the
  lossless tree. Some of it is **block logic stored outside connectors**:
  `PulseAt` keeps its fire time in `Sec=` (seconds since midnight, with
  `Typ=`/`AutP=` mode flags); `DayTimer` keeps its schedule as `<Entry>`
  child elements after the `<Co>` list (`N=` holds their count) with
  `Analog=`/`DefValue=`/`On=`/`Off=` output behavior and `Modes=`/
  `UserModes=` operating-mode gating.
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

- `K` — port key (the name the IR uses).
- `U` — the **port's own UUID**; wires reference port UUIDs, not object
  UUIDs.
- Wires are stored at the **sink**: each `<In Input="…"/>` names the source
  port's UUID. An output being wired leaves no trace on the output itself.
- A wire source needs **no on-page representation**: the system `Mode`
  objects (operating modes; no `Px/Py`, deterministic UUIDs) source wires
  directly into page blocks. The GUI's own pattern inserts an `InputRef`
  when a mode is dragged onto a page (nine corpus configs), but a save
  accepts and preserves the direct wire without creating one — verified
  by the sixth oracle run ([oracle-wine.md](oracle-wine.md)).
- `Nc` — number of incoming wires (= count of `<In>` children); omitted
  when zero.
- `Def` — the port's parameter value; omitted at type default.
- Attribute order: `K, Def, Nc, U` (an oracle save 2026-08-25 rewrote a
  compiler-emitted `K, Nc, Def, U` into this order).
- An `<In>` may carry `FLG="1"` (rarely `"2"`) — Miniserver/app-created
  wire metadata, concentrated on API-connector and central-alarm wires.
  An oracle probe (2026-08-25) showed Loxone Config treats it as inert
  stored state: round-tripped verbatim, never regenerated, and a
  stripped flag is accepted without repair or wire loss
  ([oracle-wine.md](oracle-wine.md)).
- A `<Co>` may carry `Inv="true"` — the GUI's input inversion. Its
  dominant real-world use is not inverting wires: **unwired**, it turns
  the constant-0 input into a constant 1, and that is how the GUI
  encodes boolean checkbox settings like an enabled Remanenz (every one
  of the house's 23 `LightController2`s, its `PushButton`s and
  `DayTimer`s carry `Inv="true"` on unwired `Remanence`). Wired, it
  negates the incoming signal. Either way the connector is GUI-owned in
  lxir (design decision D20): carried verbatim, refused as a source-
  declared wire/value target.

## Refs: `<C Type="InputRef"/"OutputRef" Ref=…>`

A ref is the GUI's page-local mirror of an object living elsewhere — a
periphery device port folder, a system flag, or another page's block.
Evidence (r50 house config, 2026-08-25: all 113 uuid-pinned ref externs
resolved):

- `Ref` names the mirrored **object's UUID** (not a port): a
  `LoxAIRsensor`/`LoxAIRactor` subdevice, a `VirtualIn`, a flag object,
  or a managed block.
- The signal path is ordinary wiring on top: an `InputRef` carries an
  `<In>` from the mirrored object's output port into its own `AI`
  connector (`ref.AI <- source.AQ` in IR terms), and consumers wire from
  the ref's `Q`/`AQ`. `Ref=` is the GUI's display link; the wires are
  the semantics. Decompile folds these plumbing wires (D29); the
  `mirrors:` matcher resolves through `Ref=` (D32).
- `LinkRefType` — a small integer alongside `Ref=` (observed: 71, 172,
  173, 174), plausibly encoding the mirrored object's kind/direction;
  `Analog="true"` marks analog refs. Since D33 lxir mints refs itself,
  emitting these from the corpus-learned type table
  (`connectors::ref_link_type`); on unmanaged refs they stay carried
  verbatim.
- **Every mirror carries its own ports.** `InputRef`s of one target
  have distinct `AQ`/`Q` connector uuids like any other object (r50
  before 2026-08-25 and since 2026-08-28: 19/19 multi-mirror targets;
  corpus-wide no connector uuid repeats), so a consumer wire names
  which tag it reads from. `OutputRef` mirrors likewise each carry
  their own `AQ` (27/27) and draw their own distribution wire into the
  actor. D33's fresh output uuids for a minted mirror are therefore
  what the GUI does too. *Correction (2026-10-18):* this note first
  recorded the opposite — "mirrors of one target share their
  output-port uuids, 19/19" — read off r50 snapshots of 2026-08-25
  that the first push had already damaged (see *UUIDs are unique*
  below).
- **A ref's `Title=` is derived from its target.** Loxone Config
  rewrites it to the mirrored object's title on every save, whatever it
  held — minted refs (oracle session 11) and the GUI's own alike,
  visible or not, stale ones included (session 12, 2026-10-29). The one
  exception are the inputs an alarm block (`SmokeAlarm`, `Alarm`)
  collected into its sensor list: `InputRef`s with `WF` bit 0x1000
  (house config: 46 of 189 InputRefs, all wired into such a block, 33
  of them titled "<input> <device>", the rest a plain copy). The GUI
  keeps their titles through a rename of the target or of its device.
  `WF` is the GUI's own bookkeeping: a hand-flipped 0x1000/0x0800 is
  restored on save, adding or removing `Px`/`Py` changes neither bit
  nor title. The `OutputRef`s those blocks use also carry 0x1000
  (54 in the house) and follow their target like any other ref. lxir
  keeps refs in step with title claims (D43).
- **API distribution wires are recorded in reverse**: for
  `AutoJalousie.OutputAPI → ApiActor`-style links the `<In>` entry sits
  on the *block's output* connector and names the `OutputRef.AI` (or
  `ApiActor.I`) as its `Input` — source and sink swap roles relative to
  every ordinary wire. A link drawn through a mirror is three wires,
  without exception (61 links: 45 across 24 public corpus configs, 16
  in the house config, 2026-10-29): the forward `OutputRef.AI ←
  block.OutputAPI`, the reverse `block.OutputAPI ← OutputRef.AI`, and
  the mirror's distribution `ApiActor.I ← OutputRef.AQ`; every such
  mirror's `Ref=` is an `ApiActor`, and every reverse wire sits on a
  connector keyed `OutputAPI` (12 block types, `AutoJalousie`,
  `AcControl`, `NfcCodeTouch`, `TpfController`, … — six more reverse
  wires sit on connectors keyed `API`). `FLG="1"` marks all 16 house
  reverse wires and none of the 45 public ones, so the flag is not part
  of the link. lxir writes the reverse wire as an argument naming the
  mirror's `AI` (ir-spec, *Block declaration*) and mints it without
  `FLG=`.

## Automatic Designer rules: `<C Type="AutopilotRule" APC=…>`

The app's *Automatic Designer* (German "Automatik Designer"; the feature
and block family keep the older "Autopilot" name) stores each rule as an
`AutopilotRule` block in the config. Rules are created in the app/web
UI; the **Miniserver** writes them (UUID tail = its serial), and Loxone
Config displays them — the official KB warns to *load from Miniserver*
before saving so app-created rules are not lost. There is **no public
spec**: the Structure File and Communicating-with-the-Miniserver PDFs
(v17.0, checked 2026-08-26) mention the feature only in passing, and the
rule schema below is corpus-derived. Evidence base: 10 rules — 5 in the
r50 house config, 5 across three public use-case configs (V172–V175).

- One **`AutoPilot` container** exists per config (present even with
  zero rules, 100+ corpus configs): locale-volatile title
  ("Automatik-Regeln"/"Automatic rules"/…), `Nio="2"` (`Input`,
  `output`), `SpStates=`, sitting among the system objects. Each rule's
  `ref=` attribute names this container's UUID.
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
  argument where one exists. Observations short of validation: event
  `type` selects the condition source (0 = control state with
  `controlUUID`; 1 with `descID` 21001 looks operating-mode-shaped;
  3 with `descID` 270001 weekday-shaped, weekday sets via `arrValues`
  and a `9999999999` sentinel `value`; 5 with `descID` 272001
  time-shaped, `value` = minutes since midnight — 540 on a rule titled
  "Skimmer Ein", matching the `Time` singleton's unit); `descID` reads
  as a per-control-type code × 1000 plus an offset (+001… events,
  +101… actions: 342xxx on PushButton, 493xxx on PButtonT, 527xxx on
  AcControl); the group/rule `operator` codes (observed only 1 and 3)
  presumably encode the AND/OR combination. The full `descID`/`type`
  tables ship inside the Loxone app bundle, not in any config.
- The raw payload text is **not writer-stable**: two of the house
  rules carry a raw CRLF *inside* the JSON (between tokens — the
  parsed value is unaffected), the other eight are single-line. Any
  comparison of `APC=` must therefore be structural, not textual.
- **lxir's stance (design decision D37)**: rules are app/Miniserver-
  owned — carried verbatim like all foreign content, compared by
  `lxir diff`/`drift` (the `APC=` attribute joins the semantic
  projection, canonicalized as parsed JSON per the point above), and
  rendered as comment lines in the decompile full view. lxir never
  writes them. Open oracle question: whether a GUI save preserves a
  foreign-minted rule at all — prerequisite for any authoring work.

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

Evidence: three public corpus configs with 16 tiles in all
(`gh_eisber_lox-cli_no-cleaning-room-email`, 9;
`…_UseCase-AC-TouchPureFlex`, 6; `…_Fan_Control_-_Presence_and_Temperature_Support`,
1), plus the house config.

- **`<btn>`** — one tile. `i` is its index (lxir addresses tiles by it);
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
- **Arrows are up and down.** The shading tile of the no-cleaning-room
  config sends `SET(As;Co;pulse)` (complete open) on `bT="1"` and
  `SET(As;Cc;pulse)` (complete close) on `bT="2"`, click
  `SET(As;Tg;pulse)`; the fan tile of the fan-control config opens a
  menu on click and puts the same `SET(Sts;+;Pulse)` step on both
  arrows. So `f2` is the arrow up, `f3` the arrow down.
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

lxir rewrites the functions of existing tiles and carries the rest of
`<btns>` byte for byte (D41).

## Rooms and categories: `<IoData Cr=… Pr=…>`

Rooms are `<C Type="Place">` elements under the `PlaceCaption` folder,
categories `<C Type="Category">` under `CategoryCaption` — plain
objects with user-given titles. A block's room/category assignment
lives in its `<IoData>` child: `Pr=` holds the Place UUID, `Cr=` the
Category UUID. Validated corpus-wide (2026-08-25): ~36 900 `Cr`/`Pr`
occurrences across the house config and all 132 web configs sit on
`IoData` and resolve to a Place/Category element (the only exceptions:
one hand-written non-Config file carrying them on `<C>`, and dangling
references inside template/fragment exports — so a resolver must treat
an unresolvable `Pr`/`Cr` as "no room", not an error). On managed
blocks `IoData` is D19 GUI-owned residue, carried forward unchanged. On
externs lxir reads it for composite matching (`room:` / `category:`)
and writes it for exactly one purpose: an attribute claim
(`x.room = "…"` / `x.category = "…"`, D39 in [design.md](design.md)),
which rewrites an existing `Pr=`/`Cr=` value in place — never adds the
attribute or the element.

Devices nest their inputs and outputs as child `<C>` elements, each with
its own `<IoData Pr=…>`. In the house config 673 of 687 device children
carry their device's room; the rest were moved individually. Loxone
Config does not cascade a device's room change to its children (its UI
strings have no such prompt) — the follower rule of D39 is lxir's own.

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

**Gates do not grow.** `And`/`Or` in Loxone Config 17 are fixed two-input
blocks: the GUI offers no way to add an input (wiring the last free input
does not create one; the wire-drop connector picker enumerates only
existing inputs), the corpus contains zero gates with more than two
inputs, and — verified via the Wine oracle — a hand-grown `I3 <Co>`
survives *loading* but is **silently deleted on save, together with its
wires**. Generalized: a save writes exactly the type descriptor's
connector set; off-descriptor `<Co>`s are dropped without warning. Any
tool inventing connectors will silently lose logic on the next GUI save.

**UUIDs are unique — the GUI enforces it.** No two `<C>` objects or
`<Co>` connectors share a `U=` anywhere in the corpus (~60 700 objects,
~76 600 connectors); the only repeats are `<SObj>` entries, which point
back at their object's UUID by design. Loxone Config treats a repeat as
damage and heals it on open: it keeps the first occurrence and re-mints
every later one (r50, 2026-08-28: 38 UUIDs carried 80 times → 38 kept,
42 re-minted, with an object-shaped `ffff…` tail rather than the
connector shape), so the next GUI save silently re-points whatever was
wired to a later twin. The known producer is upload tooling that maps a
ref's ports by its `Ref=` target plus connector key alone (lox-cli
`config push`, 2026-08-25; fix proposed in
[eisber/lox-cli#29](https://github.com/eisber/lox-cli/pull/29)): mirrors
of one target share that key, so all of them got the same `AQ`/`Q` UUID. lxir refuses a base with repeats (`compile`,
`adopt`) and checks its own output; `diff` warns, `drift` notes them.

## The three writers

1. **Loxone Config** — the GUI; rewrites the whole file on save, including
   locale-dependent built-in titles. The full save fingerprint (counter
   burn, `<In>` reordering, schema migrations, auto-layout) is measured in
   [oracle-wine.md](oracle-wine.md).
2. **The Miniserver** — creates objects at runtime (autopilots from the app,
   device registrations), minting UUIDs with its serial in the tail.
3. **This compiler** — see the ownership model in [design.md](design.md).

Any tooling that assumes it is the only writer, or that titles are stable,
breaks against 1 and 2.

## Related, out of crate scope

`.LoxCC` is the compressed container used for transport (magic
`0xaabbccee`); `lox config compress`/`extract` round-trips it byte-
identically. Handling lives in `lox`/`lox-cli`.
