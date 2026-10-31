# Energy: energy flow monitor and PV surplus to a heating rod

A grid meter (consumption and feed-in) and a PV meter, both 3-phase Tree energy
meters, in an energy flow monitor; feed-in above 3 kW switches a heating rod on
and below 0.5 kW off again. Saved by Loxone Config 17.2.8.28
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
2. In Loxone Config: two *Energiezähler 3-phasig Tree*, *Netzzähler* and
   *PV-Zähler*, added to the Tree branch, room Technikraum. Each brings three
   Modbus values: total active power, meter reading consumption, meter reading
   delivery.
3. Compiled from Lox Logic: the page *Energie* (room Technikraum, category
   Energie) with input references for the meter values, a `MeterAbsBi` *Netz*
   (power, consumption, feed-in), a `MeterAbsUni` *Photovoltaik*, an `EFM`
   *Energiefluss*, the surplus as `netz.OPf * -1` (a `Formula` block) into an
   `AnalogThresholdTrigger` (on 3, off 0.5), and an output reference on relay
   Q1 titled *Heizstab*. Saved in Loxone Config: empty semantic diff.
4. In Loxone Config: *Monitor konfigurieren* on the energy flow monitor, both
   meters added with *Zähler hinzufügen*, *Netz* as type *Netz* and
   *Photovoltaik* as *Erzeuger*, then saved. That save is this file; compiling
   the module onto it again gives it back byte for byte.

Files: [energy.Loxone](energy.Loxone),
[logic/energie.loxl](logic/energie.loxl) (its Lox Logic view),
[energie.svg](energie.svg) (the page as drawn).

Worth a look: the energy flow monitor's `<root>` in the file, one `<nd>` per
meter (`t="2"` grid, `t="4"` producer) pointing at the meter block, and the
nested `MeterAbsBi` *Rest* that Loxone Config adds for the unmetered
consumption (`NOEFM="true"`). The view lists *Rest* as a block of its own,
`rest = MeterAbsBi()`, although it lives inside the monitor.
