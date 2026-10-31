- **lox logic**: `lox ir` is now `lox logic` (`lox loxl` works too); the language is called Lox Logic and its files
  end in `.loxl` instead of `.lxir`. The standalone binary is `loxl` (was `lxir`), a new project's lockfile defaults
  to `logic.lock.json`, the env vars are `LOXL`/`LOXL_CORPUS`, and the editor extension and agent skill are named
  `loxl` and `lox-logic`. Existing lockfiles and fingerprints stay valid; rename your `.lxir` files to `.loxl`.
- **loxone-config 0.3.0** (on crates.io): the lxir crate is renamed to loxone-config (`use loxone_config::…`; the
  `ir` module is `logic`, `Error::IrParse` is `Error::Parse`). The last lxir release points to it.
