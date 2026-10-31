# D57: Automatic Designer rules from source

Status: accepted (2026-10-29), amended by [D60](0060-creating-rules.md), [D67](0067-deleting-rules.md),
[D73](0073-rule-conditions.md) and [D76](0076-weather-date-message-rules.md)

## Context

[D37](0037-automatic-designer-read-only.md) kept rules read-only until there was evidence for three things:
the action codes, the event shapes and how the engine activates a rule. For the rules the house has (a time of
day, optionally weekdays, and `on`/`off`/`pulse` to a button), that evidence exists. The rule format is in
[loxone-format.md](../loxone-format.md#automatic-designer-rules-c-typeautopilotrule-apc).

## Decision

A `rule <slug>` declaration (on an `AutopilotRule` extern) owns what the rule does. Its body has exactly one
`active` or `inactive` line, and `when time HH:MM`, `when weekday <days>` and `then <slug> "<command>"` lines.
[D73](0073-rule-conditions.md) and [D76](0076-weather-date-message-rules.md) add conditions and actions.

- **Codes from evidence.** An action's `descID` is a code per target type and command. The compile takes it
  from the Miniserver's capability table ([D73](0073-rule-conditions.md)). For a block the app catalogue does
  not list, it copies the code from an action in the same document with the same target type and command. It
  refuses anything else, with a message that says how to give it evidence: create one such action in the app.
- **Read as payload, write as edit.** The compile interprets the rule as read, and writes nothing when the
  declaration says the same. The raw payload is not writer-stable, so a lifted declaration must not touch it.
  A changed rule gets the app's edit:
  - unchanged events and actions keep their JSON objects;
  - a changed one keeps every field the source does not state (a single-day `value` stays that form, a
    sentinel stays a sentinel);
  - only new ones are built;
  - the text is written compact, in the app's key order.
- **Active means two flags.** The engine runs a rule only when `active` and `activeIfValid` are both true.
  `active`/`inactive` writes both, and a rule reads as active only with both. `STA="1"` follows: set right
  after `APC=` when active, removed when inactive.
- **The rule block stays the app's.** Its name, `ref=` and position are not the declaration's. A declaration
  on an extern addresses an existing rule. Removing it releases the rule unchanged, with no lock state, like
  the claims of [D41](0041-touch-pure-flex-tiles.md) and [D53](0053-daytimer-schedules.md).
  [D60](0060-creating-rules.md) creates rules by name, and [D67](0067-deleting-rules.md) deletes them.
- **Shapes not covered stay comments.** Decompile leaves a rule whose payload it cannot interpret as a
  comment ([D37](0037-automatic-designer-read-only.md)).
- **Lift.** Decompile and adopt lift every interpretable rule into a declaration, and pull the rule and its
  action targets in as externs. Adopt + compile is byte-identical (house config: all five rules).

The rule syntax is in [language.md](../language.md).

Validated live (Miniserver firmware 17.2.8.28, house config, 2026-10-29/30). Compiles that set `active` alone
were pushed, verified byte-exact, and never ran. The app's activation of the same rule ran. A compile setting
both flags ran at its compiled minute. Compiling the original declaration back restored the original bytes.
