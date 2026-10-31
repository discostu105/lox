# D52: Documented port facts: units, labels, ranges

Status: accepted (2026-10-29), amends [D27](0027-unit-suffixed-values.md)

## Context

Some ports Loxone keeps in minutes, not seconds. Reading every time suffix as seconds compiled
`AutoJalousie(AutoShadeTime: 2h)` to `7200`, which is 120 hours. The corpus cannot say which unit a port is
in: it holds numbers, not units. Loxone's block documentation (TechDoc) does.

lox-cli scraped that documentation into `loxone-block-types-full.json`. `tools/extract-port-meta.py` reads the
file from a path at generation time; it is never vendored. For the ports of the builtin table only, it keeps
the documented unit, canvas label, range and default in the generated `src/port_meta.rs`.

## Decision

- **Documentation is a fourth, weakest evidence tier.** It never admits a type or decides a direction. It
  supplies what corpus and oracle cannot.
- **Every generation cross-checks it.** Documented keys must exist in the table, and documented outputs must
  be outputs. The documented default is compared with the corpus GUI default
  ([D30](0030-observed-defaults.md)). On the 34 builtin types, every comparable default agrees except
  `AutoJalousie.Dir` (documented −1, corpus 180). The defaults of the minute-documented ports (`SRoff` 30,
  `SSoff` −30, `AlarmClockPeriod` 3) agree with the corpus, which corroborates the unit.
- **Suffixes scale into the port's unit.**
  - On a port documented in seconds or minutes, a time suffix scales exactly into that unit: `2h` → `120` on a
    minutes port, `90s` → `1.5`.
  - A value that is no exact decimal number of minutes (`10s`) is refused, not rounded.
  - `%` binds only on a port documented in percent, `K` only on one in Kelvin. A time suffix on a `%`, `K` or
    `°` port is refused.
  - A `let` carries its unit to every port that uses it, so one constant can mean `90` on one port and `1.5`
    on another.
  - Undocumented ports keep D27: time suffixes in seconds, `%` and `K` as annotations.
  - Extern ports are checked against the extern's declared type.
- **Labels are hints only.** The canvas shows `Tr`, the XML says `InputTrigger`. An unknown port that equals a
  documented label gets pointed at its key. Labels never resolve a port: several are ambiguous or noisy in the
  documentation.
- **Ranges are lint, not errors.** `loxl lint` reports `out-of-range` when a parameter, read in the port's
  unit, lies outside the documented range. Loxone Config clamps or accepts such values itself, and
  documentation is the weakest tier, so a compile never refuses one.
- **`loxl types`** prints the table with all of this: direction, unit, label, range, documented and observed
  default, attribute parameters. `--json` is for agents.
- **`loxl observe --crosscheck`** also reads the documented shape, expanding `%d` names against the observed
  keys. Corpus-wide it finds 736 direction agreements and one conflict, `Intercom.API`, which is API-like.
