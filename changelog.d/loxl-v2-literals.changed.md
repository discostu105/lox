- **`loxl` / `lox logic`: Lox Logic v2 — one spelling per value** (D80). The compile output stays
  byte-identical; each spelling v2 drops is an error that names its replacement, offered as a quick fix in
  the language server
  - `on`/`off` are boolean literals in every value position (`I2: on` compiles to `1`); `Analog:`,
    `Signed:` and `CloseAfterSend:` take only them (`Analog: "true"` → `Analog: on`), and `decompile`/`adopt`
    write them
  - Tests: `tick 5min` (a duration, a whole number of `dt` steps), bare `clock 07:30` /
    `clock 2026-01-03 07:30` (was quoted), and `expect x.Q != on`
  - Rules: one-word commands are bare (`then pumpe on`; `loxl fmt` unquotes old ones), `when date 2026-12-24`
    is bare
  - Migrating: `Analog|Signed|CloseAfterSend: "true"/"false"` → `on`/`off`, unquote `clock "…"` and
    `when date "…"`, then `loxl fmt`
