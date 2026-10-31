# Lox Logic language specification (v1)

Lox Logic, the text language, for managed Loxone logic. File
extension: **`.loxl`**. Encoding: UTF-8, no BOM. Line-oriented: one
statement per line; only a block declaration's `( … )` argument list spans
lines.

This document is normative for what `Module::parse` accepts and what
`Module::to_text` (= `loxl fmt`) emits.

v1 (2026-08-25) replaced v0's keyword statements (`block`/`wire`/`set`)
with constructor syntax — see design decision D16 in
[design.md](design.md). v0 keywords are reserved and produce migration
errors.

## Grammar

```ebnf
module     = { line } ;
line       = comment | let | extern | block-head | arg-line
           | wire | set | attr | removed | moved | template | instance
           | page | test | tiles | tiles-claim | access | schedule
           | schedule-claim | rule | blank ;

comment    = "#" any-text ;                        (* whole line *)
let        = "let" slug "=" ( number | string ) [ comment ] ;
extern     = "extern" slug "=" type "(" matcher
             { "," constraint ":" string } ")" [ comment ] ;
matcher    = ( "uuid" | "iname" | "title" ) ":" string
           | "mirrors" ":" slug ;
constraint = "room" | "category" ;  (* not with "uuid"/"mirrors"; each at most once *)

block-head = slug "=" type "(" [ args ] [ ")" ] [ comment ] ;
arg-line   = args [ ")" ] [ comment ] | comment ;  (* only inside an open call *)
args       = arg { "," arg } [ "," ] ;
arg        = string                                (* label; first argument only *)
           | port ":" binding ;
binding    = value                                 (* Def= parameter *)
           | slug "." port                         (* wire from that source *)
           | expr ;                                (* expression sugar (D26):
                                                      desugars into gate blocks *)

wire       = slug "." port "<-" ( slug "." port | expr ) [ comment ] ;
                                    (* extern target; an expression RHS
                                       desugars into gate blocks (D24) *)
set        = slug "." port "=" value [ comment ] ;            (* extern target *)
attr       = slug "." attr-name "=" ( string | number | const-ref ) [ comment ] ;
                                    (* extern target, top-level only (D39) *)
attr-name  = "title" | "room" | "category" ;      (* lowercase: never a port key *)

expr       = and-expr { "or" and-expr } ;
and-expr   = unary { "and" unary } ;
unary      = "not" unary | primary ;
primary    = comparison | arith | "(" expr ")" ;
comparison = operand cmp-op operand ;              (* no chaining *)
cmp-op     = ">=" | ">" | "<=" | "<" | "==" | "!=" ;
arith      = term { ("+" | "-") term } ;           (* standalone only (D35):
                                                      a whole RHS or binding *)
term       = factor { ("*" | "/") factor } ;
factor     = operand | "(" arith ")" ;
operand    = slug "." port | number [ unit ] | const-ref ;
removed    = "removed" slug [ comment ] ;
moved      = "moved" slug "->" slug [ comment ] ;
page       = "page" string [ "(" page_arg { "," page_arg } ")" ] [ comment ] ;
                                                  (* placement (D28), creation (D56) *)
page_arg   = ( "room" | "category" ) ":" string ;  (* room: required in the list *)

template   = "template" slug "(" [ params ] ")" [ comment ]
             { comment | block-head | arg-line | wire | set }
             "end" [ comment ] ;
params     = param { "," param } ;
param      = slug ":" ( type | "any" )             (* object parameter *)
           | slug "=" ( number [ unit ] | string ) ;   (* value parameter, default *)
instance   = slug "=" slug "(" [ inst-args ] ")" [ comment ] ;
inst-args  = param-name ":" ( slug | value ) { "," … } [ "," ] ;

test       = "test" string [ comment ]
             { comment | inject | tick | expect | clock }
             "end" [ comment ] ;
test-cmp   = "==" | ">=" | ">" | "<=" | "<" | "~=" ;
inject     = slug "." port "=" ( number [ unit ] | const-ref ) [ comment ] ;
tick       = "tick" integer [ "dt" ( number [ unit ] | const-ref ) ] [ comment ] ;
expect     = "expect" slug "." port test-cmp ( number [ unit ] | const-ref ) [ comment ] ;
clock      = "clock" string [ comment ] ;          (* "HH:MM[:SS]" or
                                                      "YYYY-MM-DD HH:MM[:SS]" *)

tiles      = "tiles" slug [ comment ]              (* Touch Pure Flex (D41) *)
             { comment | tile-head | arg-line }
             "end" [ comment ] ;
tile-head  = "tile" integer "(" [ tile-args ] [ ")" ] [ comment ] ;
tile-args  = tile-arg { "," tile-arg } [ "," ] ;
tile-arg   = ( fn | "text" | fn "_text" ) ":" ( string | number )
           | ( "to" | fn "_to" ) ":" slug ;       (* "to" may repeat *)
fn         = "click" | "up" | "down" ;
tiles-claim = slug "." "tiles" "=" slug [ comment ] ;
                                    (* extern TreeDevice, top-level only *)

access     = "access" slug [ comment ]             (* NFC Code Touch (D49) *)
             { comment | grant }
             "end" [ comment ] ;
grant      = ( "user" | "group" ) string ":" string [ comment ] ;
                                    (* principal title : schedule title *)
schedule   = "schedule" slug [ comment ]           (* DayTimer schedule (D53) *)
             { comment | sched-line }
             "end" [ comment ] ;
sched-line = day { day } ( "off" | interval { "," interval } ) [ comment ] ;
day        = "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun"
           | "holiday" | "vacation" | "dayoff"
           | "weekdays" | "weekend" | "daily"
           | "heating" | "cooling"
           | string ;                      (* any operating mode by title (D69) *)
interval   = time "-" time [ "=" number ] [ "activate" ] ;
time       = hour ":" minute ;                     (* 00:00 … 24:00 *)
schedule-claim = slug "." "schedule" "=" slug [ comment ] ;
                                    (* DayTimer, extern or managed; top-level only *)
rule       = "rule" slug [ comment ]               (* Automatic Designer rule (D57) *)
             { comment | rule-line }
             "end" [ comment ] ;
rule-line  = ( "active" | "inactive"
             | "when" "time" time                  (* 00:00 … 23:59 *)
             | "when" "weekday" rule-day { rule-day }
             | "when" slug "." ident cmp ( number | "on" | "off" )   (* D73 *)
             | "when" "mode" mode-name [ "on" | "off" ]             (* D73 *)
             | "or"                                (* starts another group, D73 *)
             | "then" slug string
             | "then" "mode" mode-name ( "on" | "off" ) ) [ comment ] ;
mode-name  = slug | string ;                       (* a Mode block, or a mode by title *)
rule-day   = "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun"
           | "weekdays" | "weekend" | "daily" ;

slug       = lowercase-letter { lowercase-letter | digit | "_" } ;
type       = uppercase-letter { letter | digit } ;          (* PascalCase *)
port       = letter-or-digit-or-underscore-sequence ;       (* as in the XML `K=` key *)
value      = number [ unit ] | string | const-ref ;
const-ref  = slug ;                                (* names a `let` constant *)
number     = [ "-" ] digits [ "." digits ] ;       (* exactly — `1.2.3`, `5.` are errors *)
unit       = "ms" | "s" | "min" | "h" | "K" | "%" ;   (* immediately adjacent (D27) *)
string     = '"' { character | escape } '"' ;
escape     = '\"' | "\\" | "\n" ;
```

Notes:

- A `#` outside a string starts a comment that runs to end of line. All
  comments are preserved by the formatter: whole-line comments are AST
  items, trailing comments attach to their statement (or argument line, or
  the closing `)`), and whole-line comments inside argument lists are
  argument items.
- An argument list may close on the declaration line (a single-line call)
  or span lines; nothing may follow the closing `)` except a comment.
- Whitespace is insignificant except as a token separator. Indentation is
  conventional (the formatter uses one tab inside argument lists).
- A bare identifier in value position is always a **constant reference**;
  it must name a `let` in the same module. A dotted identifier
  (`slug.Port`) is always a **port reference**. String values are always
  quoted.
- A number may carry a **unit suffix** (D27), written immediately
  adjacent: `40s`, `250ms`, `90min`, `1.5h`, `2700K`, `70%`. Time units
  scale exactly into the unit the port is documented in (D52): seconds
  by default (`1.5h` compiles to `Def="5400"`, byte-identical to writing
  `5400`), minutes on a minutes port (`AutoShadeTime: 2h` compiles to
  `120`; a value that is no exact decimal number of minutes, `10s`, is
  an error). `%` and `K` (color temperature) carry factor 1 and are
  accepted only on a port documented in that unit, or undocumented; a
  time suffix on a `%`/`K`/`°` port is an error. A suffixed `let`
  scales per use, into each port's unit. `loxl types <Type>` shows
  every port's documented unit. The suffix is part
  of the value's canonical spelling — `1.5h` stays `1.5h` in source and
  through `fmt`. An unknown suffix is a parse error. A *quoted* `"40s"`
  stays a string — two spellings, two meanings.
- The keywords `let`, `extern`, `removed`, `moved`, `template`, `end`,
  `page`, `test`, `tick`, `expect`, `clock`, `tiles`, `tile`, `schedule`, and the expression
  operators `and`, `or`, `not` are reserved, as are v0's `block`,
  `wire`, `set`, and `use` (migration errors) — none can be declared as
  a name. `access` is a keyword only where a line starts with `access
  <slug>`; elsewhere it stays an ordinary slug (`access = Switch()`).
  `rule` likewise, only in front of a slug.
- An instantiation is distinguished from a block declaration by the case
  of the callee: `sued = fassade(…)` (lowercase = template name) vs
  `hoch = GreaterEqual(…)` (PascalCase = block type).

## Statements

### `let` — a named constant

```text
let temp_schwelle = 28
```

Declares a name for a number or string. Any value position (argument
lists, extern port assignments) may reference it by bare identifier; the
compiler substitutes the literal before emitting `Def=`, so a `let`
reference compiles byte-identically to writing the literal in place.
Constants cannot reference other constants, and constant names share the
module's one slug namespace (they have no ports and cannot be wired).

### `extern` — reference an object owned by Loxone Config

```text
extern sonne = VirtualIn(iname: "VI3")
extern jal_sued = AutoJalousie(title: "Beschattung Süd")
extern boiler = Switch(uuid: "1d844a67-0333-5301-ffffed57184a04d2")
extern licht_buero = LightController2(title: "Deckenlicht", room: "Büro")
extern taster_flur = TreeSensor(title: "Taster 1", device: "B0A1B2C3")
extern status_alarm_ref = InputRef(mirrors: status_alarm)
```

Declares a slug for an existing object in the base config. The compiler
never creates, deletes, or moves externs; it only wires to their ports
(`<-`) and assigns their parameters (`=`).

Match semantics: the object must have the declared type **and** match the
spec. Exactly one object may match — zero is a `NoMatch` error, several is
an `AmbiguousMatch` error listing candidates. Where titles repeat per
room (every floor has a "Deckenlicht"), `room:` and/or `category:`
narrow an `iname`/`title` match: the object's `<IoData Pr=…>`/`Cr=…`
must point at a `Place`/`Category` with that title
([loxone-format.md](loxone-format.md)). Where identical hardware
repeats its I/O titles (every Tree push-button interface has a
"Taster 1"), `device: "<serial>"` narrows the match to the objects of
that device: the `Serial=` of the object's nearest enclosing element
that carries one — a Tree device, an extension, or for the
Miniserver's own I/Os and virtual folders the Miniserver (D55) — or
the object's own, for a device. An Air device's serial is the one the
Miniserver's `/data/status` lists (`50:4F:94:FF:FE:BA:1F:0E`, D71).
Serials compare without case and `:`. These constraints never combine with
`uuid:`, which pins exactly on its own. Once resolved, the UUID is
pinned in the lockfile; subsequent compiles use the pin (even if the title
has changed since) as long as an object with that UUID and type still
exists. Editing a `uuid:` value re-points the extern: the source names a
different identity, so it resolves afresh (D38) — whereas a changed
matcher *kind* must still match the pinned object (D32).

Choosing a matcher: `uuid` pins exactly; `iname` (the `IName=` attribute,
e.g. `VI1`, `AI3`) is locale-stable and preferred for built-in I/O
objects; `title` is human-friendly but locale-volatile — use it only for
objects you named yourself.

**`mirrors:` — match a ref by what it mirrors (D32).** For
`InputRef`/`OutputRef`/`OutputRefLM` externs only: `mirrors: <slug>` (a bare slug, no
quotes) matches the ref object whose `Ref=` attribute names the object
that slug resolves to — a managed block with a locked identity, or a
plain-matched extern of the module
([loxone-format.md](loxone-format.md) documents `Ref=`). Where several
refs mirror the same target, the file's `page` statement narrows the
candidates to the declaring page; a still-ambiguous match is refused —
keep `uuid:` for, say, two refs of one flag on the same page. Unlike the
other matchers, a pinned `mirrors:` re-confirms on every compile: the
claim "this ref mirrors X" must stay true, so a ref the GUI re-pointed
elsewhere stops the compile instead of being quietly tolerated.
`room:`/`category:`/`device:` never combine with it. `decompile`/`adopt` emit
`mirrors:` on their own where the target has a slug in the module and
the match is unique.

**Time functions and other singletons.** The periphery's time sources
(the GUI's *Zeitfunktionen* folder: `DayOfWeek`, `Time`, `Hour`,
`ImpulseMinute`, `ImpulseSunrise`, `StartPulse`, …) and the operating
modes (*Betriebsmodi*, type `Mode`) exist exactly once per project and
carry **no `IName=`**, so the matcher choice narrows: their titles are
locale-volatile like all built-ins (a save can rename `Wochentag` on
you), which leaves `uuid` as the durable pin:

```text
# Zeitfunktionen: singletons, no IName — pin by UUID
extern wochentag = DayOfWeek(uuid: "15ea0aa4-0093-39e4-ffffed57184a04d2")
extern minutenimpuls = ImpulseMinute(uuid: "15ea0aa4-01b6-3a20-ffffed57184a04d2")
extern montag = Mode(uuid: "00000000-0000-0004-1500000000000000")
```

Wiring them follows the normal rules — the singleton's output feeds a
managed block's input directly, no `InputRef` needed even across pages
(oracle-verified for `Mode` wires, sessions 6–7; the time sources are
the same class of periphery singleton). `title:` still *works* for a singleton
while the locale holds, and `adopt` lifts it that way; switching the
lifted spec to `uuid:` is a plain source edit (the lock pins the same
object either way). `Mode` UUIDs are deterministic system UUIDs
(`00000000-…`), identical across projects; time-function UUIDs are
minted per project — read them off `loxl decompile` or the XML.

### Block declaration — a managed logic block

```text
beschatten = And()
temp_hoch = GreaterEqual(
	"Temp über 28",
	Input1: aussentemp.Q,
	Input2: temp_schwelle,
)
```

The compiler owns this object end-to-end: it mints its UUIDs (pinned in
the lock), draws it on the target page, rebuilds it on every compile, and
deletes it when removal is explicitly authorized.

The argument list is the block's entire input situation in one place. The
value decides the meaning:

- `Port: 28`, `Port: "text"`, `Port: constant` — a **parameter**: the
  literal (or resolved `let` constant) becomes `Def=` on the port with key
  `Port`.
- `Port: slug.Port` — a **wire**: the referenced source port is wired into
  this port. Sources may be extern or managed ports (including the block
  itself — feedback is representable) and are referenced before or after
  their declaration.
- `Port: a.Q and b.AQ >= 28` — an **expression** (D26): desugars into
  gate/comparator blocks whose result is wired into the port — the same
  sugar as on `<-` (see [Expressions](#expressions--sugar-over-the-discrete-blocks)).
- An optional leading string is the display **label** (the XML `Title`);
  it defaults to the slug.

Some block types carry **attribute parameters** — block logic stored as an
element attribute rather than a connector. They bind exactly like
parameters but can never take a wire:

```text
summe = Formula("Summe", Formula: "I1+I2", Input1: a.Q, Input2: b.Q)
```

They are declared per type in `connectors::attr_params`. `Formula:` on
`Formula` blocks compiles to `Formula="I1+I2"` on the `<C>` element,
together with the observed `Valid="false"` companion.

`Code:` on `Code16` holds the PicoC program (D40). A program is hundreds
of lines, so its value is a **file**:

```text
hue = Code16("Philipps Hue Steuerung", Code: file("hue.c"), AI1: rgb.AQ)
```

- The path is relative to the `.loxl` file that names it. The file is
  read when the module loads; a missing file is an error that names the
  full path.
- CRLF line endings are normalized to LF, and the text is written the way
  Loxone Config writes it: raw line breaks and the five named entities,
  never `&#xA;` (see [loxone-format.md](loxone-format.md)).
- `file(…)` binds only program text (`connectors::file_params`); on any
  other parameter it is an error.
- The program is a **claim**. Without a `Code:` binding the block's
  program stays with Loxone Config and is carried verbatim from the base;
  with one, the file replaces it. Removing the binding hands the program
  back to the GUI. Decompile never writes `Code:`, so an adopted program
  block keeps its GUI-authored program until an author moves it into a
  file.
- `loxl diff` prints a program change as a line diff.

Rules:

- At most one parameter binding per port; multiple wire bindings on the
  same port are allowed when the sources differ (fan-in). A port takes a
  value *or* wires, never both: a wired input ignores its value, so
  `Input1: 2, Input1: x.AQ` is an error (D77). A constant Loxone Config
  keeps under a wire (it comes back when the wire is removed) is not
  written in source: decompile and adopt show it as a comment on the
  wire, `Input1: x.AQ, # stale constant 2 kept under the wire`, and the
  compile carries it from the base while the source keeps the port wired.
- The type must be in the verified builtin table
  (gates `And`/`Or`/`Not`, the comparator family `Equal`/`NotEqual`/
  `Greater`/`GreaterEqual`/`Less`/`LessEqual`, `Formula`, `Monoflop`,
  `PulseGen`, `AnalogThresholdTrigger`, … `loxl types` lists them — see
  [connector-db.md](connector-db.md)) — anything else is an error. See
  [design.md](design.md) "Refuse, never guess".
- `And`/`Or` are fixed two-input blocks (`I1`, `I2`, `Q`): binding `I3`+
  is an error — Loxone Config silently deletes grown inputs on save
  (design decision D8). Need more inputs? Cascade gates:
  `c = And(I1: a.Q, I2: b.Q)`.
- Types, port names, and wire directions on managed blocks are validated
  **statically** (`loxl check`, no base config needed); unknown names get
  a "did you mean" suggestion when a close candidate exists.
- One input port may be a wire source: the `AI` of an `OutputRef`
  mirroring an `ApiActor`, read by a block's API connector `OutputAPI`
  (on a type whose `OutputAPI` takes wires). It is the reverse half of
  an API link drawn through that mirror (D41;
  [loxone-format.md](loxone-format.md) records the shape):

  ```text
  flex_api_ost = OutputRef(mirrors: flex_api, AI: ost.OutputAPI)   # forward
  flex_api.I <- flex_api_ost.AQ                                    # distribution
  ost = AutoJalousie("Ost", OutputAPI: flex_api_ost.AI)            # reverse
  ```

  Write it this way where D34 cannot pick one of several mirrors of the
  actor on the page; with one mirror (or none) `OutputAPI: flex_api.I`
  is enough. Any other input as a source stays an error.

### Virtual inputs — minted into the "Virtual Inputs" folder (D54)

```text
party = VirtualIn("Partymodus")
soll = VirtualIn("Sollwert", Analog: "true", MinVal: 5, MaxVal: 30, DefVal: 21)
licht = And(I1: party.Q)
```

`VirtualIn` is a block type like any other, except where it lives: the
element is appended to the base's `<C Type="VirtualInCaption">` (the
folder Loxone Config calls "Virtual Inputs"), not drawn on a page. It
has no position, ignores `page` statements, and its `Q`/`Qm` outputs
wire directly into blocks on any page.

- `IName=` (`VI<n>`, the designation the GUI shows) is minted once,
  one past the highest number in use, and kept from then on.
- `Analog:` (`"true"`/`"false"`, default `"false"`), `MinVal:`,
  `MaxVal:`, `DefVal:` are attribute parameters. Each is a claim: written
  while bound, the GUI's value otherwise (like `Code:`). Make the range
  cover what the input will receive: a value outside it is reported to
  read 0 ([loxone-format.md](loxone-format.md)).
- The room/category binding (`<IoData>`) and display settings stay
  with Loxone Config.
- A base without the folder is a compile error. An existing virtual
  input is an extern (`extern x = VirtualIn(iname: "VI3")`) or, with
  `loxl adopt --uuid <it> --as <slug>`, a managed block (#198): rebuilt
  in its place in the folder, its settings left to the GUI until bound;
  an extern on it is promoted (`--as` its slug). A whole-config `adopt`
  leaves virtual inputs as externs, as it does devices.

### Virtual and Modbus devices — the object tree (D64)

```text
zaehler = VirtualHttpIn("Zähler", Address: "http://192.0.2.10/status", Poll: 60)
leistung = VirtualHttpInCmd("Leistung", device: zaehler, Check: "\"power\":\\v")
wp = ModbusServer("Wärmepumpe", Address: "192.0.2.12:502")
wp_regler = ModbusDev("Regler", device: wp)
vorlauf = ModbusASensor("Vorlauf", device: wp_regler, ModbusAddress: 1)
sollwert = ModbusAActor("Sollwert", device: wp_regler, ModbusAddress: 3, I: plus.AQ)
```

A device goes into its folder: VirtualHttpIn, VirtualUdpIn and
VirtualTextIn into "Virtual Inputs", VirtualOut into "Virtual Outputs",
ModbusServer into "Netzwerkperipherie". An I/O goes into the device
that `device:` names, by slug: a managed block or an extern of the
type that holds it (VirtualHttpInCmd → VirtualHttpIn, VirtualUdpInCmd →
VirtualUdpIn, VirtualOutCmd → VirtualOut, ModbusDev → ModbusServer, the
Modbus I/O → ModbusDev). Like VirtualIn, none of them has a position or
follows `page`.

- The settings are attribute parameters: `Address:`, `Poll:`, `Check:`,
  `ModbusAddress:`, `ModbusPollingCycle:` and the rest
  (`connectors::periphery` lists them per type). A new one is refused
  without the settings that make it work (`Address` and `Poll` for an
  HTTP input, `Check` for a command, `ModbusAddress` for a Modbus I/O).
- Strings are source strings: a backslash is written `\\`, so Loxone's
  `\v` placeholder in a command's check is `"\\v"`.
- `IName=` (VHI1, VCI3, AMQ12, …) is minted once and kept.
- `adopt --uuid <device>` takes the device and all I/O below it: each
  I/O gets the slug `<device slug>_<title>`, or keeps its slug when it
  was an extern (its statements move into the declaration, D62). An
  extern of an object the source manages is an error.
- `extern … (device: "<serial>")` is something else: an extern matched
  by the serial of a physical device.

### Operating modes — minted into the "Betriebsmodi" folder (D69)

```text
party = Mode("Party")
party.I <- partytaster.Q          # switch the mode from logic
licht = And(I1: party.Q)          # or read it
```

A user-defined operating mode is an object of the object tree, like a
virtual input: no position, no `page`. A fresh one gets the next
`Mode=` number below the lowest in use (`-1`, `-2`, …) and keeps it. It
can be named in schedules by its title. Existing modes — the built-in
ones and those made in Loxone Config — are referenced by title in a
schedule or as an extern (`extern montag = Mode(title: "Montag")`).
`adopt --uuid <mode> --as <slug>` takes over a user mode made in Loxone
Config: it keeps its uuid and `Mode=` number, and a new title is a
rename. A built-in mode (`Mode=` ≥ 0) stays an extern.

### Calendar entries — the *Betriebszeiten* that switch a mode on (D74)

```text
ferien = CalendarEntry("Sommerferien", CalType: 2, Mode: 3, From: 6393, To: 6456)
ostern = CalendarEntry("Ostermontag", Mode: 1, EasterOffset: 1)
silvester = CalendarEntry("Silvester", CalType: -8, Month: 12, Day: 31)
```

An entry of the *Betriebszeiten* calendar is an object of the object
tree, minted into the `CalendarCaption` folder. `CalType` is the
`Mode=` number of the mode it switches on (absent: 0, the holiday
mode); `Mode` says what kind of date it is:

| `Mode` | Kind | Attributes |
|---|---|---|
| absent | a fixed date, every year | `Month`, `Day` |
| `1` | relative to Easter Sunday | `EasterOffset` (days) |
| `3` | one period | `From`, `To` (days since 2009-01-01) |
| `4` | a yearly range | `Month`, `Day` to `Month2`, `Day2` |

Other kinds do not occur among the corpus's 397 entries; the attributes
are written as stated. A fresh entry carries the attributes Loxone
Config writes for an unset date (`Month`/`Day`/`Month2`/`Day2` 1,
`WeekDayInMonth="1"`). The entries Loxone Config made are taken over
with `adopt --uuid`; edits change only the attributes the source
states. Its `I` and `Q` are never wired in the corpus; the mode's `Q`
is the output to use.

### `<-` — wire onto an extern port

```text
jal_sued.AutoShade <- beschatten.Q
jal_sued.Safety <- wind_alarm.Q
```

Wires a source port onto an extern port. The source must be an output; the
extern port is open-world — it must merely exist in the base config. These
wires are recorded in the lockfile so removing the statement removes the
wire again without touching GUI-drawn wires.

`<-` is for **extern targets only**: a wire into a managed block is
written in that block's argument list — one canonical spelling per fact —
so `<-` on a managed slug is a validation error pointing at the argument
list.

### `=` on a port — write a parameter on an extern port

```text
jal_sued.TargetPos = 70
```

Rewrites the extern port's `Def=`; the original value is recorded in the
lockfile and **restored** when the statement disappears from source.

Like `<-`, this is for **extern targets only**. A managed block's
parameters belong in its argument list — assigning a managed port is a
validation error pointing there. Assigning a port reference
(`x.Port = y.Q`) is a parse error suggesting `<-`.

### `=` on a setting — the settings of a physical input or output (D65)

```text
extern garten = LoxAIRAsensor(uuid: "30000010-0000-0100-ffff504f94112233")

garten.Desc = "Bodenfühler im Beet"
garten.Unit = "<v.1> °C"
garten.MinTime = 600
```

On an extern of an extension's, Tree, Air or 1-Wire input or output
(LoxAIRsensor, LoxAIRAsensor, LoxAIRactor, LoxAIRAactor, TreeSensor,
TreeAsensor, TreeActor, TreeAactor, Actor, DigitalIn, VoltageIn,
Lox1wireAsensor), a setting name in place of a port claims that
setting: `Desc`, `Unit` (the display unit), `SourceValLow`,
`DestValLow`, `SourceValHigh`, `DestValHigh`, `MinVal`, `MaxVal`,
`MinChange`, `MinTime`, `ResetTime`, `AverageTime`, `MaxTime`. As with a
port, the original is kept in the lock and restored when the statement
leaves the source.

- `Desc`, the low ends of the correction, `MinVal`, `ResetTime` and
  `AverageTime` may be added to an object that has none (a save leaves
  them out at their default). The others are only rewritten: claiming
  one the object does not carry is an error.
- `Desc` is the name the app shows; setting it renames the control in
  the app.
- On any other type a setting name is an error.

An Air or Tree device (`LoxAIRDevice`, `TreeDevice`) has settings of its
own (D71): `Desc`, which may be added, and where it is installed —
`Installation`, `SwitchBoard`, `SwitchBoardRow`, `SwitchBoardPos` —
which are rewritten only (a Tree device has a board only when it is
mounted in one).

```text
extern relais = LoxAIRDevice(title: "Relais Jalousie", device: "50:4F:94:FF:FE:BA:1F:0E")
relais.Installation = "Fenster Ost"
relais.SwitchBoardPos = 3
```

### `(a, b, …).X = value` — one setting on several objects (D71)

```text
(garten, terrasse, pool).MinTime = 600
(garten, terrasse).room = "Garten"
```

The statement is the assignment on each listed object, in that order —
a port, a setting or `title`/`room`/`category`. `fmt` keeps it as one
line. A slug may be listed once; `.tiles`, `.schedule` and `.moods` take
one object.

### `.title` / `.room` / `.category` — assign an extern object (D39)

```text
extern klimasensor = LoxAIRDevice(uuid: "3000000e-0000-00e0-ffff504f94112233")
extern klima_temp = LoxAIRAsensor(uuid: "30000010-0000-0100-ffff504f94112233")

klimasensor.title = "Raumklima Garten"
klimasensor.room = "Garten"
klima_temp.category = "Klima"
```

Claims one attribute of an extern object — typically a device or one of
its inputs/outputs, which only Loxone Config can create:

- `.title` rewrites the object's `Title=`.
- `.room` / `.category` name a `Place` / `Category` **by title**; it must
  exist in the base exactly once (rooms and categories are created in
  Loxone Config). The compiler writes its UUID into the object's
  `<IoData Pr=…>` / `Cr=…`.

The attribute must already exist on the object — Lox Logic rewrites a
binding, it never adds one (an object without it is refused: set it once
in Loxone Config). Original and written value are recorded in the
lockfile (`attr_claims`) and **restored** when the statement disappears
(with a D31 tombstone until the base catches up).

**Followers.** A room or category claim also rewrites every descendant
(a device's inputs and outputs) whose binding equals the object's
original value — the children that were "in the device's room" move
with it, a child bound elsewhere stays. Managed blocks and descendants
with their own claim for the same attribute (and their subtrees) are
skipped. A follower changed in Loxone Config after the claim keeps the
GUI's value and leaves the follower list on the next compile.

A title claim's followers are the object's refs (D43): every
`InputRef`/`OutputRef` whose `Ref=` is the object takes the new title,
since Loxone Config derives a ref's title from its target on every save.
The inputs an alarm block collected into its sensor list (`WF` bit
0x1000 on an `InputRef`) keep theirs — the GUI owns those titles. A
minted ref (`mirrors:`) of a claimed extern carries the claimed title.

Rules:

- **Title: externs only.** A managed block's title is its label
  (`slug = Type("…", …)`); `.title` on it is a validation error naming
  the remedy. Its `.room` and `.category` are allowed (D63): the compile
  writes them on every rebuild, with no lock state and no followers.
- **Not what it is matched by.** `title:` matcher plus `.title`,
  `room:` plus `.room`, `category:` plus `.category` are refused: after
  the push the match would no longer find the object. Pin by `uuid:` or
  `iname:`.
- **One claim per object and attribute** — a duplicate statement is a
  validation error; two slugs that resolve to the same object and claim
  the same attribute are a compile error.
- **Text, not units.** The value is a string, number or `let` reference;
  a unit value is an error.
- **Top-level only** — not inside a `template` body.
- `x.room <- …` is a parse error: attributes are assigned, never wired.

### `removed` — authorize deleting a managed block

```text
removed temp_hoch
```

Declares that `temp_hoch`'s absence from source is intentional: the next
compile deletes the block from the config and moves its lockfile entry to
a removal tombstone (D31), which keeps deleting the object from bases that
predate the deployment of that compile's output. Scoped to exactly one
slug and reviewable in the diff — prefer it over the global
`--allow-removals` flag. The statement may be deleted right after the
compile that applies it (the tombstone carries the intent from there); it
is tolerated while the tombstone is pending, and once the removal has
reached the deployed config a lingering statement is a compile error.
Declaring a slug and `removed`-ing it in the same module is an error.

The same statement deletes a named rule (`rule <slug> "Name"`, D60) in
place of its declaration: the rule the lock pins is deleted with its
connectors, under the same tombstone (D67). A rule whose outputs feed a
block is refused until that wire is gone.

### `moved` — rename a managed block, keeping identity

```text
moved beschatten -> schatten_gate
```

Renames the lockfile entry so the block's identity — object *and* port
UUIDs, layout — survives a slug rename in source: rename the declaration
(and its reference sites), add the `moved` line, compile. Idempotent: once
the new slug carries the lock entry, the statement is done and can be
deleted. It is an error when neither slug is in the lockfile (typo guard),
when the old slug is still declared, or when moves are chained
(`a -> b`, `b -> c`).

### `page` — place the following blocks on a named page

```text
page "Beschattung"

wp_ein = And(
	I1: soll.AQ >= 1.5h,
)
```

Names the `<C Type="Page">` (by display title, as in the base config) that
the block declarations after it are built on — until the next `page`
statement. Blocks above the first `page` statement keep the default page
(`--page`, the project file, or the document's first page). Placement is
positional, so the synthetic blocks of an expression land on the page the
expression is written under.

The statement is **authoritative**: on every compile a governed block is
pinned to a base page with the declared title. A pin that still matches a
page so titled is kept — titles need not be unique, and adopted blocks
never move behind your back — any other pin is re-pinned to the first
matching page in document order (the block visibly moves in Loxone
Config), and a title no page carries is a compile error — unless the
statement says where to create it.

```text
page "Küche Logik" (room: "Küche")
page "Küche Logik" (room: "Küche", category: "Beleuchtung")
```

With `room:` a missing page is **created** (D56), as Loxone Config's
"Neue Seite" does: in the program, filed by title, assigned to the room
(a `Place` by title; Loxone Config does not accept the unassigned one)
and to `category:` (default: the unassigned category). A page that
already carries the title is used as is — the arguments only apply to
creation. Without `room:` a missing title stays an error, so a typo never
creates a page. Several statements for one page need the arguments only
once; given twice, they must agree.

The lockfile keeps a created page's UUID (`pages`): recompiles before the
push re-create the same page, and once the page is on the Miniserver it
is an ordinary page — rename it, move it, give it a room in Loxone
Config. Renaming a created page there while the statement still has
`room:` is an error: write the new title in the source (dropping the
arguments hands the page to Loxone Config). Removing the statement never
deletes the page.

`page` is not allowed inside a template body — placement belongs to the
module. The decompiler emits `page` statements as section headers (the
periphery, which is not a page, keeps a `# periphery` comment), so a
decompiled or adopted module records its placement in reviewable source.

### `template` — a reusable, parameterized body

```text
template fassade(jalousie: AutoJalousie, schwelle = 28, pos = 70)
	hoch = GreaterEqual(I2: schwelle)
	hoch.I1 <- temp_aussen.AQ
	beschatten = And(I1: hoch, I2: jalousie.Sd)
	jalousie.TargetPos = pos
end

sued = fassade(jalousie: jal_sued)
west = fassade(jalousie: jal_west, pos: 55)
```

A `template` declares a body of blocks, wires and sets once; each
instantiation stamps out an independent copy. The lowercase callee is what
distinguishes an instantiation from a block declaration: `sued =
fassade(…)` calls the template `fassade`, while `hoch = GreaterEqual(…)`
declares a `GreaterEqual` block (block types are PascalCase).

Parameters come in two forms. An **object parameter** (`jalousie:
AutoJalousie`) is required at every call site and passes a slug; the
annotation is checked against the argument's declared type when it is
known. `any` (`bewegung: any`) takes an object of every type — a
template over a sensor that may be a virtual input or a physical one;
the ports the body uses are checked after expansion, against the type
passed. A **value parameter** (`pos = 70`) carries a default and may be
overridden with a literal or a `let` reference. Free identifiers in the
body that are not parameters capture the module's surrounding names
(externs, lets, blocks), like any other reference.

Expansion is pure macro substitution before compile: body slug `hoch` in
instance `sued` becomes `sued_hoch`, and that **expanded slug is the
lockfile key**. Re-instantiating therefore never re-mints; editing the
body mints only what is new in each instance; `removed` and `moved` apply
per expanded slug (`removed sued_hoch`). Reference an instance's blocks by
their expanded names.

A call-site binding that names no template parameter forwards as a
**port binding** onto the body's single block — an instance is a
composite block, so its call reads exactly like a block declaration
with the shared parameters factored away:

```text
fenster = jalousie(
	titel: "Fenster Jalousie Bastian",
	zeit_hoch: 20,
	EndUp: touch_bastian_i2.Q,
	EndUp: touch_bastian_tuer_i2.Q,
	EndDown: touch_bastian_i5.Q,
)
```

Forwarded feeds may repeat, exactly as in a block's argument list, and
resolve in module scope (never against the template's parameters). The
expanded block re-validates them, so a typo surfaces as an unknown
port with the block's known ports listed. A template whose body
declares more than one block takes no forwards (a qualified form is
deferred with D23's other extensions).

A block title in the body may interpolate value parameters:
`"{titel} (scharf)"` substitutes the parameter's string at expansion, so
each instance carries its own app-visible title. The placeholder must
name a value parameter whose value is a string (a `let` reference to a
string constant also works); a brace-wrapped slug naming no value
parameter is an error (typo guard), and braces around anything not
slug-shaped pass through verbatim.

The template library (`templates/`, listed by `loxl templates`, D23)
ships tested templates for common automations; copy a file into the
module directory to call them.

The body may contain only block declarations, wires, sets and comments —
no nesting, no template-local `let`/`extern` (deferred, D23). `template`
and `end` are reserved words; `use` is reserved for the v0 migration
error.

### Expressions — sugar over the discrete blocks

```text
let schwelle = 28

jal_sued.AutoShade <- sonne.Q and aussentemp.Q >= schwelle

wp_ein = And(
	I1: wassertemp.AQ < wunschtemp and abdeckung_offen.Q,
	I2: pv_ueberschuss.AQ >= pv_schwelle,
)
```

The RHS of `<-` and the value of an argument binding (D26) may be a
boolean expression; it desugars — before compile, like template
expansion — into the verified discrete blocks:
`and`/`or` become `And`/`Or` (fixed 2-input, longer chains cascade
left-associatively), `not` becomes `Not`, and each comparison becomes
one comparator block (`>=` → `GreaterEqual`, `>` → `Greater`, `<=` →
`LessEqual`, `<` → `Less`, `==` → `Equal`, `!=` → `NotEqual`). Operand
order is preserved: the left side binds `Input1`, the right side
`Input2`; a port operand becomes a wire, a number or `let` reference
becomes the port's `Def=` parameter. Each generated block's label is
its sub-expression text, so the rule stays readable on the Loxone
Config canvas. A bare `slug.Port` stays a plain wire, a bare value a
plain parameter — parenthesizing one changes nothing (one canonical
spelling per fact).

Precedence, loosest to tightest: `or` < `and` < `not` < comparison <
`+ -` < `* /`; parentheses group. Comparisons take plain operands (a
port, a number, or a constant — at least one side a port) and do not
chain — write `a.AQ >= 5 and a.AQ < 10`, not `5 <= a.AQ < 10`. A
constant cannot drive a gate input directly; compare it. Gates take
boolean ports or sub-expressions.

**Arithmetic — one `Formula` block** (D35). `+ - * /` may form the
whole RHS of `<-` or a whole argument binding:

```text
verbrauch_kw.AI <- verbrauch.AQ / 1000
```

A maximal arithmetic tree desugars into ONE `Formula` block — here
`verbrauch_kw_ai__f1` with `Formula: "I1/1000"` and `Input1` wired from
`verbrauch.AQ`, the sink fed from the block's `AQ`. Distinct port
operands become `Input1`…`Input4` in first-appearance order (a repeated
port reuses its input; more than 4 distinct ports is an error — split
the expression). Numbers and numeric `let` references are inlined into
the compact formula text (negative constants parenthesized: `I1*(-2)`);
unit values (`40s`) and strings are rejected — formulas compute on
plain numbers. The block's label is the expression text, as with every
synthetic block, and identity works exactly like the discrete backend
(slug operator `f`, expression-owned lock entry). Arithmetic *under*
gates or comparisons (`a.Q and x.AQ + 1`, `x.AQ + 1 >= 5`) is deferred:
declare an explicit `Formula` block and use its `AQ`.

**Identity.** The generated blocks get synthetic slugs
`<sink>_<port>__<op><n>` (post-order walk, one counter per operator;
the sink is the extern port on `<-`, the managed block's port for an
argument binding) — here `jal_sued_autoshade__ge1`,
`jal_sued_autoshade__and1`, and `wp_ein_i1__lt1` — and are keyed in the
lockfile like hand-written blocks, but marked expression-owned. An unchanged expression therefore never re-mints.
Editing the expression re-derives its slugs, and the compiler
auto-removes the orphaned expression-owned entries — no `removed`
statement needed: the expression is the blocks' single source of
truth. Declaring a slug that collides with an expression's synthetic
namespace is an error. Inside a template body, desugaring runs after
expansion, so the synthetic prefix uses the instance's actual sink.

### `test` — a simulated scenario (D36, D68)

```text
test "Frostwächter schlägt an"
	wassertemp.Q = 2        # inject: override that output for the whole test
	tick 2                  # advance the simulation two steps (default dt 1s)
	expect frost_alarm.Q == 1
	wassertemp.Q = 10
	tick 2 dt 0.5           # per-step dt override, unit values allowed
	expect frost_alarm.Q == 0
end
```

A `test` block is a top-level item (not allowed inside templates) and
never affects compilation — `loxl compile` ignores tests entirely.
`loxl test` runs them: it compiles the module in memory against the
lockfile and runs each test in the Lox Logic simulator, from a fresh start
(every value 0, the clock at 2026-01-01 00:00, a Thursday). The
simulator models the blocks Loxone documents well enough —
[simulator.md](simulator.md) lists them with their sources. A test whose
expectation depends on any other block reports `ERROR` and names it:
set a port between that block and the expectation to cut it out.

Statements, in file order:

- **Injection** `slug.Port = value` — sets that signal for the rest of
  the test (a persistent override).
  Any declared extern or block port can be driven, including outputs
  (that is the normal case: drive a sensor's `Q`/`AQ`). Values are
  numbers (with units) or `let` references; strings are rejected.
- **`tick <n> [dt <v>]`** — advance the simulation `n` program cycles
  of `dt` seconds (default 1). Injections since the previous `tick`
  take effect for these cycles.
- **`expect slug.Port <cmp> <value>`** — assert after the preceding
  `tick`. Comparators: `==`, `>`, `>=`, `<`, `<=`, and `~=`
  (approximately, 5 % tolerance); `==` allows the rounding of decimal
  arithmetic. There is no `!=`. An `expect` before any `tick` is an
  error, as is a trailing injection no `tick` ever applies.
- **`clock "HH:MM[:SS]"` / `clock "YYYY-MM-DD HH:MM[:SS]"`** — set the
  simulated wall clock before the following `tick`, for time-dependent
  logic: the weekday operating modes and the `DayTimer`s follow it.

References resolve on the **flattened** module (templates expanded,
expressions desugared), so tests can name template-produced slugs
(`frost_alarm`) and expression-synthetic slugs
(`pv_kw_input__f1.AQ`) — exactly what the simulator sees. Each slug
resolves through the lockfile to its object's UUID.

`loxl test --filter <substring>` runs a subset. `--lox <path>` runs the
tests in [lox-cli](https://github.com/eisber/lox-cli)'s `lox sim`
instead, which addresses signals by object **title**: Lox Logic derives the
titles from the lockfile and refuses ambiguity unless the duplicates
are same-signal mirrors or a room qualifier separates them;
`--show-spec` prints the scenario JSON it would run.

### `tiles` — program the tiles of a Touch Pure Flex (D41)

```text
extern flex_kueche = TreeDevice(uuid: "40000001-0000-0010-ffff504f94112233")
extern jal_nord = AutoJalousie(uuid: "40000010-0000-0100-ffff504f94112233")
extern jal_sued = AutoJalousie(uuid: "40000011-0000-0110-ffff504f94112233")

tiles kueche
	tile 2()                                     # clear: the tile does nothing
	tile 3(
		click: "SET(As;Tg;pulse)",
		up: "SET(As;Co;pulse)",
		down: "SET(As;Cc;pulse)",
		text: "=ECHO(As)",
		to: jal_nord,
		to: jal_sued,
		up_to: jal_nord,                         # the up arrow opens Nord only
	)
end

flex_kueche.tiles = kueche
```

A `tiles` set names tile programs; a claim `<device>.tiles = <set>`
writes them onto one Touch Pure Flex, and one set may be claimed by
several devices. The device is an extern `TreeDevice` — pin it by
`uuid:` (titles of tree devices are free text). A tile is addressed by
its index `i` in the config: Lox Logic rewrites the three functions of tiles
that exist and carries every unlisted tile unchanged. The layout — which
tiles exist, their position, size and icon — stays with Loxone Config.

Keys of a `tile`:

- **`click:`, `up:`, `down:`** — the command (`api=`) of the click and of
  the up/down arrows, a quoted string written as Loxone Config shows it
  (`SET(As;Co;pulse)`, `SET(Lico;M+;pulse)&WAIT(500)&MENU(Lico;Mood)`).
  A function without its key has no command; `tile <i>()` clears all
  three.
- **`text:`** — the status text (`cT=`, e.g. `"=ECHO(As)"`) of every
  function that has a command.
- **`to: <slug>`** — a block the commands address (`ctrls=`); repeat it
  for several. It applies to every function that has a command. Without
  `to:` a command reaches every block of its type linked to the device
  (see *lint* below).
- **`click_text:`, `up_text:`, `down_text:`, `click_to:`, `up_to:`,
  `down_to:`** — replace `text:`/`to:` for one function. `up_text: ""`
  drops the text there.

A `to:` target is an extern or a managed block, by its flattened name
(`<instance>_<block>` for a template body block). It must be
**API-linked** to the device: its `OutputAPI` wired to the device's API
connector (`ApiActor.I`), or to the `AI` of an `OutputRef` mirroring it
whose `AQ` feeds `ApiActor.I` — in source (`OutputAPI: flex_api.I`, or
an explicit mirror as under *Block declaration*) or in
Loxone Config. A command whose abbreviations Lox Logic knows (`As` =
AutoJalousie, `Lico` = LightController2) refuses a target of another
type. Other abbreviations are written unchecked.

Rules:

- **Extern `TreeDevice` only**, with a `<btns>` tile layout; a missing
  tile index is a compile error naming the indices that exist.
- **One set per device** — a duplicate claim is a validation error; two
  externs resolving to the same device are a compile error.
- **Strings, not constants.** Commands and texts are quoted strings.
- **No restore.** Like `Code:` (D40) a claim keeps no lockfile state:
  removing it leaves the last written programs in place.
- **Top-level only** — neither the set nor the claim inside a template.
- `x.tiles <- …` is a parse error.

`loxl lint` (in a project) checks the tiles of *every* Touch Pure Flex
in the compiled config, claimed or not: `tile-broadcast` when a command
without targets reaches two or more linked blocks of its type,
`tile-dead-command` when no linked block of its type receives it.

### `access` — who may open an NFC Code Touch (D49)

```text
extern front_door = NfcCodeTouch(uuid: "50000009-0000-0090-ffff504f94112233")

access front_door
	user "alice": "Always"
	user "bob": "Workdays"                       # an access schedule
	group "Residents": "Always"
end
```

An `access` declaration owns the access rights (*Zutritt*) on one NFC
Code Touch: every user and group it lists may open it under the access
schedule named after the colon, and nobody else. Compiling it grants
what it lists, moves a right to another schedule, and revokes the rights
of users and groups it leaves out; an empty declaration revokes them
all. Code Touches without a declaration keep their rights as they are.

Users, groups and access schedules (`SwitchingTimer`) stay with Loxone
Config and the app. The declaration names them by title, exactly as
written there; a title that names nothing, or more than one object of
its kind, is a compile error. Their credentials (NFC tags, access codes,
passwords) and memberships are not part of the language.

Rules:

- **Extern `NfcCodeTouch` only**, one declaration per extern; two
  externs resolving to the same block are a compile error.
- **Each principal once** — a user or group listed twice is a
  validation error.
- **Access, not operation.** A right's `ACT=` (operating the block in
  the app) is not an access right; compiling keeps it, and revoking
  access leaves it.
- **No restore.** Like `tiles` (D41) a declaration keeps no lockfile
  state: removing it leaves the last written rights in place.
- **Top-level only** — not inside a template.

`loxl decompile` and `loxl adopt` lift the access rights of every Code
Touch that has any into a declaration, with the extern it needs. When a
right names a user or group without a unique title, the Code Touch gets
a `# access <slug>: not lifted — …` comment instead: a declaration
without that right would revoke it.

`loxl lint` (in a project) reports `no-access-right` for a user with an
NFC tag or access code but no access right on any Code Touch, neither
directly nor through a group: the tag opens nothing. Membership of the
*Full access* group does not count, since it grants no access right.

### `schedule` — the switching times of a DayTimer (D53)

```
schedule abendlicht                 # living-room evening light
	weekdays 06:00-07:30, 17:00-22:30
	weekend 08:00-23:00
	holiday vacation off
end

schaltuhr.schedule = abendlicht
```

A `schedule` names a weekly program; a claim
`<timer>.schedule = <name>` writes it onto one DayTimer — an extern or a
managed `DayTimer` block — and one schedule may be claimed by several
timers. Each line lists days, then the intervals the timer is on:

- **Days** are the built-in operating modes: `mon` … `sun`, `holiday`
  (Feiertag), `vacation` (Urlaub), `dayoff` (Freier Tag/Schulferien),
  `heating` (Heizperiode), `cooling` (Kühlperiode), and the groups
  `weekdays` (mon–fri), `weekend` (sat, sun), `daily` (mon–sun). Any
  operating mode of the config, a user-defined one included, is named
  by its title in quotes: `"Party" 18:00-24:00` (D69). The title must
  name exactly one mode of the compiled config (minted ones included).
  A day may appear on several lines; its intervals merge.
  The timer always has a column for each of the seven weekdays — a
  weekday the schedule does not name is **off all day** — plus one for
  each other mode the schedule names (one it does not name gets no
  column). Columns are written in the modes' `Mode=` order: user-defined
  modes, holiday, vacation, day off, Monday … Sunday, heating, cooling.
- **Intervals** are `HH:MM-HH:MM`, minute precision, `24:00` for
  midnight at the end of the day. Per day they must not overlap
  (touching is fine); an interval cannot wrap past midnight — split it.
- **`= <value>`** sets the output value of an *analog* timer
  (`Analog` ticked in Loxone Config) and is required there; a digital
  timer takes no value.
- **`activate`** marks an interval that needs activation (Loxone
  Config's *Aktivierung erforderlich*): the output switches on only
  after a pulse on the timer's activation input (`Act`) inside the
  interval.
- **`off`** names days with no on-interval.

The claim replaces the timer's whole schedule (`Modes=`, `UserModes=`,
`N=` and its `<Entry>` list); everything else on the timer — analog mode, default
value, room, visualization — stays with Loxone Config. Like a tiles
claim it keeps no lock state: removing it leaves the last written
schedule in place, editable in the app again. Only one schedule per
timer; two externs resolving to the same timer are a compile error.
Top-level only.

`loxl diff` compares the schedule of every DayTimer, claimed or not, per
mode column and in these terms: the columns that changed alike are
grouped under the day words (`DayTimer "Schaltuhr" schedule daily:
00:00-06:00 activate, 21:00-24:00 activate -> 00:00-06:00 activate,
22:00-24:00 activate`), a user mode by its title, a column one side
lacks as none. The semantic fingerprint includes the schedule, so
`loxl drift` reports one changed in Loxone Config or the app.

### `rule` — an Automatic Designer rule (D57)

```text
extern pumpe = PushButton(title: "Pumpe")
extern pumpe_ein = AutopilotRule(title: "Pumpe Ein")

rule pumpe_ein                      # weekday mornings
	active
	when weekday weekdays
	when time 09:00
	then pumpe "on"
end
```

A `rule` states what an existing rule of the app's Automatic Designer
does: whether it is `active`, its conditions (all must hold) and its
actions, in order. The slug is an `AutopilotRule` extern — rules are
created, named and deleted in the app; the declaration edits one.

- **`when time HH:MM`** — at that time of day.
- **`when weekday <days>`** — on those days (`mon` … `sun`, `weekdays`,
  `weekend`, `daily`; no day twice).
- **`when <slug>.<state> <op> <value>`** (D73) — a state of a block
  the app shows: `==`, `!=`, `>`, `<`, `>=`, `<=` against a number, or
  `on`/`off` (1/0) for a digital state (`when pumpe.active == on`).
  The state names and condition codes are the app's own: the
  capability table the Miniserver serves the app
  (`loxone_config::autopilot_caps`), per app control type.
- **`when mode <mode> [on|off]`** (D73) — while an operating mode is
  (not) active; `<mode>` is a `Mode` block's slug or a mode's title
  (`when mode "Party"`).
- **`or`** (D73) — the conditions after it form another group: the
  rule fires when all conditions of any one group hold.
- **`then <slug> "<command>"`** — the app's command text to a block:
  any command the capability table offers for the block's app type
  (`on`/`off` to a `PushButton`, `pulse` to a `PButtonT`, `<valN>`
  numbers filled in), or one some rule of the config already sends (the
  compile copies that action's code). Anything else is refused, with
  the commands the block offers.
- **`then mode <mode> on|off`** (D73) — activates or deactivates an
  operating mode.
- **`when weather <field> <op> <value>`** (D76) — a value of the
  weather service: `temperature`, `dewpoint`, `felt_temperature`,
  `wind`, `gusts`, `wind_direction`, `irradiance`,
  `relative_irradiance`, `solar_irradiance`, `precipitation`,
  `pressure` with the operators of a state, `sunshine == on|off`.
- **`when day <op> <1…31>`** (D76) — the day of the month.
- **`when daylight [off]`** (D76) — while it is (not) day.
- **`when sunrise`**, **`sunset`**, **`dawn`**, **`dusk`** (D76) — at
  that moment of the day.
- **`when date "YYYY-MM-DD"`** (D76) — on that day. The app stores the
  local midnight as a Unix time, so the config must state a Central
  European `Timezone` (`Europe/Vienna`, `Europe/Berlin`, …).
- **`then notify "<subject>" "<text>"`** (D76) — a push notification
  to the app's users.
- **`then mail "<to>" "<subject>" "<text>"`** (D76) — a mail through
  the Loxone Cloud Mailer.
- **`then wait <duration>`** (D76) — a pause before the next action:
  `500ms`, `2.5s`, `5min`, `1h` (0.1 s … 24 h).
- **`then message <severity> "<subject>" "<text>" [admins]`** (D76) —
  an entry in the message center (`info`, `warning`, `error`; `admins`:
  only administrators see it). A `/` in subject or text is refused: the
  command carries them `/`-separated.

Compiling writes the rule's `APC=` payload only when the declaration
differs from the rule as read, and then as the app's edit: unchanged
conditions and actions keep their JSON, `STA` follows `active`.
`decompile`/`adopt` lift every rule of these shapes into a declaration,
by its name (below) when no other rule has that name, else on an extern;
the blocks it watches or drives become externs. A rule with other
conditions or actions (text values, the weather type, calls, speech
output) stays a comment and must not be declared. `loxl adopt --uuid
<rule> --as <slug>` takes one rule into an existing project the same
way: an extern rule of the module (`--as` its slug) loses its `extern`
line and unnamed `rule`, and the named declaration takes their place;
its action targets must be in the lock already. Removing a
declaration leaves the rule as last written. Top-level only.

**Named rules (D60).** `rule <slug> "Name"` declares a rule by its name
instead of an extern — the slug is the declaration's own:

```text
rule pumpe_mittag "Pumpe Mittag"     # created if the config lacks it
	active
	when time 12:30
	then pumpe "on"
end
```

The compile takes the config's rule of that name (a name two rules
share is refused), or creates it when there is none: inactive and empty
until the body is written, placed below the config's last rule on its
page and filed like it. A config without any rule is refused (create
one in the app once — where rules live is the app's choice). The lock
pins the rule's UUID (`rules`), so a compile against a base from before
the deployment creates it again under the same UUID, the source owns
the name (changing it renames the rule), and `loxl rename` carries the
pin. Removing the declaration leaves the rule as last written; Lox Logic
never deletes one. A slug declared otherwise (an extern, a block) is
refused, as is a named rule that is also some `rule <extern>`'s.

### `moods` — the moods of a lighting controller (D58)

```text
moods wohnen                        # one set, any number of controllers
	"Viel Licht": all on, AQ4 rgb(100, 100, 100)
	"Lesen": AQ1 80%, AQ3 temp(60%, 2700K)
	"Kino": AQ4 rgb(0, 0, 20)
end

wohnzimmer_lc.moods = wohnen
esszimmer_lc.moods = wohnen
```

A `moods` set names a list of moods; a claim
`<controller>.moods = <name>` makes it the mood list of one
LightController2 (Lichtsteuerung) — an extern or a managed block — and
one set may be claimed by any number of controllers. Each line is a
mood: its name, then the values of its circuits (`AQ1` … `AQ18`, Loxone
Config's *Lichtkreise*):

- **`all <value>`**, first if given, sets every circuit it fits; a
  circuit it does not fit (`all on` on a colour circuit) stays off.
- **`AQ<n> <value>`** sets one circuit and wins over `all`. A circuit
  the mood does not mention is off.
- **Values** follow the circuit's kind, which the controller's existing
  moods tell: `on`/`off` and a percentage (`40%`) on a switch or dimmer
  circuit, `rgb(r, g, b)` (each 0–100, percent) on a colour circuit,
  `temp(<brightness>%, <kelvin>K)` on a tunable-white one, `off`
  everywhere. A value the circuit cannot take is a compile error naming
  what it takes. A bare number is written as stored, for the encodings
  Lox Logic does not spell.

The claim writes the controller's moods in the set's order, plus its
off mood ("Aus"), which every controller keeps, source never lists, and
which stays where it is (first stays first, last stays last with new
moods before it, anywhere else keeps its index). A mood keeps its identity by
**name**: one the controller already has keeps its UUID, scene id and
flags and only gets the set's values, so the buttons and the app keep
addressing it; a new one gets the next free scene id, the way Loxone
Config numbers them; one the set leaves out is deleted — except the
built-in "Viel Licht" (the motion mood) and any mood the controller's
motion settings name, which must stay in the set (with values of your
choice). A claim whose moods already match changes nothing, byte for
byte. Moods select through the controller's `Sel1`…`Sel8` inputs in list
order, so wiring a switch's buttons to them is ordinary source.

A controller minted from source has no moods until Loxone Config saves
it; a claim on it first writes what that save would — the stock "Viel
Licht" and "Aus" and their defaults (oracle-verified) — then the set.
The lock pins the UUID of every mood the compiler minted until a base
carries it ([lockfile-spec.md](lockfile-spec.md)). Removing a claim
leaves the moods as last written. One claim per controller; claims are
allowed in a template body, so a room template can give every room the
same moods: `<lc>.moods = <param>` with a string value parameter
naming the set (`template raum(stimmungen = "standard")`, called as
`raum(stimmungen: "kueche")`).

`loxl decompile` and `loxl adopt` lift a controller's own moods into a
set and a claim (#170), with every such controller as an extern;
controllers with the same moods share one set, named after the first
(`<slug>_moods`). A controller with only the stock "Viel Licht" (every
circuit on) is left out — there is nothing to own — and one whose moods
cannot be written as a set (two moods of the same name) keeps them in
Loxone Config, with a `# moods <slug>: not lifted — …` comment saying
why. Each mood is written the way it compiles back exactly: `all <v>`
for the value more than half the circuits share, then the circuits that
differ. Adopted and compiled, a config comes back byte-identical.
`loxl adopt --uuid` (one block) appends no mood claims.

`loxl diff` compares the moods of every LightController2, claimed or
not, one change per mood in these terms (`LightController2 "Licht"
mood "Lesen": all on -> AQ1 80%`) plus the order of the names; the
semantic fingerprint (v7) includes them, so `loxl drift` reports moods
edited in Loxone Config or the app.

### Push notifications: `OutputRefLM` and `recipients` (D51)

```text
extern frost = Notification(title: "Frostwarnung Garten")

warn = OutputRefLM(
	mirrors: frost,
	AI: zu_kalt.Q,
	On: "Frost im Garten",
	Off: "Kein Frost mehr",                      # optional
	MinimumTime: 12h,                            # optional, default 5 s
)

recipients frost
	user "alice"
	group "Familie"
end
```

A push notification (*Benachrichtigung*, type `Notification`) is
periphery: Loxone Config creates it under *Messages*, and the module
names it as an extern. Two things about it come from source:

- **What it says, and when.** An `OutputRefLM` block is the notification
  drawn on a page — a minted mirror (`mirrors:` names the extern
  `Notification`) with the message parameters Config keeps in its
  `<LoggerMailer>` child: `On:` is sent when `AI` turns on, `Off:` when it
  turns off (no text, no message), and `MinimumTime:` is the least time
  between two messages, a duration (`12h`, `30m`, or seconds as a number)
  that compiles to milliseconds. The parameters take values, never wires.
  The block's title follows the notification's, like every ref's.
- **Who gets it.** A `recipients` declaration owns the recipients of one
  extern `Notification`: every user and group it lists gets the message,
  and nobody else. It works like `access`, without schedules: compiling
  grants what it lists (`ACT=` on the notification), and revokes the
  users and groups it leaves out. Notifications without a declaration
  keep their recipients. Members of the *Full access* group get every
  notification whatever the declaration says; Loxone Config shows them
  as inherited recipients.

Rules for `recipients` are those of `access` — extern of the right type
(here `Notification`), one declaration per extern, each principal once,
titles that name exactly one user or group, no lock state, top-level
only — and a recipient takes no schedule. Decompile does not lift
recipients, and leaves `OutputRefLM` blocks with the GUI.

## Name resolution and validation

- Externs, blocks, `let` constants, templates, `tiles` sets and
  schedules share one namespace per module; duplicates are an error. An
  `access` or `recipients` declaration names an extern and declares
  nothing.
- Every port reference (argument bindings, `<-`, `=`) must name a declared
  extern or block; every bare-identifier value must name a declared `let`.
- `<-`, port-`=` and attribute-`=` targets must be externs (see above).
- Duplicate parameter bindings, duplicate identical wire bindings, and a
  parameter next to a wire on the same port, in one argument list are
  errors.
- `removed`/`moved` must not contradict declarations or each other
  (details under the statements).
- Statement order is free; the conventional order is lets, externs,
  blocks, extern wires, extern assignments (port and attribute), with
  lifecycle statements (`moved`, `removed`) last.

`loxl check` performs all of the above plus the static builtin-table
checks (block types, port names, wire directions) — everything that does
not need the base config. `--json` emits the result machine-readably.

## Multi-file modules and projects

A module may be a directory of `*.loxl` fragments instead of one file.
Fragments parse individually (errors name the file), then concatenate —
subdirectories included, in path order — into one module; name resolution
runs once on the whole, so a fragment may freely reference slugs declared
in a sibling file. The split carries no semantics: one namespace, one
lockfile, and **no `import` statement** (decision D25 — an import would
name a dependency that has no semantic consequence). It is source
ergonomics, with one file per page as the convention (`_periphery.loxl`
for the externs). Dot-entries (`.git`, editor caches) are skipped. One
statement is positional across the merge: a `page` statement (D28)
governs following blocks until the next one, fragment boundaries
included — so open every fragment that declares blocks with its `page`
statement (the decompiler and adopt do). Beyond that, merge order
affects nothing but determinism.

A directory with a `lox.toml` is a **project** — one deployment target:

```toml
base = "r50.Loxone"      # the deployed config (required)
module = "pages"         # module file or fragment directory (required)
lock = "r50.lock.json"   # default logic.lock.json
out = "out.Loxone"       # default out.Loxone
serial = "504F94A26236"  # optional; the first compile records it in the lock
page = "loxl"            # optional page title for newly placed blocks
```

The format is a strict flat subset of TOML — `key = "string"` pairs and
`#` comments; tables, arrays, and unquoted values are refused with
pointed errors. Paths are relative to the file. `module` deliberately has
no default: a stray `.loxl` view or backup next to the file must never be
compiled by accident. Inside the directory, `loxl compile` needs no flags
(flags override the file), and `check`/`fmt`/`drift` default to the
project's module and lock.

## Canonical form

`loxl fmt` emits: statements in source order, single spaces between
tokens, one blank line whenever the item kind changes and after every
multi-line block declaration. An argument-free call stays on one line
(`b = And()`, `c = Or("Oder")`); a call with bindings puts the label and
each argument on its own tab-indented line with a trailing comma, and the
`)` on its own line. Values keep their variant: numbers bare, strings
quoted, constant references bare, port references dotted, unit values
with their suffix as written. The one value
canonicalization happens at parse time: a quoted string that reads exactly
as a number (`"28"`) becomes the bare number, so every value has one
canonical spelling. A `tile` follows the same rule one level deeper:
`tile 2()` on one line, a tile with keys as a two-tab-indented list
with the `)` on its own tab-indented line. An `access` declaration puts
each grant on its own tab-indented line. `parse(to_text(m)) == m`, and
`to_text` is a fixpoint.

## Errors

Parse errors carry a 1-based line number. Unknown types, ports, and
constants suggest the closest known name when one is within a small edit
distance. v0 statements (`block`, `wire`, `set`, `extern … match …`) get
migration errors describing the v1 spelling. Compile errors are documented
with remedies in [agents.md](agents.md#errors-and-remedies).

## Versioning

This is v1; there are no v0 files in the wild (pre-release revision).
Anything not specified here (template nesting and template-local
declarations, the `formula` expression backend)
is future work —
see [roadmap.md](roadmap.md). Future versions will keep v1 files parsing
unchanged or provide a migration tool.
