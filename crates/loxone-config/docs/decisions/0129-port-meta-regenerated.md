# D129: Port facts for the later-admitted types

Status: accepted (2026-11-01), follows [D52](0052-port-facts.md) and [D88](0088-port-descriptions.md)

## Context

`src/port_meta.rs` was generated before the builtin table admitted the alarms, the room controller, the
meters, the NFC Code Touch, the intercom, the irrigation, the Touch Pure Flex controller, the energy
flow monitor and the math and counter blocks. Their ports had no documented unit, label or range: a
suffixed value scaled by the default reading, `adopt` wrote bare numbers, and `loxl types` showed no
label. Issue: #305.

## Decision

**Regenerate from the same source, unchanged.** `tools/extract-port-meta.py` on lox-cli's
`loxone-block-types-full.json` (sha256 `255641d4…bd665e8`) now gives 459 ports with a fact, up from
203. No existing row changed.

**A placeholder is not a unit.** The energy flow monitor's prices are documented in
`$$STRING::CURRENCY$$`, which Loxone Config replaces with the project's currency. The generator keeps
no unit for such a port, so no suffix is refused and no placeholder reaches a message.

**Effects.** The new `s`/`min` units make `adopt` and `decompile` write those parameters with a suffix
(`PulseTime: 3s`) and the `unit-suffix` lint flag bare numbers on them. `lint --fix` rewrites them;
the compiled output does not change. The new ranges feed `loxl lint` as before. The new documented
defaults make `decompile` and `adopt` state `Port: 0` where a connector has no `Def=` and the default is
not 0 ([D128](0128-no-def-at-zero.md)), on the room controller and the energy flow monitor among others.

**Port descriptions gain nothing from it.** D88 already used the TechDoc labels. The ports still
without a description are the ones the TechDoc has no label for either: the alarm's sensor and arming
inputs (`I1`–`I5`, `Active*`), the CentralLight inputs, the bidirectional and storage meters' readings.
Their labels need a check in Loxone Config (`canvas_labels`).

## Rejected

- **Labels for the gaps by pattern** (`ORd1` as `Rdc` because `Mr1` is `Mrc`): a guess, as in D88.
- **Keeping the placeholder as a unit**: it refuses every suffix with a message no one can act on.
