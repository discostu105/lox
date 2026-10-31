# Heating: intelligent room controller with window contact

A bedroom heated by an intelligent room controller: temperature, humidity and
CO₂ from a room climate sensor, a window contact on the Miniserver's first
digital input, and the heating output on a valve actuator. The controller's
schedule follows the operating modes. Saved by Loxone Config 17.2.8.28
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
2. In Loxone Config: a *Raumklima Sensor Tree* and a *Stellantrieb Tree* added
   to the Tree branch, room Schlafzimmer.
3. Compiled from Lox Logic: the page *Heizung Schlafzimmer* (room
   Schlafzimmer, category Heizung) with input references for temperature,
   humidity, CO₂ and the window contact (input I1, titled *Fenster
   Schlafzimmer*), a `HeatIRoomController2` (`Temp`, `Window`, `InHumid`,
   `InCo2`, `AMode` 1) and an output reference from `AQh` to the valve. Saved
   in Loxone Config: empty semantic diff. That save is this file.

Files: [heating.Loxone](heating.Loxone),
[logic/heizung_schlafzimmer.loxl](logic/heizung_schlafzimmer.loxl) (its Lox
Logic view), [heizung_schlafzimmer.svg](heizung_schlafzimmer.svg) (the page as
drawn).

Worth a look: the controller's `<Timer>` in the file, written by Loxone Config
on the first save. Its `Modes=` lists 13 operating modes (*Haus im
Tiefschlaf*, *Erhöhter Wärmebedarf*, *Party*, *Feiertag*, *Urlaub*, *Freier
Tag/Schulferien*, Monday to Sunday), with an all-day entry for each: values 4,
3 and 2 for the first three, 1 for the rest. The heating and cooling periods
are the built-in *Heizperiode* (1 October to 30 April) and *Kühlperiode* (1 May
to 30 September) calendar entries of the factory default.
