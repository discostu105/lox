- **`lox ir`: the house control blocks are managed** — alarm, smoke alarm, irrigation, the intelligent room
  controller, the energy flow monitor and the four meter types can be declared, adopted and rebuilt from source;
  Code Touch, Intercom and Touch Pure Flex adopt (Loxone Config creates them when it pairs the device)
  - Their internals (zones, timers, statistic groups) pass through unchanged, and a minted block passes a save in
    Loxone Config with an empty diff
- **`lox ir adopt --uuid <x> --as <extern>`** promotes an extern to a managed block: its wires and port values move
  into the declaration, and the other modules keep their references
- **`lox ir coverage`** lists the config's blocks by type (managed, extern, rule, untouched) and the blocks left to
  adopt (`--json`)
- **Managed blocks take `.room` and `.category`**, so a block made in source shows up in its room in the app
