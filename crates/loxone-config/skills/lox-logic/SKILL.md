---
name: lox-logic
description: Author and change Loxone Miniserver logic as source code with the Lox Logic toolchain — .loxl modules compiled against a base .Loxone config with a UUID-pinning lockfile. Use when asked to add, change, review or explain automations (blocks, wires, parameters, schedules, Automatic Designer rules, light moods, virtual inputs) in a Loxone config, or to work in a directory with a lox.toml.
---

# Loxone logic with Lox Logic

The `.Loxone` file is a build artifact. You write `.loxl` modules, and
`loxl compile` turns them into a new config against the current one. The
lockfile pins every UUID the compiler minted. The full guide is
`crates/loxone-config/docs/agents.md` in the lox repository; the language is
`crates/loxone-config/docs/language.md`. `lox logic …` and `loxl …` are the same CLI.

## Never

- Edit a `.Loxone` file, or a `*.lock.json` beyond its documented
  operations. Both corrupt identity in ways the GUI will not report.
- Upload to a Miniserver. Compiling produces a file; pushing it is the
  human's step.
- Invent block types or port names. `loxl types` lists what can be
  minted and what each type is for (`loxl types --search delay`),
  `loxl types <Type>` its ports, units, labels and ranges.
  An extern's ports are whatever the base config has: the error lists them.
- Commit a real config. `loxl scan <file|dir>` exits 1 when one carries
  personal data (it says where, never what).
- Guess past `NoMatch` / `AmbiguousMatch`: ask which object is meant.

## The loop

```sh
loxl decompile current.Loxone               # what exists, by page (read-only view)
loxl templates                               # ready-made, tested templates
loxl types Monoflop                          # a type's ports before using it
$EDITOR pages/<page>.loxl                    # declare blocks, wires, parameters
loxl check --json pages/                     # types, ports, directions; no base needed
loxl compile                                 # in a lox.toml project; else
                                             #   --base --module --lock --out
loxl diff --summary current.Loxone out.Loxone   # show this to the human
loxl test                                    # the module's `test … end` blocks, simulated
loxl test --filter <name> --trace            # a failing test cycle by cycle: every value change
```

`loxl diff --summary` names every added block, changed parameter and wire
by title and port, and its first line is a fine commit subject. Show it
before anyone pushes.

## Writing modules

```text
extern sonne = VirtualIn(iname: "VI3")            # existing object: reference it
extern taster = TreeSensor(title: "Taster 1", device: "B0A1B2C3")
party = VirtualIn("Partymodus")                    # new app switch (Virtual Inputs folder)
soll = VirtualIn("Sollwert", Analog: on)           # on/off: the boolean literals
warm = GreaterEqual("Warm", Input1: sonne.Q, Input2: 28)
licht = And(I1: warm.Q, I2: party.Q)
extern jal = AutoJalousie(title: "Beschattung Süd")
jal.AutoShade <- licht.Q                           # wire onto an extern

test "warm und Party"
	sonne.Q = 30
	party.Q = on
	clock 07:30                                    # bare, like every time
	tick 5min                                      # a duration, or a count
	expect licht.Q == on
	party.Q = off
	tick 1
	expect licht.Q != on
end
```

- A managed block's argument list is its whole input side: `Port: x.Q`
  wires, anything else sets the parameter.
- Match externs by `iname:` or `uuid:` where you can. Titles are
  locale-volatile; narrow a repeated title with `room:`, `category:` or
  `device: "<serial>"`.
- Virtual and Modbus devices are blocks too (D64): `wp = ModbusServer("WP",
  Address: "192.0.2.12:502")`, `regler = ModbusDev("Regler", device: wp)`,
  `vorlauf = ModbusASensor("Vorlauf", device: regler, ModbusAddress: 1)`.
  On a block, `device:` names the managed or extern device (by slug) that
  the I/O hangs in. Write Loxone's `\v` as `"\\v"`. `loxl adopt --uuid
  <device>` takes an existing device over together with its I/O.
- The settings of a physical input or output are claims on its extern
  (D65): `fuehler.Unit = "<v.1> °C"`, `fuehler.MinTime = 600`,
  `taster.Desc = "Klingel"`. The original returns when the line goes.
  `Desc` is the app's name for the control. Only `Desc`, the low ends of
  the correction, `MinVal`, `ResetTime` and `AverageTime` can be added;
  the rest must already be on the object.
- `page "Title"` puts the blocks that follow on that page. When a page
  is full, `page "Title" (room: "…")` creates a new one (the room is
  required); without `room:` a missing title is an error.
- Deleting a block needs `removed <slug>`; renaming a slug needs
  `moved <old> -> <new>`. Without them the compile refuses, on purpose.
- A push notification is created in Loxone Config and named as an
  extern `Notification(…)`; `OutputRefLM(mirrors: <it>, AI: x.Q, On: "…")`
  sends it, and `recipients <it> … end` (`user "<title>"`) says who gets it.
- An Automatic Designer rule is `rule <slug> "Name" … end` (`active`,
  `when time 07:30`, `when weekday mon`, `when <block>.active == on`,
  `when <block>.value >= 65`, `when mode "Party"`, `or` between groups,
  `then <block> on`, `then licht "Mood 2"`, `then mode "Party" off`): the
  config's rule of that name, else created; `removed <slug>` deletes it.
  `loxl adopt --uuid <rule> --as <slug>` takes over one made in the app.
- A DayTimer's week is `schedule <set> … end` (`weekdays 06:00-08:00`,
  `weekend off`, `sat 07:00-22:00`, `holiday …`, `heating …`) and
  `timer.schedule = <set>`. Any operating mode can be named by title
  (`"Party" 18:00-24:00`); `party = Mode("Party")` creates a mode of your
  own, and `party.Q` is on while it is active. Users are never declared:
  they belong to the Miniserver's user management.
- Before building a common automation by hand, look in `loxl templates`
  (motion and staircase light, push-button, heating and humidity
  hysteresis, CO2 ventilation, door and water alarms, intrusion
  alarm, awning protection). `loxl templates <name> > pages/licht.loxl`
  copies the file into the module; one line calls it
  (`flur = bewegungslicht(bewegung: pir_flur)`) and its result is
  `<instance>_ausgang.Q`, wired to the actuator
  (`licht_flur.I <- flur_ausgang.Q`). Sensors are read at `Q`.
- Light moods: `moods <set> … end` lists moods by name (`"Lesen": all on,
  AQ3 temp(60%, 2700K)`; values `on`/`off`/`40%`/`rgb(r, g, b)`/`temp(…)`
  per circuit kind), `<controller>.moods = <set>` writes them onto a
  LightController2. Claim it in a room template to give every room the
  same moods; switch buttons go to `Sel1`…`Sel8` (T5 inputs: `Sel<n>`
  selects the mood with scene id n, i.e. the n-th mood created). The off mood "Aus"
  is implicit. `loxl adopt` lifts existing moods into
  sets (`<slug>_moods`), so start from those rather than retyping them.
- Ports are XML keys, not the KB's labels: on an `AutoJalousie`
  `EndUp`/`EndDown` are complete open/close (Co/Cc), `Safety` the wind
  alarm (Wa), `Window` the window contact, `Stop` Off; on a
  `LightController2` `Move` is motion (Mo), `Reset` Off, `MoveOn` the
  motion extend time. `loxl types <T>` shows the labels.
- `loxl test` simulates only documented behavior
  (`docs/simulator.md`) and names any block or input in the way. A
  lighting controller simulates motion, Off, M+/M−, single clicks on
  T5, Mood, On, Buzzer, Alarm (set Afi and MaxAbr) and output M; double
  clicks, long clicks, Lc1–8 and motion after a manual step to off are
  refused.
- To take over logic someone drew in the GUI, use `loxl adopt`, then
  confirm with `loxl diff --exit-code` that the takeover changed nothing.
  `loxl coverage` lists what is not managed yet, including the
  attributes of managed blocks carried from the base (a PulseAt's time
  is `At: 19:00`); adopting an extern
  `--as` its own slug promotes it (its wires move into the declaration).
  A managed block takes `.room`/`.category`, never `.title` (its label).
  The app lists a control only with both; the push adds the entry for
  the types Lox Logic knows (switches, inputs, jalousies, central blocks,
  meters, …) and names the rest, which a save from Loxone Config lists.
  The decompile view is for reading and does not compile.

## When the compiler refuses

Read the message: it names the line, the object and usually the fix.
`docs/agents.md` → "Errors and remedies" maps every refusal to its
remedy. A refusal is never something to work around by editing XML.
