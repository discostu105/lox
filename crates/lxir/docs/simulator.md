# The simulator behind `lxir test`

> **Status: implemented** (D68, `src/sim/`). The block models listed below
> run; every other type is refused by name.

`lxir test` runs a module's `test … end` blocks ([ir-spec.md](ir-spec.md))
against the compiled config, cycle by cycle, with no Miniserver. The rule
it follows ([design.md](design.md) D68): **a test passes only on
documented behavior.** Each model below is written from Loxone's
documentation of the block (the Knowledge Base page and its timing
diagram) or from a format fact in [loxone-format.md](loxone-format.md).
Anything else stops the test with an error naming the object, instead
of a guessed value.

## How a run works

- Each test starts fresh: every value 0, the simulated clock at
  2026-01-01 00:00, a Thursday. `clock "…"` sets it.
- `tick n dt d` runs `n` program cycles. Each cycle advances time and
  clock by `d`, evaluates the operating modes, then every modelled block
  in wiring order. Blocks in a feedback loop follow in document order and
  read the previous cycle's values.
- An input gets, in this order: what the test set; its wires (negated
  when the connector carries `Inv=`); the constant 1 for an unwired
  `Inv=`; its `Def=` value; the documented default (`port_meta`) when
  `Def=` is omitted; else 0.
- A test may set any port: an input, a parameter or a block's output.
  The value holds until the test sets it again and overrides whatever
  the block computes.
- Reading a port for an `expect` first walks the logic it depends on,
  given what the test sets at that moment, and refuses:
  - a block without a model;
  - an output the model does not compute (a Monoflop's remaining time,
    say);
  - several wires into one input of a block whose fan-in is
    undocumented;
  - an operating mode switched by logic (its `I` wired);
  - a block that failed earlier in the test: a timer with two of its
    modes active, a formula without a value, manual mode on a timer.

  The error names the object. Setting a port between it and the
  expectation (often the block's own output) makes the test independent
  of it. A failure in a block the test does not depend on is ignored.
- Sources read 0 until the test sets them: virtual inputs (`VirtualIn`,
  HTTP/UDP inputs and their commands, `VirtualTextIn`), digital and
  analog inputs, Tree, Air, 1-Wire, RS485, KNX and Modbus sensors.

## Block models

The KB pages are at `https://www.loxone.com/enen/kb/<page>/`.

| XML type | Loxone name | Source | Modelled | Not modelled |
|---|---|---|---|---|
| `And` | AND | kb `and` | Q on when all *connected* inputs are on; several wires into one input are AND-linked | — |
| `Or` | OR | kb `or` | Q on when any input is on; fan-in OR-linked | — |
| `Xor` | Exclusive OR | kb `exclusive-or` | Q on when I1 and I2 differ; fan-in OR-linked | — |
| `Not` | NOT | kb `not` | Q = not I | — |
| `Equal`, `NotEqual`, `Greater`, `GreaterEqual`, `Less`, `LessEqual` | comparators | kb `comparator` family | Q = Input1 op Input2 | — |
| `Add`, `Sub`, `Mult`, `Div`, `Add4` | arithmetic | kb `arithmetic-blocks` | AQ | division by 0 fails the block |
| `Formula` | Formula | kb `formula`; both decimal separators (loxone-format.md) | AQ from `Formula=`: `+ - * / ^`, comparisons, I1–I4, PI and the documented functions | the error output; a formula without a value fails the block |
| `Memory` | Memory flag | kb `memory-flags` | AQ = Input when wired, else the held value; Q = AQ ≠ 0 | — |
| `InputRef` | input reference | loxone-format.md *Refs* | AQ = AI; Q = I, or AI ≠ 0 when I is open (refs of block outputs wire AI only) | — |
| `OutputRef`, `OutputRefLM` | output reference | loxone-format.md *Refs* | AQ = AI | — |
| `Monoflop` | Monoflop | kb `monoflop`, timing diagram | a rising edge at Tr: Q on for D seconds, from the latest edge, whatever the input does; Reset ends and, held, locks | remanence, the remaining-time output |
| `OnDelay` | Switch-on delay | kb `switch-on-delay`, timing diagram | Q on once Tr has been on for D without a break, off with Tr; Reset resets and locks | remanence |
| `OffDelay` | Switch-off delay | kb `switch-off-delay`, timing diagram | rising Tr: Q on; off D seconds after the falling edge (a new edge extends); Reset off until the next rising edge | remanence |
| `OnOffDelay` | Switch-on and switch-off delay | kb `switch-on-and-off-delay`, timing diagram | on after Tr has been on for TimeOn, off after it has been off for TimeOff; Reset resets | remanence |
| `PushButton` | Switch | kb `switch`, timing diagram | Tg toggles on its rising edge; On holds Q on; Off (`Reset`) wins and locks; DisPc disables all; the On/Off outputs pulse for one cycle | remanence, the API input |
| `PButtonT` | Push-button | kb `push-button`, timing diagram | rising Tr: Q on for Don from the latest edge; Don 0: Q follows Tr; Off resets and locks; DisPc disables Tr and Off | remanence, the API input |
| `SRFlipFlop` | SR flip-flop | kb `flipflop-sr`, timing diagram | S sets and dominates, R resets, a rising Tg toggles | remanence |
| `EdgeDetection` | Edge detection | kb `edge-detection`, timing diagram | pulses of Pd at every edge (P), rising (On), falling (Off); a new edge restarts the pulse | — |
| `AnalogThresholdTrigger` | Threshold switch | kb `threshold-switch` | hysteresis between Von and Voff (inverted when Von < Voff, only the exact value when equal); On/Off pulse for Pd | remanence |
| `Mode` | operating mode | loxone-format.md *Operating modes* | Monday–Sunday (built-in modes 4–10) follow the clock; all others off unless the test sets them | a mode with `I` wired is refused |
| `DayTimer` | Schedule | kb `timerscheduler`; loxone-format.md *DayTimer* | the entry of the active mode's column covering the time of day: digital 1, analog its value, else 0 / the default value; entries needing activation switch on with a rising edge at Act, for Don seconds when set; Reset switches off; On/Off pulse for one cycle | manual mode (refused); two active modes of the timer (refused: which one wins is undocumented); remanence, the remaining-time output |

## Not modelled

Every other type: lighting and shading controllers, intelligent room
controllers, alarms, pulse generators (the enable input is
undocumented), constants (where the value is stored is not yet a format
fact) and all integrations. A test can still cover logic downstream of
them by setting their outputs. A new model needs a KB page or a format
fact behind every behavior it claims, and a test per behavior in
`src/sim/tests.rs`.
