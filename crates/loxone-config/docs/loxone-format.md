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
- Pages are the direct children of the one `<C Type="Program">` (title:
  the Miniserver's). A page is `<C Type="Page" V=… U=… Title=… LtE=…
  WF="16384">` with `<IoData Cr="<Category>" Pr="<Place>"/>` as its first
  child, then its objects. `LtE=` is the last edit in seconds since
  2009-01-01 UTC; the special page "Automatik-Regeln" carries `SpNo="1"`,
  no `LtE=` and the unassigned `Place`. "Neue Seite" (Tools ribbon) asks
  for a title and a room — Save stays disabled while the room is the
  unassigned one — and files the page among the others in
  case-insensitive title order; the GUI never re-sorts existing pages,
  so real lists are not sorted (oracle 2026-10-29, 17.2). A page without
  `LtE=` loads and saves unchanged (D56).
- `PaperFormat=` on the Document is the paper sheet every page is drawn
  on; pages carry no size of their own. `2` is A3 landscape, the format
  of 109 of 112 corpus configs and the largest the GUI offers. The GUI
  draws the sheet at 420 × 297 mm in twips (1/1440 in): x 0..23808,
  y 0..16800, with a title block printed along the bottom from
  y ≈ 16080. Objects past the edge are drawn on
  the grey canvas beyond it. `1` and `3` occur (one and two configs)
  with unknown sizes (D50).

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
escape only the five named entities. Lox Logic reads `&#…;` as literal text, and
`loxl drift` lists every attribute that holds one (design decision D40).
`loxl repair` writes a copy with each reference replaced by the raw
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
- `V` — block-format version, one per document: Loxone Config writes
  every object at its release's version on save (`"175"` for 17.0/17.1,
  `"178"` for 17.2; see design decision D46).
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

- `K` — port key (the name the source uses).
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
- `Def` — the port's parameter value; omitted at type default. A wired
  `<Co>` can carry one too (the Loxcall library entry: an `Equal`'s
  `Input1` with `Def="2"` and an `<In>`): Loxone Config keeps the value
  when a wire is drawn onto the port. The wire supplies the input; the
  constant is what the port reads again once the wire is removed. Lox
  Logic carries it (design decision D77).
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
  Lox Logic (design decision D20): carried verbatim, refused as a source-
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
  connector (`ref.AI <- source.AQ` in Lox Logic terms), and consumers wire from
  the ref's `Q`/`AQ`. `Ref=` is the GUI's display link; the wires are
  the semantics. Decompile folds these plumbing wires (D29); the
  `mirrors:` matcher resolves through `Ref=` (D32).
- `LinkRefType` — a small integer alongside `Ref=` (observed: 71, 172,
  173, 174), plausibly encoding the mirrored object's kind/direction;
  `Analog="true"` marks analog refs. Since D33 Lox Logic mints refs itself,
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
- **An `InputRef` of an input carries both of its outputs.** The ref's
  `AI` reads the input's `AQ` and its `I` reads `Q`: house config, all
  143 `InputRef`s of sensors and virtual inputs (10 types, `LoxAIRsensor`
  to `VirtualIn`); refs of `Memory`, `OvertempShutdown` and other block
  outputs wire `AI` only. Oracle session 15: a minted ref of a
  `LoxAIRAsensor` with only `AI` wired gained `I ← Q` on save. A module
  writes both (`AI: x.AQ, I: x.Q`).
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
  (54 in the house) and follow their target like any other ref. Lox Logic
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
  of the link. Lox Logic writes the reverse wire as an argument naming the
  mirror's `AI` (ir-spec, *Block declaration*) and mints it without
  `FLG=`.

## Push notifications: `Notification`, `OutputRefLM`, `<LoggerMailer>`

Evidence: oracle session 15 (Loxone Config 17.2.8, 2026-10-29: a
notification created in *Messages*, placed on a page, a recipient
ticked, saved; see oracle-wine.md), the house config and the 133 corpus
configs (113 with at least one `Notification`).

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
  `WF="147456"` (the known save fingerprint, oracle-wine.md). Its title
  follows the notification's.
- **The message is a `<LoggerMailer>` child** after the connectors:
  `RefNotification=` repeats the notification, `On=`/`Off=` are the texts
  sent when `AI` turns on and off, `MinimumTime=` the least time between
  two messages in milliseconds (GUI default `5000`, also 53 times in
  the corpus), `NL="1"` always with `RefNotification=`. An empty `Off`
  text is not written. The GUI's defaults for `On`/`Off` are `<v>`.
  Inputs and outputs that log, mail or notify carry the same child with
  other `Ref…=` attributes (`RefLogger`, `RefMailer`, …); Lox Logic compares
  all of it (`message <attr>` in the diff).
- **Recipients are `ACT=` rights** on the notification: ticking a user
  in Config 17.2 appends `<PRM REF="<Notification>" ACT="3"/>` to the
  user's `<PH>` list and raises `NUM=` — the same edit as an access
  right (see *Users, groups and access rights*). Public corpus configs
  hold `ACT="1"` on notifications (5 entries in 4 configs), two of them
  together with a `TIM=`. Members of the *Full access* group show as
  inherited recipients that cannot be unticked, and get no `<PRM>`.
  **Unverified:** whether the Miniserver delivers to a recipient that
  `permissions.bin` does not list; Config writes both.

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
  `ref=` attribute names this container's UUID. Its `Input` lists, as
  `<In Input=… FLG="1"/>`, every source a rule watches: the `DayOfWeek`
  block's `Q` for a weekday condition, the state UUID for a state
  condition, the mode's `Q` for a mode condition; a time condition adds
  none. A save writes them in reverse document order of the rules'
  conditions, each once (oracle session 31). Older corpus files mark
  them `Hidden="true"`; one corpus config (V254, two rules on states
  and a mode) has none at all. Why is not known; Lox Logic leaves such
  a list alone until a compile changes the rules (D73).
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
- **The codes, from the Miniserver** (2026-10-31, D73): the app builds
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
- **Weather, clock, date and message shapes** (2026-10-31, D76; from
  the capability table's `sections` and the web app's
  `_applyEventToRule`/`_applyActionToRule`): a weather condition
  `{"type": 2, "descID": 37001…37010, "id": <weatherFieldTypes id>,
  "value", "operator"}` (1 temperature, 2 dewpoint, 4 wind, 5 wind
  direction, 6 gusts, 7 irradiance, 8 relative irradiance, 9
  precipitation, 10 weather type, 11 pressure, 26 felt temperature, 27
  solar irradiance, 34 sunshine — digital); clock conditions `type` 3
  with `id` = `descID / 1000`: 269001 day of month (analog), 281001
  daylight (digital), 294001 sunrise, 295001 sunset, 298001 dawn,
  299001 dusk (moments, `value` 1); a date `{"type": 4, "descID":
  233001, "id": 233, "value": <Unix time of the local midnight>}`.
  Actions carry `arrValues` and the command the table's `action`
  template gives: 2 mailer (92001, `[to, subject, text]`), 3 caller
  (93001), 4 notification (96001, `[subject, text]`), 5 pause (341801,
  milliseconds, `command` the number), 6 speech output (223101), 7
  message center (98101, `autopilotMessage/<severity>/<subject>/<text>`,
  `arrValues` `[severity, subject, text, 2 everyone | 4 admins]`).
- The raw payload text is **not writer-stable**: two of the house
  rules carry a raw CRLF *inside* the JSON (between tokens — the
  parsed value is unaffected), the other eight are single-line. Any
  comparison of `APC=` must therefore be structural, not textual.
- **Lox Logic's stance (design decision D37)**: rules are app/Miniserver-
  owned — carried verbatim like all foreign content, compared by
  `loxl diff`/`drift` (the `APC=` attribute joins the semantic
  projection, canonicalized as parsed JSON per the point above), and
  rendered as comment lines in the decompile full view. Since D57 a
  `rule` declaration edits the rules of the documented shapes (time,
  weekday; actions with an observed `descID`; since D73 control states,
  operating modes, OR groups and any action of the capability table);
  since D60 a named one
  creates a rule the config lacks (below), and Lox Logic never deletes one.
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
  A rule deleted in the app (live, 2026-10-31, one sample) stays a
  custom change under the same name: the whole rule element, `STA` and
  payload as before, without the `<TPC>` child, `WF="33570816"` (the
  program's `16384` plus `0x2000000`). Loading an upload removes it.
- **Edits, as the app makes them** (house config history and a live
  push, 2026-10-29): deactivating a rule in the app changed exactly
  `"active": true → false` and removed `STA`; a compiled payload in the
  app's key order (`type, descID, id, controlUUID, active,
  activeIfValid, name, description, value, operator, invalid,
  forceEdge, command, arrValues, eventGroups, events, actions`,
  compact) with `STA="1"` right after `APC=` was accepted by the
  Miniserver and came back byte-identical in the verifying download. Weekday indices count from Monday = 0 (`arrValues`
  `[3, 4]` = Thursday, Friday).
- **A rule created outside the app** (D60, oracle session 24,
  2026-10-30): an element shaped like the house's rules — `V`, `Cl`,
  `Nio="7"`, `WF` and `ref=` copied from an existing rule, the seven
  connectors minted right before the rule (port UUIDs = the rule's
  counter − 7 + index, index byte in segment 4, as 5/5 house rules
  show), `<IoData></IoData>`, placed in the column below the last rule
  (the house's gap: 168 between `Py2` and the next `Py`, four of five)
  and a payload in the app's key order with `id` = its UUID and
  `name` = `Title` — survives a Loxone Config open+save byte-identically
  but for `WF=`, which the save drops from **every** rule (the five
  Miniserver-written ones too), and the semantic diff is empty.
  A Miniserver run of such a rule is not yet verified.

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

Lox Logic rewrites the functions of existing tiles and carries the rest of
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
externs Lox Logic reads it for composite matching (`room:` / `category:`)
and writes it for exactly one purpose: an attribute claim
(`x.room = "…"` / `x.category = "…"`, D39 in [design.md](design.md)),
which rewrites an existing `Pr=`/`Cr=` value in place — never adds the
attribute or the element.

Devices nest their inputs and outputs as child `<C>` elements, each with
its own `<IoData Pr=…>`. In the house config 673 of 687 device children
carry their device's room; the rest were moved individually. Loxone
Config does not cascade a device's room change to its children (its UI
strings have no such prompt) — the follower rule of D39 is Lox Logic's own.

## Hardware identity: `Serial=`

Hardware elements carry their serial number in `Serial=`, written as
hex digits in upper case (web corpus, 2026-10-29; four Miniservers end
in a placeholder `X` instead): 12 digits on the Miniserver (`LoxLIVE`; the same serial forms the tail
of the UUIDs it mints, see below), 8 on everything else — Tree
Extensions (`LoxTree`) and Tree devices (`TreeDevice`), the Air base
(`LoxAIR`), Extensions (`LoxMORE`, `LoxDIMM`, `LoxAout`, `LoxOCEAN`),
and the bus interfaces (`Comm485`, `Comm1wire`, `CommDMX`, `CommIR`,
`EIBline`). They nest the way the hardware is cabled: a Tree device
inside its Tree Extension inside the Miniserver, each with its own
serial, the I/O objects as children of the innermost. An empty
`Serial=` occurs (`LoxDMXdevice`) and means none. Air devices
(`LoxAIRDevice`) have no `Serial=`; a 32-digit hex `IP=` identifies
them: the link-local IPv6 address formed from the device's MAC (EUI-64),
and that MAC is its serial — `/data/status` on the Miniserver lists
`IP="FE80000000000000524F94FFFEBA1F0E"` as
`Serial="50:4F:94:FF:FE:BA:1F:0E"` (the universal/local bit of the first
byte flipped back, `52` → `50`; checked on the house, 38 of 38). Lox Logic
reads `Serial=`, and an Air device's serial from its `IP=`, for
`device:` extern matching (D55, D71); `loxl scan` reports both as
personal data.

A device's own attributes: an Air device always carries `Installation`,
`SwitchBoard`, `SwitchBoardRow`, `SwitchBoardPos` (103 of 103 in the
corpus); a Tree device `Installation`, the board only where it is
mounted in one — a save drops a board written onto a room device
(oracle session 30). Every Air device in the corpus has
`PWMFrequency="123"`, `UseDimmer="true"` and (99 of 103)
`UseRGBDimmer="true"`. A save writes `TId=` (`NanoIO`,
`RoomsensorTree`) right after a `Desc` it finds on a device.

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
- `Mode` — the kind of date: absent, a fixed date every year (`Month`,
  `Day`); `1`, relative to Easter Sunday (`EasterOffset` in days, which
  may be negative); `3`, one period (`From`, `To`: days since
  2009-01-01 — `2375` is 4 July 2015); `4`, a yearly range (`Month`,
  `Day` to `Month2`, `Day2`). No other value occurs.
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
Touches, for an `access` declaration (D49), and the recipients of push
notifications, for a `recipients` declaration (D51).

Evidence: the 132 public web configs (154 users in 131 configs, 402
groups) and the house config, Config 12 – 17.

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
  `;` and reads a value without one as one entry. Loxone Config 17.2
  writes an access code as 32 upper-case hex digits that are neither the
  plain MD5 nor the SHA-1 of the code; the algorithm is unknown (oracle
  session 33).
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
  `REF=` is the object the right is on. `ACT=` is a UI right (`3` in
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
  Touch>" TIM="<schedule>"/>` to the user's list and raises `NUM=`;
  nothing else in the users tree changes, the user's `LastEdit=`
  included (Config 17.2, one save on 2026-10-29: the same grant compiled
  by Lox Logic onto the config from before the save reproduces Config's
  users, groups and schedules byte for byte). **Unverified:** what a
  revoke writes. Lox Logic removes `TIM=`, then a `<PRM>` left with only
  `REF=`, then a list left empty.
- **Full access is no access right.** A user in the full access group
  without a `TIM=` right on a Code Touch is refused there (live
  Miniserver, FW 17.x).
- **Named credentials** of a Code Touch are objects: `Keycode` (access
  codes) and `NFCID` (tags), in a `KeycodeCaption`/`NFCIDCaption` of the
  device or as children of the `NfcCodeTouch` block, with the hashed
  value in `Cde=` and an optional validity window in `StartT=`/`EndT=`.

Lox Logic treats every hash and salt as secret: the diff shows a credential
as a name, a count or an eight-digit fingerprint (a SHA-256 over the
stored value), never the value itself.

## The access table: `permissions.bin`

Loxone Config writes `permissions.bin` into the program ZIP, next to
`sps0.LoxCC` and `LoxAPP3.json`: for each object the app shows, the
users and groups that may see and use it. `loxone_config::permissions` parses it
and generates it from the program and the ZIP's `LoxAPP3.json`.

Evidence: 19 program ZIPs of one installation, program versions 243 to
274 (Aug 2024 – Oct 2026), 2.8 to 37.7 KB of `permissions.bin` each:
Loxone Config saves, and ZIPs in which an upload replaced `sps0.LoxCC`
and kept Config's `permissions.bin` and `LoxAPP3.json`. The generator
reproduces all 19 byte for byte (`LOX_CORPUS=<dir> cargo test
corpus_permissions`).

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
- **A grant, regenerated.** For the Config 17.2 save that granted a
  user access (*Users, groups and access rights* above), the file
  generated from Lox Logic's compile of that grant and the `LoxAPP3.json`
  from *before* the save equals the `permissions.bin` Config wrote,
  byte for byte: the push's situation, which keeps the live
  `LoxAPP3.json`.
- **What `FD` means** is unknown; it is written exactly where listed.
- **The Miniserver stores it as uploaded.** After an upload that
  replaced only `sps0.LoxCC` (2026-10-29), the next download was the
  uploaded ZIP byte for byte, `permissions.bin` and `LoxAPP3.json`
  included. **Unverified:** whether the Miniserver enforces this file
  or derives the rights from the program, so whether a stale file
  matters.

## The app catalogue: `LoxAPP3.json`

Loxone Config writes `LoxAPP3.json` into the program ZIP when it saves
to the Miniserver: the structure the app reads. The Miniserver serves it
as `/data/LoxApp3.json` exactly as uploaded and reports its
`lastModified` as `/jdev/sps/LoxAPPversion3`; it never rebuilds it
(#155). `loxone_config::catalogue` edits it for a compile.

Evidence: three Loxone Config 17.2 saves of one installation
(2026-10-29, 148 controls each), their catalogue's `lastModified` equal
to the program's `Date=`; and two pushes to the same Miniserver
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
  Hiding these objects per save (140, of them 51 a central block or the
  energy flow monitor names, dropped by hand) and showing them again
  gives back
  Loxone Config's entries byte for byte (120), but for the 20 with an icon
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
  block its special states on the first save (oracle session 28: seven
  on a jalousie, fourteen and the sequence lists on a light controller,
  `Rec=""` and two `<statGrp>` on a meter); Lox Logic mints a jalousie's (D66),
  which the save keeps.
- **Unknown:** the entries of the other block types (taken over as they
  are: the energy flow monitor, intercom, alarms, irrigation, room
  controllers — one each in the corpus, with details and states no
  setting or connector explains, such as the irrigation's
  `maxExpectedPrecipitation` naming a connector the block does not
  write), a light controller minted from source (no `NameAI<n>=`, no
  circuits; a plain save does not add them either, even with an
  `OutputRef` on the output: Loxone Config writes `NameAI<n>=`,
  `CapAI<n>=`, `PresM<n>=` and `COName="1"` when its mood editor is
  confirmed, oracle session 33), analog day timers,
  display texts of an object's own, other `StatsType` values, whether Loxone Config drops an entry when the room is removed,
  and where it puts a new entry (Lox Logic appends it).

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
of one target share that key, so all of them got the same `AQ`/`Q` UUID. Lox Logic refuses a base with repeats (`compile`,
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

## Reported by lox-cli, checked against the corpus

lox-cli's backlog files (`REQUIREMENTS-*.md` in eisber/lox-cli, read
2026-10-29) record what its authors saw on a live Miniserver. The
claims below are restated in our words with the corpus check run here
(web corpus, 132 configs). A fact this document relies on elsewhere has
its own evidence; these are leads, marked by how far they hold.

- **The GUI repairs malformed blocks by re-minting connectors.** A
  block written without layout attributes and with a partial connector
  set (a `Formula` with `Nio="4"` and only two inputs) came back from a
  GUI save with `Nio="6"`, all connectors in canonical order, and
  *fresh UUIDs on the regenerated ones*: the wires on them were lost,
  and a live light stuck off. Consistent with the connector facts above
  (Loxone writes every connector); Lox Logic always writes the full set in
  canonical order plus layout, which is why it never saw this. Not
  reproduced with the oracle.
- **`Formula=` grammar.** Inputs `I1`–`I4`; `+ - * / ^`; comparisons
  `== != > >= < <=`; `IF(cond;a;b)` with `;` as argument separator;
  functions such as `ABS SQRT INT SIGN MIN MAX LN LOG EXP`, the trig
  family, `PI`; no boolean `&&`/`||`/`!` (nest `IF` or use arithmetic).
  Corpus: `ABS`, `SQRT`, `INT`, `SIN`, `COS`, `ARCTAN`, `IF` occur, `IF`
  with `;`. **Decimal separator:** six corpus formulas use a decimal
  comma (`0,5`), none a decimal point, and lox-cli reports the comma in
  Loxone's own examples. D35's expression folding writes constants
  with a point (`I1*0.5`). **Live-verified 2026-10-30** (firmware
  17.2.8.28, four pushed Formula blocks, `R` read via
  `/dev/sps/io/<uuid>/all`): `0.5*4` and `0,5*4` both give 2, `2.5`
  and `2,5` both 2.5 — the Miniserver reads either separator. **Oracle
  2026-10-30** (Loxone Config 17.2.8.28): a compiled `Formula="I1*0.5+2.25"`
  opened without a message and came back from the save verbatim, so the
  GUI neither rewrites nor rejects the point. The compiler keeps it.
  Integer constants are unaffected.
- **`AnalogMultiplexer2` selects 1-based.** Live: `Select=0` drove the
  output to 0, `1` passed `Input1`, `2` `Input2` — like the four-way
  `AnalogMultiplexer`, and unlike lox-sim's model (`0`→`Input1`,
  `1`→`Input2`). A digital flag on `Select` therefore switches between
  off and `Input1`. Not checkable from configs; recorded in
  [connector-db.md](connector-db.md).
- **A `VirtualIn` outside its range reads 0.** An analog virtual input
  with `MaxVal="1000"` fed air pressure (~1024 hPa) read 0 until
  `MaxVal` was raised. `MinVal=`/`MaxVal=` are attribute parameters
  (D54); set them to cover the values the input will receive.
- **LightController2 moods** (D58).
  - **Shape.** A `<LightscenesC Num= Outputs= FC=>` holds one
    `<LightsceneC Name= UUID= [Move="true"|ReadOnly="true"] Outputs= SID=
    CID= Q1=…Q<Outputs>>` per mood, in the app's list order. `Outputs` is
    18 on every observed controller. `<LSConfig OpM= ScMv= ScAc=>` names
    moods by `SID`.
  - **`Q<n>` belongs to circuit `AQ<n>`**, whose name is `NameAI<n-1>`.
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
  - **New moods.** The house's GUI-made moods take `SID` 1, 2, 3… and
    `CID` 12, 13…, i.e. the lowest free above the off mood's. lox-cli's
    "from `SID` 2 / `CID` 9" does not match that; the corpus also has
    `CID` 7 and 8. What `CID` means is unknown; Lox Logic only keeps it unique.
  - **Order.** The off mood's place varies: of 43 corpus controllers
    with moods, 16 keep it last, 2 first, 4 in the middle, and 21 have
    none at all (older configs).
  - **First save.** A controller's first Loxone Config save writes the
    stock moods and defaults, recorded in
    [oracle-wine.md](oracle-wine.md) (sessions 21–22).
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
