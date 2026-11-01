# D51: Push notifications from source

Status: accepted (2026-10-29), amended by [D83](0083-access-recipients-sets.md)

## Context

A push notification is three pieces in the config (see
[loxone-format.md](../loxone-format.md#push-notifications-notification-outputreflm-loggermailer)):

- the `Notification` object under *Messages*;
- an `OutputRefLM` on a page, whose `<LoggerMailer>` child holds the texts and the minimum time;
- the recipients, as `ACT=` rights on the notification.

Logic that should warn someone needed the GUI for all three.

## Decision

- **The notification stays periphery.** Loxone Config creates it, like a device. The module names it as an
  extern `Notification(…)`. No compile writes into the *Messages* tree.
- **The page object is a minted ref** ([D33](0033-minted-mirrors.md)): `OutputRefLM(mirrors: <notification>,
  AI: …)`. Its `mirrors:` must name a `Notification`. It gets the shape Config gives a fresh one: `Cl="0,0,0"`,
  no `WF=`, no `LinkRefType=`.
- **The message is parameters on the block:** `On`, `Off` and `MinimumTime` (a duration, compiled to
  milliseconds, default 5 s as in the GUI). An empty `Off` is not written, as in Config. The parameters are
  compiler-owned like the rest of the block, so the `<LoggerMailer>` is rebuilt on each compile.
- **Messages are semantics.** The diff compares the attributes of every `<LoggerMailer>` (`message On`, …),
  and the fingerprint includes them (v5). `loxl drift` accepts a v4 fingerprint recorded before.
- **Recipients are a `recipients <notification>` declaration.** It works like [D49](0049-nfc-access-rights.md)
  with `ACT=` in place of `TIM=`:
  - it owns every `ACT=` on the notification;
  - it grants with `ACT="3"` (Config 17.2) and keeps an existing `ACT=` of any value (older configs write
    `1`);
  - it revokes by removing `ACT=`;
  - it keeps no lock state.

  Users and groups are named by title ([D70](0070-users-stay-miniserver.md)). The *Full access* group is not
  listed: Config shows its members as inherited recipients and writes nothing for them.
- **Not lifted.** Decompile leaves `OutputRefLM` blocks and recipients with the GUI. A module takes them over
  by declaring them.

Validated: a house compile with a notification, a recipient and an `OutputRefLM` open+saves with an empty
semantic diff (oracle session 15, see [oracle-sessions.md](../oracle-sessions.md)). Live on the house
(2026-10-31): a declared recipient gets the push. The phone's own push settings (`PCfg`) can still block it.
