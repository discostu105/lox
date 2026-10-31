# D58: Lighting moods from source

Status: accepted (2026-10-30)

## Context

A user asked for "four moods in each room, with the same colors, with all the buttons on the switches… Loxone
Config won't let me for loop it". A LightController2 keeps its moods in `<LightscenesC>` (see
[loxone-format.md](../loxone-format.md#reported-by-lox-cli-checked-against-the-corpus), *LightController2
moods*).

## Decision

A `moods <set>` declaration lists moods by name, with circuit values. `<controller>.moods = <set>` writes the
set onto a LightController2's `<LightscenesC>`. The claim also works from a template body, which gives the loop
the GUI lacks.

- **The name is the identity.** Buttons, the app and `<LSConfig>` address a mood by `SID=`. Matching by name
  keeps an existing mood's `UUID=`, `SID=`, `CID=` and flags, and rewrites only its `Q<n>=`. An adopted
  controller compiles byte-identical, and a recoloured mood stays on its button.
- **New moods** take the lowest free `SID` from 1 (776–778 are built in), and the lowest free `CID` above the
  off mood's. The house's GUI-made moods show the same.
- **Built-in moods.** The off mood (`SID` 778) is implicit. The motion mood (777) and any mood `<LSConfig>`
  names cannot be dropped, since the controller depends on them. They can only be re-valued.
- **Order.** A rewrite keeps the off mood at its index; it is not always first or last.
- **Values per circuit kind, read off the moods.** The top four bits of a `Q` value belong to the circuit, not
  the mood. The compile reads each circuit's kind from the existing moods and encodes `on`, `N%`, `rgb()` and
  `temp()` for it. It refuses a value that does not fit. A value outside the decoded packings round-trips as a
  raw number.
- **Seeding a minted controller.** Loxone Config backfills a bare controller's moods on its first save. A claim
  on a controller with no `<LightscenesC>` writes the same: stock moods, `<LSConfig>`, `<IoData>`, `<PSD>`,
  `<HCL>` and `<COHist>`. Then it applies the set. Its circuits read as switch or dimmer circuits until Loxone
  Config assigns an actuator.
- **Lock state for minted moods only.** A minted mood's UUID is pinned (`moods`, lock v6) until a base carries
  it, so compiles before the push are a lock fixpoint. After that the base's `UUID=` is the identity and the
  entry retires. Removing a claim leaves the moods as written, like [D41](0041-touch-pure-flex-tiles.md) and
  [D53](0053-daytimer-schedules.md).
- **Semantics.** Diff and fingerprint (v7) carry every controller's moods as rendered source values. Drift
  accepts a lock's v6 fingerprint.
- **Lift** (#170). Decompile and adopt write every controller's own moods as a set, plus a claim, and pull the
  controller in as an extern. There is one set per distinct mood list, shared by name of the first controller.
  This includes controllers adopt refuses to manage: a claim owns their moods all the same. The stock "Viel
  Licht" alone is not lifted. Rendering factors out the majority value (`all on, AQ3 off`) and compiles back to
  the same `Q` values.

The `moods` syntax is in [language.md](../language.md).

Validated: adopt + compile is byte-identical on the house (9 controllers) and the corpus (20 controllers in 9
configs, each also surviving a reorder and restore). Loxone Config 17.2.8.28 keeps minted and seeded moods
byte-identical (oracle sessions 21–22, see [oracle-sessions.md](../oracle-sessions.md)). Live on the house
Miniserver.
