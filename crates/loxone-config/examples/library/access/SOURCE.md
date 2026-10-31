# Access: NFC Code Touch at the front door

An NFC Code Touch in the entrance, two users with an access code each, the
door opener on a relay, and the Code Touch's access log (the last 10 entries).
Saved by Loxone Config 17.2.8.28 (`ConfigVersion 17020828`).

- Source: made for this repository (see [the library README](../README.md#our-own-entries)
  for how these entries are built and what Loxone Config stamps into them)
- License: Apache-2.0, like the crate
- Changes: placeholders for 10 values with `loxl scan --redact`: the
  defaults of Loxone's factory project (coordinates, the user `admin`, the
  remote-access address, the Miniserver and Tree serials, as in the other
  entries), the two invented users' names (*Bewohner* is now `redacted-2`,
  *Gast* `redacted-3`) and their access codes (emptied). Nothing else after
  the last save in Loxone Config.

How it was made:

1. Loxone's factory default `Default2.Loxone`, opened and saved in Loxone Config.
2. In Loxone Config:
   - an *NFC Code Touch Tree* added to the Tree branch, named *Code Touch
     Eingang*, room Windfang;
   - the device dragged onto the page, which creates the `NfcCodeTouch` block
     (category Zutritt, *Anzahl Meldungen* 10, saved as `HSize="10"`);
   - the page renamed *Zutritt Eingang*, room Windfang, category Zutritt;
   - two users added, *Bewohner* and *Gast*, each with an access code under
     *Authentifizierung bearbeiten*. The codes were invented; Loxone Config
     saved them as hashes in `CodeArr=`, which the redaction empties.
3. Compiled from Lox Logic: relay Q1 titled *Türöffner Haustür* (room
   Windfang) with an output reference from the Code Touch's `Q1`, and an
   `access` declaration that lets both users in under the built-in schedule
   *Immer*. Saved in Loxone Config: empty semantic diff. That save is this
   file.

Files: [access.Loxone](access.Loxone),
[logic/zutritt_eingang.loxl](logic/zutritt_eingang.loxl) (its Lox Logic view),
[zutritt_eingang.svg](zutritt_eingang.svg) (the page as drawn).

Worth a look: the `access` block at the end of the view, lifted from the users'
`<PRM REF="…" TIM="…"/>` rights. The redaction gives each user a name of its
own, so the rights still name them apart. Users, codes and schedules stay with Loxone
Config; the language only grants and revokes access.
