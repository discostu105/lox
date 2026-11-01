- **`lox logic compile`** sets up light controller circuits from source: `Circuit1: "dimmer"` (or `"switch"`)
  on a `LightController2` names the circuit after the actuator `AQ1` drives and makes its moods dim, as the
  mood editor in Loxone Config does. `lox logic lint` warns when a mood sets a percentage on a circuit
  that is no dimmer.
- **`lox logic decompile`** and **`lox logic adopt`** lift push notifications (`OutputRefLM`) and their
  recipients, and circuits, so a config with notifications can be taken over from a real file.
