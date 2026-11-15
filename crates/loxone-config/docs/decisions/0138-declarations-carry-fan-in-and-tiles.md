# D138: Hardware declarations carry a fan-in and a tiles claim

Status: accepted (2026-11-15)

## Context

`adopt --uuid <device>` ([D101](0101-hardware-declarations.md)) refused two kinds of device in the
house config, so both stayed externs.

- **The three Touch Pure Flex.** Each carries a `.tiles` claim
  ([D41](0041-touch-pure-flex-tiles.md)), and adopt refused any extern with one. The refusal dated
  from page-block adoption, where a block can never be a Touch Pure Flex. The check also accepted a
  claim only on an extern, so a declared device could not carry one.
- **The 24 Nano IO Air.** Their "Akustisches Klick-Signal" output is driven from two pages: the
  burglar alarm's `Silent` and the smoke alarm's `PreAlarmSignal`, each through its own `OutputRef`.
  The actor's `I` holds both wires (`Nc="2"`). As an extern that is two `<-` statements. Adopt moved
  them into the declaration one after the other, and the second replaced the first, so the no-op
  verification failed with a removed wire. The lowering also refused a port bound twice.

A third problem showed up while adopting devices one at a time. The first adopt of a type factors
`defaults <Type>(…)`, and a later device whose values differ is refused. Adopt meant to write a
`defaults` only for types the module did not use yet, but it looked for blocks of the type in the
expanded module, where a hardware declaration is an extern plus what it owns. Declared hardware
therefore never counted.

## Decision

- **A port of a hardware declaration takes several sources**, written as the fan-in list every
  block has (`I: [alarm.Silent, brandmelder.PreAlarmSignal]`). Each source lowers to a wire onto
  the extern the declaration becomes, like several `<-` onto an extern. A value is still bound
  once and never next to a source.
- **Adopt keeps every `<-` onto a promoted port.** A second wire joins the first as a list. A `=`
  still replaces the config's value.
- **A `.tiles` claim holds on a `TreeDevice` declaration** as on an extern, and adopt leaves the
  claim where it is, like `.room` and `.category`.
- **Adopt writes a `defaults` only for a type the module declares nowhere**, hardware declarations
  included.

No new syntax: the list and the claim are the forms the language already has. The compiled config
is unchanged, so the drift fingerprint stays at v14.

## Consequences

- The house config adopts its 3 Touch Pure Flex and 24 Nano IO Air with a byte-identical
  recompile. No Tree or Air device or I/O of it is an extern any more.
- A declaration's list order is the order adopt read the statements, which can differ from the
  `<In>` order in the config. The compile keeps the wires it finds where they are, so the house
  config's 24 lists recompile byte-identically.
- Adopting a type the module does not declare yet still factors a `defaults` among the adopted
  blocks. A later device with other values is refused, and the refusal names the port.
