# D96: Light controller circuits from source

Status: accepted (2026-11-01)

## Context

Issue #284: a `LightController2` minted from source has no circuits. Loxone Config writes them only when
someone opens the mood editor and presses OK (oracle session 33). Until then every output is a switch, so
a mood with `AQ1 80%` cannot dim.

Oracle sessions 45 and 46 and the corpus show what the editor writes
([loxone-format.md](../loxone-format.md), *The mood editor's circuits*):

- `NameAI<n-1>=` with the title of the actuator `AQ<n>` drives, and `PresM<n-1>="65535"`, in index order
  after `Nio=`.
- The circuit's kind (*Typ*) is stored only as the mood prefix (`Q<n> >> 28`). *Schalter* is 0,
  *Dimmer 0-100%* is 1. Switching the kind rewrites `Q<n>` of every mood to the new prefix with value 0.
- Named circuits with prefix 0 or 1 carry no `CapAI<n>=` anywhere in the corpus.
- A renamed circuit sets its bit in `COName=`.
- Circuits written into a compiled file survive a save unchanged, apart from `COName=` (session 46).

## Decision

- `Circuit1:` … `Circuit5:` on a managed `LightController2` take `"switch"` or `"dimmer"`.
- A declared circuit gets `NameAI<n-1>=` with the actuator's title and `PresM<n-1>="65535"` when it has no
  name yet. A name already there stays: Loxone Config or the user gave it.
- Each mood's `Q<n>` gets the kind's prefix. When the prefix changes, the value is cleared, as the
  editor's *Typ* does; the moods in source are then written on top.
- The actuator comes from the wires: exactly one object consumes `AQ<n>`, an `Actor` or `TreeAactor`,
  directly or through an `OutputRef`. No actuator, several, an untitled one or a colour actuator
  (`CCap=`) is refused by name.
- `CapAI=` and `COName=` are never written.
- `lint` warns (`no-actuator`) when a mood sets a percentage on a circuit that is not a dimmer.
- Decompile lifts `Circuit<n>:` from a named circuit with prefix 0 or 1. `loxl coverage` counts
  `NameAI<n>`/`PresM<n>` as owned by the circuit's parameter.

## Not decided

- The prefixes of *Dimmer 0-10V*, *Dimmer 1-10V*, *RGB*, *Lumitech DMX* and *Smart-Aktor* are unknown, so
  those kinds are refused.
- No real config shows the circuit settings of `AQ6` and up, so `Circuit6:` and up are refused.
- Dimming on a Miniserver is not tested: uploads were not allowed. #284 stays open for it.

## Rejected

- **Name the circuit after the controller output** (`AQ1`): the editor never does; each save would rename
  it.
- **Write `COName=`**: it marks a name the user typed. A name from the actuator is the editor's default.
