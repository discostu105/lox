# D49: Access rights on NFC Code Touch blocks from source

Status: accepted (2026-10-29)

## Context

Who may open a door is part of the program. A user or group may open a Code Touch when one of its `<PRM>`
rights names the block with a `TIM=` access schedule (see
[loxone-format.md](../loxone-format.md#users-groups-and-access-rights)). Lox Logic showed these rights in the
diff, and `lox config push` kept them as they were live, but no source could set them.

## Decision

An `access <code_touch>` declaration lists the users and groups that may open the block, each with its
schedule.

- **The declaration owns the block's access completely.** Every `TIM=` on the block, across all users and
  groups, belongs to it. Compiling grants what it lists, moves a right to another schedule, and revokes what it
  leaves out. Blocks without a declaration are not touched, as with the claims of
  [D39](0039-attribute-claims.md).
- **Users, groups and schedules stay GUI-owned and are referenced by title.** Loxone Config or the app creates
  them, together with passwords, tags and codes the source must never hold ([D70](0070-users-stay-miniserver.md)).
  Schedules are the `SwitchingTimer` objects of the access schedule folder.
- **A title must name exactly one object of its kind.** Otherwise the compile fails and names the candidates.
  A user renamed in the app stops the compile instead of silently losing access.
- **Access only.** A right's `ACT=` (operating the block in the app) stays as it is. A revoke removes `TIM=`.
  A `<PRM>` left with only `REF=` goes, and a list left empty goes with it: no corpus config has an empty
  `<PH>`.
- **The edit is Loxone Config's own.** A grant appends `<PRM REF="…" TIM="…"/>` to the user's or group's list,
  creating the list if there is none. A reschedule rewrites `TIM=` in place. `NUM=` follows the count. Nothing
  else changes, not even `LastEdit=`. A declaration that matches the config writes nothing.
- **No lock state.** As with tile programs ([D41](0041-touch-pure-flex-tiles.md)), the rights are the source's
  while it declares them. Removing the declaration leaves the last written rights in place.
- **`NfcCodeTouch` only.** Config 16 and 17 put `TIM=` only on those blocks. Config 12 rights on modes, actors
  and right groups are out of scope. An `access` on any other type is refused.
- **A contextual keyword.** `access` starts a declaration only before a slug at the start of a line.
  Reserving the word would turn every existing `access` slug into an error.
- **Decompile and adopt lift the rights** of every Code Touch that has any, so adopting a config puts its
  access under source control unchanged. A right whose user or group has no unique title cannot be referenced.
  The block then gets a comment, not a declaration that would revoke the right.
- **Lint `no-access-right`** reports a user with an NFC tag or access code but no access right on any Code
  Touch, directly or through a group. Such a tag opens nothing. *Full access* does not help (live-verified).
- **`permissions.bin` follows when it can be trusted.** Loxone Config writes it into the program ZIP: per
  object, the users and groups with any right on it (see
  [loxone-format.md](../loxone-format.md#the-access-table-permissionsbin)). A grant to someone without a right
  on the block changes it. A reschedule, or a revoke that keeps `ACT=`, does not. `loxone_config::permissions`
  generates it from the program and the ZIP's `LoxAPP3.json`, byte for byte on every ZIP we have.
  `lox config push` uses the generated file only where the generator reproduces the live file exactly.
  Otherwise the live file stays, and the report flags it as `permissions_stale` if the compiled config assigns
  users and groups differently.

## Rejected

- A grant-only form. It could never take a right away, and taking rights away is half of the point.
- Reserving `access` as a keyword (see above).

Validated: a Loxone Config 17.2 save granted a user access. Compiling the declaration lifted from it onto the
config from before that save reproduces Config's users, groups and schedules byte for byte. The
`permissions.bin` generated with the earlier `LoxAPP3.json` is the one Config wrote. Every declaration lifted
from the corpus (3 in public configs, 16 in the house's history) compiles to the same bytes. Revoking one
changes only access rights, and granting it again reverts that exactly.

Not covered: the revoke edit is inferred, not observed in a Config save. The Miniserver stores an uploaded
`permissions.bin` unchanged; whether it enforces that file or derives the rights from the program needs a live
test.
