# D95: Block settings from source

Status: accepted (2026-11-01)

## Context

[D79](0079-pulseat-fire-time.md) made a PulseAt's fire time a claim and made `loxl coverage` list what the
compile still carries from the base. On the house config that list held the names the app shows, memory
flag types, the blinds' sunshine flag, meter units and DayTimer texts: settings a user changes, kept only
in Loxone Config. Issues: #253, #278, #279.

## Decision

- **Each setting is an attribute parameter and a claim.** While unbound it is carried; once bound it is
  written from source, at the base's position, or where a save writes it on a block that lacks it.
  Decompile and adopt lift it, except the value a save writes when nothing is set
  (`connectors::attr_save_default`), so an adopted block states only what was set.
  - `Desc:` on the types with an app entry (`connectors::DESC_TYPES`): the app name. Written right
    after `Type=`, as every save does.
  - `Type:` on Memory → `Tp=`, and `Pulse:` on PulseAt → `Typ=`. Both take words, not codes
    (`connectors::choice_values`), named for what the pane offers. `At:` goes only with `Pulse: "time"`.
  - `Sun: on|off` on AutoJalousie. Off leaves `Sun=` out, as a save does
    ([D91](0091-off-is-no-attribute.md), `connectors::omitted_when_off`).
  - `On:`/`Off:` on DayTimer and `PwrN:`/`Un1:`/`Un2:`/`UnSt:`/`MaxLvl:` on meters take text
    (`connectors::text_params`). Only the DayTimer texts may be empty: a save writes them empty.
  - The parameter names are the attribute names where the attribute name is the plain word (`Desc`,
    `Sun`, `On`, `PwrN`), and the pane's word where it is a code (`Type`, `Pulse`).
- **What has no field, or no known meaning, is refused by name** (`connectors::unclaimed_reason`):
  - The member lists of alarms and central blocks (`Objects=`, `Entries=`, `CentralOut=`, `rec=`).
    They mix block UUIDs with hidden refs, the corpus has few of them, and no save has shown how
    Loxone Config keeps an edited list. #278 stays open for them.
  - A meter's `Rec=`, which names its energy flow monitor, and `StatsType=` (always 8).
  - A virtual input's `MinChange=`, `MinTime=`, `ValOT=` and `Idx=`: no pane shows them.
  - A one-off PulseAt date: `Sec=` of a day or more stays with Loxone Config.
- **`Task="1"` on a program is no setting.** Every save writes it and the pane has no field for it.
  `loxl coverage` no longer lists it, nor a value a save writes when nothing is set. It counts a
  `.schedule` claim as owning a DayTimer's `Modes=`, `N=` and `UserModes=`.
- **The drift fingerprint v12 includes the settings.** A lock recorded with v11 still reads as in sync.

## Evidence

Oracle sessions 43 and 44 ([oracle-sessions.md](../oracle-sessions.md)) and the corpus survey recorded in
[loxone-format.md](../loxone-format.md) (*Objects*).
