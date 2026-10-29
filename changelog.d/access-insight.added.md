- **Users and door access, read-only** (#136): `lox user ls`, `lox user show <user>` (state, validity, groups,
  NFC tags, number of access codes) and `lox user groups` read the Miniserver's user management.
  `lox access log <code-touch>` decodes an NFC Code Touch's access log (who opened when, with which tag or code,
  denied attempts). `lox access check <user> [<code-touch>]` says why a user may or may not open a Code Touch:
  it joins the live user with the access rights in a freshly downloaded program file and answers `granted`
  (schedule, direct or through a group), `no-access-right`, `no-credential`, `user-disabled` or
  `unknown-full-access`, with warnings where the file and the live state disagree. `lox access perms <control>`
  lists the users and groups assigned to a control. All with `-o json`; new MCP tools `list_users`, `get_user`,
  `access_log` and `access_check`.
- `lox config users` also shows each user's UUID, number of NFC tags and access codes, tag names, groups, own
  access rights and `last_edit`, and reads backup ZIPs too.
