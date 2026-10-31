# Example library

Loxone Config projects, each with the Lox Logic view of its logic pages, so
you can see what a config looks like as code. Every entry is a config whose
license allows redistributing it.

Real configs from elsewhere:

| Entry | What it does | Config | License |
|---|---|---|---|
| [nibe-heat-pump](nibe-heat-pump/) | heating handover to a Nibe heat pump, heating stop, hot-water boost | 9.3 | MIT |
| [loxcall](loxcall/) | SIP call states parsed by a Program block | 12.1 | BSD-3-Clause |

Typical scenarios, made for this repository (see [Our own entries](#our-own-entries)):

| Entry | What it does | Config | License |
|---|---|---|---|
| [lighting](lighting/) | living room: presence sensor, light controller with moods, Touch keys | 17.2 | Apache-2.0 |
| [shading](shading/) | two blinds with automatic shading, weather station, wind alarm | 17.2 | Apache-2.0 |
| [heating](heating/) | intelligent room controller with window contact, valve, operating-mode schedule | 17.2 | Apache-2.0 |
| [access](access/) | NFC Code Touch, two users with access codes, door opener, access log | 17.2 | Apache-2.0 |
| [energy](energy/) | energy flow monitor with grid and PV meters, PV surplus to a heating rod | 17.2 | Apache-2.0 |

Each directory holds:

- `SOURCE.md`: where the file comes from (repository, commit, sha256 of the
  original), the author, the license, and every change made to it;
- `LICENSE`: the upstream license text (not for our own entries, which are
  Apache-2.0 like the crate);
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

## Our own entries

The scenario entries were built offline in Loxone Config 17.2.8.28 under Wine
(the oracle rig of [docs/oracle-wine.md](../../docs/oracle-wine.md), never
connected to a Miniserver), from Loxone's own factory default project
`Default2.Loxone` (shipped with Loxone Config in `programdata/SDcard/prog`), so
they carry no installation's data:

1. `Default2.Loxone` opened and saved once in Loxone Config.
2. The Tree devices added in Loxone Config, and whatever else only the GUI
   writes (named in each `SOURCE.md`).
3. The logic compiled from a Lox Logic module
   (`loxl compile --serial 504F94A00000 --time 1793404800`), the result opened
   and saved in Loxone Config, and `loxl diff` between the two: every entry's
   last compile came back with an empty semantic diff.

Like every entry they went through `loxl scan --redact`. Nothing in them was
personal: the six values it replaces in each are defaults of the factory
project (its coordinates, the user `admin`, the remote-access address
`connect.loxonecloud.com`, the Miniserver serial `504F94A00000` and the Tree
serial `13000001`). *access* adds two invented users and their access codes.
Devices added offline carry `Serial="00000000"`.

Loxone Config itself stamps the save date (`Date=`, `DateS=`, `LtE=`,
`LastEdit=`), a per-document `APPKEY`/`APPID`, a salt in the document's
`<HP>` (no password is set), and per-session tails in the UUIDs it mints for
connectors. None of these names the machine or the user it ran under.

To add one: build it the same way, redact the last save as above, write
`SOURCE.md` with `Source: made for this repository`, `License: Apache-2.0` and
`Changes:`, then run the `decompile` and `render` lines above.
