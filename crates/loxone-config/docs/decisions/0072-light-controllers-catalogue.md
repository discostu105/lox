# D72: Light controllers in the app catalogue

Status: accepted (2026-10-31)

## Context

[D66](0066-app-blocks.md) left a light controller's app entry for a save, because its sub-controls come
from settings and actuators, not from the block alone. Issue: #184.

## Decision

The push writes a `LightControllerV2` entry for a light controller the source shows
([D61](0061-app-catalogue.md)). It has details, states, a sub-control per named circuit and the masters.
They are read off:

- the block's circuit settings (`NameAI<i>=`, `CapAI<i>=`);
- its special states and sequence lists;
- its moods: the movement mood's place, and whether a circuit only switches;
- the actuators its outputs drive, for the colour range, through an `OutputRef`.

The entry format is in [loxone-format.md](../loxone-format.md#the-app-catalogue-loxapp3json).

Not listed:

- A controller that drives Lumitech outputs, whose kind the block does not carry. It stays for a save
  ("Lox Logic does not write this kind of LightController2 entry").
- A controller minted from source. It has no named circuits until a save.

Not derived: the alarm, smoke alarm, irrigation and room controller entries. The corpus has one sample
each, with fields no setting explains (#190).

Validated: 22 of the house's 23 controllers reproduced byte for byte in each of three saves. The 23rd is
the Lumitech one.
