- `lox access learn` and the MCP tool `learn_nfc_tag` work at a secured NFC Code Touch (`isSecured`): learn mode
  starts and ends as secured commands signed with the visualization password, instead of failing with code 500.
  A 500 on a secured command now says "wrong visualization password, or the user has none", and a tag that is
  already paired (it opens the door instead of being learned) is reported at once instead of after the timeout
  (#159)
- `lox user show` prints the last edit in local time; the Miniserver counts `lastEdit` from 2009-01-01 UTC, so it was
  off by the UTC offset (JSON keeps the raw value) (#160)
