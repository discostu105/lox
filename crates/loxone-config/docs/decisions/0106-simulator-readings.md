# D106: The simulator takes the plain reading of a block's page

Status: accepted (2026-11-01), amends [D68](0068-own-simulator.md)

## Context

[D68](0068-own-simulator.md) models only documented behavior. The KB pages of Automatic Shading,
its central block and the meters describe much of the behavior in a few words per input:

- Po "Partial open with push & hold", minTd "Minimum travel time at pulse on input (Po)", Tlc
  "Long-click duration on inputs (Po), (Pc) for complete open / close", Tdc "Double click duration".
- Tg "Toggles between open, stop, close".
- Mld "Duration of motor lock between direction changes".
- Rd … Rly "Reading today" … "Reading last year"; the Structure File's `totalDay` "Meter reading
  today".
- The central block: "Central commands are not blocked by an active (DisPc) input", Co "Stopping
  not possible", No "Number of open shades".

None of these comes with a timing diagram, and no live test or Loxone Config session was
available for this work.

## Decision

A model may take the plain reading of such a sentence, and refuses by name every case where two
readings would give different results. Each reading is written next to the code and in
[simulator.md](../simulator.md). For this change:

- **Push & hold:** the output drives from the press while it is held, at least minTd. Held for
  Tlc, or pressed again within Tdc of the press, it becomes the complete open or close. Refused:
  a second press within Tdc of the release but not of the press (measured from the press or the
  release?), a press while the shade moves, a press at the end position.
- **Toggle:** a press stops a moving shade, the next drives completely the other way. Refused:
  a press on a shade at rest, where the cycle's start is not stated.
- **Motor lock:** a drive against the running or the last one starts Mld after that one stopped.
- **Period readings:** calendar day, month and year of the simulated clock, the amount metered in
  each. The week readings have no row on the block's page and stay refused.
- **R:** its rising edge clears every reading, the Structure File's "Resets all values to 0";
  "On: Block is locked" stops counting. Refused with Mro ≠ 0 (does the reset reading show 0 or
  Mro?) and with a mapped reading (Mr).
- **No/Nc:** a shade at rest at position 0 is open, at 1 closed. Refused while a shade moves or
  rests in between.

## Rejected

- Refusing every input without a timing diagram. It leaves push buttons, the central block and
  the period readings, common in real programs, untestable, although their pages state the
  behavior well enough for the cases above.
- Picking one reading where the text allows two (Tdc from the press or from the release). A
  test would pass on a guess.
