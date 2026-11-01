- **`loxl` / `lox logic`: template instances are named by the instance.** `flur.Q` reads the result of instance
  `flur`, `flur.taster.Q` any other block of it. The lock keys stay `flur_ausgang`, so the compile output does
  not change
  - The primary block is the body's only block, or the one `template name(…) -> <block>` names. It is the
    instance itself (`flur.Q`) and takes the call-site port bindings, now also in templates of several blocks
  - Works everywhere a block is named: arguments, `<-`, tests, tile `to:`, rules, claims, `removed`, `moved`,
    `loxl rename`. `adopt` writes these names, and `loxl test` reports them
  - The old `flur_ausgang.Q` is an error that names the new spelling; the language server offers it as a quick fix
- **`loxl` / `lox logic`: set parameters.** `template raum(stimmungen: moods = standard)` takes a `moods`,
  `schedule` or `tiles` set by its name (`raum(stimmungen: kueche)`). The string form `stimmungen = "standard"`
  is an error that names the replacement. `.schedule` and `.tiles` claims work in template bodies too
