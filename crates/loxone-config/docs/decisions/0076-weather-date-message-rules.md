# D76: Weather, dates and messages in rules

Status: accepted (2026-10-31)

## Context

[D73](0073-rule-conditions.md) covered conditions on states and modes. The capability table's `sections`
and the web app's rule builder give the codes of the weather, clock, date and message shapes (see
[loxone-format.md](../loxone-format.md#automatic-designer-rules-c-typeautopilotrule-apc)). Issue: #208.

## Decision

- **New rule syntax:**
  - conditions: `when weather <field> <op> <v>`, `when day`, `when daylight`,
    `when sunrise|sunset|dawn|dusk`, `when date Y-M-D` (bare, [D80](0080-v2-one-spelling.md));
  - actions: `then notify`, `then mail`, `then wait`, `then message`.
- **Codes are the Miniserver's own**, never guessed.
- **A date is the Unix time of the local midnight.** The compile computes it for the Central European
  zones only: the config's `Timezone`, with the EU summer-time rule. It refuses a config in another zone
  or with none, rather than guess an offset.
- **A pause is stored in milliseconds.** It reads back as `5s` when whole, else as `2500ms`.
- **A message's subject and text travel `/`-separated in its command**, so a `/` in them is refused.
- **The AutoPilot container's `Input`** gets the weather field's `WeatherData` block, the `Day` block and
  the `Daylight` block, as a save wires them ([D73](0073-rule-conditions.md)).
- **Not covered:** text conditions, the weather type (a multi-select), the caller and speech output. No
  corpus rule uses them and the house has no caller service. Such a rule stays a comment.

Validated: oracle session 32. The save kept the payload and wired the same blocks into the AutoPilot
container as the compile. See [oracle-sessions.md](../oracle-sessions.md).
