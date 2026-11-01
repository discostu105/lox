# D97: Decompile lifts notifications and their recipients

Status: accepted (2026-11-01)

## Context

Issue #296: `decompile` and `adopt` left `OutputRefLM` push notifications and their recipient lists in the
base. A config with notifications could be compiled, but not taken over from a real file
([loxone-format.md](../loxone-format.md#push-notifications-notification-outputreflm-loggermailer)).

## Decision

- Decompile lifts each `OutputRefLM` whose `LoggerMailer` matches what the compiler writes (texts, `On`/
  `Off`, `MinimumTime`, `NL="1"`, no children) as a managed block with `mirrors:`. Its minimum time comes
  in `h`/`min`/`s`; the 5000 ms default is left out.
- Its recipients become a `<name>_recipients` set. Notifications with the same recipients share one set.
- The `Notification` objects they reference become externs.
- `adopt` accepts `Ref=` on an `OutputRefLM` and such a `LoggerMailer`. Any other `LoggerMailer` is
  refused, as before.
- An adopted config compiles back byte-identical (tested on the r50 base).
