- **Lox Logic v3, part 1: ports have source names.** A port is named by a readable name instead of the
  connector key Loxone Config stores: `In1`/`In2` on every two-input block, `Trigger`, `Enable`,
  `Disable`, `Power`, `LedRed`, … (`loxl types <Type>` lists them, with the stored key where it
  differs). A v2 key is an error that names the port; `loxl fmt --write` rewrites a v2 module.
- **Lox Logic: a bare block name is its default output.** `sonne and aussentemp >= 28` reads the
  outputs of `sonne` and `aussentemp`; `pumpe.On <- wp_bedarf` wires the signal. A block with several
  meaningful outputs still takes `x.Port`. Decompile and adopt write the bare form.
- **Lox Logic: `let` aliases a port.** `let aussen = sensor.AQ` names the port wherever a port may
  stand.
