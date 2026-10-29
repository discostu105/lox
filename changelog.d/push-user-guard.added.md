- `lox config push` no longer deletes user changes silently (#138). Loading an upload deletes what the Miniserver
  holds apart from its program file, users, groups, NFC tags and access codes changed in the app or with
  `lox user` among them, and the drift check only sees what a download holds. The push now compares every live
  user with the program file (exists, name, last edit, groups, NFC tag names and count, number of access codes):
  staging lists the differences (`user_changes_pending` in `-o json`), `--force` refuses with error
  `user_changes_pending` unless `--discard-custom-changes`. The check needs an admin connection; users it cannot
  read stop `--force`.
