# Working with Lox Logic as an AI agent

Operational guidance for AI agents (and automation generally) authoring
Loxone logic through this toolchain. Lox Logic exists so you never have to edit
`.Loxone` XML — and you must not.

A condensed version ships as a Claude Code skill,
[skills/lox-logic/SKILL.md](../skills/lox-logic/SKILL.md): link the directory into
`~/.claude/skills/` (or a house repository's `.claude/skills/`) and an
agent there loads it when the task is Loxone logic. `tests/skill.rs`
keeps its example module valid.

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

Compile places new blocks beside what they wire to, and keeps a block
where Loxone Config last dragged it. `--relayout "<page>"` places a
page's managed blocks afresh. Blocks wired to a ref tag drawn in the GUI
stay where they are (D42). Placement keeps to the page's A3 sheet, packed
tighter if it must be; compile warns about managed objects past its edge
(D50).

Present step 4's output when proposing a change: it shows added blocks,
wires, and parameter changes in reviewable form, and flags locale-rename
noise (`[locale?]`) so it isn't mistaken for a real edit. Users, groups,
credentials and access rights come in a section of their own (`user`,
`group`, `access code`, `NFC tag` lines); credentials show as names,
counts and short fingerprints only. `--json` gives the same diff as JSON.
`--summary` says it by block title and port name — the form to show a
human, and the commit message for the module change.

The decompile view is for **reading, not compiling**: compiling it against
the same base would mint duplicates of every managed block and claim
ownership of every shown wire. To take over existing logic, use `adopt` —
it produces the compilable pair:

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
                                     # "Virtual Inputs" folder (D54)
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
heiss = sonne.Q >= 30 and not party.Q  # a named signal (D81): gate blocks,
                                     # the top one is `heiss` (`heiss.Q`)
jal.TargetPos = 70                   # Def= on an extern port; original
                                     # auto-restored when the line is removed
extern sensor = LoxAIRDevice(uuid: "…")  # a device (created in Loxone Config)
sensor.title = "Raumklima Bad"       # assign title, room, category (D39);
sensor.room = "Bad"                  # children in the device's room follow

extern tuer = NfcCodeTouch(uuid: "…")
access tuer                          # who may open it, under which schedule
	user "alice": "Always"           # (D49): users, groups and schedules
	group "Residents": "Workdays"    # by title; the rest lose access
end

extern regel = AutopilotRule(title: "Pumpe Ein")
rule regel                           # an app rule (D57): active, all
	active                           # `when`s must hold, `then`s in order
	when weekday weekdays
	when time 09:00
	then pumpe on
end

rule party "Party-Licht"             # D73: a block's state, a mode,
	active                           # `or` for another group
	when bewegung.active == on
	when mode "Party"
	or
	when helligkeit.value < 20
	then mode "Party" off
end

rule mittag "Pumpe Mittag"           # a named rule (D60): the config's
	inactive                         # rule of that name, else created
	when time 12:30
	then pumpe on
end

moods wohnen                         # light moods (D58), by name; the off
	"Viel Licht": all on             # mood ("Aus") is implicit
	"Lesen": AQ1 80%, AQ3 temp(60%, 2700K)
end
extern licht = LightController2(title: "Lichtsteuerung")
licht.moods = wohnen                 # also inside a template body

removed old_gate                     # confirmed delete of a managed block
moved beschatten -> schatten_gate    # slug rename, UUIDs survive
```

The meaning of an argument is decided by its value: `Port: slug.Port`
wires that source in, anything else sets the port's parameter. `<-` and
port-`=` work only on extern targets — everything about a managed block is
in its own argument list.

A named signal `x = <expr>` names an expression's result: its top block
is `x` (`x.Q`; an arithmetic one is a `Formula`, `x.AQ`), its sub-blocks
`x__<op><n>`. Prefer it over spelling an inline expression's synthetic
slug (`jal_autoshade__and1`) in a test or a second consumer. `x` counts
as a declared block: deleting the line needs `removed x`.

Removing a block declaration makes the next compile **fail on purpose**. When
deletion is the confirmed intent, add `removed <slug>` to the module — it
is scoped to that one block and shows the intent in the diff. For a slug
rename, rename the declaration and add `moved <old> -> <new>`; both
statements become no-ops once applied. (`--allow-removals` still exists but
authorizes *every* pending removal at once — prefer `removed`.)

## Errors and remedies

Many wrong-name errors end with a ``did you mean `…`?`` suggestion — trust
it only when it matches what you intended.

| Error contains | Meaning | Remedy |
|---|---|---|
| `line N` (from `check`) | syntax/reference error in the module | fix that line; `loxl fmt` shows canonical form |
| `no match for extern` | spec matched nothing of that type | check type + spelling; `loxl decompile` the base to see candidates |
| `ambiguous` + candidate UUIDs | several objects match | switch to `uuid: "<one of the candidates>"` — ask if unclear |
| `not in the verified builtin table` | block type can't be minted in v0 | use a verified type, or declare the object `extern` (create it once in Loxone Config) |
| `unknown port … known ports: …` | wrong port name on a managed block | use a listed port (caught by `check`, no base needed) |
| `is the label Loxone Config shows on the canvas` | a canvas label (`Tr`, `D`) used as a port name | use the key the error names (`InputTrigger`, `Time`) |
| `does not fit — the port is documented in …` | unit suffix that cannot mean the port's unit (`Time: 70%`) | write the value in the named unit (`loxl types <Type>`) |
| `is no exact decimal number of minutes` | time value on a minutes port that minutes cannot hold exactly (`10s`) | write it in `min` (`0.5min`), or pick a representable value |
| `ends before it starts` / `overlap on` (schedule) | a schedule interval wraps midnight, or two intervals of one day overlap | split at `24:00`/`00:00`; merge or shorten the intervals |
| `the timer is analog — … needs a value` / `the timer is digital` | interval values do not fit the DayTimer's analog flag | analog: `HH:MM-HH:MM = <value>` on every interval; digital: drop the values |
| `the config has no operating mode titled "…"` | a quoted mode title in a schedule line that names no `Mode` object | use one of the titles the message lists, or mint the mode (`party = Mode("Party")`) |
| `no <C Type="VirtualInCaption"> to hold a minted VirtualIn` | the base has no "Virtual Inputs" folder | create one virtual input in Loxone Config once, then recompile |
| `` `Analog:` is `on` or `off` `` | a VirtualIn's `Analog:` got another value | write `Analog: on` (or drop it: digital) |
| `` write `X` instead of `Y` `` | a v1 spelling v2 dropped (D80): `Analog: "true"`, `clock "07:30"`, `when date "2026-12-24"` | write what the message names (the language server offers it as a quick fix) |
| `` is on or off `` | a text-boolean attribute (`Analog:`, `Signed:`, `CloseAfterSend:`) got another value | write `on` or `off` |
| `is no whole number of ticks of` | `tick <duration>` that `dt` does not divide (`tick 1s dt 300ms`) | make the duration a multiple of `dt`, or write `tick <n>` |
| `` has no `!=` `` (from `test --lox`) | lox-cli's simulator lacks `!=` | run the test in the built-in simulator, or rewrite as `==` |
| `out-of-range` (from `lint`) | value outside the documented range, read in the port's unit | usually a unit slip (`AlarmClockPeriod: 2h` = 120 min); fix, or keep if deliberate |
| `has no port … present ports: …` | extern port's `<Co>` missing from base | use a listed port, or have the port wired/set once in Loxone Config so it exists |
| `in the lockfile but not in the source` | managed block vanished from source | typo → restore the line; delete → add `removed <slug>`; rename → `moved <old> -> <new>`; unmanage → `remove_object` |
| `changed type` | slug reused for a different block type | new slug, or `remove_object` first |
| `is an output port and cannot be used as …` | wire direction wrong | swap source and sink |
| `targets managed block` | `<-` or port-`=` used on a block you declared | move the wire/parameter into that block's argument list |
| `targets named signal` | `<-`, port-`=` or an attribute on a `x = <expr>` signal | change the expression instead; its inputs are its operands |
| `an alias is not a block` | `x = a.Q` — a named signal needs an expression | use `a.Q` directly where `x.Q` would go |
| `changed its top operator` | a named signal's top `and`/`or`/comparison changed, so its block type did | give it a new slug and add `removed <old>` |
| `is v0 syntax` | `block`/`wire`/`set`/`match` keyword from the old grammar | the error shows the v1 spelling — rewrite the line |
| `duplicate parameter` / `duplicate wire` | same argument twice in one argument list | delete one (fan-in with *different* sources is allowed) |
| `takes both a value … and a wire` | a port bound to a constant and to a wire in one argument list | keep one: a wired input ignores its value. A stale constant under a wire in the base stays there without a binding (D77) |
| `undeclared constant` | bare identifier in value position | declare `let <name> = …`, or quote the value if a string was intended |
| `neither slug is in the lockfile` (from `moved`) | rename references unknown identity | check both spellings against the lock; a first compile needs no `moved` |
| `is an attribute parameter, not a connector` | wire aimed at e.g. `Formula:` | pass a value instead: `Formula: "I1+I2"` |
| `warning: cannot adopt` (from `adopt`) | that block's rebuild would lose something (e.g. GUI input inversion `Inv=`) | leave it unmanaged, or remove the offending state in Loxone Config and re-adopt |
| `base carries N duplicate UUID(s)` | two objects/connectors in the base share one identity (a push tool re-keyed refs) | open and save the config once in Loxone Config — it re-mints the later twins — then use that save as the base; never hand-edit UUIDs |
| `output would carry duplicate UUID(s)` | this compile would emit a repeat: a fresh mint hit a base UUID minted in the same second, or the lock maps two identities onto one UUID | compile again (or another `--time`); if it persists, inspect the lock entries carrying the named UUIDs |
| `targets managed block` + `give it a label` / `D19` | `.title`/`.room`/`.category` on a block you declared | title → the block's label; room/category stay with Loxone Config |
| `rewrites what extern … is matched by` | the extern is matched by the attribute it claims | pin it by `uuid:` (or `iname:`) |
| `no room titled` / `no category titled` | `.room`/`.category` names no `Place`/`Category` in the base | use a listed title, or create the room/category in Loxone Config first |
| `carries no room binding` / `carries no category binding` | the object has no `<IoData Pr=/Cr=>` to rewrite | assign a room/category once in Loxone Config, then claim it from source |
| `claim the … of the same object` | two externs resolve to one object and claim the same attribute | keep one assignment |
| `numeric character references as literal text` (from `drift`) | a foreign XML writer left `&#xA;` text in programs, rules or descriptions; Loxone reads it verbatim | `loxl repair <cfg> --out <fixed>`, then compile against `<fixed>` as the base |
| `no user titled` / `no group titled` / `no access schedule titled` | an `access` grant names nothing in the base | use the title as Loxone Config or the app shows it; create the user, group or schedule there first |
| `users are titled` / `groups are titled` / `schedules are titled` | several objects share the title an `access` grant names | give them distinct titles in Loxone Config or the app |
| `not an NFC Code Touch` | `access` on an extern of another type | declare `access` only on an `NfcCodeTouch` extern |
| `but the base config has no <C Type="Page"> titled` | no page carries the title | fix the title, or create the page: `page "…" (room: "…")` (D56) |
| `a page needs a real room` | `page … (room: "Nicht zugeordnet")` | name the room the page belongs to |
| `was created by the compiler as … the base titles it` | a page Lox Logic created was renamed in Loxone Config | write the new title in the source (without the arguments the page is Loxone Config's), or rename it back |
| `not a push notification` / `mirrors a push notification` | `recipients` or an `OutputRefLM` on something other than a `Notification` | name an extern `Notification(…)`; create the notification in Loxone Config first |
| `a recipient takes no schedule` | `user "x": "…"` in a `recipients` body | write `user "x"` |
| `a rule belongs to an Automatic Designer rule` / `not an Automatic Designer rule` | `rule` on a block of another type | declare `rule` only on an `AutopilotRule` extern |
| `loxl writes only the documented shapes` | the rule has conditions Lox Logic cannot write (a control's state, an operating mode, OR groups) | drop the declaration; edit that rule in the app |
| `knows no action code (descID)` | a `then` to a block type/command no rule of the config uses yet | create one rule action of that kind in the app once; the compile copies its code |
| `does not fit a switch or dimmer circuit` (and the other kinds) | a mood value of the wrong kind for that circuit (`rgb()` on a relay, `on` on a colour circuit) | use what the message says the circuit takes; `off` fits everywhere |
| `is the controller's built-in off mood` | the set lists "Aus" (SID 778) | leave it out — every controller keeps it, always off |
| `cannot be dropped` (moods) | the set leaves out "Viel Licht" (the motion mood) or a mood the motion settings name | keep it in the set; its values are yours to set |
| `the controller has N circuits` | `AQ<n>` above the controller's circuit count | name an existing circuit |
| `recorded for block … no longer exists` | the pinned page was deleted from the base | re-pin: edit the lock entry's `page_uuid`, or `remove_object` the block to place it afresh |

After any failed `compile`, the lockfile on disk is untouched (the CLI saves
only on success) — just fix and re-run.

## Invariants you can rely on

- Compilation is deterministic: unchanged inputs → byte-identical output
  and lockfile. If you see a diff, something actually changed.
- Recompiling the compiler's own output is a no-op (fixpoint) — safe to
  re-run freely.
- The compiler touches only: its managed blocks, wires it drew onto extern
  ports (`<-`), `Def=` values it assigned on extern ports, and the
  title/room/category it claimed on externs (plus that claim's followers),
  tile programs it declared, access rights (`TIM=`) on Code Touches
  with an `access` declaration, and recipients (`ACT=`) of notifications
  with a `recipients` declaration, and the `APC=`/`STA=` of rules with a
  `rule` declaration whose content changed, and the moods
  (`<LightscenesC>`) of controllers with a `moods` claim. Everything else in the config is
  preserved byte-for-byte.
- Extern-port originals and extern wires are tracked in the lock, so
  deleting Lox Logic lines cleanly reverts their effects on the next compile.
