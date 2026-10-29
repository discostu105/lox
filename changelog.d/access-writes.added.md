- **NFC tags and access codes** (#139): `lox access learn <code-touch>` reads an NFC tag's ID at a Code Touch
  (learn mode over the WebSocket, ended again on success, error, timeout and Ctrl-C) and says whether the tag is
  known; with `--user` and `--name` it pairs the tag with that user. `lox user nfc add <user> <id> <name>` and
  `lox user nfc rm <user> <id|name>` pair and remove tags by ID in any spelling. `lox user code set <user>` sets
  an access code from a hidden prompt or stdin, never from the command line; the code stays out of output,
  errors and the `-v` log, and a code another user has fails with `code_in_use`. Every change asks for
  confirmation (`--yes` skips it; with `-o json`, `--non-interactive` or no terminal it needs `--yes`),
  supports `--dry-run`, is sent once without retries, and reminds you that it may live only in the
  Miniserver's custom changes until Loxone Config saves the program. New MCP tools `learn_nfc_tag`,
  `assign_nfc_tag`, `remove_nfc_tag` and `set_access_code`, all confirmed by the user.
