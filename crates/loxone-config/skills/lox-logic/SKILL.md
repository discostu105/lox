---
name: lox-logic
description: Author and change Loxone Miniserver logic as source code with the Lox Logic toolchain — .loxl modules compiled against a base .Loxone config with a UUID-pinning lockfile. Use when asked to add, change, review or explain automations (blocks, wires, parameters, schedules, Automatic Designer rules, light moods, virtual inputs) in a Loxone config, or to work in a directory with a lox.toml.
---

# Loxone logic with Lox Logic

The `.Loxone` file is a build artifact. You write `.loxl` modules, and `loxl compile` turns them into
a new config against the current one. The lockfile pins every UUID the compiler created. `lox logic …`
and `loxl …` are the same CLI.

Read the full guide before writing a module: `crates/loxone-config/docs/agents.md` in the lox
repository (https://github.com/discostu105/lox/blob/main/crates/loxone-config/docs/agents.md). It has
the writing tips and the error table. The language spec is `crates/loxone-config/docs/language.md`.

## Never

- Edit a `.Loxone` file, or a `*.lock.json` beyond its documented operations. Both corrupt identity in
  ways Loxone Config will not report.
- Upload to a Miniserver. Compiling produces a file; pushing it is the human's step.
- Invent block types or port names. `loxl types` lists the types the compiler can create
  (`loxl types --search delay`), `loxl types <Type>` their ports. An extern's ports are whatever the
  base config has; the error lists them.
- Commit a real config. `loxl scan <file|dir>` exits 1 when one carries personal data.
- Guess past `NoMatch` / `AmbiguousMatch`. Ask which object is meant.

## The loop

```sh
loxl decompile current.Loxone                   # what exists, by page (read-only view)
loxl templates                                  # ready-made, tested templates
loxl types Monoflop                             # a type's ports before using it
$EDITOR pages/<page>.loxl                       # declare blocks, wires, parameters
loxl check --json pages/                        # types, ports, directions; no base needed
loxl lint                                       # advisory findings by kind; --fix writes unit suffixes
loxl compile                                    # in a lox.toml project; else --base --module --lock --out
loxl diff --summary current.Loxone out.Loxone   # show this to the human
loxl test                                       # the module's `test … end` blocks, simulated
loxl test --filter <name> --trace               # a failing test cycle by cycle
```

`loxl diff --summary` names every added block, changed parameter and wire by title and port. Its
first line is a commit subject. Show it before anyone pushes.

## A module

```text
extern sonne = VirtualIn(iname: "VI3")            # existing object: reference it
party = VirtualIn("Partymodus")                    # new app switch (Virtual Inputs folder)
warm = GreaterEqual("Warm", Input1: sonne.Q, Input2: 28)
licht = warm.Q and party.Q                         # named signal: an And named licht
extern jal = AutoJalousie(title: "Beschattung Süd")
jal.AutoShade <- licht.Q                           # wire onto an extern

test "warm und Party"
    sonne.Q = 30
    party.Q = on
    tick 5min
    expect licht.Q == on
end
```

- A managed block's argument list is its whole input side: `Port: x.Q` wires, anything else sets the
  parameter.
- `name = <expression>` (`and`/`or`/`not`, comparisons, `+ - * /`) is a named signal. Its top block
  is `name` (`name.Q`; arithmetic is a Formula, `name.AQ`). Reference it in tests instead of an inline
  expression's synthetic slug. Deleting it needs `removed name`. `name = a.Q` alone is refused.
  `name.title = "…"` on the next line labels its top block (default: the expression text).
- A template instance is named by itself: `flur.Q` is its primary block (the only block, or the one
  `template t(…) -> <block>` names), `flur.taster.Q` any other block. Never write the lock key
  `flur_taster`. A set goes in by a set parameter: `template raum(stimmungen: moods = standard)`.
- `defaults <Type>(Port: value, …)` gives every managed block of the type the values it does not bind
  itself. Values only, one per type. Look for it at the top of a file before repeating a value.
- Tiles, schedules, light moods, access rights and notification recipients are named sets:
  `access bewohner … end` declares one, `tuer.access = bewohner` claims it for one block, and many
  blocks may claim one set. A set owns what it lists: users missing from an `access` set lose access,
  recipients missing from a `recipients` set get no message.
- Write time values with their unit (`Delay: 3h`, `Time: 500ms`). `loxl lint` names bare ones
  (`unit-suffix`), and `loxl lint --fix` writes the units without changing the compiled config.
- Deleting a block needs `removed <slug>`; renaming a slug needs `loxl rename` or
  `moved <old> -> <new>`. Without them the compile refuses, on purpose.
- To take over logic drawn in Loxone Config, use `loxl adopt`, then check with
  `loxl diff --exit-code` that nothing changed. The decompile view is for reading and does not compile.

## When the compiler refuses

Read the message: it names the line, the object and usually the fix. The "Errors and remedies" table
in `docs/agents.md` maps each refusal to its remedy. Never work around a refusal by editing XML.
