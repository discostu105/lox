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
   type's ports, directions, units, canvas labels and ranges with
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
loxl types Monoflop                       # purpose, ports, units, labels of a mintable type
loxl templates                            # the template library (loxl templates <name> prints one)

# 1. Edit the module (.loxl). Whole-line # comments are preserved.

# 2. Validate cheaply before compiling (no base config needed — includes
#    block types, port names, and wire directions on managed blocks):
loxl check modules/beschattung.loxl       # human-readable, line numbers
loxl check --json modules/beschattung.loxl # machine-readable diagnostics
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

## The language in 30 seconds

See [language.md](language.md) for the full spec.

```text
# comment (all comments survive formatting)
let schwelle = 28                    # named constant

extern sonne = VirtualIn(iname: "VI3")           # existing object, not yours
party = VirtualIn("Partymodus")      # a new app switch, minted into the
                                     # "Virtual Inputs" folder
extern jal = AutoJalousie(title: "Beschattung Süd")

temp_hoch = GreaterEqual(            # yours, compiler-owned; the argument
	"Temp über 28",                  # list is the block's whole input side:
	Input1: sonne.Q,                 # port reference  -> wire into Input1
	Input2: schwelle,                # literal or let  -> Def= parameter
)
beschatten = And(                    # gates are fixed 2-input
	I1: temp_hoch.Q,
	I2: sonne.Q,
)

jal.AutoShade <- beschatten.Q        # wire onto an extern port
heiss = sonne.Q >= 30 and not party.Q  # a named signal: gate blocks,
                                     # the top one is `heiss` (`heiss.Q`)
jal.TargetPos = 70                   # Def= on an extern port; original
                                     # auto-restored when the line is removed
extern sensor = LoxAIRDevice(uuid: "…")  # a device (created in Loxone Config)
sensor.title = "Raumklima Bad"       # assign title, room, category;
sensor.room = "Bad"                  # children in the device's room follow

extern tuer = NfcCodeTouch(uuid: "…")
access tuer                          # who may open it, under which schedule
	user "alice": "Always"           # users, groups and schedules
	group "Residents": "Workdays"    # by title; the rest lose access
end

extern regel = AutopilotRule(title: "Pumpe Ein")
rule regel                           # an app rule: active, all
	active                           # `when`s must hold, `then`s in order
	when weekday weekdays
	when time 09:00
	then pumpe on
end

rule party "Party-Licht"             # a block's state, a mode,
	active                           # `or` for another group
	when bewegung.active == on
	when mode "Party"
	or
	when helligkeit.value < 20
	then mode "Party" off
end

rule mittag "Pumpe Mittag"           # a named rule: the config's
	inactive                         # rule of that name, else created
	when time 12:30
	then pumpe on
end

moods wohnen                         # light moods, by name; the off
	"Viel Licht": all on             # mood ("Aus") is implicit
	"Lesen": AQ1 80%, AQ3 temp(60%, 2700K)
end
extern licht = LightController2(title: "Lichtsteuerung")
licht.moods = wohnen                 # also inside a template body

removed old_gate                     # confirmed delete of a managed block
moved beschatten -> schatten_gate    # slug rename, UUIDs survive
```

The value decides what an argument means: `Port: slug.Port` wires that source in, anything else sets
the port's parameter. `<-` and port-`=` work only on externs; everything about a managed block is in
its own argument list.

A named signal `x = <expr>` names an expression's result. Its top block is `x` (`x.Q`; an arithmetic
one is a `Formula`, read at `x.AQ`), and its sub-blocks are `x__<op><n>`. Use it instead of spelling an
inline expression's synthetic slug (`jal_autoshade__and1`) in a test or a second consumer. `x` counts as
a declared block, so deleting the line needs `removed x`.

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
- **Pages.** `page "Title"` puts the blocks that follow on that page. `page "Title" (room: "…")`
  creates the page if it is missing (the room is required); without `room:` a missing title is an
  error.
- **Templates first.** Before building a common automation by hand, look in `loxl templates` (motion
  and staircase light, push-button, heating and humidity hysteresis, CO2 ventilation, door and water
  alarms, intrusion alarm, awning protection). `loxl templates <name> > pages/licht.loxl` copies one
  into the module. One line calls it (`flur = bewegungslicht(bewegung: pir_flur)`), and its result is
  `<instance>_ausgang.Q`, wired to the actuator (`licht_flur.I <- flur_ausgang.Q`). Sensors are read
  at `Q`.
- **Virtual and Modbus devices are blocks:** `wp = ModbusServer("WP", Address: "192.0.2.12:502")`,
  `regler = ModbusDev("Regler", device: wp)`,
  `vorlauf = ModbusASensor("Vorlauf", device: regler, ModbusAddress: 1)`. On a block, `device:` names
  the device (by slug) the I/O hangs in. Write Loxone's `\v` as `"\\v"`. `loxl adopt --uuid <device>`
  takes over an existing device with its I/O.
- **Settings of a physical input or output** are claims on its extern: `fuehler.Unit = "<v.1> °C"`,
  `fuehler.MinTime = 600`, `taster.Desc = "Klingel"`. The original returns when the line goes. `Desc`
  is the app's name for the control. Only `Desc`, the low ends of the correction, `MinVal`,
  `ResetTime` and `AverageTime` can be added; the rest must already be on the object.
- **Push notifications** are created in Loxone Config and named as an extern `Notification(…)`.
  `OutputRefLM(mirrors: <it>, AI: x.Q, On: "…")` sends one, and `recipients <it> … end`
  (`user "<title>"`) says who gets it.
- **Schedules and modes.** A DayTimer's week is `schedule <set> … end` (`weekdays 06:00-08:00`,
  `weekend off`, `holiday …`) with `timer.schedule = <set>`. Any operating mode can be named by title
  (`"Party" 18:00-24:00`). `party = Mode("Party")` creates a mode, and `party.Q` is on while it is
  active.
- **Users are never declared.** They belong to the Miniserver's user management; source names them by
  title.
- **Light moods.** Values per circuit kind are `on`/`off`/`40%`/`rgb(r, g, b)`/`temp(…)`. Claim a set
  in a room template to give every room the same moods. Switch buttons go to `Sel1`…`Sel8`: `Sel<n>`
  selects the mood with scene id n, the n-th mood created. `loxl adopt` lifts existing moods into sets
  (`<slug>_moods`); start from those instead of retyping them.
- **Taking over GUI logic.** Use `loxl adopt`, then confirm with `loxl diff --exit-code` that the
  takeover changed nothing. Adopting an extern `--as` its own slug promotes it, and its wires move
  into the declaration. `loxl coverage` lists what is not managed yet, including attributes of managed
  blocks still carried from the base (a PulseAt's time is `At: 19:00`).
- **The app.** A managed block takes `.room`/`.category`, never `.title` (its label is the title). The
  app lists a control only with both. The push adds app entries for the types Lox Logic knows
  (switches, inputs, jalousies, central blocks, meters, …) and names the rest; a save from Loxone
  Config lists those.
- **Tests simulate documented behavior only** ([simulator.md](simulator.md)) and name any block or
  input in the way. A lighting controller simulates motion, DisP, Off, M+/M−, T5 (single, double and
  triple clicks, long clicks and mixing, the 30 s rule), Mood, On, Buzzer, Lc1–8, MBr, Alarm (set Afi
  and MaxAbr) and output M, as a Miniserver did in a live test. A circuit without an actuator reads 1
  for any percentage, and a controller created from source has none.

## Errors and remedies

Many wrong-name errors end with a ``did you mean `…`?`` suggestion — trust
it only when it matches what you intended.

| Error contains | Meaning | Remedy |
|---|---|---|
| `line N` (from `check`) | syntax/reference error in the module | fix that line; `loxl fmt` shows canonical form |
| `no object matches` | the extern's spec matches nothing of that type | check type and spelling; `loxl decompile` the base to see candidates |
| `matches N objects` + candidate UUIDs | several objects match | pin one with `uuid: "<one of the candidates>"`; ask if unclear |
| `not in the verified builtin table` | the compiler cannot create this type | use a verified type, or declare the object `extern` (create it once in Loxone Config) |
| `unknown port … known ports: …` | wrong port name on a managed block | use a listed port (caught by `check`, no base needed) |
| `is the canvas label of port` | a canvas label (`Tr`, `D`) used as a port name | use the port the error names (`InputTrigger`, `Time`) |
| `does not fit. The port is documented in …` | a unit suffix that cannot mean the port's unit (`Time: 70%`) | write the value in the named unit (`loxl types <Type>`) |
| `is not an exact number of minutes` | a time on a minutes port that minutes cannot hold exactly (`10s`) | write it in `min` (`0.5min`), or pick a value that fits |
| `ends before it starts` / `overlap on` (schedule) | a schedule interval wraps midnight, or two intervals of one day overlap | split at `24:00`/`00:00`; merge or shorten the intervals |
| `the timer is analog, so … needs a value` / `the timer is digital, so` | interval values do not fit the DayTimer's analog flag | analog: `HH:MM-HH:MM = <value>` on every interval; digital: drop the values |
| `the config has no operating mode titled "…"` | a quoted mode title in a schedule line that names no `Mode` object | use one of the titles the message lists, or create the mode (`party = Mode("Party")`) |
| `no <C Type="VirtualInCaption"> to hold a new VirtualIn` | the base has no "Virtual Inputs" folder | create one virtual input in Loxone Config once, then compile again |
| `` `Analog:` is `on` or `off` `` | a VirtualIn's `Analog:` got another value | write `Analog: on` (or drop it: digital) |
| `` Write `X` instead of `Y` `` | an old spelling: `Analog: "true"`, `clock "07:30"`, `when date "2026-12-24"` | write what the message names (the language server offers it as a quick fix) |
| `` is on or off `` | a text-boolean attribute (`Analog:`, `Signed:`, `CloseAfterSend:`) got another value | write `on` or `off` |
| `is not a whole number of … ticks` | `tick <duration>` that `dt` does not divide (`tick 1s dt 300ms`) | make the duration a multiple of `dt`, or write `tick <n>` |
| `` uses `!=`, which lox-cli's `lox sim` lacks `` (from `test --lox`) | lox-cli's simulator has no `!=` | run the test in the built-in simulator, or rewrite as `==` |
| `out-of-range` (from `lint`) | value outside the documented range, read in the port's unit | usually a unit slip (`AlarmClockPeriod: 2h` = 120 min); fix, or keep if deliberate |
| `has no port … in the base config (ports: …)` | the extern port's `<Co>` is missing from the base | use a listed port, or wire or set the port once in Loxone Config so it exists |
| `in the lockfile but not in the source` | managed block vanished from source | typo → restore the line; delete → add `removed <slug>`; rename → `moved <old> -> <new>`; unmanage → `remove_object` |
| `changed type` | slug reused for a different block type | new slug, or `remove_object` first |
| `is an output port and cannot be used as …` | wire direction wrong | swap source and sink |
| `targets managed block` | `<-` or port-`=` used on a block you declared | move the wire/parameter into that block's argument list |
| `targets named signal` | `<-`, port-`=` or an attribute on an `x = <expr>` signal | change the expression; its inputs are its operands |
| `an alias is not a block` | `x = a.Q`: a named signal needs an expression | use `a.Q` directly where `x.Q` would go |
| `changed its top operator` | a named signal's top `and`/`or`/comparison changed, and with it its block type | give the signal a new name and add `removed <old>` |
| `is old syntax` | a `block`/`wire`/`set`/`match`/`use` keyword from the old grammar | rewrite the line in the spelling the error shows |
| `duplicate parameter` / `duplicate wire` | same argument twice in one argument list | delete one (fan-in with *different* sources is allowed) |
| `has both a value … and a wire` | a port bound to a constant and to a wire in one argument list | keep one, since a wired input ignores its value |
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
| `not an NFC Code Touch` | `access` on an extern of another type | declare `access` only on an `NfcCodeTouch` extern |
| `which the base config does not have` (page) | no page has the title | fix the title, or create the page: `page "…" (room: "…")` |
| `needs a real room for a page` | `page … (room: "Nicht zugeordnet")` | name the room the page belongs to |
| `page "…" (…) is titled "…" in the base config` | a page Lox Logic created was renamed in Loxone Config | write the new title in the source (without arguments the page belongs to Loxone Config), or rename it back |
| `not a push notification` / `mirrors a push notification` | `recipients` or an `OutputRefLM` on something other than a `Notification` | name an extern `Notification(…)`; create the notification in Loxone Config first |
| `a recipient takes no schedule` | `user "x": "…"` in a `recipients` body | write `user "x"` |
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
  - access rights (`TIM=`) on Code Touches with an `access` declaration;
  - recipients (`ACT=`) of notifications with a `recipients` declaration;
  - the `APC=`/`STA=` of rules with a `rule` declaration whose content changed;
  - the moods (`<LightscenesC>`) of controllers with a `moods` claim;
  - schedules of DayTimers with a `schedule` claim, and the settings it claimed.
- Extern-port originals and extern wires are tracked in the lock, so
  deleting Lox Logic lines cleanly reverts their effects on the next compile.
