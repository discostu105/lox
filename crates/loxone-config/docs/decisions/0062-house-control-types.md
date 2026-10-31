# D62: House control types, adopt-only devices, promoting an extern

Status: accepted (2026-10-30)

## Context

The page blocks of a house were references only: alarm, smoke alarm, irrigation, the room controller, the
energy flow monitor, the meters, Code Touch, Intercom and Touch Pure Flex. A source that wanted to own one
had to give up the extern and its wires.

## Decision

- **Connector tables from the corpus**, by the rules of [connector-db.md](../connector-db.md):
  - a direction proven by a wire in the corpus wins;
  - otherwise the legacy map, where the corpus does not contradict it;
  - an input that is never wired and never carries a `Def` stays an input.

  A save from Loxone Config does not prove a direction, since 17.2.8 keeps a wrong-direction wire
  (see [loxone-format.md](../loxone-format.md#connectors-and-wires-co--in)).
- **Carried internals.** What the GUI writes inside these blocks is carried per type, not modelled
  (`connectors::carried_attrs`, `carried_children`). That covers zones, timers, statistic groups, `IoData`,
  `SpStates` and attributes like `PwrN` or `StatsType`. A rebuild keeps them. A mint starts without them
  and a save fills them in; a recompile after that save is byte-identical.
- **Adopt-only.** Loxone Config creates Code Touch, Intercom and Touch Pure Flex when it pairs the device.
  Lox Logic adopts and rebuilds them, but refuses to mint one (`connectors::adopt_only`).
- **Promotion.** `loxl adopt --uuid <x>` of an object the source already names as extern `x` keeps the
  slug.
  - The extern and the statements about it (wires into it, port values) become the block's own
    declaration.
  - Wires to it from other files are kept, and the lock drops the extern's entries.
  - Its room, category and schedule claims move to the block (issue #191).
  - Promotion is refused, naming the reason, when a template writes onto the extern, when the extern
    carries a tiles claim, or when a removal is pending.
- **Coverage.** `loxl coverage` counts every object by type against the lock (managed, extern, rule,
  untouched) and lists what is left to adopt.

Validated: oracle session 25 (all nine minted types: empty semantic diff, and a save then a recompile
byte-identical; see [oracle-sessions.md](../oracle-sessions.md)). Live on the house (2026-10-30): nine
minted probe blocks were pushed, ran (`all` answered for each), moved to a room and removed. The house
adopted all 42 blocks of these types with a byte-identical compile.
