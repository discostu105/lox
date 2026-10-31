# Nibe heat pump gateway

A heating page that hands room temperature, target temperature and presence to a
Nibe heat pump through a gateway, with an outdoor-temperature heating stop and a
hot-water boost that resets after three hours. Saved by Loxone Config 9.3
(`ConfigVersion 9030326`).

- Source: https://github.com/DerFlash/Loxone-Nibe-Gateway/blob/bccfc358dec210033be1026c6fa916ffaf0d4b6a/Loxone%20Vorlagen/Nibe.Loxone
- Commit: `bccfc358dec210033be1026c6fa916ffaf0d4b6a` (sha256 of the original:
  `749b8269e3742401a156171a795a69329634084ba6fe548a1a6a1fc71d0b556d`)
- Author: Björn Teichmann (DerFlash)
- License: MIT, see [LICENSE](LICENSE)
- Changes: personal data replaced by placeholders with `loxl scan --redact`
  (5 values: the document's and the mailer's mail address, the user name,
  the Miniserver's serial and remote-access address). Nothing else.

Files: [Nibe.Loxone](Nibe.Loxone) (the redacted config),
[logic/heizung.loxl](logic/heizung.loxl) (its Lox Logic view),
[heizung.svg](heizung.svg) (the page as drawn).
