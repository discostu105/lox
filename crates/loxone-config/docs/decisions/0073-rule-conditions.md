# D73: Rule conditions on states and modes, OR groups

Status: accepted (2026-10-31), amended by [D76](0076-weather-date-message-rules.md)

## Context

[D57](0057-automatic-designer-rules.md) and [D60](0060-creating-rules.md) covered rules on time and
weekdays only, with actions whose `descID` the corpus showed. The Miniserver serves the app a capability
table with every event and action code (see
[loxone-format.md](../loxone-format.md#automatic-designer-rules-c-typeautopilotrule-apc)).

## Decision

- **Codes come from the Miniserver, not from corpus guesses.** Its capability table
  (`capabilities/getraw`) is generated into `loxone_config::autopilot_caps`, codes only, by
  `scripts/autopilot_caps.py`. The web app's enums name the event, action and operator codes.
- **New rule syntax:**
  - `when <slug>.<state> <op> <value>`;
  - `when mode <mode> [on|off]`;
  - `or`, which starts a new event group;
  - `then mode <mode> on|off`;
  - a `then` takes any command the table offers for the target's app type.
- **Where the codes come from.** The app type and state UUIDs come from the block's catalogue entry. For a
  block the catalogue does not list, or a type outside the table, only the codes the config's own rules
  already use for it work.
- **Picking a condition.** A condition is picked from the table by its state: a digital event with that
  value for `==`, else the state's one analog event. An ambiguous choice is refused.
- **Decompile** lifts every rule of these shapes; the blocks it names become externs. Other rules stay a
  comment. On the corpus, every lifted rule compiles back byte for byte
  (`corpus_rule_declarations_are_byte_fixpoints`).
- **The AutoPilot container's `Input` list.** Loxone Config wires every source a rule watches into it
  (see [loxone-format.md](../loxone-format.md#automatic-designer-rules-c-typeautopilotrule-apc)).
  - A compile that changes the rules (rewrites a payload, creates or deletes a rule) sets that list from
    them, so a compiled file has what a save writes.
  - Rules the compile leaves as the base has them leave the list as the base has it. One corpus config
    has two rules and no list at all; deriving the list there broke the rule fixpoint, and nothing in the
    source asked for the change (#224).
  - The list is left alone when a rule watches a source Lox Logic cannot place.
  - `adopt`'s no-op check does not count these wires.

Validated: oracle session 31 (see [oracle-sessions.md](../oracle-sessions.md)).
