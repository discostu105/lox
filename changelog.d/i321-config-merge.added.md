- **`lox config merge`** takes over what changed on the Miniserver since the base, so the next push loses nothing:
  rules edited or deleted in the app become source, other changes the source doesn't own (a plugin added in
  Loxone Config) stay as they are, and the download becomes the base.
