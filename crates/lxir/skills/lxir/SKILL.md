---
name: lxir
description: Author and change Loxone Miniserver logic as source code with the lxir toolchain — .lxir modules compiled against a base .Loxone config with a UUID-pinning lockfile. Use when asked to add, change, review or explain automations (blocks, wires, parameters, schedules, virtual inputs) in a Loxone config, or to work in a directory with a lox.toml.
---

# Loxone logic with lxir

The `.Loxone` file is a build artifact. You write `.lxir` modules, and
`lxir compile` turns them into a new config against the current one. The
lockfile pins every UUID the compiler minted. The full guide is
`crates/lxir/docs/agents.md` in the lox repository; the language is
`crates/lxir/docs/ir-spec.md`. `lox ir …` and `lxir …` are the same CLI.

## Never

- Edit a `.Loxone` file, or a `*.lock.json` beyond its documented
  operations. Both corrupt identity in ways the GUI will not report.
- Upload to a Miniserver. Compiling produces a file; pushing it is the
  human's step.
- Invent block types or port names. `lxir types` lists what can be
  minted, `lxir types <Type>` its ports, units, labels and ranges.
  An extern's ports are whatever the base config has: the error lists them.
- Commit a real config. `lxir scan <file|dir>` exits 1 when one carries
  personal data (it says where, never what).
- Guess past `NoMatch` / `AmbiguousMatch`: ask which object is meant.

## The loop

```sh
lxir decompile current.Loxone               # what exists, by page (read-only view)
lxir types Monoflop                          # a type's ports before using it
$EDITOR pages/<page>.lxir                    # declare blocks, wires, parameters
lxir check --json pages/                     # types, ports, directions; no base needed
lxir compile                                 # in a lox.toml project; else
                                             #   --base --module --lock --out
lxir diff --summary current.Loxone out.Loxone   # show this to the human
lxir test                                    # the module's `test … end` blocks via lox sim
```

`lxir diff --summary` names every added block, changed parameter and wire
by title and port, and its first line is a fine commit subject. Show it
before anyone pushes.

## Writing modules

```text
extern sonne = VirtualIn(iname: "VI3")            # existing object: reference it
extern taster = TreeSensor(title: "Taster 1", device: "B0A1B2C3")
party = VirtualIn("Partymodus")                    # new app switch (Virtual Inputs folder)
warm = GreaterEqual("Warm", Input1: sonne.Q, Input2: 28)
licht = And(I1: warm.Q, I2: party.Q)
extern jal = AutoJalousie(title: "Beschattung Süd")
jal.AutoShade <- licht.Q                           # wire onto an extern
```

- A managed block's argument list is its whole input side: `Port: x.Q`
  wires, anything else sets the parameter.
- Match externs by `iname:` or `uuid:` where you can. Titles are
  locale-volatile; narrow a repeated title with `room:`, `category:` or
  `device: "<serial>"`.
- Deleting a block needs `removed <slug>`; renaming a slug needs
  `moved <old> -> <new>`. Without them the compile refuses, on purpose.
- A push notification is created in Loxone Config and named as an
  extern `Notification(…)`; `OutputRefLM(mirrors: <it>, AI: x.Q, On: "…")`
  sends it, and `recipients <it> … end` (`user "<title>"`) says who gets it.
- To take over logic someone drew in the GUI, use `lxir adopt`, then
  confirm with `lxir diff --exit-code` that the takeover changed nothing.
  The decompile view is for reading and does not compile.

## When the compiler refuses

Read the message: it names the line, the object and usually the fix.
`docs/agents.md` → "Errors and remedies" maps every refusal to its
remedy. A refusal is never something to work around by editing XML.
