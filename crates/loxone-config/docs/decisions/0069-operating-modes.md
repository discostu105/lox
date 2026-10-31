# D69: Operating modes from source

Status: accepted (2026-10-31)

## Context

A schedule ([D53](0053-daytimer-schedules.md)) could name only the weekday modes. Operating modes are
`Mode` objects in the "Betriebsmodi" folder; the built-in ones have fixed UUIDs and `Mode=` numbers, user
modes negative numbers (see [loxone-format.md](../loxone-format.md#operating-modes)).

## Decision

- **Any mode in a schedule line, by title:** `"Party" 18:00-24:00`. The keywords `heating` and `cooling`
  name the built-in modes 11 and 12 (`Heizperiode`, `Kühlperiode`).
- **Minting a user mode:** `party = Mode("Party")` creates an object of the "Betriebsmodi" folder, like a
  virtual input of its folder ([D54](0054-virtual-inputs.md), [D64](0064-virtual-modbus-devices.md)). It
  has `I` (switch it from logic) and `Q` (its state).
- **Title resolution.** A title resolves against the compiled document, minted modes included.
  - A missing title is an error that lists the modes the config has.
  - Two modes with one title are an error.
  - A title naming a built-in mode merges with its keyword: `mon` and `"Montag"` are one column.
- **Timer columns** are written in ascending `Mode=` order, and `UserModes=` counts the columns before
  Monday, as a save writes them.
- **A fresh mode** gets the next `Mode=` number below the lowest in the document. It gets what a save gives
  it: `Nio="2"`, `WF="147456"`, `<IoData></IoData>`, `<Display StateOnly="true"/>`.

Validated: oracle session 29. An open and save of a mint and of a timer with a minted mode, a GUI user mode,
the weekdays and `heating` came back byte-identical (the mode element; `Modes=`, `UserModes=`, `N=`,
entries). See [oracle-sessions.md](../oracle-sessions.md).

Not covered: adopting an existing user-defined mode, to rename or delete it from source. It is referenced
by title or as an extern (`extern party = Mode(title: "Party")`). Calendar entries are
[D74](0074-calendar-entries.md).
