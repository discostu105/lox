# D37: Reading Automatic Designer rules

Status: accepted (2026-08-26), amended by [D57](0057-automatic-designer-rules.md)

## Context

The app's Automatic Designer stores each rule as an `AutopilotRule` block. Its whole logic (conditions, actions,
activation) lives in the `APC=` JSON attribute
([loxone-format.md](../loxone-format.md#automatic-designer-rules-c-typeautopilotrule-apc)). The Miniserver writes
these blocks, so they are GUI-owned as a whole ([D20](0020-inv-connector-gui-owned.md)). A reader blind to `APC=`
missed app edits: a changed schedule time or a disabled rule showed as no change.

## Decision

Lox Logic reads rules. Writing them is [D57](0057-automatic-designer-rules.md),
[D60](0060-creating-rules.md) and later decisions.

- **`diff`, `drift` and the fingerprint compare `APC=`** like an attribute parameter, the way
  [D18](0018-adoption.md) treats `Formula=`. `APC` is not in `connectors::attr_params`, because that table is
  language surface and the payload must never be bindable. A diff-local set (`diff_attrs`) holds both.
- **Comparison is structural.** The raw payload is not stable across writers: two house rules carry a raw CRLF
  inside the JSON. `APC=` is compared and fingerprinted as canonical parsed JSON. A reformatted rule is not drift;
  an edited one is. The fingerprint moves only for configs that carry rules.
- **`loxl diff` shows a change as JSON field paths**, such as `eventGroups[0].events[0].value: 540 -> 600`.
- **The full decompile view renders each rule** as comment lines above its extern (`# when … / # then …`). Control
  UUIDs in the payload resolve to titles. Unknown codes print raw (`descID 272001`); labels are not guessed. The
  adoptable view stays bare: comments are for readers, and that view owes the compiler byte fidelity
  ([D29](0029-full-and-adoptable-view.md)).
- **The payload is untyped JSON** end to end. A typed struct would drop fields a newer app adds, and the diff must
  show exactly what changed.

## Rejected

- Writing rules before the evidence existed. It needed the `descID` and event-type tables (only in the app
  bundle), proof that Loxone Config keeps foreign rules, and proof that the Miniserver runs them. A rule that is
  written but never runs is the silent failure [D8](0008-fixed-gate-inputs.md) guards against.
  [D57](0057-automatic-designer-rules.md) opened writing for the shapes with that evidence.
