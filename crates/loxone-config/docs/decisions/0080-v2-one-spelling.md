# D80: Language v2, one spelling per value

Status: accepted (2026-10-31), amended by [D91](0091-off-is-no-attribute.md)

## Context

Language v1 spelled the same thing several ways. A digital value could be `1` or an attribute's
`"true"`, a test clock was a string, a one-word rule command was quoted. It also had gaps: no `!=` in
tests, and ticks only as counts.

## Decision

v2 makes the value forms uniform. Every change keeps the compile byte-identical. Each dropped spelling is
an error that states the exact replacement ("write `X` instead of `Y`"), and the language server offers it
as a quick fix.

- **`on` and `off` are reserved literals** (`Value::Bool`), valid in every value position.
  - They compile to `1` and `0`, except on a text-boolean attribute, where they compile to `"true"` and
    `"false"`. This is the one port-dependent resolution (`resolve_value_for`).
  - The text-boolean attributes are `connectors::bool_params`: `VirtualIn`/`VirtualOutCmd` `Analog`,
    `VirtualHttpInCmd`/`VirtualUdpInCmd` `Signed`/`Analog`, and `VirtualOut` `CloseAfterSend`. The corpus
    (149 files) shows them only as `"true"`/`"false"`.
  - On those attributes the strings `"true"`/`"false"` are refused. Elsewhere a quoted `"true"` stays a
    string, such as a `Formula:` text.
  - Numbers are not canonicalized to `on`/`off`: `I2: 1` stays `1`. A numeric port's `1` may be an analog
    value, and the source should say what the author wrote.
  - Decompile and adopt lift the text-boolean attributes as `on`/`off`.
  - The formula backend refuses them, since it computes on numbers.
- **`tick <duration>`** advances by a time. The count is the duration over `dt`. A duration that is no
  whole number of steps is an error, not a rounding. The AST keeps the spelling (`TickDecl::duration`), so
  `fmt` round-trips `tick 5min`.
- **`clock` and `when date` are bare.** They are lexed like `At: 19:00` ([D79](0079-pulseat-fire-time.md))
  and canonicalized the same way: two-digit hours, no zero seconds. The quoted v1 form is an error, not a
  silent canonicalization. A string there was the only spelling, so every v1 file names it, and the error
  is the migration.
- **`expect … != …`** (`TestCmp::Ne`) is the negation of `==`, with its tolerance. lox-cli's `lox sim` has
  none, so `--lox` refuses it.
- **One-word rule commands are bare** (`then pumpe on`). A quoted one-word command canonicalizes at parse
  time, like `"28"` → `28`, so v1 files still parse and `fmt` migrates them. Multi-word or non-identifier
  commands (`"Mood 2"`, `"+"`) stay quoted.
- The "expected a statement" error lists every statement kind.
