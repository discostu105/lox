- **`lox logic test`** (also `loxl test`) simulates more of a real house: the push switch (`Switch2Button`),
  pulse at a time of day (`PulseAt`), the fixed value meter, meter and bidirectional meter (power and meter
  reading), automatic shading (`AutoJalousie`: complete open/close by run time, position, wind alarm, window
  contact, Off/lock, slightly open) and the lighting controller (`LightController2`: its motion mood from motion
  or presence below the brightness threshold, the extend time, Off/lock and the automatic timeout after a manual
  off). What Loxone does not document, such as the sun automatic, slats or mood stepping, stops the test with an
  error naming the input
- **`loxl types`** shows the canvas labels of these blocks' inputs (`EndUp` is Co, complete open), read off
  Loxone Config
