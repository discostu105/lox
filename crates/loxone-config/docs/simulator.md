# The simulator behind `loxl test`

`loxl test` runs a module's `test … end` blocks ([language.md](language.md)) against the compiled
config, cycle by cycle, with no Miniserver (`src/sim/`). Why it has its own simulator:
[decision 0068](decisions/0068-own-simulator.md).

**A test passes only on documented behavior.** Each model below is written from Loxone's documentation
of the block (the Knowledge Base page and its timing diagram) or from a format fact in
[loxone-format.md](loxone-format.md). Every other type stops the test with an error that names the
object.

## How a run works

- Each test starts fresh: every value 0, the simulated clock at
  2026-01-01 00:00, a Thursday. `clock 06:30` sets it.
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
  given what the test sets at that moment. An input or parameter reads
  what its wires carry on any block, one without a model too: an
  actuator's command input (`VirtualOutCmd.I`, an output's `I`) can be
  expected on. It refuses:
  - an output of a block without a model;
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
- `loxl test --trace` prints each test as it runs: what a step sets,
  every change of the ports an expectation depends on (the first cycle
  lists them all), named by slug, and each `expect` with its actual
  value. The trace keeps 500 cycles with changes.
- Sources read 0 until the test sets them: virtual inputs (`VirtualIn`,
  HTTP/UDP inputs and their commands, `VirtualTextIn`), digital and
  analog inputs, Tree, Air, 1-Wire, RS485, KNX and Modbus sensors, and a
  device's thermal-shutdown status (`OvertempShutdown`).
- A model refuses an input it does not simulate only when that input
  matters: a pulse input when it is on, a value input (a position, a
  mood number) when it is connected. Such an input wired from a block
  without a model still stops the test; set it, or the port behind it.

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
| `Switch2Button` | Push switch | kb `push-switch`, its table and timing diagram | Q pulses for Don on each edge at Tr, Qon on the rising, Qoff on the falling one; On latches Q until Off; Off switches off and, held, locks; DisPc disables Tr, On and Off. Qon and Qoff also pulse when On and Off switch Q: the diagram shows it, the output table does not (the model follows the diagram) | a Tr edge while Q still pulses or while On holds it (undocumented: refused); Don 0 (refused) |
| `PulseAt` | Pulse at | kb `pulse-at`; loxone-format.md (`Sec=`) | the fixed time of day (`Typ="0"`): Q on for Don from the moment a tick passes `Sec=`, every day; a pulse shorter than the tick shows for that cycle; Off ends it and, held, locks. A `clock` statement that jumps over the time is not time passing and starts no pulse | the other kinds (sunrise, hour pulse, …: refused, their format is not documented), `AutP=`, one-off dates, remanence |
| `MeterDig` | Fixed value meter (not a pulse meter, checked in Loxone Config) | kb `fixed-value-meter`; units from kb `meter`; Structure File *Meter* | OPf = Ppwr while A (S) is 1, else 0; OMr counts Ppwr × the on-time in hours, plus Mro. Each step counts with the power of the cycle before it. the period readings Rd, Rld, Rm, Rlm, Ry, Rly (`ORd` … `ORly`): what was metered today, this month, this year by the simulated clock's calendar, and in the previous day, month and year (Structure File *Meter*: "Meter reading today"); a tick across a boundary splits by time, a `clock` jump over a whole period leaves the previous one 0, and a test starts with nothing metered. R: its rising edge clears OMr and every period reading ("Resets all values to 0"), and while it is on the meter counts nothing ("On: Block is locked"); OPf still follows S | A other than 0 or 1 (refused); R with Mro ≠ 0 (whether the reset reading shows 0 or Mro is undocumented); the week readings `ORw`/`ORlw` (not on the block's page; which day a week starts is undocumented); the clock going back to an earlier day, month or year (refused) |
| `MeterAbsUni` | Meter | kb `meter`, *Meter reading and units*; Structure File *Meter* | Pf alone: OMr integrates Pf over hours, plus Mro, and R acts as on `MeterDig`; with Mr: OMr = the reading + Mro, a decreasing reading is discarded. OPf = Pf; with Mr and no Pf it is the power estimated from the last two readings: their difference per hour of the time between them, 0 for the same or a smaller value, unchanged until a new value arrives (a value arrives when it changes). The period readings as on `MeterDig`; from Mr they count the increase of the accepted reading | a negative Pf; a jump of more than 10000 (the discard rule is ambiguous); R with Mr (what a reset does to a mapped reading is undocumented); a rise after a discarded reading without Pf (which two readings the power comes from is undocumented); a block with `Acum=` (probably the relative-metering property, unverified); the week readings |
| `MeterAbsBi` | Meter bidirectional | kb `meter-bidirectional`; Structure File *Meter* | OMr1 (Mrc) and OMr2 (Mrd) map their readings plus Mro1/Mro2, decreasing readings discarded; OPf = Pf; the period readings `ORd1` … `ORly1` and `ORd2` … `ORly2` count the increase of each reading as on `MeterAbsUni` | Pf alone (how the block splits it into consumption and delivery is not on its page, and the power-direction property's attribute is unknown), Mr without Pf (how the power of two readings combines), R on, `Acum=`, the week readings |
| `AutoJalousie` | Automatic shading | kb `automatic-blinds`, *Complete movement, end positions and remanence*; Structure File *Jalousie*; [D106](decisions/0106-simulator-readings.md) for the readings | Co: Op for the whole Opd (a reference run); Cc: Cl for the remaining run time (the whole Cld with Rdd); Co or Cc while moving stops. Pos (0 open … 1 closed) follows the run time linearly over Opd/Cld — the reading of "remaining run time"; a fresh simulation starts open. Po/Pc (push & hold): the output drives from the press while it is held, at least minTd; held for Tlc it becomes the complete open or close (Opd counted from Tlc; Tlc 0: every press); a second press within Tdc of the first does too. Tg: a press stops a moving shade, the next press drives completely the other way. Pos (%): a change moves to that position (0 open, 100 closed) for the share of Opd or Cld. Mld: a drive against the running or the last one starts Mld after that one's output went off. Wa moves to Wap (0 open, 1 closed) and locks, Dwc opens and locks, Wds = Wa or Dwc; at the end of Wa the lock lifts and nothing moves. Off stops, locks while held and overrules Wa/Dwc; OutputLock = Off. So moves a roller blind, curtain or awning to the position Rd. DisPc disables Tg, Po, Pc, Co, Cc and So, not Pos or a central block | Sps, Spr, T5 (refused when on), Slat (refused when connected): the sun automatic (needs the shading period from the sun's position, sunrise/sunset offsets and Dir±Dts/Dte), the T5 value encoding, slats, backlash. Refused: Tg on a shade at rest (where the cycle starts), Po/Pc or Pos while the shade moves, a second press within Tdc of releasing the first but not of pressing it, Po/Pc at or held up to the end position, Pc or a closing Pos with Rdd, Pos 0 % or 100 % (a complete run or not), DisPc while Po/Pc is held, a Pos change or central command during a lock, two commands in one cycle, So on venetian blinds, the end of Dwc, Off ending during Wa/Dwc, Wa to close with Dwc at once; TPos, AQpp, Slat, Sp |
| `CentralShade` | Automatic shading central | kb `shading-overview`; kb `automatic-blinds` (Co "via … central function block" is a reference run) | Co, Cc and So reach every shade of its `rec=` list as that shade's own Co (whole Opd), Cc (remaining run time) and So, "not blocked by an active (DisPc) input" at the shade; its own DisPc disables them. No (`OutOpen`) and Nc (`OutClose`) count the shades at rest fully open (position 0) and fully closed (1) | Tg, Po, Pc, Sps, Spr, Wa, Off, T5 (refused when on), Pos and Slat (refused when connected); a shade without a model in `rec=` (the block is not simulated); a central command while a shade moves ("Stopping not possible") or is locked by Off, Wa or Dwc; No/Nc while a shade moves or rests between open and closed (whether it counts is undocumented) |
| `LightController2` | Lighting controller | kb `lighting-controller`; moods: loxone-format.md *LightController2 moods*; the Miniserver where tested live, else Loxone Config's simulation (both in [oracle-sessions.md](oracle-sessions.md)) | Mo or P switch on the motion mood (the mood flagged `Move="true"`, which `ScMv=` names) when every light is off and Br ≤ Brt; Mo: off Moet after its falling edge (Met when shorter, Moet 0 never); P: off when it ends; DisP 1 ignores Mo and P: lights P switched on go off when DisP rises, lights motion switched on go off Moet (Met when shorter) after it rises — a Moet already running starts again (kb, input DisP). Off resets every circuit and, held, locks; 2C pulses for one cycle; after a switch-off by Off or by stepping to the off mood (M+, M−, T5) the automatic waits until there was no motion for Pto (0.01 s with P wired). M+, M− and T5 click: a click acts Tdc after the press or on release, whichever is later; a second or third press within Tdc makes a double (triple) click, which switches off and pulses 2C (and 3C). M+/M− step through the moods in list order, the off mood one of them, around the ends; T5/n selects the mood `SID="n"`, and steps on like M+ within 30 s of its last click or while its mood shows. T5 held for Mmd is a long click, acting while held: from off it selects its mood, else it mixes the mood in (M −3; a circuit two moods of the mix use shows the higher value, kb *Moods*) or takes it out of the mix (the last out switches off); a short T5 click during a mix selects its mood alone. On selects `SID="777"` (ID 99) and Buzzer `SID="776"`, else 777, when they rise; Buzzer's mood goes off Met after it falls. Mood selects by ID when its value changes (a constant selects nothing): 0 and 778 off, a user mood's SID that mood, any other value 777. A mood chosen by hand stays; motion keeps it, and it goes off Met (not Moet) after motion ends — also when chosen during motion. Lc1–8 toggle their circuit between 0 and 1 (circuits without an actuator), the others stay, M 0. MBr acts when its value changes: on circuits without an actuator 0 switches off and any other value on, M 0; while every light is off it changes nothing. Alarm (with Afi and MaxAbr set on the block) flashes every circuit between MaxAbr and 0 with period Afi, on first, M 99 and 0 in step, and the mood returns when it ends. DisPc disables Off, the pulse inputs, Lc, MBr and Mood. A circuit's output is its mood value: 0 for off, 1 for `on`, the percentage on a circuit with an actuator (`NameAI<n>=`) and 1 on one without (live). M: 0 off, 99 for 777, 98 for 776, −3 a mix, 0 after Lc/MBr, else the SID | Rtd (refused when on); colour values (refused when shown, and under Alarm); Lc on a circuit with an actuator, MBr on a used one, Lc or MBr over the motion mood or a mix, a mood or M+/M−/T5 after Lc or MBr; Pm other than 0; Mo and P at once; DisP rising over a mood chosen by hand during motion, motion during DisP while the automatic is held back, DisP ending during motion, presence or while the motion mood shows; Met 0 with motion; four clicks within Tdc, a click on another input within Tdc, a long click after a click, Mmd not longer than Tdc, motion after a double/triple or long click switched off; stepping onto 776, Mood 98/776 with an alarm-clock mood, On without 777, a mood change while On or Buzzer is held, M+/M−/On/Buzzer during a mix, a manual change while P is on, while motion is held back or while the lights wait to go off, motion during the Buzzer's mood or over a mix or Lc/MBr values, P with a manual mood, Alarm without Afi/MaxAbr on the block, Alarm on a used circuit without an actuator or over a mix or Lc/MBr values, any input or timer during Alarm; AQ19/AQ20, fading |
| `Mode` | operating mode | loxone-format.md *Operating modes* | Monday–Sunday (built-in modes 4–10) follow the clock; all others off unless the test sets them | a mode with `I` wired is refused |
| `DayTimer` | Schedule | kb `timerscheduler`; loxone-format.md *DayTimer* | the entry of the active mode's column covering the time of day: digital 1, analog its value, else 0 / the default value; entries needing activation switch on with a rising edge at Act, for Don seconds when set; Reset switches off; On/Off pulse for one cycle | manual mode (refused); two active modes of the timer (refused: which one wins is undocumented); remanence, the remaining-time output |

## Not modelled

Every other type: central lighting, intelligent
room controllers, alarms, access schedules (`SwitchingTimer`, not program
logic), pulse generators (the enable input is
undocumented), constants (where the value is stored is not a documented format
fact) and all integrations. A test can still cover logic downstream of
them by setting their outputs. A new model needs a KB page or a format
fact behind every behavior it claims, and a test per behavior in
`src/sim/tests.rs`.
