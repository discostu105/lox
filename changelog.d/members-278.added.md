- **`lox logic` central block members**: `alle.members = (licht_eg, licht_og)` sets the lighting
  controllers of a `CentralLight`, or the blinds of a `CentralShade`, in the order the app lists them.
  `adopt` and `decompile` lift the lists, and `diff` compares them.
- **`lox logic` energy flow units**: `Un1:`, `Un2:` and `UnSt:` on an `EFM` set its power, energy and
  storage units. The settings of room controllers, irrigation and alarm member lists stay with Loxone
  Config, refused by name with the reason.
