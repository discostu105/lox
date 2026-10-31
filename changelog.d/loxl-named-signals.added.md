- **`loxl` / `lox logic`: named signals.** `wp_bedarf = wasser.Q < 26 and not abdeckung.Q` gives an
  expression's result a name. Tests, tiles, rules and other blocks use `wp_bedarf.Q` instead of a generated slug
  - The top block is `wp_bedarf` and its sub-blocks are `wp_bedarf__<op><n>`. An arithmetic signal is a
    Formula, read at `.AQ`
  - The top block is yours: deleting the line needs `removed`, and `moved` renames it with its sub-blocks
  - Works in template bodies. Arithmetic under a comparison takes two signals: `kw = pv.Q / 1000`,
    `hoch = kw.AQ >= 1.5`
