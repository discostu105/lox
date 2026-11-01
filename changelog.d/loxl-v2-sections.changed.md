- **`loxl` / `lox logic`: `access` and `recipients` are named sets**, like `tiles`, `schedule` and `moods`.
  `access bewohner … end` declares who may open, `haustuer.access = bewohner` gives it to a Code Touch, and
  several Code Touches can share one set (`frost.recipients = familie` for notifications). The compile output
  does not change
  - The old `access haustuer … end` on the Code Touch is an error that names the set and the claim to write;
    the language server applies both as a quick fix
  - `decompile` and `adopt` write `<slug>_access` sets, shared by Code Touches with the same rights
  - Set parameters take `access` and `recipients` (`template tuer(rechte: access)`)
