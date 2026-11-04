- **`lox config merge`** takes over Automatic Designer rules edited or deleted in the app since the base: it
  rewrites their `rule` declarations from the live config, moves `base` to the download and records the change
  time, so the next `lox config push` loses nothing. Conflicts with source edits and other live changes stop it.
