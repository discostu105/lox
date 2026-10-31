# Shading: automatic shading with weather station and wind alarm

Two external blinds (south and west) with automatic shading when the sun shines
and it is warmer than 22 °C outside, a wind alarm from the weather station
(over 40 km/h or the station's own wind warning) held for 10 minutes after the
last gust, and up/down on a Touch. Saved by Loxone Config 17.2.8.28
(`ConfigVersion 17020828`).

- Source: made for this repository (see [the library README](../README.md#our-own-entries)
  for how these entries are built and what Loxone Config stamps into them)
- License: Apache-2.0, like the crate
- Changes: placeholders for 6 values with `loxl scan --redact`, all of them
  defaults of Loxone's factory project: the coordinates (48.6021/13.8377), the
  user `admin`, the remote-access address `connect.loxonecloud.com`, the
  Miniserver serial `504F94A00000` (also the tail of the UUIDs Lox Logic
  minted, now `000000000001`) and the Tree serial `13000001`. Nothing else
  after the last save in Loxone Config.

How it was made:

1. Loxone's factory default `Default2.Loxone`, opened and saved in Loxone Config.
2. In Loxone Config: a *Touch Tree* (room Wohnzimmer) and a *Wetterstation
   Tree* (room Garten) added to the Tree branch.
3. Compiled from Lox Logic: the page *Beschattung Wohnzimmer* (room
   Wohnzimmer, category Beschattung) with input references for wind speed,
   wind warning, outside temperature, sunshine and the two keys; the wind
   alarm (`Greater`, `Or`, `OffDelay` 10 min); the shading condition
   (`Greater`, `And`); two `AutoJalousie` blocks (type 1, direction 180° and
   270°, run times 60 s up and 58 s down) with `AutoShade` and `Safety`; output
   references on relays Q1–Q4 with their titles. Saved in Loxone Config: empty
   semantic diff. That save is this file.

Files: [shading.Loxone](shading.Loxone),
[logic/beschattung_wohnzimmer.loxl](logic/beschattung_wohnzimmer.loxl) (its Lox
Logic view), [beschattung_wohnzimmer.svg](beschattung_wohnzimmer.svg) (the page
as drawn).

Worth a look: the weather station's own wind warning threshold (`WW="35"` on
the device) sits next to the page's 40 km/h comparator; both feed the same
alarm.
