# Example library

Real Loxone Config projects, each with the Lox Logic view of its logic pages, so
you can see what a config looks like as code. Every entry is a config whose
license allows redistributing it:

| Entry | What it does | Config | License |
|---|---|---|---|
| [nibe-heat-pump](nibe-heat-pump/) | heating handover to a Nibe heat pump, heating stop, hot-water boost | 9.3 | MIT |
| [loxcall](loxcall/) | SIP call states parsed by a Program block | 12.1 | BSD-3-Clause |

Each directory holds:

- `SOURCE.md`: where the file comes from (repository, commit, sha256 of the
  original), the author, the license, and every change made to it;
- `LICENSE`: the upstream license text;
- `<name>.Loxone`: the config, with personal data replaced by placeholders
  (`loxl scan --redact`). For reading only, never for uploading;
- `logic/<page>.loxl`: the full decompiled view (`loxl decompile --out-dir logic`);
- `<page>.svg`: each logic page as Loxone Config draws it (`loxl render`).

## Adding an entry

Only configs we may redistribute: our own, ones under a license that allows it
(MIT, BSD, Apache, CC-BY, CC0, ...), or ones whose author gave written
permission (link the permission in `SOURCE.md`). Everything else stays in the
local test corpus (`tools/fetch-corpus.py`, see
[docs/implementation.md](../../docs/implementation.md)).

```sh
mkdir examples/library/<name> && cd examples/library/<name>
loxl scan --redact <original.Loxone> --out <name>.Loxone   # must leave 0 findings
loxl decompile --out-dir logic <name>.Loxone
loxl render <name>.Loxone --page "<Page>" --out <page>.svg  # one per logic/<page>.loxl
# then write SOURCE.md and copy the upstream LICENSE
```

`tests/library.rs` checks that every entry names its source and license and that
the views and drawings are what the current decompiler and renderer produce. When
either changes, regenerate them with the `decompile` and `render` lines above.
