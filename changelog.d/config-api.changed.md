- **loxone-config 0.4.0** (on crates.io): a smaller public API. Internal modules (`autopilot`, `blockdoc`,
  `coverage`, `periphery`, `port_meta`, `scan`, `settings`) are private now, the ones only `loxl`, lox and the
  tests use (`cli`, `lsp`, `library`, `observed_defaults`, the older `semantic_fingerprint_v*` forms) are hidden
  from the docs, and `Module`, `CompileOptions` and `DocDiff` are re-exported at the top. The program-file
  modules no longer depend on the Lox Logic ones (`lock::Attr`, formerly `logic::Attr`, which still re-exports it).
