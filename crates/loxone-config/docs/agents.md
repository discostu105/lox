# Working with Lox Logic as an AI agent

How AI agents (and scripts) write Loxone logic with this toolchain. Lox Logic exists so you never edit
`.Loxone` XML, and you must not.

The Claude Code skill [skills/lox-logic/SKILL.md](../skills/lox-logic/SKILL.md) carries the hard rules
and points here. Link its directory into `~/.claude/skills/` (or a house repository's
`.claude/skills/`) and an agent loads it when the task is Loxone logic. `tests/skill.rs` keeps its
example module valid.

## Hard rules

1. **Never edit `.Loxone` files directly.** All config changes go through
   `loxl compile`. The XML's identity model (UUIDs, counters, per-port
   identifiers, three concurrent writers) makes manual edits corrupting.
2. **Never hand-edit the lockfile**, except through the documented
   operations (`remove_object`, `rename_object` — exposed via the library;
   see [lockfile-spec.md](lockfile-spec.md)). Commit the lockfile together
   with the module.
3. **Never invent block types or port names.** Valid types for a block
   declaration (`slug = Type(…)`) are exactly the builtin table
   (`src/connectors.rs`) — list it with `loxl types`, and read one
   type's ports, what each does, directions, units, canvas labels and ranges with
   `loxl types <Type>` (`--json` for machines). Valid ports on an extern are whatever the base config actually has —
   discover them with `loxl decompile` / `loxl observe`, or read them from
   the error, which lists them (and suggests the closest name for typos).
   `loxl check` catches wrong types/ports/directions on managed blocks
   without needing the base config.
4. **Do not upload.** Compiling produces a file; sending it to a Miniserver
   is a separate, human-gated step (lox's `config` commands, outside this
   crate).
   Treat everything here as read-only toward the house.
5. **Prefer `iname`/`uuid` matching over `title`** for externs — titles are
   locale-volatile and may change under you. Where identical devices
   repeat a title ("Taster 1"), narrow it with `device: "<serial>"`.
6. Treat `NoMatch` / `AmbiguousMatch` as **stop-and-ask**: choosing among
   candidates is a human decision unless instructions already cover it.
7. **Keep real configs out of repositories.** A house config carries the
   owner's address, users, access codes and serials. `loxl scan <file or
   dir>` lists where (never what); exit 1 means it must not be committed.
   `loxl scan --redact <cfg> --out <copy>` writes a shareable copy.
   Synthetic fixtures scan clean.

## The loop

```sh
# 0. Orientation: what does the config contain, what is already managed?
loxl decompile current.Loxone            # full Lox Logic view, `page` sections
loxl decompile --out-dir view/ current.Loxone   # one module per page
loxl observe current.Loxone              # port evidence per block type
loxl types --search delay                 # which type does what (name or purpose)
loxl types Monoflop                       # purpose, ports (with what each does), units, labels
loxl templates                            # the template library (loxl templates <name> prints one)

# 1. Edit the module (.loxl). Whole-line # comments are preserved.

# 2. Validate cheaply before compiling (no base config needed — includes
#    block types, port names, and wire directions on managed blocks):
loxl check modules/beschattung.loxl       # human-readable, line numbers
loxl check --json modules/beschattung.loxl # machine-readable diagnostics
#    Every independent error comes back in one run, sorted by file and
#    line: fix them all, then check again (a later stage may find more).
loxl fmt --write modules/beschattung.loxl # canonicalize (non-destructive)

# 3. Compile against the current base:
loxl compile --base current.Loxone \
            --module modules/beschattung.loxl \
            --lock modules/beschattung.lock.json \
            --out out.Loxone
# --serial only needed the first time (recorded in the lock afterwards);
# --time only for reproducible builds (lock pins minted UUIDs regardless).
# In a directory with a lox.toml (a project: base/module/lock/out/serial/
# page as flat key = "value" lines), `loxl compile` alone does all of
# this, and check/fmt/drift default to the project's module and lock.

# 4. Show your work as a SEMANTIC diff, never an XML diff:
loxl diff current.Loxone out.Loxone
loxl diff --summary current.Loxone out.Loxone   # by name: a commit message

# 5. Sanity: the output must round-trip.
loxl roundtrip out.Loxone

# 6. Look at a page you changed: metrics, plus an SVG with the managed
#    blocks in blue and backward wires in red.
loxl render out.Loxone --page "Beschattung" \
            --lock modules/beschattung.lock.json --out page.svg
```

Compile places new blocks beside what they wire to, and keeps a block where Loxone Config last dragged
it. `--relayout "<page>"` places a page's managed blocks afresh; blocks wired to a ref tag drawn in
Loxone Config stay where they are. Placement keeps to the page's A3 sheet, packed tighter if it must
be, and compile warns about managed objects past its edge.

Show step 4's output when you propose a change:

- It lists added blocks, wires and parameter changes, and flags locale renames (`[locale?]`) so they
  are not mistaken for edits.
- Users, groups, credentials and access rights have their own section (`user`, `group`,
  `access code`, `NFC tag` lines). Credentials show as names, counts and short hashes only.
- `--json` gives the same diff as JSON.
- `--summary` names blocks by title and ports by name. Show that form to a human, and use it as the
  commit message.

The decompile view is for **reading, not compiling**. Compiled against the same base, it would
duplicate every block and claim every wire it shows. To take over existing logic, use `adopt`, which
writes a module and lockfile that compile:

```sh
# One-time takeover of existing managed-type blocks (never modifies the
# config; refuses to overwrite existing outputs):
loxl adopt current.Loxone --out-module modules/haus.loxl \
                          --out-lock modules/haus.lock.json
# Blocks whose rebuild would not be faithful are SKIPPED with a warning
# (they stay unmanaged and appear as pinned externs) — read the warnings.
# Then verify the takeover is a semantic no-op before editing anything:
loxl compile --base current.Loxone --module modules/haus.loxl \
             --lock modules/haus.lock.json --serial <serial> --out out.Loxone
loxl diff --exit-code current.Loxone out.Loxone   # must be empty
```

Blocks an expression or named signal compiled to come back as that expression, where it compiles to
the same blocks. A signal's name is not in the config, so an adopted signal is named after its label;
rename it with `loxl rename`.

## The language in 30 seconds

See [language.md](language.md) for the full spec. The sample is in canonical form (`loxl fmt`) and
passes `loxl check`.

```text
# Comments survive formatting.

let schwelle = 28 # a named constant

extern sonne = VirtualIn(iname: "VI3") # an existing object: yours to use, not to own
extern jal = AutoJalousie(title: "Beschattung Süd")
extern pumpe = PushButton(title: "Pumpe")

party = VirtualIn("Partymodus") # a new app switch in the "Virtual Inputs" folder

# Managed blocks. The argument list is the whole input side: a port
# reference wires, a value or constant sets the parameter.

temp_hoch = GreaterEqual("Temp über 28", Input1: sonne.Q, Input2: schwelle)
beschatten = And(I1: temp_hoch.Q, I2: sonne.Q)

jal.AutoShade <- beschatten.Q # a wire onto an extern port

heiss = sonne.Q >= 30 and not party.Q # a named signal: its top block is `heiss`

jal.TargetPos = 70 # an extern parameter, restored when the line goes

defaults Monoflop(Time: 5min) # for every Monoflop that does not bind `Time`

nachlauf = Monoflop(InputTrigger: heiss.Q)

extern sensor = LoxAIRDevice(uuid: "…") # a device created in Loxone Config

sensor.title = "Raumklima Bad"
sensor.room = "Bad" # its inputs and outputs in the old room follow

# A named set and its claims: who may open, under which schedule. One
# set serves any number of Code Touches. Users not listed lose access.

access bewohner
	user "alice": "Always"
	group "Residents": "Workdays"
end

extern tuer = NfcCodeTouch(uuid: "…")

tuer.access = bewohner

# An app rule: all `when`s must hold, the `then`s run in order. `or`
# starts another group of conditions.

extern regel = AutopilotRule(title: "Pumpe Ein")

rule regel
	active
	when weekday weekdays
	when time 09:00
	or
	when pumpe.active == on
	then pumpe on
end

rule party_licht "Party-Licht" # a named rule: the config's rule of that name, else a new one
	inactive
	when mode "Party"
	then mode "Party" off
end

moods wohnen # light moods by name; the off mood ("Aus") is implicit
	"Viel Licht": all on
	"Lesen": AQ1 80%, AQ3 temp(60%, 2700K)
end

extern licht = LightController2(title: "Lichtsteuerung")

licht.moods = wohnen

removed alt_gate # a confirmed delete of a managed block

moved alt_schatten -> beschatten # a rename that keeps the UUIDs
```

The value decides what an argument means: `Port: slug.Port` wires that source in, anything else sets
the port's parameter. `<-` and port-`=` work only on externs; everything about a managed block is in
its own argument list.

A named signal `x = <expr>` names an expression's result. Its top block is `x` (`x.Q`; an arithmetic
one is a `Formula`, read at `x.AQ`), and its sub-blocks are `x__<op><n>`. Use it instead of spelling an
inline expression's synthetic slug (`jal_autoshade__and1`) in a test or a second consumer. `x` counts as
a declared block, so deleting the line needs `removed x`. Its top block's label is the expression text;
`x.title = "Beschatten"` on the next line gives it another.

A template instance names its blocks through the instance. `bad.Q` is the output of its primary block
(the body's only block, or the one `template t(…) -> <block>` names), and `bad.melder.Q` of any other
block. The lock keys are `bad_melder`; source never writes those. A `moods`, `schedule`, `tiles`, `access` or `recipients` set
goes into a template by a set parameter: `template raum(stimmungen: moods = standard)`, called as
`raum(stimmungen: kueche)`.

Removing a block declaration makes the next compile **fail on purpose**. When deletion is intended,
add `removed <slug>`: it covers that one block and shows the intent in the diff. For a slug rename,
use `loxl rename`, or rename the declaration and add `moved <old> -> <new>`. Both statements become
no-ops once applied. `--allow-removals` authorizes every pending removal at once; prefer `removed`.

## Writing modules

- **Match externs by `iname:` or `uuid:`** where you can. Titles change with the locale. Narrow a
  repeated title with `room:`, `category:` or `device: "<serial>"`.
- **Ports are XML keys, not the KB's labels.** On an `AutoJalousie`, `EndUp`/`EndDown` are complete
  open/close (Co/Cc), `Safety` the wind alarm (Wa), `Window` the window contact, `Stop` Off. On a
  `LightController2`, `Move` is motion (Mo), `Reset` Off, `MoveOn` the motion extend time.
  `loxl types <T>` shows the labels.
- **Pages.** `page "Title"` puts the blocks that follow on that page, up to the next `page` or the end
  of the file. In a directory module, open every file that declares blocks with its `page`: a file
  without one puts its blocks on the default page, whatever the previous file said. `page "Title"
  (room: "…")` creates the page if it is missing (the room is required); without `room:` a missing
  title is an error.
- **Templates first.** Before building a common automation by hand, look in `loxl templates` (motion
  and staircase light, push-button, heating and humidity hysteresis, CO2 ventilation, door and water
  alarms, intrusion alarm, awning protection). `loxl templates <name> > pages/licht.loxl` copies one
  into the module. One line calls it (`flur = bewegungslicht(bewegung: pir_flur)`), and its result is
  the instance itself, `flur.Q`, wired to the actuator (`licht_flur.I <- flur.Q`). Sensors are read
  at `Q`.
- **Virtual and Modbus devices are blocks:** `wp = ModbusServer("WP", Address: "192.0.2.12:502")`,
  `regler = ModbusDev("Regler", device: wp)`,
  `vorlauf = ModbusASensor("Vorlauf", device: regler, ModbusAddress: 1)`. On a block, `device:` names
  the device (by slug) the I/O hangs in. Write Loxone's `\v` as `"\\v"`. `loxl adopt --uuid <device>`
  takes over an existing device with its I/O.
- **State every parameter the block needs.** One the source leaves out is 0 in Loxone Config, not the
  default `loxl types` shows: a `Monoflop` without `Time` pulses for 0 s, a `LightController2` without
  `MoveTimeout` has it at 0.
- **Shared parameters: `defaults <Type>(Port: value, …)`.** Every managed block of the type takes
  these values for the ports it does not bind (`defaults ModbusASensor(ModbusPollingCycle: 60)`).
  Values only, one statement per type, at the top level. A block's own binding wins. Decompile and
  adopt write it for a type whose blocks sit on one page and mostly share a value; hovering a block
  in the editor shows what it takes.
- **Settings of a physical input or output** are claims on its extern: `fuehler.Unit = "<v.1> °C"`,
  `fuehler.MinTime = 600`, `taster.Desc = "Klingel"`. The original returns when the line goes. `Desc`
  is the app's name for the control. Only `Desc`, the low ends of the correction, `MinVal`,
  `ResetTime` and `AverageTime` can be added; the rest must already be on the object.
- **Hardware is declared to own it:** `taster = TreeDevice("Taster", serial: "B0A1B2C3")`,
  `klingel = TreeSensor("Klingel", device: taster, iname: "I0", Desc: "Haustür", Statistics: on)`.
  The source owns title, room, category and the stated settings; nothing is restored when the line
  goes, and the element is never deleted. Bind ports in the argument list. `loxl adopt --uuid
  <device>` declares an existing device with all its I/O. Pairing stays in Loxone Config.
- **A Tree device can be created:** `melder = TreeDevice("Melder", kind: "PresenceSensorTree", tree:
  tree1)`, for the kinds in `tree_kinds.rs` only. It is added unpaired; pair it in Loxone Config.
  Delete it with `removed melder`. Never delete other hardware.
- **Push notifications** are created in Loxone Config and named as an extern `Notification(…)`.
  `OutputRefLM(mirrors: <it>, AI: x.Q, On: "…")` sends one, and a `recipients <set> … end`
  (`user "<title>"`) claimed with `<it>.recipients = <set>` says who gets it. `loxl adopt` lifts
  both.
- **Central blocks.** `zentral.members = (licht_1, licht_2)` lists the `LightController2`s of a
  `CentralLight` (or the `AutoJalousie`s of a `CentralShade`) in order. An alarm's member list stays
  with Loxone Config.
- **Schedules and modes.** A DayTimer's week is `schedule <set> … end` (`weekdays 06:00-08:00`,
  `weekend off`, `holiday …`) with `timer.schedule = <set>`. `party = Mode("Party")` creates a mode,
  and `party.Q` is on while it is active. Rules and schedules name a mode one way: a built-in one in a
  schedule by its day word (`mon`), one the module declares or externs by its slug
  (`party 18:00-24:00`, `when mode party`), any other by title (`when mode "Urlaub"`). A calendar
  entry names the mode it switches on the same way and states its date as a date,
  `CalendarEntry("Heizperiode", switches: heating, from: 10-01, to: 04-30)`, never with its
  `CalType` number or its stored `Mode`, `Month`, … attributes.
- **Users are never declared.** They belong to the Miniserver's user management; source names them by
  title.
- **Light moods.** Values per circuit kind are `on`/`off`/`40%`/`rgb(r, g, b)`/`temp(…)`. Claim a set
  in a room template to give every room the same moods. Switch buttons go to `Sel1`…`Sel8`: `Sel<n>`
  selects the mood with scene id n, the n-th mood created. `loxl adopt` lifts existing moods into sets
  (`<slug>_moods`); start from those instead of retyping them. A controller created from source
  dims only with its circuits declared: `Circuit1: "dimmer"` names the circuit after the actuator
  `AQ1` drives (wire it first).
- **Taking over GUI logic.** Use `loxl adopt`, then confirm with `loxl diff --exit-code` that the
  takeover changed nothing. Adopting an extern `--as` its own slug promotes it, and its wires move
  into the declaration. `loxl coverage` lists what is not managed yet, including attributes of managed
  blocks still carried from the base and the parameter that claims each (a PulseAt's time is
  `At: 19:00`, the name the app shows is `Desc: "…"`). `loxl types <Type>` lists a type's settings.
- **The app.** A managed block takes `.room`/`.category`, never `.title` (its label is the title). The
  app lists a control only with both. The push adds app entries for the types Lox Logic knows
  (switches, inputs, jalousies, central blocks, meters, …) and names the rest; a save from Loxone
  Config lists those.
- **Tests simulate documented behavior only** ([simulator.md](simulator.md)) and name any block or
  input in the way. A lighting controller simulates motion, DisP, Off, M+/M−, T5 (single, double and
  triple clicks, long clicks and mixing, the 30 s rule), Mood, On, Buzzer, Lc1–8, MBr, Alarm (set Afi
  and MaxAbr) and output M, as a Miniserver did in a live test. A circuit without an actuator reads 1
  for any percentage, and a controller created from source has none until `Circuit<n>:` declares
  it: `loxl lint` reports it as `no-actuator`.

## Errors and remedies

Many wrong-name errors end with a ``did you mean `…`?`` suggestion — trust
it only when it matches what you intended.

| Error contains | Meaning | Remedy |
|---|---|---|
| `line N` (from `check`) | syntax/reference error in the module | fix that line; `loxl fmt` shows canonical form. `check` lists every error at once: fix them all before checking again |
| `no object matches` | the extern's spec matches nothing of that type | check type and spelling; `loxl decompile` the base to see candidates |
| `matches N objects` + candidate UUIDs | several objects match | pin one with `uuid: "<one of the candidates>"`; ask if unclear |
| `not in the verified builtin table` | the compiler cannot create this type | use a verified type, or declare the object `extern` (create it once in Loxone Config) |
| `unknown port … known ports: …` | wrong port name on a managed block | use a listed port (caught by `check`, no base needed) |
| `is the canvas label of port` | a canvas label (`Tr`, `D`) used as a port name | use the port the error names (`InputTrigger`, `Time`) |
| `a Loxone Config save turns it into` | Config would store the value differently (`Poll: 5` as 10, a digital input's `MaxVal: 5` as 1) | write the value the error names |
| `a Loxone Config save writes it as` | Config would store the title differently (`Max<Min` as `Max_Min`, a space at the end as `_`) | write the title the error names |
| `does not fit. The port is documented in …` | a unit suffix that cannot mean the port's unit (`Time: 70%`) | write the value in the named unit (`loxl types <Type>`) |
| `is not an exact number of minutes` | a time on a minutes port that minutes cannot hold exactly (`10s`) | write it in `min` (`0.5min`), or pick a value that fits |
| `ends before it starts` / `overlap on` (schedule) | a schedule interval wraps midnight, or two intervals of one day overlap | split at `24:00`/`00:00`; merge or shorten the intervals |
| `the timer is analog, so … needs a value` / `the timer is digital, so` | interval values do not fit the DayTimer's analog flag | analog: `HH:MM-HH:MM = <value>` on every interval; digital: drop the values |
| `the config has no operating mode titled "…"` | a quoted mode title in a schedule line that names no `Mode` object | use one of the titles the message lists, or create the mode (`party = Mode("Party")`) |
| `no <C Type="VirtualInCaption"> to hold a new VirtualIn` | the base has no "Virtual Inputs" folder | create one virtual input in Loxone Config once, then compile again |
| `` `Analog:` is `on` or `off` `` | a VirtualIn's `Analog:` got another value | write `Analog: on` (or drop it: digital) |
| `` Write `X` instead of `Y` `` | an old spelling: `Analog: "true"`, `clock "07:30"`, `when date "2026-12-24"`, a mode by title that the module names by slug (`"Party"` for `party`), a built-in mode by title in a schedule (`"Montag"` for `mon`), a calendar entry's mode number (`CalType: 10` for `switches: heating`) or stored date (`Mode: 4, Month: 10, …` for `from: 10-01, to: 04-30`) | write what the message names (the language server offers it as a quick fix; `loxl fmt` on the module directory rewrites declared modes) |
| `unknown day` | a schedule day that is no day word and no `Mode` of the module | fix the spelling, declare the mode (`extern x = Mode(title: "…")`), or quote its title |
| `` Write `a.Q` instead of `a_b.Q` `` | an instance's block named by its lock key | write the scoped name; `a.Q` when `b` is the primary block |
| `has no primary block` | `a.Q` on an instance whose template has several blocks | name the block (`a.<block>.Q`), or mark one with `template t(…) -> <block>` |
| `` is not a `moods` set `` (or `schedule`/`tiles`) | a set parameter got a name that no set of that kind declares | declare the set, or pass one of its kind |
| `` is on or off `` | a text-boolean attribute (`Analog:`, `Signed:`, `CloseAfterSend:`) got another value | write `on` or `off` |
| `is not a whole number of … ticks` | `tick <duration>` that `dt` does not divide (`tick 1s dt 300ms`) | make the duration a multiple of `dt`, or write `tick <n>` |
| `a plain tick count or a duration` / `positive whole number` (tick) | a `tick` constant that holds no count or time (`let x = on`, `let x = 1.5`) | give the `let` a whole count or a time, or write the value on the `tick` line |
| `is a reserved word and cannot be used as a name` | a name that is a keyword everywhere (`tick`, `page`, `on`, `and`, …) | pick another name; section kinds (`moods`, `rule`, …) are fine as names |
| `cannot be a rule target` | `then wait …` (or `mode`, `notify`, `mail`, `message`) where a block of that name was meant | rename the block: `loxl rename wait <new>` |
| `would both name their blocks` | a named signal `a_i1 = …` and an expression on `a.I1` would generate the same block names | rename the signal (or the block `a`) with `loxl rename` |
| `` uses `!=`, which lox-cli's `lox sim` lacks `` (from `test --lox`) | lox-cli's simulator has no `!=` | run the test in the built-in simulator, or rewrite as `==` |
| `out-of-range` (from `lint`) | value outside the documented range, read in the port's unit | usually a unit slip (`AlarmClockPeriod: 2h` = 120 min); fix, or keep if deliberate |
| `has no port … in the base config (ports: …)` | the extern port's `<Co>` is missing from the base | use a listed port, or wire or set the port once in Loxone Config so it exists |
| `in the lockfile but not in the source` | managed block vanished from source | typo → restore the line; delete → add `removed <slug>`; rename → `moved <old> -> <new>`; unmanage → `remove_object` |
| `changed type` | slug reused for a different block type | new slug, or `remove_object` first |
| `is an output port and cannot be used as …` | wire direction wrong | swap source and sink |
| `targets managed block` | `<-` or port-`=` used on a block you declared | move the wire/parameter into that block's argument list |
| `targets named signal` | `<-`, port-`=`, `.room` or `.category` on an `x = <expr>` signal | change the expression; its inputs are its operands. Only `.title` is allowed |
| `an alias is not a block` | `x = a.Q`: a named signal needs an expression | use `a.Q` directly where `x.Q` would go |
| `changed its top operator` | a named signal's top `and`/`or`/comparison changed, and with it its block type | give the signal a new name and add `removed <old>` |
| `is old syntax` | a `block`/`wire`/`set`/`match`/`use` keyword from the old grammar | rewrite the line in the spelling the error shows |
| `duplicate parameter` / `duplicate wire` | same argument twice in one argument list | delete one (fan-in with *different* sources is allowed) |
| `has both a value … and a wire` | a port bound to a constant and to a wire in one argument list | keep one, since a wired input ignores its value |
| `` two `defaults T(…)` statements `` | a type has two `defaults` statements | merge their arguments into one |
| `` `defaults` … is not a value `` / `takes no label string` | a wire, expression or label in `defaults` | write it on each block; `defaults` takes values only |
| `says which object a block is` | `device:`/`mirrors:` in `defaults` | write it on each block |
| `cannot adopt` + `would set it to` (from `adopt --uuid`) | the adopted block leaves a defaulted port unset | take the port out of the `defaults` statement and bind it on the other blocks, or set the port on the block in Loxone Config |
| `unit-suffix` (from `lint`) | a bare time value on a port documented in milliseconds, seconds or minutes (`Delay: 10800`), also in a port assignment or a template parameter | `loxl lint --fix` writes the unit (`Delay: 3h`); the compile output does not change. The language server offers it as a quick fix |
| `default-page` (from `lint`) | a fragment of a directory module without its own `page` line, while others have one | add the `page` line; the language server offers the previous file's page as a quick fix |
| `dead-outputs` (from `lint`) | a block, extern or device whose outputs reach nothing in the compiled config: no wire, not shown in the app as a value, no statistic, no energy flow, no message | wire it, delete it, or keep it with `# lint: allow dead-outputs <reason>` on the line above |
| `no-actuator` (from `lint`) | a mood gives a percentage to a lighting circuit without an actuator: the Miniserver switches it on instead of dimming | declare the circuit (`Circuit1: "dimmer"`, with `AQ1` wired to the actuator), assign the actuator in Loxone Config's mood editor (the next base has it), or write `on` |
| `tile-dead-command` (from `lint`) | Touch Pure Flex commands for a block type that no block linked to the device has | link a block of that type, empty the tiles (`tile 2()`), or `# lint: allow tile-dead-command` above the extern |
| `unused-allow` (from `lint`) | a `# lint: allow` that covers no finding, names no kind or stands above no declaration | delete it, fix the kind, or move it right above its declaration; the language server offers "Remove this allow" |
| `unused-default` (from `lint`) | no block takes the default: no block of the type, or every block binds the port | delete the statement or the port |
| `undeclared constant` | bare identifier in value position | declare `let <name> = …`, or quote the value if a string was intended |
| `neither slug is in the lockfile` (from `moved`) | rename references unknown identity | check both spellings against the lock; a first compile needs no `moved` |
| `is an attribute parameter, not a connector` | wire aimed at e.g. `Formula:` | pass a value instead: `Formula: "I1+I2"` |
| `warning: cannot adopt` (from `adopt`) | rebuilding that block would lose something (e.g. an input inversion `Inv=`); it stays unmanaged | leave it unmanaged, or remove that setting in Loxone Config and adopt again |
| `base has N duplicate UUIDs` | two objects or connectors in the base share one UUID (another push tool re-keyed refs) | open and save the config once in Loxone Config, which gives the duplicates new UUIDs, then use that save as the base. Never edit UUIDs by hand |
| `Output would contain duplicate UUIDs` | a new UUID hit one the base created in the same second, or the lock maps two blocks onto one UUID | compile again or pass another `--time`; if it persists, inspect the lock entries with the named UUIDs |
| `targets managed block` + `Give it a label` | `.title` on a block you declared | write the title as the block's label: `x = Type("…", …)` |
| `changes what extern … is matched by` | the extern is matched by the attribute it claims | pin it by `uuid:` or `iname:` |
| `no room titled` / `no category titled` | `.room`/`.category` names no room or category in the base | use a listed title, or create the room or category in Loxone Config first |
| `has no room binding` / `has no category binding` | the object has no `<IoData Pr=/Cr=>` to change | assign a room or category once in Loxone Config, then claim it from source |
| `claim the … of the same object` | two externs resolve to one object and claim the same attribute | keep one assignment |
| `numeric character references as literal text` (from `drift`) | a foreign XML writer left `&#xA;` text in programs, rules or descriptions; Loxone reads it verbatim | `loxl repair <cfg> --out <fixed>`, then compile against `<fixed>` as the base |
| `no user titled` / `no group titled` / `no access schedule titled` | an `access` grant names nothing in the base | use the title as Loxone Config or the app shows it; create the user, group or schedule there first |
| `users are titled` / `groups are titled` / `schedules are titled` | several objects share the title an `access` grant names | give them distinct titles in Loxone Config or the app |
| `access rights belong to an NFC Code Touch` | `.access` claimed on an extern of another type | claim `access` sets only on an `NfcCodeTouch` extern |
| `but access rights are a named set` / `but recipients are a named set` | the v1 form `access <extern> … end` / `recipients <extern> … end` | rename the set as the error says and add the claim after its `end` (`<extern>.access = <extern>_access`); the language server does both |
| `A block takes one` (`access`/`recipients`) | two claims of the same kind on one block | merge the sets into one; several blocks may share a set, a block takes one |
| `which the base config does not have` (page) | no page has the title | fix the title, or create the page: `page "…" (room: "…")` |
| `needs a real room for a page` | `page … (room: "Nicht zugeordnet")` | name the room the page belongs to |
| `page "…" (…) is titled "…" in the base config` | a page Lox Logic created was renamed in Loxone Config | write the new title in the source (without arguments the page belongs to Loxone Config), or rename it back |
| `recipients belong to a push notification` / `mirrors a push notification` | a `.recipients` claim or an `OutputRefLM` on something other than a `Notification` | name an extern `Notification(…)`; create the notification in Loxone Config first |
| `a recipient takes no schedule` | `user "x": "…"` in a `recipients` body | write `user "x"` |
| `members belong to a` / `the members of a … are` | `.members` on a block that is no central block, or a member of the wrong type | claim `.members` on a `CentralLight` (members `LightController2`) or `CentralShade` (members `AutoJalousie`) |
| `a rule belongs to an Automatic Designer rule` / `not an Automatic Designer rule` | `rule` on a block of another type | declare `rule` only on an `AutopilotRule` extern |
| `which Lox Logic cannot write` (rule) | the rule has conditions or actions Lox Logic cannot write (text values, the weather type, calls, speech output) | remove the declaration; edit that rule in the app |
| `` knows no action code (`descID`) `` | a `then` to a block type/command no rule of the config uses yet | create one rule action of that kind in the app once; the compile copies its code |
| `does not fit a switch or dimmer circuit` (and the other kinds) | a mood value of the wrong kind for that circuit (`rgb()` on a relay, `on` on a colour circuit) | use what the message says the circuit takes; `off` fits everywhere |
| `is the controller's built-in off mood` | the set lists "Aus" (SID 778) | leave it out; every controller keeps it, always off |
| `cannot be dropped` (moods) | the set leaves out "Viel Licht" (the motion mood) or a mood the motion settings name | keep it in the set; its values are yours to set |
| `the controller has N circuits` | `AQ<n>` above the controller's circuit count | name an existing circuit |
| `recorded for block … no longer exists` | the pinned page was deleted from the base | re-pin: edit the lock entry's `page_uuid`, or `remove_object` the block to place it afresh |

After a failed `compile` the lockfile on disk is untouched (the CLI saves only on success). Fix and
run again.

## Invariants you can rely on

- Compilation is deterministic: unchanged inputs → byte-identical output
  and lockfile. If you see a diff, something actually changed.
- Recompiling the compiler's own output is a no-op (fixpoint) — safe to
  re-run freely.
- The compiler touches only what the source declares. Everything else in the config is kept byte for
  byte. It touches:
  - its managed blocks;
  - wires it drew onto extern ports (`<-`) and `Def=` values it set on extern ports;
  - the title, room and category it claimed on externs, and that claim's followers;
  - tile programs it declared;
  - access rights (`TIM=`) on Code Touches with an `access` claim;
  - recipients (`ACT=`) of notifications with a `recipients` claim;
  - the `APC=`/`STA=` of rules with a `rule` declaration whose content changed;
  - the moods (`<LightscenesC>`) of controllers with a `moods` claim;
  - schedules of DayTimers with a `schedule` claim, and the settings it claimed.
- Extern-port originals and extern wires are tracked in the lock, so
  deleting Lox Logic lines cleanly reverts their effects on the next compile.
