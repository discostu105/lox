# D75: A redacted copy

Status: accepted (2026-10-31)

## Context

An issue or a test fixture often needs a real config, and a real config holds personal data. `loxl scan`
lists that data. Issue: #205.

## Decision

`loxl scan --redact <cfg> --out <copy>` writes a copy with every finding of `loxl scan` replaced by a
placeholder of the same kind.

- Placeholders: `redacted`, `redacted@example.invalid`, `192.0.2.1`, coordinates 0, MAC 0, codes and
  credentials empty.
- Hardware serials become a counter, also inside the UUIDs that embed them.
- Distinct values of one attribute get distinct names (`redacted`, `redacted-2`, …; #227). Redacted users
  stay apart, so an `access` or `recipients` declaration still lifts from the copy.
- It exits 1 when a finding is left, so a copy that passes can be shared.
- The copy is for reading, never for uploading.
