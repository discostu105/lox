- **`lox logic`: a PulseAt's fire time in the source** — `At: 19:00` (or `06:58:30`) on a `PulseAt` sets the time of
  day it fires, which used to come only from Loxone Config (#248)
  - `adopt` and `decompile` write it out, `fmt` normalizes it (`6:58` → `06:58`), `lox logic types PulseAt` lists it,
    and `diff` shows a change as `At: 18:58 -> 06:58`; `drift` now sees a changed fire time too
  - Without `At:` the time is carried from the base as before, so existing sources compile unchanged. A block set to
    a time function (sunrise, hour pulse) in Loxone Config refuses `At:`
- **`lox logic coverage`** lists, per managed block type, the attributes the compile still carries from the base
  instead of writing them from source, with the parameter that claims one where Lox Logic has it
