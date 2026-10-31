# D61: The app catalogue follows the compile

Status: accepted (2026-10-30), amended by [D66](0066-app-blocks.md) and
[D72](0072-light-controllers-catalogue.md)

## Context

The Miniserver serves the `LoxAPP3.json` of the upload as it is (see
[loxone-format.md](../loxone-format.md#the-app-catalogue-loxapp3json)). A push that replaces only
`sps0.LoxCC` leaves the app on the last save from Loxone Config. Issue: #155.

## Decision

`loxone_config::catalogue::update` carries a compile into the catalogue of the config it compiled against.

- **Text edits, not a rewrite.** Only the values that change are replaced. The rest stays as Loxone Config
  wrote it: its key order and its number formatting. `lastModified` becomes the push time, which makes the
  app reload.
- **What the compile changes, nothing else.** An entry changes where the live and compiled objects differ
  in name, room, category or favourite flag. An entry goes when the compile deletes or hides its object,
  unless another entry names it (a central block's list). Staleness from an older push stays: fixing it
  would mean trusting the rules for every entry.
- **New entries only for evidenced types.** These are push buttons, digital virtual inputs, memory flags,
  pulses at a time, digital day timers and info displays of sensors and inputs. A new entry is written
  only where Loxone Config would list it: a room and a category the catalogue knows, or a virtual input a
  page uses.
- **The generator proves itself on the live file first**, like `permissions.bin`. A type with a listed
  entry the generator does not reproduce gets no new entries.
- **The rest is named.** The push reports a room the catalogue does not list, an icon and other block
  types, for a save from Loxone Config.

Validated: three Config saves (every entry of the known types back byte for byte, 62 per save); a push to
the house and back, where the served catalogue and `LoxAPPversion3` followed.
