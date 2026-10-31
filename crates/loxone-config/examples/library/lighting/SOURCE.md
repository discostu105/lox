# Lighting: living room with presence and moods

A living room with a presence sensor, a light controller with four moods, a
ceiling light on a relay, an LED strip on an RGBW dimmer, and a Touch whose two
lower keys step through the moods. Saved by Loxone Config 17.2.8.28
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
2. In Loxone Config: a *Präsenzmelder Tree*, a *Touch Tree* and an *RGBW 24V
   Dimmer Tree* added to the Tree branch, room Wohnzimmer.
3. Compiled from Lox Logic: the page *Licht Wohnzimmer* (room Wohnzimmer,
   category Beleuchtung) with input and output references for the sensor, the
   keys, relay Q1 and the dimmer's first channel, the light controller (`Move`,
   `Presence`, `Brightness`, `InputTriggerUp`/`Down`, `BrightnessLimit` 80),
   titles for the devices. Saved in Loxone Config: empty semantic diff.
4. In Loxone Config: the light controller's mood editor opened and closed with
   OK, then saved. This is the step that writes the circuits (`NameAI<n>`,
   `CapAI<n>`, `PresM<n>`, `COName`); a plain save does not.
5. Compiled from Lox Logic: the moods *Viel Licht*, *Lesen*, *Abendlicht* and
   *Kino*. Saved in Loxone Config: empty semantic diff. That save is this file.

Files: [lighting.Loxone](lighting.Loxone),
[logic/licht_wohnzimmer.loxl](logic/licht_wohnzimmer.loxl) (its Lox Logic view),
[licht_wohnzimmer.svg](licht_wohnzimmer.svg) (the page as drawn).

Worth a look: the moods block in the view, with the RGBW circuit's colours as
`rgb(…)`. Confirming the mood editor in step 4 had set the stock *Viel Licht*
on that circuit to a colour temperature (80 %, 4700 K); step 5 sets it back to
the white the module states.
