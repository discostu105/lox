- **`lox autopilot changed --explain`** (#156): says what moved the rules' `changed` time. It compares
  the newest program file in `/prog` with the one before it (or `--since FILE`), sorted into rules,
  users & access and other changes, and lists the custom changes pending in `/sys/custom`. With
  `-o json`, `rules_changed` tells a guard script whether a rule really changed, so a password set in
  the app no longer looks like an edited rule.
