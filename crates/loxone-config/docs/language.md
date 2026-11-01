# Lox Logic language specification (v2)

Lox Logic is the text language for managed Loxone logic.

- File extension `.loxl`, UTF-8 without BOM.
- One statement per line. Only a block declaration's `( … )` argument list spans lines.
- This spec defines what `Module::parse` accepts and what `Module::to_text` (`loxl fmt`) writes.
- This is v2, where every value has one spelling ([Changes from v1](#changes-from-v1)). The v0
  keywords `block`, `wire` and `set` are reserved and give migration errors.

## Grammar

```ebnf
module     = { line } ;
line       = comment | let | extern | block-head | arg-line | signal
           | wire | set | attr | removed | moved | template | instance
           | bulk | page | test | tiles | tiles-claim | access | access-claim
           | recipients | recipients-claim | schedule | schedule-claim
           | moods | moods-claim | rule | defaults | blank ;

comment    = "#" any-text ;                        (* whole line *)
allow      = "#" "lint:" "allow" kind { [ "," ] kind } [ any-text ] ;
             (* a comment; the line right above a declaration *)
let        = "let" slug "=" ( number [ unit ] | string | bool ) [ comment ] ;
extern     = "extern" slug "=" type "(" matcher
             { "," constraint ":" string } ")" [ comment ] ;
matcher    = ( "uuid" | "iname" | "title" ) ":" string
           | "mirrors" ":" ref ;
constraint = "room" | "category" ;  (* not with "uuid"/"mirrors"; each at most once *)

block-head = slug "=" type "(" [ args ] [ ")" ] [ comment ] ;
defaults   = "defaults" type "(" [ def-args ] [ ")" ] [ comment ] ;
                                    (* top-level only; one per type; the
                                       argument list may span lines *)
def-args   = port ":" value { "," port ":" value } [ "," ] ;
arg-line   = args [ ")" ] [ comment ] | comment ;  (* only inside an open call *)
args       = arg { "," arg } [ "," ] ;
arg        = string                                (* label; first argument only *)
           | port ":" binding ;
binding    = value                                 (* Def= parameter *)
           | ref "." port                          (* wire from that source *)
           | expr ;                                (* expression sugar:
                                                      desugars into gate blocks *)

signal     = slug "=" expr [ comment ] ;            (* named signal: not a
                                                      bare operand; the top
                                                      block is `slug` *)
wire       = slug "." port "<-" ( ref "." port | expr ) [ comment ] ;
                                    (* extern target; an expression RHS
                                       desugars into gate blocks *)
set        = slug "." port "=" value [ comment ] ;            (* extern target *)
attr       = slug "." attr-name "=" ( string | number | const-ref ) [ comment ] ;
                                    (* extern target, top-level only; or
                                       `.title` on a named signal *)
attr-name  = "title" | "room" | "category" ;      (* lowercase: never a port key *)
bulk       = "(" ref { "," ref } ")" "." ( port | attr-name ) "=" value [ comment ] ;
                                    (* each ref once; one line *)

expr       = and-expr { "or" and-expr } ;
and-expr   = unary { "and" unary } ;
unary      = "not" unary | primary ;
primary    = comparison | arith | "(" expr ")" ;
comparison = operand cmp-op operand ;              (* no chaining *)
cmp-op     = ">=" | ">" | "<=" | "<" | "==" | "!=" ;
arith      = term { ("+" | "-") term } ;           (* standalone only:
                                                      a whole RHS or binding *)
term       = factor { ("*" | "/") factor } ;
factor     = operand | "(" arith ")" ;
operand    = ref "." port | number [ unit ] | bool | const-ref ;
removed    = "removed" ref [ comment ] ;
moved      = "moved" ref "->" ref [ comment ] ;
page       = "page" string [ "(" page_arg { "," page_arg } ")" ] [ comment ] ;
                                                  (* placement, creation *)
page_arg   = ( "room" | "category" ) ":" string ;  (* room: required in the list *)

template   = "template" slug "(" [ params ] ")" [ "->" slug ] [ comment ]
                                    (* "-> block": the primary block *)
             { comment | block-head | arg-line | signal | wire | set
             | tiles-claim | schedule-claim | moods-claim | access-claim
             | recipients-claim }
             "end" [ comment ] ;
params     = param { "," param } ;
param      = slug ":" ( type | "any" )             (* object parameter *)
           | slug ":" set-kind [ "=" slug ]        (* set parameter, default *)
           | slug "=" ( number [ unit ] | string | bool ) ;  (* value parameter, default *)
set-kind   = "moods" | "schedule" | "tiles" | "access" | "recipients" ;
instance   = slug "=" slug "(" [ inst-args ] ")" [ comment ] ;
inst-args  = param-name ":" ( ref | value ) { "," … } [ "," ] ;
ref        = slug                                  (* a name; an instance stands
                                                      for its primary block *)
           | slug "." slug ;                       (* instance.block *)

test       = "test" string [ comment ]
             { comment | inject | tick | expect | test-clock }
             "end" [ comment ] ;
test-cmp   = "==" | "!=" | ">=" | ">" | "<=" | "<" | "~=" ;
inject     = ref "." port "=" ( number [ unit ] | bool | const-ref ) [ comment ] ;
tick       = "tick" ( integer | number time-unit | const-ref )
                                    (* a count, or a duration: a whole
                                       number of dt steps; a constant
                                       holds either *)
             [ "dt" ( number [ unit ] | const-ref ) ] [ comment ] ;
time-unit  = "ms" | "s" | "min" | "h" ;
expect     = "expect" ref "." port test-cmp ( number [ unit ] | bool | const-ref ) [ comment ] ;
test-clock = "clock" [ date ] clock [ comment ] ;  (* bare: 07:30, 2026-01-03 07:30:15 *)
date       = digit digit digit digit "-" digit digit "-" digit digit ;

tiles      = "tiles" slug [ comment ]              (* Touch Pure Flex *)
             { comment | tile-head | arg-line }
             "end" [ comment ] ;
tile-head  = "tile" integer "(" [ tile-args ] [ ")" ] [ comment ] ;
tile-args  = tile-arg { "," tile-arg } [ "," ] ;
tile-arg   = ( fn | "text" | fn "_text" ) ":" ( string | number )
           | ( "to" | fn "_to" ) ":" ref ;        (* "to" may repeat *)
fn         = "click" | "up" | "down" ;
tiles-claim = slug "." "tiles" "=" slug [ comment ] ;
                                    (* extern TreeDevice; in a template body
                                       through a set parameter *)

access     = "access" slug [ comment ]             (* a named set of access rights *)
             { comment | grant }
             "end" [ comment ] ;
grant      = ( "user" | "group" ) string ":" string [ comment ] ;
                                    (* principal title : schedule title *)
access-claim = ref "." "access" "=" slug [ comment ] ;
                                    (* NFC Code Touch, extern or managed *)
recipients = "recipients" slug [ comment ]         (* a named set of recipients *)
             { comment | recipient }
             "end" [ comment ] ;
recipient  = ( "user" | "group" ) string [ comment ] ;
recipients-claim = slug "." "recipients" "=" slug [ comment ] ;
                                    (* Notification, extern *)
schedule   = "schedule" slug [ comment ]           (* DayTimer schedule *)
             { comment | sched-line }
             "end" [ comment ] ;
sched-line = day { day } ( "off" | interval { "," interval } ) [ comment ] ;
day        = "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun"
           | "holiday" | "vacation" | "dayoff"
           | "weekdays" | "weekend" | "daily"
           | "heating" | "cooling"
           | mode-name ;                   (* a Mode of the module, or another mode by title *)
interval   = time "-" time [ "=" number ] [ "activate" ] ;
time       = hour ":" minute ;                     (* 00:00 … 24:00 *)
schedule-claim = ref "." "schedule" "=" slug [ comment ] ;
                                    (* DayTimer, extern or managed *)
moods      = "moods" slug [ comment ]              (* LightController2 moods *)
             { comment | mood }
             "end" [ comment ] ;
mood       = string ":" mood-set { "," mood-set } [ comment ] ;
mood-set   = ( "all" | "AQ" integer ) mood-value ;  (* `all` first; AQ1 … *)
mood-value = "on" | "off" | integer "%"
           | "rgb" "(" integer [ "%" ] "," integer [ "%" ] "," integer [ "%" ] ")"
           | "temp" "(" integer [ "%" ] "," integer "K" ")"
           | integer ;                             (* the stored value verbatim *)
moods-claim = ref "." "moods" "=" slug [ comment ] ;
                                    (* LightController2, extern or managed *)
rule       = "rule" slug [ comment ]               (* Automatic Designer rule *)
             { comment | rule-line }
             "end" [ comment ] ;
rule-line  = ( "active" | "inactive"
             | "or"                                (* starts another group *)
             | "when" "time" clock                 (* 00:00 … 23:59, whole minutes *)
             | "when" "weekday" rule-day { rule-day }
             | "when" ref "." ident cmp-op ( number | bool )
             | "when" "mode" mode-name [ bool ]
             | "when" "weather" ident cmp-op ( number | bool )
             | "when" "day" cmp-op integer         (* 1 … 31 *)
             | "when" "daylight" [ bool ]
             | "when" ( "sunrise" | "sunset" | "dawn" | "dusk" )
             | "when" "date" date                  (* bare *)
             | "then" "mode" mode-name bool
             | "then" "wait" number time-unit
             | "then" "notify" string string        (* subject, text *)
             | "then" "mail" string string string   (* to, subject, text *)
             | "then" "message" ( "info" | "warning" | "error" ) string string [ "admins" ]
             | "then" ref ( command-word | string ) ) [ comment ] ;
                                    (* after `then`, the action words win:
                                       a block named `wait` is no target *)
mode-name  = slug | string ;                       (* one spelling per mode: see Operating modes *)
command-word = letter { letter | digit | "_" } ;  (* one word, bare: `then pumpe on`;
                                                      anything else stays quoted *)
rule-day   = "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun"
           | "weekdays" | "weekend" | "daily" ;

slug       = lowercase-letter { lowercase-letter | digit | "_" } ;
type       = uppercase-letter { letter | digit } ;          (* PascalCase *)
port       = letter-or-digit-or-underscore-sequence ;       (* as in the XML `K=` key *)
value      = number [ unit ] | string | bool | const-ref | clock | date | day-of-year ;
day-of-year = digit digit "-" digit digit ;        (* MM-DD; a date and a day of the year
                                                      bind only a calendar entry's
                                                      `on:`, `from:`, `to:` *)
bool       = "on" | "off" ;                        (* digital 1/0; "true"/"false" on a
                                                      text-boolean attribute *)
clock      = digit [ digit ] ":" digit digit [ ":" digit digit ] ;
                                                   (* a time of day, 00:00 … 23:59:59 *)
const-ref  = slug ;                                (* names a `let` constant *)
number     = [ "-" ] digits [ "." digits ] ;       (* exactly — `1.2.3`, `5.` are errors *)
unit       = "ms" | "s" | "min" | "h" | "K" | "%" ;   (* immediately adjacent *)
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
  (`slug.Port`) is always a **port reference**. With three parts
  (`instance.block.Port`) it names a block of a template instance. String values are always
  quoted.
- **`on` and `off`** are the boolean literals in every value position: block and extern parameters,
  `let`, template defaults, expression operands, test injections and expectations.
  - On a port they compile to `1`/`0`: `I2: on` is byte-identical to `I2: 1`.
  - On an attribute that Loxone Config stores as the text `"true"`/`"false"` (`VirtualIn`'s
    `Analog:`, `VirtualHttpInCmd`'s `Signed:`/`Analog:`, …) they compile to that text. Such an
    attribute takes only `on`/`off`; `Analog: "true"` is an error naming `Analog: on`.
  - Except on a `VirtualIn`, `off` compiles to no attribute, which is how Loxone Config stores an
    unticked box. A `VirtualIn`'s `Analog: off` is `Analog="false"`. Decompile and adopt write `off`
    for an unticked box that a new object would tick (`Signed`, `Analog` on the HTTP and UDP commands,
    `CloseAfterSend`).
  - A number stays a number: `1` is never rewritten to `on`.
  - The formula backend rejects them; write `1`/`0`.
- **0 on a setting** that Loxone Config leaves out at 0 (`MinVal: 0` on a `VirtualIn`, the range, low
  ends and timing of the virtual and Modbus objects and physical inputs, `ModbusDataType: 0`, …)
  compiles to no attribute, which is how a save stores it. A Modbus server's `Timeout: 0` stays
  `Timeout="0"`.
- A value that a Loxone Config save would rewrite is an error that names what the save writes: a
  virtual HTTP input's `Poll` is a whole number of seconds from 10 to 604800, its `Timeout` whole
  milliseconds from 10, its `Allowedtimeouts` a whole number; a digital `VirtualIn`'s `MaxVal` is 1 and
  its `MinVal` 0.
- A number may carry a **unit suffix**, written immediately adjacent: `40s`, `250ms`, `90min`,
  `1.5h`, `2700K`, `70%`. `loxl types <Type>` shows each port's documented unit.
  - Time units scale exactly into the port's unit. That is seconds by default (`1.5h` compiles to
    `Def="5400"`, byte-identical to `5400`), minutes on a minutes port (`AutoShadeTime: 2h` compiles
    to `120`), milliseconds on a milliseconds port (`VirtualHttpIn`'s `Timeout: 4s` compiles to
    `4000`). A value that is not an exact decimal number of minutes (`10s`) is an error there.
  - `%` and `K` (color temperature) have factor 1. They are accepted only on a port documented in
    that unit, or on an undocumented one. A time suffix on a `%`, `K` or `°` port is an error.
  - A suffixed `let` scales per use, into each port's unit.
  - The suffix is part of the canonical spelling: `1.5h` stays `1.5h` through `fmt`.
  - An unknown suffix is a parse error. A quoted `"40s"` stays a string.
  - Decompile and adopt write the suffix on a port documented in milliseconds, seconds or minutes:
    the largest of
    `h`, `min`, `s`, `ms` that gives a whole number (`10800` → `3h`, `0.5` → `500ms`, `58` → `58s`),
    and only when it compiles back to the same digits. Zero and the other ports stay bare.
  - `loxl lint` reports a bare number that this rule would write with a suffix (`unit-suffix`:
    `Delay: 10800` → `Delay: 3h`), on blocks, in `defaults`, on extern port assignments
    (`x.Time = 300` → `x.Time = 5min`) and in template bodies. A template value parameter
    (`zeit = 75`) and an instance's argument for it count when every use of the parameter in the
    body is a port of the same unit; an argument the instance forwards to its primary block counts
    by that block's port. A bulk line `(a, b).Port = 300` changes only as a whole.
  - A setting with no known unit stays a bare number: the Modbus sensors' `MinTime`. The units
    of the device settings are in [loxone-format.md](loxone-format.md#units-of-the-settings).
    `loxl lint --fix` writes the suffixes; the compile output stays byte-identical, and in a project
    the fix is refused if it would not.
- **Reserved words** can never be a name: the statement keywords `let`, `extern`, `removed`,
  `moved`, `template`, `end`, `page`, `test`, `tick`, `expect`, `clock`, `tile`, the boolean
  literals `on` and `off`, the expression operators `and`, `or`, `not`, and v0's `block`, `wire`,
  `set` and `use` (migration errors). `tick = Not()` is an error saying so.
- **Contextual words** are keywords only in their place and ordinary names everywhere else. One
  rule covers them: the section kinds `tiles`, `schedule`, `moods`, `access`, `recipients`, `rule`
  and `defaults` are keywords at the start of a line in front of a name or type (`moods kueche`,
  `defaults Monoflop(…)`) and after the dot of a claim (`x.moods = kueche`); `moods = Not()` is a
  block and `moods.Q` its output. The rule words `wait`, `mode`, `notify`, `mail` and `message` are
  keywords after `then` in a rule body (`then wait 5s`, `then mode urlaub on`), so a block of that
  name cannot be a rule action's target; the error names `loxl rename`. Conditions carry a dot
  (`when mode.Q == 1`) and take a block of any name. `decompile` and `adopt` never pick a
  contextual word as a slug.
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
extern tree = LoxTree(serial: "13000003")
extern status_alarm_ref = InputRef(mirrors: status_alarm)
```

Declares a slug for an existing object in the base config. The compiler
never creates, deletes, or moves externs; it only wires to their ports
(`<-`) and assigns their parameters (`=`).

Match semantics: the object must have the declared type and match the spec. Exactly one object may
match. Zero is a `NoMatch` error; several is an `AmbiguousMatch` error that lists the candidates.

- **`room:` and `category:`** narrow an `iname` or `title` match where titles repeat per room (every
  floor has a "Deckenlicht"). The object's `<IoData Pr=…>`/`Cr=…` must point at a `Place` or
  `Category` with that title ([loxone-format.md](loxone-format.md)).
- **`device: "<serial>"`** narrows the match to one device's objects, where identical hardware repeats
  its I/O titles (every Tree push-button interface has a "Taster 1"). The serial is the `Serial=` of
  the object's nearest enclosing element that has one: a Tree device, an extension, or the Miniserver
  for its own I/Os and virtual folders. For a device, it is the device's own. An Air device's serial
  is the one the Miniserver's `/data/status` lists (`50:4F:94:FF:FE:BA:1F:0E`). Serials compare
  ignoring case and `:`.
- **`serial:`** matches a device by its own serial, compared the same way.
- These constraints never combine with `uuid:` or `serial:`, which pin exactly on their own.

Once resolved, the UUID is pinned in the lockfile. Later compiles use the pin, even if the title has
changed, as long as an object with that UUID and type still exists. Editing a `uuid:` value re-points
the extern: it names a different identity and resolves afresh. A changed matcher kind must still match
the pinned object.

Choosing a matcher: `uuid` pins exactly; `iname` (the `IName=` attribute,
e.g. `VI1`, `AI3`) is locale-stable and preferred for built-in I/O
objects; `title` is human-friendly but locale-volatile — use it only for
objects you named yourself.

**`mirrors:` — match a ref by what it mirrors.** For `InputRef`, `OutputRef` and `OutputRefLM`
externs only. `mirrors: <slug>` (a bare name, or `inst.block` for a block of a template instance) matches the ref whose `Ref=` attribute names the object
that slug resolves to: a managed block with a locked identity, or a plain-matched extern
([loxone-format.md](loxone-format.md) documents `Ref=`).

- Where several refs mirror the same target, the file's `page` statement narrows the candidates to
  that page. A match that is still ambiguous is refused; use `uuid:` for, say, two refs of one flag on
  the same page.
- A pinned `mirrors:` is checked again on every compile. If Loxone Config re-pointed the ref, the
  compile stops.
- `room:`, `category:` and `device:` never combine with it.
- `decompile` and `adopt` write `mirrors:` where the target has a slug in the module and the match is
  unique.

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

Wiring them follows the normal rules: the singleton's output feeds a managed block's input directly,
with no `InputRef`, even across pages (checked in Loxone Config for `Mode` wires; the time sources are
the same kind of singleton).

- `title:` works for a singleton while the locale holds, and `adopt` lifts it that way. Switching to
  `uuid:` is a plain source edit; the lock pins the same object either way.
- `Mode` UUIDs are fixed system UUIDs (`00000000-…`), the same in every project. Time-function UUIDs
  differ per project; read them off `loxl decompile` or the XML.

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
  `Port`. A 0 writes no `Def=`: Loxone Config writes none at 0, and a
  missing `Def=` is 0. A port the source gives no value gets the default
  Loxone Config places a block with (`loxl types` shows it), but only on a
  block the compiler creates: the lock records that (`gui_defaults`), and
  every later compile writes the same values. A block that existed before,
  created by an older compiler or adopted, has 0 on such a port; `loxl lint`
  reports it (`implicit-zero`) and `--fix` writes the `Port: 0`.
- `Port: slug.Port` — a **wire**: the referenced source port is wired into
  this port. Sources may be extern or managed ports (including the block
  itself — feedback is representable) and are referenced before or after
  their declaration.
- `Port: a.Q and b.AQ >= 28` — an **expression**: desugars into
  gate/comparator blocks whose result is wired into the port — the same
  sugar as on `<-` (see [Expressions](#expressions--sugar-over-the-discrete-blocks)).
- An optional leading string is the display **label** (the XML `Title`);
  it defaults to the slug. A label that a Loxone Config save would rewrite is an error that names
  what the save writes: the save drops `{`, `}`, tabs and line breaks, turns a space at either end
  into `_`, and turns `%`, `<` and `|` into `_` on most types (the per-type sets are in
  [loxone-format.md](loxone-format.md#titles-a-save-rewrites)). An empty label is an error too. The
  same holds for `.title` claims.

Some block types carry **attribute parameters** — block logic stored as an
element attribute rather than a connector. They bind exactly like
parameters but can never take a wire:

```text
summe = Formula("Summe", Formula: "I1+I2", Input1: a.Q, Input2: b.Q)
```

They are declared per type in `connectors::attr_params`. `Formula:` on
`Formula` blocks compiles to `Formula="I1+I2"` on the `<C>` element,
together with the observed `Valid="false"` companion.

`At:` on `PulseAt` is the time of day the block fires,
written `HH:MM` or `HH:MM:SS`:

```text
jalousie_schliessen = PulseAt("Jalousie schließen", At: 19:00, Time: 1s)
```

- It compiles to `Sec=` on the `<C>` element, the seconds since midnight
  (`19:00` → `Sec="68400"`). The canonical form has two-digit hours and
  drops zero seconds: `6:58` formats as `06:58`, `19:00:00` as `19:00`.
  A time of day also works as a `let` constant (`let abends = 19:00`).
- `At:` takes a time of day and nothing else; a time of day binds
  nothing but `At:` (`connectors::clock_params`).
- It is a **claim**, like `Code:`: without `At:` the block's fire time
  stays with Loxone Config and is carried from the base; with it, the
  source writes it, at the attribute's place in the base element.
  Decompile and adopt lift it, so an adopted PulseAt states its time.
- It means a fixed time of day. A PulseAt set to a time function in
  Loxone Config (sunrise, an hour pulse) refuses `At:` unless the source
  also says `Pulse: "time"`, and is lifted without it.
- `loxl diff` shows a change as `At: 18:58 -> 06:58`.

`Pulse:` on `PulseAt` says when the block fires: `"time"` (at `At:`),
`"year"`, `"month"`, `"day"`, `"hour"`, `"minute"`, `"second"`, `"start"`
(when the Miniserver starts), `"dusk"`, `"dawn"`, `"sunset"` or
`"sunrise"`. It writes `Typ=`. `At:` goes only with `"time"`. A one-off
pulse on a date stays with Loxone Config.

```text
licht_an = PulseAt("Licht an", Pulse: "sunset", Time: 1s)
```

#### Block settings

These settings live on the block element. Each is a claim like `At:`:
carried from the base while the source leaves it out, written from source
once bound, at the place a save writes it. Decompile and adopt lift them,
except a value a save writes when nothing is set.

| Parameter | Blocks | Takes | Loxone Config field |
|-----------|--------|-------|---------------------|
| `Desc:` | most blocks with an app entry | text | Anzeigename: the name the app shows |
| `Type:` | `Memory` | `"digital"`, `"analog"` or `"text"` | Typ |
| `Pulse:` | `PulseAt` | see above | Impuls |
| `Sun:` | `AutoJalousie` | `on` or `off` | Sonnenschein verwenden |
| `On:`, `Off:` | `DayTimer` | text, may be `""` | Bezeichnung Ein / Aus |
| `PwrN:` | meters | text | Anzeigename of the power value |
| `Un1:`, `Un2:` | meters | text | Einheit of the two values |
| `UnSt:`, `MaxLvl:` | `MeterAbsSt` | text, a number | storage unit, Max. Speicherstand |

```text
merker = Memory("Anrufer", Desc: "Letzter Anrufer", Type: "text", Input: tel.TQ1)
zaehler = MeterDig("Strom", PwrN: "Leistung", Un1: "kW", Un2: "kWh", Input: s0.Q)
```

- `Desc:` is the name the app shows. Without it the app shows the label.
  It is written right after `Type=`, as a save writes it.
- `Sun: off` leaves the attribute out, as a save does.
- A text setting refuses `""`, except `On:` and `Off:`, where empty is
  what a save writes. Leave the setting out to keep what Loxone Config has.
- Some attributes stay with Loxone Config and are refused by name: the
  member lists of alarms and central blocks (`Objects=`, `Entries=`,
  `CentralOut=`, `rec=`), a meter's energy flow monitor (`Rec=`),
  statistics settings (`StatsType=`), a virtual input's hidden `MinChange=`,
  `MinTime=`, `ValOT=` and `Idx=`, and a program's `Task="1"`, which every
  save writes.
- `loxl types <Type>` lists the settings a type takes and their values.

`Code:` on `Code16` holds the PicoC program. A program is hundreds
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

- At most one parameter binding per port. Several wire bindings on one port are allowed when the
  sources differ (fan-in).
- A port takes a value or wires, never both: a wired input ignores its value, so
  `Input1: 2, Input1: x.AQ` is an error.
- Loxone Config keeps a constant under a wire, and it comes back when the wire is removed. Source does
  not write it. Decompile and adopt show it as a comment on the wire
  (`Input1: x.AQ, # stale constant 2 kept under the wire`), and the compile carries it from the base
  while the port stays wired.
- The type must be in the verified builtin table
  (gates `And`/`Or`/`Not`, the comparator family `Equal`/`NotEqual`/
  `Greater`/`GreaterEqual`/`Less`/`LessEqual`, `Formula`, `Monoflop`,
  `PulseGen`, `AnalogThresholdTrigger`, … `loxl types` lists them — see
  [connector-db.md](connector-db.md)) — anything else is an error. See
  [design.md](design.md) "Refuse, never guess".
- `And`/`Or` are fixed two-input blocks (`I1`, `I2`, `Q`): binding `I3`+
  is an error — Loxone Config silently deletes grown inputs on save. Need more inputs? Cascade gates:
  `c = And(I1: a.Q, I2: b.Q)`.
- Types, port names, and wire directions on managed blocks are validated
  **statically** (`loxl check`, no base config needed); unknown names get
  a "did you mean" suggestion when a close candidate exists.
- One input port may be a wire source: the `AI` of an `OutputRef`
  mirroring an `ApiActor`, read by a block's API connector `OutputAPI`
  (on a type whose `OutputAPI` takes wires). It is the reverse half of
  an API link drawn through that mirror
  ([loxone-format.md](loxone-format.md) records the shape):

  ```text
  flex_api_ost = OutputRef(mirrors: flex_api, AI: ost.OutputAPI)   # forward
  flex_api.I <- flex_api_ost.AQ                                    # distribution
  ost = AutoJalousie("Ost", OutputAPI: flex_api_ost.AI)            # reverse
  ```

  Write it this way where the compiler cannot pick one of several mirrors of the
  actor on the page; with one mirror (or none) `OutputAPI: flex_api.I`
  is enough. Any other input as a source stays an error.

### Virtual inputs — minted into the "Virtual Inputs" folder

```text
party = VirtualIn("Partymodus")
soll = VirtualIn("Sollwert", Analog: on, MinVal: 5, MaxVal: 30, DefVal: 21)
licht = And(I1: party.Q)
```

`VirtualIn` is a block type like any other, except where it lives: the
element is appended to the base's `<C Type="VirtualInCaption">` (the
folder Loxone Config calls "Virtual Inputs"), not drawn on a page. It
has no position, ignores `page` statements, and its `Q`/`Qm` outputs
wire directly into blocks on any page.

- `IName=` (`VI<n>`, the designation the GUI shows) is minted once,
  one past the highest number in use, and kept from then on.
- `Analog:` (`on`/`off`, default `off`), `MinVal:`,
  `MaxVal:`, `DefVal:` are attribute parameters. Each is a claim: written
  while bound, the GUI's value otherwise (like `Code:`). Make the range
  cover what the input will receive: a value outside it is reported to
  read 0 ([loxone-format.md](loxone-format.md)).
- The room/category binding (`<IoData>`) and display settings stay
  with Loxone Config.
- A base without the folder is a compile error. An existing virtual
  input is an extern (`extern x = VirtualIn(iname: "VI3")`) or, with
  `loxl adopt --uuid <it> --as <slug>`, a managed block: rebuilt
  in its place in the folder, its settings left to the GUI until bound;
  an extern on it is promoted (`--as` its slug). A whole-config `adopt`
  leaves virtual inputs as externs, as it does devices.

### Virtual and Modbus devices — the object tree

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
- A UDP command's receive timeout: `MaxTime: 30s` sets it (0 is off).
  When nothing arrives in that time, the value becomes `DefVal:`, or
  stays as it was with `ValOT: 1`. HTTP commands have no receive
  timeout.
- Strings are source strings: a backslash is written `\\`, so Loxone's
  `\v` placeholder in a command's check is `"\\v"`.
- `IName=` (VHI1, VCI3, AMQ12, …) is minted once and kept.
- `adopt --uuid <device>` takes the device and all I/O below it: each
  I/O gets the slug `<device slug>_<title>`, or keeps its slug when it
  was an extern (its statements move into the declaration). An
  extern of an object the source manages is an error.
- `extern … (device: "<serial>")` is something else: an extern matched
  by the serial of a physical device.

### Hardware — Tree and Air devices and what hangs below them

```text
taster = TreeDevice("Taster Bad", serial: "B0A1B2C3", Installation: "Neben der Tür")
klingel = TreeSensor("Klingel", device: taster, iname: "I0", Desc: "Haustür", Statistics: on)
temp = TreeAsensor("Raum", device: taster, iname: "AI0", Unit: "<v.1> °C", MinTime: 600)
led = TreeActor("Status", device: taster, iname: "Q0", I: klingel.Q)
tree = LoxTree("Tree", serial: "13000003", SwitchBoard: "Verteiler 2")
klingel.room = "Flur"
```

A hardware declaration takes over a device that Loxone Config paired, or
an input or output below one. The element stays where it is; the source
owns its title (the label), its room and category, and the settings it
states. They are written on every compile.

- **Devices:** `TreeDevice`, `LoxAIRDevice`, and the extensions `LoxTree`
  and `LoxAIR`. A device is matched by `serial:` (an Air device's is
  the one `/data/status` lists, from its `IP=`) or by `uuid:`.
- **Inputs and outputs:** `TreeSensor`, `TreeAsensor`, `TreeActor` and
  `TreeAactor` below a `TreeDevice`; `LoxAIRsensor`, `LoxAIRAsensor`,
  `LoxAIRactor` and `LoxAIRAactor` below a `LoxAIRDevice`. `device:`
  names the device's declaration. The I/O is matched by `iname:` inside
  that device, which needs the device matched by `serial:`, or by
  `uuid:`.
- **Settings** are those of `=` on a setting (below), with the same
  rules for adding one. The extensions have `Installation`,
  `SwitchBoard`, `SwitchBoardRow` and `SwitchBoardPos`, rewritten only.
- **`Statistics: on`** records the I/O's value: `StatsType="7"` on a
  digital input or output, `11` on an analog sensor (TreeAsensor,
  LoxAIRAsensor). `off` removes it. Left out, the statistics stay as
  Loxone Config set them.
- **Ports** are bound in the argument list (`I: klingel.Q`, `I: 30`).
  `x.Port <- …`, `x.Port = …` and `x.title = …` on a declaration are
  errors. `.room` and `.category` are allowed; they only rewrite a
  binding the element has.
- **Removing** a declaration from the source releases the hardware: the
  element keeps the values the source wrote last, and its wires from
  the argument list leave. Nothing is deleted, except a device the
  source created (below) under `removed <slug>`.
- `adopt --uuid <device>` declares the device with every I/O below it,
  with the values the config holds. An extern among them keeps its
  slug, and its statements move into its declaration. Adopting an I/O
  alone is an error. Pairing a device stays in Loxone Config.

#### Creating a Tree device

```text
tree1 = LoxTree("Tree", serial: "13000003")
melder = TreeDevice("Melder Flur", kind: "PresenceSensorTree", tree: tree1, branch: "left")
bewegung = TreeSensor("Bewegung Flur", device: melder, iname: "I1", MinTime: 20)
```

`kind:` creates the device instead of matching one. The compile adds it
to the Tree extension that `tree:` names, unpaired, and from then on
the lockfile keeps it. Pair it with a Tree search in Loxone Config.

- **Kinds:** `TouchTree`, `PresenceSensorTree`, `CorridorLightTree`
  (LED Ceiling Light RGBW) and `ValveTree` (Valve Actuator). Any other
  is an error; declare it by `serial:` once Loxone Config has paired it.
- **`branch:`** is `"left"` or `"right"`. It can be left out when the
  extension has one branch.
- **Its I/O** is declared by `iname:` (`I1`, `AI1`, `AQ1`, …) with
  `device:` naming it. The title is the kind's English default when the
  declaration gives none.
- **Not paired yet:** `loxl compile` and `lox config push` print a note
  for each such device. How the Miniserver runs a device it has not
  searched for is untested.
- **Deleting** it takes `removed <slug>`; without it, a declaration
  that leaves the source is an error. A device that feeds a wire drawn
  in Loxone Config is not deleted. Declaring it by `uuid:` (or
  `serial:` once paired) keeps it as a plain declaration.
- A base without the device, an older one or one where it was deleted
  in Loxone Config, gets it back under the same UUID.
- No compile deletes any other hardware.

### Operating modes — minted into the "Betriebsmodi" folder

```text
party = Mode("Party")
party.I <- partytaster.Q          # switch the mode from logic
licht = And(I1: party.Q)          # or read it
```

A user-defined operating mode is an object of the object tree, like a virtual input: no position, no
`page`.

- A new one gets the next `Mode=` number below the lowest in use (`-1`, `-2`, …) and keeps it.
- Existing modes, built-in or made in Loxone Config, are an extern
  (`extern urlaub = Mode(title: "Urlaub")`), or are named by title where no extern declares them.
- `adopt --uuid <mode> --as <slug>` takes over a user mode made in Loxone Config. It keeps its UUID
  and `Mode=` number; a new title is a rename. A built-in mode (`Mode=` ≥ 0) stays an extern.

**One spelling per mode.** Rules (`when mode`, `then mode`), schedule lines and a calendar entry's
`switches:` name a mode one way:

1. a built-in mode in a schedule or calendar entry by its day word (`mon`, `holiday`, `heating`, …);
2. a mode the module declares or externs by its slug (`when mode party`, `party 18:00-24:00`);
3. any other mode by its quoted title (`when mode "Urlaub"`).

A slug that is a day word cannot name a mode in a schedule or calendar entry, since the day word
wins; that mode keeps its title there. Another spelling is an error that states the line as it should read: write
`party 18:00-24:00` instead of `"Party" 18:00-24:00`. `loxl check` catches what the source shows,
`loxl compile` the rest (a mode an extern matches by UUID, a built-in mode by title). `loxl fmt` on a
module directory rewrites the spellings the source shows; `decompile` and `adopt` write them.

### Calendar entries — the *Betriebszeiten* that switch a mode on

```text
party = Mode("Party")
ferien = CalendarEntry("Sommerferien", switches: vacation, from: 2026-07-04, to: 2026-09-05)
ostern = CalendarEntry("Ostermontag", switches: holiday, easter: 1)
silvester = CalendarEntry("Silvester", switches: party, on: 12-31)
heizperiode = CalendarEntry("Heizperiode", switches: heating, from: 10-01, to: 04-30)
```

An entry of the *Betriebszeiten* calendar is an object of the object
tree, minted into the `CalendarCaption` folder. `switches:` names the
operating mode it switches on, spelled as everywhere else (see
[One spelling per mode](#operating-modes--minted-into-the-betriebsmodi-folder)):
a built-in mode by one day word (`holiday`, `vacation`, `dayoff`,
`mon` … `sun`, `heating`, `cooling`; not `weekdays`, which is several
modes), a mode the module declares or externs by its slug, any other by
its quoted title (`switches: "Urlaub"`). It compiles to the entry's
`CalType=`, the mode's `Mode=` number (none for the holiday mode, 0).
Without `switches:`, a fresh entry switches the holiday mode, and an
adopted one keeps its mode.

The date is one of four kinds, written bare like `when date`:

| Source | Kind | Compiles to |
|---|---|---|
| `on: 12-31` | a day every year (`MM-DD`) | no `Mode=`; `Month`, `Day` |
| `easter: 1` | days after Easter Sunday (`easter: -2`: before it) | `Mode="1"`, `EasterOffset` |
| `from: 2026-07-04, to: 2026-09-05` | one period (`YYYY-MM-DD`) | `Mode="3"`, `From`, `To` (days since 2009-01-01) |
| `from: 10-01, to: 04-30` | a range every year (`MM-DD`), across New Year when `to` is earlier | `Mode="4"`, `Month`, `Day`, `Month2`, `Day2` |

Month and day have two digits, and `02-29` is a day of the year. A
period starts on 2009-01-01 or later. `on:` takes no year: for one day,
write `from: 2026-12-24, to: 2026-12-24`. The attributes the kind does
not read stay as they are, as in a Loxone Config save. Without a date,
a fresh entry is 1 January (what Loxone Config writes for an unset
date), and an adopted one keeps its date.

The stored attributes are no longer source. `CalType: 10` is an error
that names `switches: heating`: `loxl check` names it for a built-in
mode, `loxl compile` for a user mode (negative numbers, which only the
config resolves). `Mode: 4, Month: 10, Day: 1, Month2: 4, Day2: 30` is
an error that names `from: 10-01, to: 04-30`. `Mode`, `Month`, `Day`,
`Month2`, `Day2`, `From`, `To`, `EasterOffset` and `WeekDayInMonth`
(1 on every entry) are all refused, and a `Mode` other than none, 1, 3
or 4 is no date. `loxl fmt` rewrites a built-in mode's number and the
stored date. The entries Loxone Config made are taken over with
`adopt --uuid`, which writes their mode with `switches:` and their date
as a date; edits change only what the source states. An entry whose
mode is gone from the config (a deleted user mode) is adopted without
`switches:`, with a comment that names its `CalType`: it keeps that
number until `switches:` names a mode. Its `I` and `Q`
are never wired in the corpus; the mode's `Q` is the output to use.

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

Rewrites the extern port's `Def=` (a 0 removes it); the original value is recorded in the
lockfile and **restored** when the statement disappears from source.

Like `<-`, this is for **extern targets only**. A managed block's
parameters belong in its argument list — assigning a managed port is a
validation error pointing there. Assigning a port reference
(`x.Port = y.Q`) is a parse error suggesting `<-`.

### `=` on a setting — the settings of a physical input or output

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
own: `Desc`, which may be added, and where it is installed —
`Installation`, `SwitchBoard`, `SwitchBoardRow`, `SwitchBoardPos` —
which are rewritten only (a Tree device has a board only when it is
mounted in one). The Tree and Air extensions (`LoxTree`, `LoxAIR`) have
the place and the board.

```text
extern relais = LoxAIRDevice(title: "Relais Jalousie", device: "50:4F:94:FF:FE:BA:1F:0E")
relais.Installation = "Fenster Ost"
relais.SwitchBoardPos = 3
```

### `(a, b, …).X = value` — one setting on several objects

```text
(garten, terrasse, pool).MinTime = 600
(garten, terrasse).room = "Garten"
```

The statement is the assignment on each listed object, in that order —
a port, a setting or `title`/`room`/`category`. A block of a template instance is listed as
`inst.block` (`(sued.hoch, nord).room = "Garten"`). `fmt` keeps it as one
line. A name may be listed once; `.tiles`, `.schedule` and `.moods` take
one object.

### `.title` / `.room` / `.category` — assign an extern object

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
(with a tombstone until the base catches up).

**Followers.** A room or category claim also rewrites every descendant
(a device's inputs and outputs) whose binding equals the object's
original value — the children that were "in the device's room" move
with it, a child bound elsewhere stays. Managed blocks and descendants
with their own claim for the same attribute (and their subtrees) are
skipped. A follower changed in Loxone Config after the claim keeps the
GUI's value and leaves the follower list on the next compile.

A title claim's followers are the object's refs: every
`InputRef`/`OutputRef` whose `Ref=` is the object takes the new title,
since Loxone Config derives a ref's title from its target on every save.
The inputs an alarm block collected into its sensor list (`WF` bit
0x1000 on an `InputRef`) keep theirs — the GUI owns those titles. A
minted ref (`mirrors:`) of a claimed extern carries the claimed title.

Rules:

- **Title: externs and named signals.** A managed block's title is its label
  (`slug = Type("…", …)`); `.title` on it is a validation error naming
  the remedy. On a named signal `.title` sets the top block's label (see
  [Named signals](#named-signals-slug--expr)). Its `.room` and `.category` are allowed: the compile
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
- **Top-level only** — not inside a `template` body, except a named
  signal's `.title`.
- `x.room <- …` is a parse error: attributes are assigned, never wired.

### `removed` — authorize deleting a managed block

```text
removed temp_hoch
```

Declares that `temp_hoch`'s absence from source is intentional. The next compile deletes the block from
the config and moves its lockfile entry to a removal tombstone. The tombstone keeps deleting the
object from bases older than the deployment of that output.

- It covers one slug and shows in the diff. Prefer it over the global `--allow-removals` flag.
- You may delete the statement right after the compile that applies it; the tombstone carries the
  intent from there. It is tolerated while the tombstone is pending. Once the removal has reached the
  deployed config, a lingering statement is a compile error.
- Declaring a slug and `removed`-ing it in the same module is an error.

The same statement deletes a named rule (`rule <slug> "Name"`) in
place of its declaration: the rule the lock pins is deleted with its
connectors, under the same tombstone. A rule whose outputs feed a
block is refused until that wire is gone.

### `moved` — rename a managed block, keeping identity

```text
moved beschatten -> schatten_gate
```

Renames the lockfile entry, so the block's identity (object and port UUIDs, layout) survives a slug
rename in source. Rename the declaration and its references, add the `moved` line, and compile.
Once the new slug has the lock entry, the statement is done and can be deleted.

It is an error when neither slug is in the lockfile (typo guard), when the old slug is still
declared, or when moves are chained (`a -> b`, `b -> c`). `loxl rename` does all of this in one step.

### `page` — place the following blocks on a named page

```text
page "Beschattung"

wp_ein = And(
	I1: soll.AQ >= 1.5h,
)
```

Names the `<C Type="Page">` (by display title, as in the base config) that
the block declarations after it are built on — until the next `page`
statement or the end of the file. Blocks above a file's first `page` statement
keep the default page (`--page`, the project file, or the document's first
page), also in a directory module where an earlier file names a page. Placement
is positional, so the synthetic blocks of an expression land on the page the
expression is written under. Objects of the object tree (a `VirtualIn`, a
`Mode`, a device's I/O) have no page and ignore the statement.

There is no statement that goes back to the default page. Put those blocks
above the file's first `page`, or in a file of their own, or name the page:
`page "<title>"` works for the default page too.

The statement is **authoritative**. On every compile, each block it governs is pinned to a base page
with the declared title:

- A pin that still points at a page with that title is kept. Titles need not be unique, and adopted
  blocks never move on their own.
- Any other pin moves to the first page with that title in document order. The block visibly moves in
  Loxone Config.
- A title no page has is a compile error, unless the statement says where to create the page.

```text
page "Küche Logik" (room: "Küche")
page "Küche Logik" (room: "Küche", category: "Beleuchtung")
```

With `room:` a missing page is **created**, as Loxone Config's "Neue Seite" does. It goes into the
program, filed by title, and is assigned to the room (a `Place` by title; Loxone Config does not
accept the unassigned room) and to `category:` (default: the unassigned category).

- A page that already has the title is used as is. The arguments apply only to creation.
- Without `room:` a missing title stays an error, so a typo never creates a page.
- Several statements for one page need the arguments once. Given twice, they must agree.

The lockfile keeps a created page's UUID (`pages`), so recompiles before the push create the same
page. Once the page is on the Miniserver it is an ordinary page: rename it, move it or give it a room
in Loxone Config. Renaming it there while the statement still has `room:` is an error; write the new
title in the source, or drop the arguments to hand the page to Loxone Config. Removing the statement
never deletes the page.

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

Parameters come in three forms:

- An **object parameter** (`jalousie: AutoJalousie`) is required at every call site and passes a name:
  an extern, a block, or a block of an instance (`jalousie: sued`, `jalousie: sued.hoch`). The
  annotation is checked against the argument's declared type when it is known. It takes no default.
  `any` (`bewegung: any`) takes an object of any type, for a template over a sensor that may be virtual
  or physical; the ports the body uses are checked after expansion, against the type passed.
- A **value parameter** (`pos = 70`) has a default and may be overridden with a literal or a `let`
  reference.
- A **set parameter** (`stimmungen: moods = standard`) passes a `moods`, `schedule`, `tiles`, `access` or `recipients` set by
  its bare name (`stimmungen: kueche`) to a claim in the body (`lc.moods = stimmungen`). The set must
  be declared and of that kind. Without a default, the set must be given.

Free identifiers in the body that are not parameters refer to the module's names (externs, lets,
blocks), like any other reference.

**Instance names.** Source names a block of an instance through the instance: `sued.hoch.Q` is the
output `Q` of body block `hoch` in instance `sued`.

- An instance stands for its **primary block**: the body's only block, or the one the header names
  with `-> <block>` (`template fassade(…) -> beschatten`). `sued.Q` reads the primary block's output.
- The primary block has one spelling: write `sued.Q`, not `sued.beschatten.Q`.
- An instance whose body has several blocks and no primary block must name the block.
- These names work everywhere a block is referenced: arguments, `<-`, `=`, tests, a tile's `to:`,
  rules, claims, `removed sued.hoch` and `moved sued.alt -> sued.hoch`. `loxl rename` of an instance
  renames them all.

Expansion is pure macro substitution before compile: body slug `hoch` in
instance `sued` becomes `sued_hoch`, and that **expanded slug is the
lockfile key**. Re-instantiating therefore never re-mints; editing the
body mints only what is new in each instance; `removed` and `moved` apply
per expanded slug. Source never writes the expanded slug:
`sued_hoch.Q` is an error that names its replacement, `sued.hoch.Q`.
Compile errors name blocks by their lock keys; `loxl test` reports them
as source spells them.

A call-site binding that names no template parameter forwards as a
**port binding** onto the primary block — an instance is a
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
port with the block's known ports listed. A template without a primary block takes no forwards.
Mark one with `-> <block>`.

A block title in the body may interpolate value parameters:
`"{titel} (scharf)"` substitutes the parameter's string at expansion, so
each instance carries its own app-visible title. The placeholder must
name a value parameter whose value is a string (a `let` reference to a
string constant also works); a brace-wrapped slug naming no value
parameter is an error (typo guard), and braces around anything not
slug-shaped pass through verbatim.

The template library (`templates/`, listed by `loxl templates`)
ships tested templates for common automations; copy a file into the
module directory to call them.

The body may contain only block declarations, named signals, wires, sets, claims (`.moods`, `.schedule`,
`.tiles`, `.access`, `.recipients`, through a set parameter) and comments. No nesting, no template-local
`let`/`extern`. `template` and `end` are reserved words; `use` is reserved for the old-syntax error.

### `defaults` — the parameters every block of a type shares

```text
defaults ModbusASensor(
	ModbusPollingCycle: 60,
	MinTime: 1000,
	MaxTime: 300000,
)

vorlauf = ModbusASensor(
	device: wp,
	ModbusAddress: 1,
)

ruecklauf = ModbusASensor(
	device: wp,
	ModbusAddress: 2,
	ModbusPollingCycle: 10,
)
```

A `defaults` statement gives every managed block of a type the values it does not bind itself:
`vorlauf` takes all three, `ruecklauf` keeps its own `ModbusPollingCycle: 10` and takes the other
two. The source above compiles to the same bytes as writing the values on each block.

- **Values only**: numbers (with a unit), strings, `on`/`off`, clock times and `let` constants. A
  wire (`Reset: taster.Q`), an expression or a label string is an error; write it on the block.
  `device:` and `mirrors:` say which object a block is, so they stay on each block too.
- **One statement per type**, at the top level of the module, in any file. Two statements for one
  type are an error; merge their arguments. In a template body it is an error.
- **Every managed block of the type**: template body blocks too, after expansion, so an instance
  takes the default unless the body or the call site binds the port. Blocks that an expression or a
  named signal generates (`And`, `GreaterEqual`, …) take none: they bind what their operands say.
- A block that binds the port in any way keeps its binding: a value, a constant, a wire or a
  forwarded call-site argument.
- `loxl lint` reports a statement that no block of the type takes (`unused-default`): no block of the
  type, or every block binds the port itself.
- In the language server, hovering a block lists the values it takes from its type's `defaults`.

Decompile and adopt write `defaults` when it saves repetition. For a type with at least three managed
blocks, all on one page (or all in the periphery), a port becomes a default when every block binds
it to a value and more than half of them share one value. Those blocks drop the binding; the others
keep theirs. The statement goes at the top of that page's file, after the `page` line. A type whose
blocks spread over several pages keeps every value on its blocks, so a file never changes the meaning
of another. `loxl adopt --uuid` (one block into an existing module) drops the bindings that equal the
module's defaults, and refuses a block that leaves a defaulted port unset: the default would change
it.

### Expressions — sugar over the discrete blocks

```text
let schwelle = 28

jal_sued.AutoShade <- sonne.Q and aussentemp.Q >= schwelle

wp_ein = And(
	I1: wassertemp.AQ < wunschtemp and abdeckung_offen.Q,
	I2: pv_ueberschuss.AQ >= pv_schwelle,
)
```

The right side of `<-` and the value of an argument binding may be a boolean expression. Before
compile, like template expansion, it desugars into verified blocks:

- `and`/`or` become `And`/`Or`. They have two inputs; longer chains cascade left-associatively.
- `not` becomes `Not`.
- Each comparison becomes one comparator block: `>=` → `GreaterEqual`, `>` → `Greater`, `<=` →
  `LessEqual`, `<` → `Less`, `==` → `Equal`, `!=` → `NotEqual`.
- Operand order is kept: the left side binds `Input1`, the right side `Input2`. A port operand becomes
  a wire; a number or `let` reference becomes the port's `Def=`.
- Each generated block is labeled with its sub-expression, so the rule stays readable on the Loxone
  Config canvas. A save would turn `<` and `%` in a title into `_`, so the label writes `<=` as `≤`,
  `<` as `＜` and `%` as `％` (`p1.Q ≤ 5 and p2.Q ＜ 70％`).
- A bare `slug.Port` stays a plain wire and a bare value a plain parameter. Parentheses around one
  change nothing.

Precedence, loosest to tightest: `or` < `and` < `not` < comparison <
`+ -` < `* /`; parentheses group. Comparisons take plain operands (a
port, a number, or a constant — at least one side a port) and do not
chain — write `a.AQ >= 5 and a.AQ < 10`, not `5 <= a.AQ < 10`. A
constant cannot drive a gate input directly; compare it. Gates take
boolean ports or sub-expressions.

**Arithmetic — one `Formula` block**. `+ - * /` may form the
whole RHS of `<-` or a whole argument binding:

```text
verbrauch_kw.AI <- verbrauch.AQ / 1000
```

A whole arithmetic tree desugars into one `Formula` block. Here that is `verbrauch_kw_ai__f1` with
`Formula: "I1/1000"`, `Input1` wired from `verbrauch.AQ`, and the sink fed from the block's `AQ`.

- Distinct port operands become `Input1`…`Input4` in order of first appearance. A repeated port reuses
  its input. More than four distinct ports is an error; split the expression.
- Numbers and numeric `let` references are inlined into the formula text. Negative constants are
  parenthesized: `I1*(-2)`.
- Unit values (`40s`) and strings are rejected: formulas compute on plain numbers.
- The label is the expression text. Identity works as for the gate blocks (slug operator `f`,
  expression-owned lock entry).
- Arithmetic inside gates or comparisons (`a.Q and x.AQ + 1`, `x.AQ + 1 >= 5`) is not supported.
  Name the arithmetic as a signal (below) and compare its `AQ`.

**Identity.** The generated blocks get synthetic slugs `<sink>_<port>__<op><n>`: a post-order walk with
one counter per operator. The sink is the extern port on `<-`, or the managed block's port for an
argument binding. Here: `jal_sued_autoshade__ge1`, `jal_sued_autoshade__and1` and `wp_ein_i1__lt1`.

- They are keyed in the lockfile like hand-written blocks, marked expression-owned. An unchanged
  expression never creates new identities.
- Editing the expression derives new slugs, and the compiler removes the orphaned expression-owned
  entries without a `removed` statement.
- Declaring a slug inside an expression's synthetic namespace is an error.
- In a template body, desugaring runs after expansion, so the prefix uses the instance's sink.

### Named signals: `slug = <expr>`

```text
let wunschtemp = 26

wp_bedarf = wasser.Q < wunschtemp and pv.Q >= 1500 and not abdeckung.Q
wp.On <- wp_bedarf.Q

pv_kw = pv.Q / 1000                  # arithmetic: one Formula, read at AQ
pv_hoch = pv_kw.AQ >= 1.5
```

A line `<slug> = <expression>` names an expression's result. It desugars exactly like an inline
expression: the same blocks, the same labels (the expression text), the same rules. Only the names
differ:

- The **top block is `<slug>` itself**; the sub-blocks are `<slug>__<op><n>`.
- The signal's output is the top block's: `<slug>.Q` for a gate or comparator, `<slug>.AQ` for the
  `Formula` of an arithmetic signal. `<slug>.Q` on a Formula is an unknown-port error.
- It is referenced like any block: from arguments, `<-`, tests, tile `to:` and rules.
- Arithmetic under a comparison, which an inline expression cannot hold, is two signals
  (`pv_kw = pv.Q / 1000`, then `pv_hoch = pv_kw.AQ >= 1.5`).

The right side must be an expression:

- A bare port (`x = a.Q`) is an error, since an alias is not a block. Write `a.Q` where `x.Q` would go.
- A bare value (`x = 5`, `x = on`) is a constant; write `let x = 5`.
- A signal's inputs are its expression. `<-`, port-`=`, `.room` and `.category` on a signal are
  errors, and a template body's only signal takes no call-site port forwards.

**Label.** The top block's label is the expression text, unless a `.title` line gives it one:

```text
beschatten = sonne.Q and aussentemp.Q >= schwelle
beschatten.title = "Beschatten Süd"
```

- The value is a string, number or `let` reference, as for an extern's `.title`. It is the label, not
  a claim: nothing goes into the lockfile and the lock keys stay the same.
- Only the top block takes it. The sub-blocks keep their expression text.
- One `.title` per signal. `fmt` keeps it on the line below the signal.
- In a template body a signal's `.title` may interpolate the parameters (`"Warm {name}"`).

**Identity.** The top slug is written by hand, so it is **hand-owned**:

- It keeps its UUID while the expression below it changes.
- Deleting the statement needs `removed <slug>`, like any block.
- A rename is `moved <old> -> <new>` or `loxl rename`. `moved` takes the expression-owned sub-blocks
  along (`old__ge1` → `new__ge1`).
- Changing the top operator (`and` → `or`) changes the top block's type, which a pinned block never
  does. The compile refuses and names the way out: a new slug plus `removed <old>`.

The sub-blocks are **expression-owned**, as inline: derived from the text, orphans removed
automatically. A declared name in the `<slug>__` namespace is an error. In a template body a signal is
body-local like a block: instance `sued` of `warm = …` gives `sued_warm` and `sued_warm__ge1` in the
lock, and source names it `sued.warm` (`sued` when it is the primary block).

**Decompile and adopt** write the blocks an expression compiled to as that expression again, if it
compiles to the same blocks:

- A chain used once, by a block or `<-` on its own page, goes back into that argument or wire.
- Any other chain becomes a named signal, its label as `.title` when that is not the expression text.
- A constant whose name a label shows comes back as `let`.
- The operands take the names the labels give them, where the labels agree.

Anything else, such as a gate drawn in Loxone Config, stays a declared block.

### `test` — a simulated scenario

```text
test "Frostwächter schlägt an"
	wassertemp.Q = 2        # inject: override that output for the whole test
	tick 2                  # advance the simulation two steps (default dt 1s)
	expect frost.Q == 1
	wassertemp.Q = 10
	tick 2 dt 0.5           # per-step dt override, unit values allowed
	expect frost.Q == off
	clock 06:30             # the wall clock, bare
	tick 5min               # a duration: 300 steps of 1s
	expect frost.Q != on
end
```

A `test` block is a top-level item, not allowed inside templates. `loxl compile` ignores it.

`loxl test` compiles the module in memory against the lockfile and runs each test in the Lox Logic
simulator from a fresh start: every value 0, the clock at 2026-01-01 00:00 (a Thursday). The simulator
models the blocks Loxone documents well enough; [simulator.md](simulator.md) lists them. A test whose
expectation depends on another block reports `ERROR` and names it. Set a port between that block and
the expectation to cut it out.

Statements, in file order:

- **Injection** `slug.Port = value` — sets that signal for the rest of
  the test (a persistent override).
  Any declared extern or block port can be driven, including outputs
  (that is the normal case: drive a sensor's `Q`/`AQ`). Values are
  numbers (with units), `on`/`off` or `let` references; strings are
  rejected.
- **`tick <n> [dt <v>]`** — advance the simulation `n` program cycles
  of `dt` seconds (default 1). Injections since the previous `tick`
  take effect for these cycles.
- **`tick <duration> [dt <v>]`** — advance by a time: `tick 5min`
  is `tick 300`, `tick 2s dt 500ms` is `tick 4 dt 500ms`. The duration
  must be a whole number of `dt` steps (`tick 1s dt 300ms` is an
  error); `fmt` keeps the spelling written.
- **`tick <constant> [dt <constant>]`** — a `let` constant holds the count or the duration
  (`let warten = 5min` … `tick warten`), and `dt` takes one too. A constant that is neither a whole
  count nor a time, or a duration that is no whole number of `dt` steps, is an error.
- **`expect slug.Port <cmp> <value>`** — assert after the preceding
  `tick`. Comparators: `==`, `!=`, `>`, `>=`, `<`, `<=`, and `~=`
  (approximately, 5 % tolerance); `==` and `!=` allow the rounding of
  decimal arithmetic. An `expect` before any `tick` is an
  error, as is a trailing injection no `tick` ever applies. `--lox`
  (below) has no `!=`.
- **`clock HH:MM[:SS]` / `clock YYYY-MM-DD HH:MM[:SS]`** — set the
  simulated wall clock before the following `tick`, for time-dependent
  logic: the weekday operating modes and the `DayTimer`s follow it.
  Written bare; the time canonicalizes like a `clock` value
  (`7:30:00` → `07:30`). v1's quoted `clock "07:30"` is an error naming
  `clock 07:30`.

References resolve on the **flattened** module (templates expanded,
expressions desugared), so tests can name the blocks of template
instances (`frost`, `frost.alarm`) and expression-synthetic slugs
(`pv_kw_input__f1.AQ`) — exactly what the simulator sees. Each slug
resolves through the lockfile to its object's UUID.

`loxl test --filter <substring>` runs a subset. `--lox <path>` runs the
tests in [lox-cli](https://github.com/eisber/lox-cli)'s `lox sim`
instead, which addresses signals by object **title**: Lox Logic derives the
titles from the lockfile and refuses ambiguity unless the duplicates
are same-signal mirrors or a room qualifier separates them;
`--show-spec` prints the scenario JSON it would run.

### `tiles` — program the tiles of a Touch Pure Flex

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

A `tiles` set names tile programs. A claim `<device>.tiles = <set>` writes them onto one Touch Pure
Flex; several devices may claim one set.

- The device is an extern `TreeDevice`. Pin it by `uuid:`, since tree device titles are free text.
- A tile is addressed by its index `i` in the config. Lox Logic rewrites the three functions of tiles
  that exist and keeps every unlisted tile unchanged.
- The layout (which tiles exist, their position, size and icon) stays with Loxone Config.

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

A `to:` target is an extern or a managed block, a block of a template instance by its instance name
(`to: ost`, `to: ost.jal`). It must be **API-linked** to the device:

- its `OutputAPI` is wired to the device's API connector (`ApiActor.I`), or
- its `OutputAPI` is wired to the `AI` of an `OutputRef` mirroring it, whose `AQ` feeds `ApiActor.I`.

The link may be drawn in source (`OutputAPI: flex_api.I`, or an explicit mirror as under *Block
declaration*) or in Loxone Config. A command whose abbreviation Lox Logic knows (`As` = AutoJalousie,
`Lico` = LightController2) refuses a target of another type. Other abbreviations are not checked.

Rules:

- **Extern `TreeDevice` only**, with a `<btns>` tile layout; a missing
  tile index is a compile error naming the indices that exist.
- **One set per device** — a duplicate claim is a validation error; two
  externs resolving to the same device are a compile error.
- **Strings, not constants.** Commands and texts are quoted strings.
- **No restore.** Like `Code:` a claim keeps no lockfile state:
  removing it leaves the last written programs in place.
- **Top-level only** — neither the set nor the claim inside a template.
- `x.tiles <- …` is a parse error.

`loxl lint` (in a project) checks the tiles of *every* Touch Pure Flex
in the compiled config, claimed or not: `tile-broadcast` when a command
without targets reaches two or more linked blocks of its type,
`tile-dead-command` when no linked block of its type receives it.

### `access` — who may open an NFC Code Touch

```text
extern haustuer = NfcCodeTouch(uuid: "50000009-0000-0090-ffff504f94112233")
extern garage = NfcCodeTouch(uuid: "5000000a-0000-00a0-ffff504f94112233")

access bewohner
	user "alice": "Always"
	user "bob": "Workdays"                       # an access schedule
	group "Residents": "Always"
end

haustuer.access = bewohner
garage.access = bewohner
```

An `access` set names access rights (*Zutritt*): users and groups, each
with the access schedule named after the colon. A claim
`<code_touch>.access = <set>` makes the set the access rights of one NFC
Code Touch: everyone it lists may open it under that schedule, and
nobody else. Compiling a claim grants what the set lists, moves a right
to another schedule, and revokes the rights of users and groups it
leaves out; an empty set revokes them all. Several Code Touches may
claim one set. Code Touches without a claim keep their rights as they
are.

Users, groups and access schedules (`SwitchingTimer`) stay with Loxone
Config and the app. The set names them by title, exactly as written
there; a title that names nothing, or more than one object of its kind,
is a compile error. Their credentials (NFC tags, access codes,
passwords) and memberships are not part of the language.

Rules:

- **An `NfcCodeTouch` only**, an extern or a managed block, one claim per
  block. Two claims that reach the same block (two externs resolving to it)
  are a compile error.
- **Each principal once** — a user or group listed twice in a set is a
  validation error.
- **Access, not operation.** A right's `ACT=` (operating the block in
  the app) is not an access right; compiling keeps it, and revoking
  access leaves it.
- **No restore.** Like `tiles` a claim keeps no lockfile state:
  removing it leaves the last written rights in place.
- **Top-level sets.** The set is declared at the top level; a template
  body claims one through a set parameter (`rechte: access`).
- **A set is not the block.** `access haustuer … end`, where `haustuer`
  is an extern or block, is the v1 form: an error that names the set and
  the claim to write instead.

`loxl decompile` and `loxl adopt` lift the access rights of every Code
Touch that has any into a set `<slug>_access` and a claim, with the
extern it needs. Code Touches with the same rights share one set. When a
right names a user or group without a unique title, the Code Touch gets
a `# access <slug>: not lifted — …` comment instead: a set without that
right would revoke it.

`loxl lint` (in a project) reports `no-access-right` for a user with an
NFC tag or access code but no access right on any Code Touch, neither
directly nor through a group: the tag opens nothing. Membership of the
*Full access* group does not count, since it grants no access right.

### `schedule` — the switching times of a DayTimer

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

- **Days** are the built-in operating modes: `mon` … `sun`, `holiday` (Feiertag), `vacation`
  (Urlaub), `dayoff` (Freier Tag/Schulferien), `heating` (Heizperiode), `cooling` (Kühlperiode), and
  the groups `weekdays` (mon–fri), `weekend` (sat, sun), `daily` (mon–sun).
  - A `Mode` the module declares or externs is named by its slug: `party 18:00-24:00`.
  - Any other operating mode of the config is named by its title in quotes: `"Party" 18:00-24:00`.
    The title must name exactly one mode of the compiled config (new ones included).
  - Each mode has one of these spellings (see [Operating modes](#operating-modes--minted-into-the-betriebsmodi-folder)):
    `"Montag"` is an error naming `mon`, and `"Party"` one naming `party` where the module declares
    it.
  - A day may appear on several lines; its intervals merge.
  - The timer always has a column for each of the seven weekdays. A weekday the schedule does not name
    is **off all day**. Each other mode the schedule names gets a column; modes it does not name get
    none.
  - Columns are written in `Mode=` order: user-defined modes, holiday, vacation, day off, Monday …
    Sunday, heating, cooling.
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

### `rule` — an Automatic Designer rule

```text
extern pumpe = PushButton(title: "Pumpe")
extern pumpe_ein = AutopilotRule(title: "Pumpe Ein")

rule pumpe_ein                      # weekday mornings
	active
	when weekday weekdays
	when time 09:00
	then pumpe on
end
```

A `rule` states what an existing rule of the app's Automatic Designer
does: whether it is `active`, its conditions (all must hold) and its
actions, in order. The slug is an `AutopilotRule` extern — rules are
created, named and deleted in the app; the declaration edits one.

- **`when time HH:MM`** — at that time of day, written like every time of day (`clock`, `At:`):
  `7:30` reads as `07:30`, seconds must be `:00`, and a quoted time is an error naming the bare one.
- **`when weekday <days>`** — on those days (`mon` … `sun`, `weekdays`,
  `weekend`, `daily`; no day twice).
- **`when <slug>.<state> <op> <value>`** — a state of a block
  the app shows: `==`, `!=`, `>`, `<`, `>=`, `<=` against a number, or
  `on`/`off` (1/0) for a digital state (`when pumpe.active == on`).
  The state names and condition codes are the app's own: the
  capability table the Miniserver serves the app
  (`loxone_config::autopilot_caps`), per app control type.
- **`when mode <mode> [on|off]`** — while an operating mode is
  (not) active; `<mode>` is the slug of a `Mode` the module declares or externs
  (`when mode party`), else the mode's title (`when mode "Urlaub"`). A title of a mode the module
  names is an error naming its slug.
- **`or`** — the conditions after it form another group: the
  rule fires when all conditions of any one group hold.
- **`then <slug> <command>`** — the app's command text to a block. It is bare when it is one word
  (`then pumpe on`, `then tor pulse`) and quoted otherwise (`then licht "Mood 2"`); a quoted one-word
  command is canonicalized to bare. Allowed are:
  - any command the capability table offers for the block's app type (`on`/`off` to a `PushButton`,
    `pulse` to a `PButtonT`, `<valN>` numbers filled in), and
  - any command a rule of the config already sends (the compile copies that action's code).

  Anything else is refused, with a list of the commands the block offers.
- **`then mode <mode> on|off`** — activates or deactivates an
  operating mode.
- **`when weather <field> <op> <value>`** — a value of the
  weather service: `temperature`, `dewpoint`, `felt_temperature`,
  `wind`, `gusts`, `wind_direction`, `irradiance`,
  `relative_irradiance`, `solar_irradiance`, `precipitation`,
  `pressure` with the operators of a state, `sunshine == on|off`.
- **`when day <op> <1…31>`** — the day of the month.
- **`when daylight [off]`** — while it is (not) day.
- **`when sunrise`**, **`sunset`**, **`dawn`**, **`dusk`** — at
  that moment of the day.
- **`when date YYYY-MM-DD`** — on that day. The app stores the
  local midnight as a Unix time, so the config must state a Central
  European `Timezone` (`Europe/Vienna`, `Europe/Berlin`, …).
- **`then notify "<subject>" "<text>"`** — a push notification
  to the app's users.
- **`then mail "<to>" "<subject>" "<text>"`** — a mail through
  the Loxone Cloud Mailer.
- **`then wait <duration>`** — a pause before the next action:
  `500ms`, `2.5s`, `5min`, `1h` (0.1 s … 24 h).
- **`then message <severity> "<subject>" "<text>" [admins]`** —
  an entry in the message center (`info`, `warning`, `error`; `admins`:
  only administrators see it). A `/` in subject or text is refused: the
  command carries them `/`-separated.

Compiling writes the rule's `APC=` payload only when the declaration differs from the rule as read.
It then writes it the way the app edits it: unchanged conditions and actions keep their JSON, and
`STA` follows `active`.

- `decompile` and `adopt` lift every rule of these shapes into a declaration: by its name (below) when
  no other rule has that name, else on an extern. The blocks it watches or drives become externs.
- A rule with other conditions or actions (text values, the weather type, calls, speech output) stays
  a comment and must not be declared.
- `loxl adopt --uuid <rule> --as <slug>` takes one rule into an existing project. An extern rule of
  the module with that slug loses its `extern` line and unnamed `rule`, and the named declaration
  takes their place. Its action targets must be in the lock already.
- Removing a declaration leaves the rule as last written.
- Top-level only.

**Named rules.** `rule <slug> "Name"` declares a rule by its name
instead of an extern — the slug is the declaration's own:

```text
rule pumpe_mittag "Pumpe Mittag"     # created if the config lacks it
	active
	when time 12:30
	then pumpe on
end
```

The compile takes the config's rule of that name; a name two rules share is refused. When there is
none, it creates the rule: inactive and empty until the body is written, placed below the config's
last rule on its page and filed like it. A config without any rule is refused. Create one in the app
once, since the app decides where rules live.

- The lock pins the rule's UUID (`rules`). A compile against a base from before the deployment
  creates it again under the same UUID.
- The source owns the name: changing it renames the rule. `loxl rename` carries the pin.
- Removing the declaration leaves the rule as last written. Only `removed <slug>` deletes it.
- A slug declared otherwise (an extern, a block) is refused, as is a named rule that is also the
  target of a `rule <extern>`.

### `moods` — the moods of a lighting controller

```text
moods wohnen                        # one set, any number of controllers
	"Viel Licht": all on, AQ4 rgb(100, 100, 100)
	"Lesen": AQ1 80%, AQ3 temp(60%, 2700K)
	"Kino": AQ4 rgb(0, 0, 20)
end

wohnzimmer_lc.moods = wohnen
esszimmer_lc.moods = wohnen
```

A `moods` set names a list of moods. A claim `<controller>.moods = <name>` makes it the mood list of
one LightController2 (Lichtsteuerung), an extern or a managed block. Any number of controllers may
claim one set. Each line is a mood: its name, then the values of its circuits (`AQ1` … `AQ18`,
Loxone Config's *Lichtkreise*):

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

The claim writes the controller's moods in the set's order, plus its off mood ("Aus"). Every
controller keeps the off mood and source never lists it. It stays where it is: first stays first,
last stays last with new moods before it, anywhere else keeps its index.

A mood keeps its identity by **name**:

- A mood the controller already has keeps its UUID, scene id and flags and only gets the set's
  values, so buttons and the app still address it.
- A new mood gets the next free scene id, the way Loxone Config numbers them.
- A mood the set leaves out is deleted. The built-in "Viel Licht" (the motion mood) and any mood the
  controller's motion settings name must stay in the set, with values of your choice.
- A claim whose moods already match changes nothing, byte for byte.

Wiring a switch's buttons to the controller's T5 inputs (`Sel1`…`Sel8`) is ordinary source. T5/n
selects the mood with scene id n, which is the n-th mood made, whatever its place in the list. So the
assignment is stored only in the moods' `SID`s ([loxone-format.md](loxone-format.md),
*LightController2 moods*).

A controller created from source has no moods until Loxone Config saves it. A claim on it first
writes what that save would (the stock "Viel Licht" and "Aus" with their defaults), then the set.
Its circuits have no actuator either: Loxone Config writes `NameAI<n>=` only from its mood editor. On
the Miniserver such a circuit is a switch, and a percentage reads 1 on its output
([loxone-format.md](loxone-format.md)). Declare the circuits in source, or set them up in Loxone
Config.

#### Circuits

```text
licht = LightController2("Licht", Circuit1: "dimmer", Circuit2: "switch")
dimmer.I <- licht.AQ1
relais.I <- licht.AQ2
```

`Circuit<n>:` declares the circuit behind `AQ<n>` the way the mood editor's *Lichtkreise* tab does:

- **`"switch"`** (*Schalter*) or **`"dimmer"`** (*Dimmer 0-100%*). The kind is stored in every mood's
  value of that circuit, so changing it clears those values, as the editor does. A `moods` claim then
  writes them anew.
- **The name.** A circuit without one is named after the actuator `AQ<n>` drives: a relay or Tree
  actuator, wired directly or through an `OutputRef`. A name the circuit has already, from Loxone
  Config or an earlier compile, stays.
- A controller without moods first gets the stock moods, as with a claim.
- Refused: `AQ<n>` driving nothing, more than one object, something other than an actuator, or a
  colour actuator (RGB and tunable white circuits stay with Loxone Config), and `Circuit6` and above.

`loxl decompile` and `loxl adopt` lift the named switch and dimmer circuits of a managed controller.

- The lock pins the UUID of every mood the compiler created until a base carries it
  ([lockfile-spec.md](lockfile-spec.md)).
- Removing a claim leaves the moods as last written.
- One claim per controller.
- Claims are allowed in a template body, so a room template can give every room the same moods:
  `<lc>.moods = <param>` with a set parameter (`template raum(stimmungen: moods = standard)`, called
  as `raum(stimmungen: kueche)`).

`loxl decompile` and `loxl adopt` lift a controller's own moods into a set and a claim, with the
controller as an extern.

- Controllers with the same moods share one set, named after the first (`<slug>_moods`).
- A controller with only the stock "Viel Licht" (every circuit on) is left out.
- A controller whose moods cannot be written as a set (two moods of the same name) keeps them in
  Loxone Config, with a `# moods <slug>: not lifted — …` comment that says why.
- Each mood is written so it compiles back exactly: `all <v>` for the value more than half the
  circuits share, then the circuits that differ. Adopted and compiled, a config comes back
  byte-identical.
- `loxl adopt --uuid` (one block) adds no mood claims.

`loxl diff` compares the moods of every LightController2, claimed or
not, one change per mood in these terms (`LightController2 "Licht"
mood "Lesen": all on -> AQ1 80%`) plus the order of the names; the
semantic fingerprint (v7) includes them, so `loxl drift` reports moods
edited in Loxone Config or the app.

### Push notifications: `OutputRefLM` and `recipients`

```text
extern frost = Notification(title: "Frostwarnung Garten")

warn = OutputRefLM(
	mirrors: frost,
	AI: zu_kalt.Q,
	On: "Frost im Garten",
	Off: "Kein Frost mehr",                      # optional
	MinimumTime: 12h,                            # optional, default 5 s
)

recipients familie
	user "alice"
	group "Familie"
end

frost.recipients = familie
```

A push notification (*Benachrichtigung*, type `Notification`) is
periphery: Loxone Config creates it under *Messages*, and the module
names it as an extern. Two things about it come from source:

- **What it says, and when.** An `OutputRefLM` block is the notification drawn on a page: a new mirror
  (`mirrors:` names the extern `Notification`) with the message parameters Loxone Config keeps in its
  `<LoggerMailer>` child.
  - `On:` is sent when `AI` turns on, `Off:` when it turns off. No text means no message.
  - `MinimumTime:` is the least time between two messages, a duration (`12h`, `30min`, or seconds as a
    number) that compiles to milliseconds.
  - The parameters take values, never wires. The block's title follows the notification's, like every
    ref's.
- **Who gets it.** A `recipients` set names users and groups. A claim
  `<notification>.recipients = <set>` makes them the recipients of one extern `Notification`: everyone
  the set lists gets the message, and nobody else. It works like `access` without schedules.
  - Compiling grants what the set lists (`ACT=` on the notification) and revokes the users and groups
    it leaves out.
  - Several notifications may claim one set. Notifications without a claim keep their recipients.
  - Members of the *Full access* group get every notification whatever the set says. Loxone
    Config shows them as inherited recipients.

Rules for `recipients` are those of `access` — a claim on an extern of
the right type (here `Notification`), one claim per extern, each
principal once, titles that name exactly one user or group, no lock
state, sets declared at the top level, the v1 form `recipients <extern>`
an error naming the replacement — and a recipient takes no schedule.
`loxl decompile` and `loxl adopt` lift both: a notification with recipients or an `OutputRefLM` becomes
an extern, its recipients a set and a claim (notifications with the same recipients share one set,
named after the first, `<slug>_recipients`), and each `OutputRefLM` whose message names its own
notification a managed block with `mirrors:` and its message.

## Name resolution and validation

- Externs, blocks, named signals, `let` constants, templates and
  `tiles`, `schedule`, `moods`, `access` and `recipients` sets share one
  namespace per module; duplicates are an error. A claim
  (`x.access = bewohner`) declares nothing: it names an object and a set
  of the claim's kind.
- Every port reference (argument bindings, `<-`, `=`) must name a declared
  extern or block, or a block of a template instance (`inst.Port` for its
  primary block, `inst.block.Port` for any other); every
  bare-identifier value must name a declared `let`.
- Instance names share the namespace too: an instance may not be named
  like a declaration.
- `<-`, port-`=` and attribute-`=` targets must be externs (see above).
- Duplicate parameter bindings, duplicate identical wire bindings, and a
  parameter next to a wire on the same port, in one argument list are
  errors.
- `removed`/`moved` must not contradict declarations or each other
  (details under the statements).
- Statement order is free; the conventional order is `defaults`, lets, externs,
  blocks, extern wires, extern assignments (port and attribute), with
  lifecycle statements (`moved`, `removed`) last.

`loxl check` performs all of the above plus the static builtin-table
checks (block types, port names, wire directions) — everything that does
not need the base config. `--json` emits the result machine-readably.

## Multi-file modules and projects

A module may be a directory of `*.loxl` fragments instead of one file.

- Fragments parse one by one (errors name the file), then join into one module, subdirectories
  included, in path order.
- Names resolve once over the whole module, so a fragment may use slugs declared in a sibling file.
- The split means nothing beyond the scope of `page` (below): one namespace, one lockfile, and **no
  `import` statement** (it would name a dependency with no effect). The convention is one file per page, with
  `_periphery.loxl` for the externs.
- Dot-entries (`.git`, editor caches) are skipped.
- `page` governs only its own fragment: from the statement to the next `page` or the end of the file.
  Blocks above a fragment's first `page` take the default page, whatever page the previous file
  named. Start every fragment that declares blocks with its `page` statement, as decompile and adopt
  do; `loxl lint` reports a fragment that does not while others do (`default-page`). Merge order only
  affects determinism.

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

## Lint

`loxl lint` gives advice; it never changes the compiled config. In a project it also reads the
compiled config. `dead-outputs` reports a block, extern or device whose outputs feed nothing there,
GUI wires included. These count as used:

- a wire from the output, an energy flow monitor that reads the block, a statistic it records;
- a value the app shows (an info-only control: `Visu` on, display "state only");
- a block that logs, mails or notifies (an `OutputRefLM`), and what feeds it;
- operating modes and calendar entries: schedules and rules read them by number;
- a block without outputs (a virtual output command, a Modbus actuator) when something drives it.

A switch or controller in the app that drives nothing is still reported; for a room controller and a
Touch Pure Flex controller the finding names what to wire. Two or more inputs of one device (Modbus
device, virtual HTTP or UDP input) that feed nothing are one finding on the device.
The dead commands of one Touch Pure Flex and command type are one `tile-dead-command` finding.

`no-actuator` reports a mood set that gives a lighting circuit a percentage while the circuit has no
actuator. Loxone Config assigns the actuator in its mood editor. Without one the Miniserver treats
the circuit as a switch, so `AQ1 40%` turns the light on and does not dim it. A controller created
from source has no actuators. Declare the circuit (`Circuit1: "dimmer"`), assign the actuator in
Loxone Config, or write `on`.

A finding that is deliberate stays out of the report with an allow comment on the line right above
the declaration:

```text
# lint: allow dead-outputs registers kept for later
solax_x3 = ModbusDev("Solax Hybrid X3", device: solax, Channel: 1)
```

It names one or more kinds (comma or space separated); the rest of the line is the reason. It covers
the declaration below it: a block, extern, instance (all its blocks), device (all its inputs),
constant, template, signal or `defaults`. Inside a template body it covers that block in every
instance. An allow that covers no finding, names no kind or stands above no declaration is reported
as `unused-allow`. Outside a project the kinds that need the compiled config (`dead-outputs`,
`tile-broadcast`, `tile-dead-command`, `no-access-right`, `no-actuator`, `implicit-zero`) are not
judged.

`implicit-zero` reports a block that already existed and has a port at 0 only because nothing sets it:
the compiled config has no `Def=` and no wire there, the source no value, and Loxone Config places the
block with another value (a `Monoflop` adopted without `Time` pulses for 0 s, not 2 s). `loxl lint --fix`
writes `Port: 0` into the block, at the port's place, and the compiled config stays byte-identical.
Write the value you want instead where 0 is not meant. A block a template creates is reported, not
fixed.

The language server offers both as quick fixes: on a finding it inserts the allow comment above the
declaration (or adds the kind to the allow already there), and on an `unused-allow` it removes the
comment.

## Canonical form

`loxl fmt` writes:

- statements in source order, single spaces between tokens, a blank line where the item kind
  changes and before and after every item that spans several lines; single-line items of one kind
  stay together;
- an argument-free call on one line (`b = And()`, `c = Or("Oder")`);
- a call on one line when it has at most three arguments (the label counts as one), no comments,
  and the line fits in 100 columns with its indentation (a tab counts four):
  `b = And(I1: x.Q, I2: on)`. Past three arguments the same holds when every argument but the label
  is a number, a unit value, a time or `on`/`off`:
  `s = ModbusASensor("Kompressor", ModbusAddress: 1100, ModbusDataType: 0, DestValHigh: 100)`;
- any other call with the label and each argument on its own tab-indented line with a trailing
  comma, and the `)` on its own line;
- a `tile` the same way one level deeper (`tile 2(click: "…", to: licht)` on one line, or a
  two-tab-indented list with the `)` on its own tab-indented line);
- each grant of an `access` or `recipients` set on its own tab-indented line.
- a whole-line comment directly above the statement it was written above, with no blank line
  between them. A blank line the source has below a comment stays (one, however many there were).

Values keep their variant: numbers bare, strings quoted, constant references bare, port references
dotted, unit values and `on`/`off` as written. Some values are canonicalized at parse time, so every
value has one spelling: a quoted string that reads exactly as a number (`"28"`) becomes the number, a
quoted one-word rule command (`then pumpe "on"`) the bare word, and a `clock` time its two-digit form.

`parse(to_text(m)) == m`, and `to_text` is a fixpoint.

## Errors

- Parse errors carry a 1-based line number.
- Unknown types, ports and constants suggest the closest known name when one is within a small edit
  distance.
- v0 statements (`block`, `wire`, `set`, `extern … match …`) get migration errors with the current
  spelling.
- v1 spellings that v2 dropped get errors that name the replacement ("write `clock 07:30` instead of
  `clock "07:30"`"). The language server offers it as a quick fix.
- A line that starts no statement gets an error listing every statement kind.
- In a directory module an error of the module check names the fragment and its line
  (`src/sued.loxl:12: compile error: …`); `check --json` gives them as `path` and `line`.
- One run reports every independent error, sorted by file and line (at most 50):
  - A line with a syntax error is skipped, and parsing goes on at the next statement. The rest of
    a failed argument list, or a failed body up to its `end`, is skipped with it.
  - When a file has syntax errors, only those are reported. Its names would otherwise show up as
    undeclared.
  - Name, type and port errors are reported for each statement. A name that is not declared is
    reported at its first use only. Statements about a block of an unknown type are not checked;
    the type error covers them.
  - The checks run in stages: syntax, then templates, then names, then types and ports. A stage
    with errors stops the later ones.

Compile errors and their remedies are in [agents.md](agents.md#errors-and-remedies).

## Changes from v1

v2 keeps every v1 file's meaning but one (`page` scope, below); what
changed is spelling, and each dropped spelling is an error that names its
replacement. The compile output of a migrated file is byte-identical.

- **`on`/`off`** are reserved boolean literals in every value position
  (`I2: on` = `I2: 1`). Text-boolean attributes (`Analog:`, `Signed:`,
  `CloseAfterSend:`) take only them: `Analog: "true"` → `Analog: on`.
  `decompile`/`adopt` write them.
- **`tick <duration>`**: `tick 5min`, `tick 2s dt 500ms` — a whole
  number of `dt` steps.
- **`clock` is bare**: `clock 07:30`, `clock 2026-01-03 07:30:15`
  (was `clock "07:30"`).
- **`expect … != …`** joins the test comparators.
- **Rule actions**: a one-word command is bare (`then pumpe on`; a
  quoted one is canonicalized, not refused); **`when date`** is bare
  (`when date 2026-12-24`, was quoted).
- The "expected a statement" error lists every statement kind.
- **Named signals**: `wp_bedarf = a.Q and b.AQ >= 5` names an expression's result (see
  [Named signals](#named-signals-slug--expr)). New syntax only; no v1 file changes.
  `wp_bedarf.title = "…"` gives the top block a label of its own.
- **Decompile and adopt write expressions**: the gate chains and `Formula` blocks an expression or
  a named signal compiled to come back as that expression, signal and `let`, where they compile to
  the same blocks again. The rest stays declared blocks.
- **Instance names**: a block of a template instance is `inst.block.Port`, its primary block (the
  body's only block, or the one `template t(…) -> block` names) is `inst.Port`. The expanded name
  `inst_block.Port` is an error naming the scoped one. The lock keys stay `inst_block`. Call-site
  forwards go to the primary block.
- **Set parameters**: `template raum(stimmungen: moods = standard)` passes a set by its bare name
  (`raum(stimmungen: kueche)`). The string forms `stimmungen = "standard"` and
  `stimmungen: "kueche"` are errors naming that spelling. `.schedule` and `.tiles` claims work in
  template bodies too. An object parameter takes no default.
- **`access` and `recipients` are named sets**, like `tiles`, `schedule` and `moods`:
  `access bewohner … end` declares a set, `haustuer.access = bewohner` claims it, and several Code
  Touches (or notifications, `frost.recipients = familie`) may share one. The v1 form
  `access haustuer … end` on the extern is an error naming the set and the claim. Decompile writes
  `<slug>_access` sets. Set parameters take `access` and `recipients`.
- **`page` is scoped to its file.** In a directory module a `page` statement governs the blocks after
  it up to the next `page` or the end of its fragment. v1 carried it into the following files, so a
  file without its own `page` took the previous file's page. Such a file now takes the default page;
  `loxl lint` reports it (`default-page`). A single-file module does not change.
- **`defaults <Type>(Port: value, …)`** gives every managed block of a type the values it does not
  bind (see [`defaults`](#defaults--the-parameters-every-block-of-a-type-shares)). Decompile and adopt
  factor shared values into it, write time values with a unit suffix (`Delay: 3h`) and keep the `<-`
  wires into one extern together. New syntax only; no v1 file changes, and `defaults` stays usable
  as a slug.
- **One rule for contextual words**: the section kinds (`tiles`, `schedule`, `moods`, `access`,
  `recipients`, `rule`, `defaults`) are names everywhere but at their section head and claim;
  `tiles`, `schedule` and `moods` were reserved in v1. `then wait|mode|notify|mail|message` is
  always the rule action; a block of that name as a `then` target is an error naming `loxl rename`.
- **Short calls go on one line**: `fmt` writes a call with up to three arguments that fits in 100
  columns on one line, and puts a blank line around every multi-line item. A call of more arguments
  goes on one line too when they are all numbers, unit values, times or `on`/`off`. A whole-line
  comment stays attached to the statement below it unless a blank line separates them. The compile
  output does not change.
- **`tick` and `dt` take `let` constants** (`tick warten`, `tick 3 dt takt`).
- **`when time`** reads the time like `clock` (`7:30` = `07:30`).
- **Scoped names everywhere**: `(inst.block, x).room = …` and `mirrors: inst.block` (extern
  matcher and minted ref) take the instance spelling. Compile, check and test errors spell a block
  of an instance that way too.
- A named signal whose name equals an expression's generated prefix (`a_i1 = …` next to an
  expression on `a.I1`) is an error naming both; v1 let them share block names.
- **Tooling**: `loxl lint` groups its findings by kind with counts (`--summary`: counts only) and
  reports bare durations that read better with their unit (`unit-suffix`); `loxl lint --fix` writes
  the suffixes. A new block gets Loxone Config's defaults on the ports the source leaves out;
  `implicit-zero` reports an older block that has 0 there, and `--fix` writes the 0. Module-check errors name the fragment and line. The language server hovers and
  goes to the definition of `inst.block` and `inst.block.Port`, completes argument keys (in
  `defaults` too, leaving out the ones the call binds already) and signal names, shows the
  defaults a template body's block takes, and offers the `unit-suffix` and `default-page` fixes. `dead-outputs` counts what the app shows,
  statistics, energy flow, messages and modes as used and reports a device's unused inputs once;
  `# lint: allow <kind>` keeps a deliberate finding out (see [Lint](#lint)).
- **One spelling per operating mode** in rules and schedules: a built-in mode in a schedule by its day
  word, a mode the module declares or externs by its slug (schedule lines take slugs now,
  `party 18:00-24:00`), any other by title. `when mode "Party"` next to `party = Mode("Party")` is
  an error naming `when mode party`; `"Montag" 06:00-07:00` one naming `mon 06:00-07:00`. `fmt`,
  `decompile` and `adopt` write the one spelling.
- **A calendar entry names its mode with `switches:`**, in the same one spelling
  (`switches: heating`, `switches: party`, `switches: "Urlaub"`), instead of the stored number
  `CalType: 10`. The number is an error naming the replacement; `fmt` rewrites a built-in mode's
  number, `adopt` writes `switches:`.
- **A calendar entry states its date as a date**: `on: 12-31`, `easter: 1`,
  `from: 2026-07-04, to: 2026-09-05` (one period), `from: 10-01, to: 04-30` (every year), instead
  of the stored `Mode:`, `Month:`, `Day:`, `Month2:`, `Day2:`, `From:`, `To:`, `EasterOffset:` and
  `WeekDayInMonth:`. They are an error naming the replacement; `fmt` and `adopt` write the date.

Migrating a v1 project: `Analog|Signed|CloseAfterSend: "true"/"false"`
→ `on`/`off`, unquote `clock "…"` and `when date "…"`, rewrite
`inst_block` as `inst.block` (or `inst` for the primary block) and turn
string-valued set parameters into set parameters, give each fragment
whose blocks relied on the previous file's `page` that `page` line, rename each
`access <extern>` / `recipients <extern>` to `access <extern>_access` /
`recipients <extern>_recipients` and add the claim
(`<extern>.access = <extern>_access`) after its `end`, rename blocks named `wait`, `mode`,
`notify`, `mail` or `message` that a rule sends commands to, then `loxl fmt`
(which also unquotes one-word `then` commands, puts short calls on one line and names a mode the
module declares by its slug, a calendar entry's built-in `CalType:` by its day word and its
stored date as a date; `loxl
compile` names the modes it cannot see) and, if you like,
`loxl lint --fix` for the unit suffixes.

## Versioning

This is v2 (see [Changes from v1](#changes-from-v1)). v0 was a pre-release revision; no v0 files are in
use. Anything not specified here, such as template nesting and template-local declarations, is not
part of the language. A future version keeps v2 files parsing unchanged, or every spelling it drops
is an error that names the replacement.
