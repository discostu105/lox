# D88: One documented line per port

Status: accepted (2026-11-01)

## Context

Port keys are the raw XML keys: `Lr`, `Lbr`, `Au` on the NFC Code Touch, `AQh`, `TSaveL` on the
intelligent room controller. Many say little without the Loxone documentation, and some say
something else (`EndUp` is *complete open*, `EnMove` disables motion). `loxl types` and the
language server showed a port's direction, unit, range and canvas label, but not what it does.

## Decision

**A one-line description per port, from the type's knowledge-base page.** `src/port_doc.rs` holds,
per builtin type, the port key, its abbreviation on the page (reference `kb-block-pages`, the page
`blockdoc::kb_page` names) and one line written from the page's summary and description. `loxl types`
prints the line under the port, `loxl types --json` gives it as `description`, the language server
puts it in the port hover and the completion detail.

**A key maps to an abbreviation only by evidence.** In order: the canvas label (`port_meta`, which
includes the labels checked in Loxone Config), the TechDoc label (reference `lox-cli-block-types`),
a key that spells the abbreviation (`Lr`, `RtD`, `OPf` for `Pf`, `Remanence` for `Rem`,
`OutputAPI` for `API`), or the mapping the simulator already models from the same page (`Reset` for
`Off`). A port with none of these keeps no description. A wrong line is worse than none.

**Checked against the pages.** A unit test checks that every entry names a real port and that its
abbreviation equals the canvas label where one is known. `tools/check-port-docs.py` downloads the
pages into `refs/kb/` and checks that every abbreviation is a row of its page. It also prints the
coverage and the rows no entry uses yet.

**Our words.** The lines are short and written for Lox Logic. They name other ports by their keys
(`for Time`, not `for D`), since keys are what a module writes.

## Rejected

- **Aliases** (`Lr` as `led_red`): a second name for every port, in the language, the lockfile and
  decompile, for what a hover line already gives.
- **Copying the page text**: Loxone's pages carry no licence to redistribute, and their descriptions
  run to paragraphs.
- **Mapping by position or by similar names** (the remaining `Off` row to the remaining input): a
  guess, however likely.

## Result

554 of the 696 ports of the 48 builtin page types, in 40 types, have a description (the virtual and
Modbus devices have no per-port tables on their pages). Gaps: the alarm's sensor and arming inputs
(`I1`–`I5`, `Active*`), CentralLight's inputs, the bidirectional and storage meters' readings, and
`Off`/`Reset` on blocks where nothing ties the key to the row. OffDelay's page has no tables.
