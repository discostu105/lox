# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Entries for the next release wait in [`changelog.d/`](changelog.d/README.md), one file per change, and move here
when it is released.

## [Unreleased]

## [0.71.1] — 2026-11-01

### Fixed
- **`loxl compile`** and **`loxl check`** refuse a title that Loxone Config rewrites on save, and name the title it writes: `%`, `<` and `|` become `_` on most objects (other sets on lighting controllers, operating modes and calendar entries), `{`, `}`, tabs and line breaks go, and a space at either end becomes `_`. An empty title is refused too. Before, the next save showed the object as renamed. The blocks of an expression write `<=`, `<` and `%` in their labels as `≤`, `＜` and `％`, which a save keeps.

## [0.71.0] — 2026-11-01

### Added
- **`loxl lsp`** offers a quick fix on a lint finding that keeps it: `# lint: allow <kind>` above its declaration, indented like it, or the kind added to the allow already there. An `unused-allow` finding now shows on its comment and offers to remove it.

## [0.70.1] — 2026-11-01

### Fixed
- **`loxl compile`** and **`loxl check`** refuse a value that Loxone Config rewrites on save, and name the value it writes: a virtual HTTP input's `Poll` below 10 or above 604800, a `Timeout` below 10, a fraction on either or on `Allowedtimeouts`, and a digital `VirtualIn`'s `MaxVal` other than 1 or `MinVal` other than 0. Before, the next save showed them as changed.

## [0.70.0] — 2026-11-01

### Changed
- **loxl** names each operating mode one way in rules and schedules: a built-in mode in a schedule by its day word (`mon`), a mode the module declares or externs by its slug (`when mode party`, and now `party 18:00-24:00` in a schedule), any other by its title. Another spelling is an error that names the replacement, with a quick fix in the language server; `loxl fmt` on a module directory, `decompile` and `adopt` write the one spelling. The compiled config does not change.
- **`loxl lint`**: a `dead-outputs` finding on a room controller or a Touch Pure Flex controller that drives nothing says what to wire: the heating or cooling output to the valve actuators, or the controller to the API Connector of a Touch Pure Flex.

### Fixed
- **`loxl adopt`** and **`loxl decompile`** write `Signed: off`, `Analog: off` or `CloseAfterSend: off` for an unticked box on a virtual HTTP or UDP command or a virtual output. Before, they wrote nothing, and the box came out ticked when the source was compiled onto another config.

## [0.69.0] — 2026-11-01 — hardware in source

Tree and Air devices, with their inputs and outputs, can now be taken over in Lox Logic. Pairing stays in Loxone Config.

### Added
- **`loxl compile`** takes over existing hardware by declaration: `taster = TreeDevice("Taster", serial: "…")` and `klingel = TreeSensor("Klingel", device: taster, iname: "I0", …)`. The source owns the title, room, category and the settings it states, including `Statistics: on`/`off`. Tree and Air devices and their inputs and outputs, and the Tree and Air extensions, are supported. Removing a declaration leaves the hardware as it is.
- **`loxl adopt --uuid <device>`** declares a Tree or Air device with every input and output below it.
- **`extern … (serial: "…")`** matches a device by its own serial.

### Changed
- **`loxl lint`** reports bare durations also on extern port assignments (`x.TimeEnd = 120`), template value parameters and instance arguments, and `--fix` writes their units across the files of a module. The compiled config does not change.
- **Lox Logic** knows the units of the Virtual HTTP Input from Loxone's documentation: `Poll` in seconds, `Timeout` in milliseconds (`Timeout: 4s` compiles to `4000`). Modbus settings have no documented unit and stay bare numbers.

### Fixed
- **`loxl compile`** writes a 0 on a setting such as `MinVal`, `ModbusDataType` or a calendar entry's `CalType` as no attribute, the way Loxone Config stores it, so a save no longer shows it as changed. Adopt writes such a missing setting as 0 where a new object would get another default.
- **`loxl types`** and the LSP hover no longer link Greater and Less to the Comparator page of the Loxone knowledge base, which documents another block. The knowledge base has no page for these two.

## [0.68.1] — 2026-11-01

### Fixed
- **`loxl compile`** writes `Signed: off`, `Analog: off` and `CloseAfterSend: off` on virtual commands and outputs as no attribute, the way Loxone Config stores an unticked box, so a save no longer shows them as changed. A virtual input's `Analog: off` stays `Analog="false"`, which Loxone Config writes too.

## [0.68.0] — 2026-11-01

### Added
- **`loxl lint`**: a `# lint: allow <kind>` comment on the line above a declaration keeps a deliberate finding out of the report. An allow that covers nothing is reported as `unused-allow`.

### Changed
- **`loxl lint`** no longer reports what is in use: values the app shows, statistics, energy flow meters, messages, modes and calendar entries. A device's unused inputs are one finding, and so are one Touch Pure Flex's dead commands of one type. On a house config this took 231 findings down to 22.

## [0.67.0] — 2026-11-01

### Added
- **loxl types** and the language server say what each port does: one line per port from the block's
  Loxone documentation page, under the port in `loxl types <Type>`, as `description` in `--json`, and
  in the port hover and completion. 554 of 696 ports of the builtin types have one; a port whose
  meaning is not documented has none.

## [0.66.0] — 2026-11-01

### Added
- **loxl lint** groups its findings by kind with counts (`--summary`: the counts only) and reports
  bare time values that read better with their unit (`unit-suffix`: `Delay: 10800` → `Delay: 3h`).
  **`loxl lint --fix`** writes the units; in a project it refuses if the compiled config would change.
- **loxl lsp** hovers and goes to the definition of a template instance's blocks (`inst.block`,
  `inst.block.Port`), completes argument keys (in `defaults` too) and signal names, and offers quick
  fixes for `unit-suffix` and for a file without its `page` line.
- **loxl check** names the file and line of an error in a directory module (`check --json`: `path`
  and `line`).
- **Lox Logic for VS Code** has snippets for every named set (`tiles`, `schedule`, `moods`, `access`,
  `recipients`), rules and tests, and highlights the rule words (`when daylight`, `then wait`, `or`).

## [0.65.0] — 2026-11-01

### Changed
- **loxl** `fmt` writes short calls on one line (`b = And(I1: x.Q, I2: on)`: up to three arguments,
  no comments, within 100 columns) and puts a blank line around every multi-line item. Run
  `loxl fmt --write` once; the compile output does not change.
- **loxl** keyword rules: section kinds (`moods`, `schedule`, `tiles`, `rule`, …) are names outside
  their section head, `then wait|mode|notify|mail|message` is always the rule action, `tick` and
  `dt` take `let` constants, `when time` reads times like `clock`, and bulk statements and
  `mirrors:` take `inst.block`. Errors spell instance blocks as the source does, and a named signal
  that would share an expression's generated block names is an error.

## [0.64.0] — 2026-11-01 — shared block parameters, stated once

In Lox Logic, `defaults <Type>(…)` states the values a type's blocks share in one place, and decompile
and adopt factor them out of repetitive periphery.

### Added
- **`loxl` / `lox logic`: `defaults <Type>(Port: value, …)`** gives every managed block of a type the
  values it does not bind itself, template bodies included. It compiles to the same bytes as writing
  the values on each block
  - Values only, one statement per type, at the top level. `loxl lint` reports one that no block takes
    (`unused-default`), and hovering a block in the editor lists the values it takes
  - `loxl decompile` and `loxl adopt` write it for a type whose blocks sit on one page and mostly
    share a value. They also write time values with a unit (`Delay: 3h`, `Time: 500ms`) and keep the
    wires into one extern together

## [0.63.0] — 2026-11-01

### Changed
- **`loxl` / `lox logic`: a `page` statement governs only its own file.** In a directory module, blocks
  above a file's first `page` take the default page instead of the page the previous file named, so
  renaming or adding a file no longer moves blocks between pages. A single-file module does not change
  - `loxl lint` reports `default-page` for a file whose blocks come before its first `page` while other
    files name their page. Add the `page` line the file relied on

## [0.62.0] — 2026-11-01

### Changed
- **`loxl` / `lox logic`: `access` and `recipients` are named sets**, like `tiles`, `schedule` and `moods`.
  `access bewohner … end` declares who may open, `haustuer.access = bewohner` gives it to a Code Touch, and
  several Code Touches can share one set (`frost.recipients = familie` for notifications). The compile output
  does not change
  - The old `access haustuer … end` on the Code Touch is an error that names the set and the claim to write;
    the language server applies both as a quick fix
  - `decompile` and `adopt` write `<slug>_access` sets, shared by Code Touches with the same rights
  - Set parameters take `access` and `recipients` (`template tuer(rechte: access)`)

## [0.61.0] — 2026-11-01

### Changed
- **`loxl` / `lox logic`: template instances are named by the instance.** `flur.Q` reads the result of instance
  `flur`, `flur.taster.Q` any other block of it. The lock keys stay `flur_ausgang`, so the compile output does
  not change
  - The primary block is the body's only block, or the one `template name(…) -> <block>` names. It is the
    instance itself (`flur.Q`) and takes the call-site port bindings, now also in templates of several blocks
  - Works everywhere a block is named: arguments, `<-`, tests, tile `to:`, rules, claims, `removed`, `moved`,
    `loxl rename`. `adopt` writes these names, and `loxl test` reports them
  - The old `flur_ausgang.Q` is an error that names the new spelling; the language server offers it as a quick fix
- **`loxl` / `lox logic`: set parameters.** `template raum(stimmungen: moods = standard)` takes a `moods`,
  `schedule` or `tiles` set by its name (`raum(stimmungen: kueche)`). The string form `stimmungen = "standard"`
  is an error that names the replacement. `.schedule` and `.tiles` claims work in template bodies too

## [0.60.1] — 2026-11-01

### Changed
- **`loxl`** error messages are shorter and say what to do: no decision IDs, internal terms or `docs/` pointers.
- **`loxl <command> --help`** (and `loxl help <command>`) shows the help of one command: what it does and one line per option. `loxl help` is a short overview with one line per command
- **`loxl drift`** reports a lockfile written by an older Lox Logic in one short message

## [0.60.0] — 2026-10-31

### Added
- **`lox logic test`** (also `loxl test`) simulates the lighting controller as a Miniserver behaved in a live test:
  double and triple clicks (off, 2C, 3C), long T5 clicks that select or mix moods (M −3), the T5 30-second rule,
  motion held back after a manual step to off, Lc1–8, MBr and DisP. A circuit without an actuator (a controller
  minted from source has none) reads 1 for any percentage.

### Changed
- **Help, errors and MCP tool descriptions** are shorter: `lox config push`, `lox access learn`, `lox autopilot changed`, `lox stream --reconnect` and the NFC and access messages say what is wrong and what to do in two sentences at most.

## [0.59.0] — 2026-10-31

### Added
- **`loxl` / `lox logic`: named signals.** `wp_bedarf = wasser.Q < 26 and not abdeckung.Q` gives an
  expression's result a name. Tests, tiles, rules and other blocks use `wp_bedarf.Q` instead of a generated slug
  - The top block is `wp_bedarf` and its sub-blocks are `wp_bedarf__<op><n>`. An arithmetic signal is a
    Formula, read at `.AQ`
  - The top block is yours: deleting the line needs `removed`, and `moved` renames it with its sub-blocks
  - Works in template bodies. Arithmetic under a comparison takes two signals: `kw = pv.Q / 1000`,
    `hoch = kw.AQ >= 1.5`

## [0.58.0] — 2026-10-31

### Changed
- **`loxl` / `lox logic`: Lox Logic v2 — one spelling per value** (D80). The compile output stays
  byte-identical; each spelling v2 drops is an error that names its replacement, offered as a quick fix in
  the language server
  - `on`/`off` are boolean literals in every value position (`I2: on` compiles to `1`); `Analog:`,
    `Signed:` and `CloseAfterSend:` take only them (`Analog: "true"` → `Analog: on`), and `decompile`/`adopt`
    write them
  - Tests: `tick 5min` (a duration, a whole number of `dt` steps), bare `clock 07:30` /
    `clock 2026-01-03 07:30` (was quoted), and `expect x.Q != on`
  - Rules: one-word commands are bare (`then pumpe on`; `loxl fmt` unquotes old ones), `when date 2026-12-24`
    is bare
  - Migrating: `Analog|Signed|CloseAfterSend: "true"/"false"` → `on`/`off`, unquote `clock "…"` and
    `when date "…"`, then `loxl fmt`

## [0.57.0] — 2026-10-31

### Added
- **`lox logic test`** (also `loxl test`) simulates more of the lighting controller: Mood selects a mood by ID when
  its value changes, Alarm flashes every circuit at Afi with M showing 99, a mood chosen by hand stays until Off
  and goes off Met after motion ends, and M+/M−/T5 act when the input is released (as Loxone Config's simulation
  does).

## [0.56.1] — 2026-10-31

### Fixed
- **`lox setup set`, `lox alias add/remove`** (anything that saves the config): with `LOX_CONFIG` set, the config is
  saved to that file. It used to overwrite `~/.lox/config.yaml`.

## [0.56.0] — 2026-10-31

### Added
- **`lox logic test`** (also `loxl test`) simulates the lighting controller's manual inputs: M+/M− step through
  the moods in list order (the off mood one of them), T5/1–8 select their mood (scene id n) and step on, On selects
  "Viel Licht", Buzzer the alarm-clock mood and switches off after Met, and output M shows the mood's ID. A mood
  circuit at `on` reads 1. Double clicks, long clicks, Mood, Alarm and motion after a manual change are still
  refused, by name.

## [0.55.0] — 2026-10-31

### Added
- **`lox logic`: a PulseAt's fire time in the source** — `At: 19:00` (or `06:58:30`) on a `PulseAt` sets the time of
  day it fires, which used to come only from Loxone Config (#248)
  - `adopt` and `decompile` write it out, `fmt` normalizes it (`6:58` → `06:58`), `lox logic types PulseAt` lists it,
    and `diff` shows a change as `At: 18:58 -> 06:58`; `drift` now sees a changed fire time too
  - Without `At:` the time is carried from the base as before, so existing sources compile unchanged. A block set to
    a time function (sunrise, hour pulse) in Loxone Config refuses `At:`
- **`lox logic coverage`** lists, per managed block type, the attributes the compile still carries from the base
  instead of writing them from source, with the parameter that claims one where Lox Logic has it
- **loxone-client 0.1.3** (on crates.io): `loxone_client::system::code_of()` reads the `Code` of a parsed `LL` answer
  in both forms (Gen 1 string, Gen 2 number); `lox`'s commands, MCP server, user management and NFC learning
  use it instead of their own copies.

### Fixed
- **`lox set`**: a Gen 2 Miniserver's answer (`"Code": 200` as a number) is no longer reported as `Error ?`.

## [0.54.0] — 2026-10-31

### Added
- **`lox logic test`** (also `loxl test`) simulates more of a real house: the push switch (`Switch2Button`),
  pulse at a time of day (`PulseAt`), the fixed value meter, meter and bidirectional meter (power and meter
  reading), automatic shading (`AutoJalousie`: complete open/close by run time, position, wind alarm, window
  contact, Off/lock, slightly open) and the lighting controller (`LightController2`: its motion mood from motion
  or presence below the brightness threshold, the extend time, Off/lock and the automatic timeout after a manual
  off). What Loxone does not document, such as the sun automatic, slats or mood stepping, stops the test with an
  error naming the input
- **`loxl types`** shows the canvas labels of these blocks' inputs (`EndUp` is Co, complete open), read off
  Loxone Config

### Fixed
- **`loxl types MeterDig`** describes the fixed value meter it is, not a pulse meter; `Switch2Button` is the push
  switch (a pulse on each edge), not a toggle

## [0.53.0] — 2026-10-31

### Added
- **`lox logic test --trace`** (also `loxl test --trace`) prints each test cycle by cycle: what a step sets,
  every change of the ports its expectations depend on, named by slug, and each `expect` with its actual value.
  A failing run without it points to the flag.

## [0.52.1] — 2026-10-31

### Added
- **loxone-client 0.1.2** (on crates.io): more typed reads — `LoxClient::network()` (IP, mask, gateway, DNS, MAC,
  DHCP, NTP), `bus_stats()` (CAN bus counters), `lan_stats()` (LAN counters), `sdtest()` (the parsed SD card test)
  and `log()` (the system log, with a clear error when the user isn't an administrator), plus `check_ll()` for the
  `Code` of a command's answer. `lox status`, the TUI and `lox otel` now read these through the client instead of
  each keeping its own list of endpoints.

### Changed
- **`lox status --bus/--lan -o json`**: the counters are numbers (`"overruns": 0`), and `null` when the Miniserver
  leaves one empty, as with `--diag`.

## [0.52.0] — 2026-10-31

### Added
- **`lox input set/pulse`** reach virtual inputs that are not shown in the app by name: a name that matches no
  control is looked up among the virtual inputs of the config (config repo or cached download), e.g.
  `lox input pulse "Test Frostwarnung"`

## [0.51.0] — 2026-10-31

### Added
- **`lox config push`** keeps the push settings of registered phones: when someone changes which notifications
  their phone receives in the Loxone app, the Miniserver holds that as a custom change, and the push used to refuse
  (or, with `--discard-custom-changes`, silently reset the phone, which then got no notifications). Now the push
  carries these changes into the upload (`custom_changes_kept` in `-o json`); other custom changes still refuse

## [0.50.1] — 2026-10-31

### Added
- **loxone-client 0.1.1** (on crates.io): typed reads of the system state — `LoxClient::system_status()` (firmware,
  PLC state, heap), `heap()`, `diagnostics()` (CPU and PLC load, tasks, interrupts, context switches) and
  `value(path)` for any `LL` value, with the parsers public in `loxone_client::system`.

### Changed
- **loxone-config 0.5.0** (on crates.io): the D76 rule conditions and actions (weather, day, daylight, sun, date;
  notify, mail, wait, message) and D77 (a stale constant under a wire) for library users. The rule enums in
  `logic` gained variants, so exhaustive matches need new arms.
- **lox status --diag**: shows the PLC's CPU share next to the CPU load; the JSON reports `cpu`, `sps`, `tasks` and
  the counters as numbers (`null` when the Miniserver leaves them empty). `lox status`, the TUI, `lox otel` and the
  MCP `system_status` tool now read the system state through the same loxone-client calls.

### Fixed
- **loxl decompile/adopt**: the *Rest* meter an energy flow monitor creates for itself is part of the monitor, no longer a top-level `rest = MeterAbsBi()` block; adopting it alone is refused (D78, #236).
- **lox otel**: `loxone.system.cpu` is recorded again on current firmware (`CPU:12 SPS:26` read as no value).

## [0.50.0] — 2026-10-31

### Added
- **loxone-config** example library: five typical scenarios made for this repository with Loxone Config 17.2 from its factory default project (lighting with presence and moods, automatic shading with weather station and wind alarm, an intelligent room controller with window contact, an NFC Code Touch with users and access codes, an energy flow monitor with PV surplus to a heating rod), each with its Lox Logic view and page drawing
- **`loxl scan --redact`** gives each distinct name its own placeholder (`redacted`, `redacted-2`, …), so redacted users stay apart and their access rights still lift

## [0.49.0] — 2026-10-31

### Added
- **lox mcp serve**: `get_wiring` takes `live: true` and fills in the current value on every wire whose source the app streams, plus the control's own value in its app format (`22000lx`), so an assistant answers "why is this light on?" in one call
  - One WebSocket state read for all wires; a wire without a streamed source gets no value, and a failed read leaves the config wiring with a `live_error`

## [0.48.1] — 2026-10-31

### Fixed
- **`lox logic decompile`** no longer prints a stale constant under a wire as a second argument for the same
  port (`Input1: 2, Input1: x.AQ`): the wire stays the argument and the constant becomes a comment on it, and
  `lox logic compile` keeps it in the config. **`lox logic check`** refuses a port bound to both a value and a
  wire, with the error on the port in the call (#226)

## [0.48.0] — 2026-10-31

### Added
- **`lox energy flow`** breaks down the energy flow right now: production, consumption, grid import/export,
  battery charge/discharge and state of charge, read from the Energy Flow Monitor (`-o json` adds every node)
  - **`lox energy --top [N]`** ranks the meters by current power, highest first (default 10)
  - MCP: the new `get_energy_flow` tool returns the same breakdown; `top` adds the meter ranking
  - `lox tui` and `lox energy flow` share one interpretation of the monitor's nodes and roles (`roles:` in
    `~/.lox/tui.yaml` applies to both); `lox energy` now lists the flow monitor (type `EFM`) too

### Fixed
- **`lox logic compile`** no longer rewrites the Automatic Designer's watch list when no rule changed, so compiling
  the rules a config already has reproduces it byte for byte; `OutputAPI` of `LightController2` and `MeterAbsSt`
  takes wires in both directions (API links), as the public corpus shows

## [0.47.0] — 2026-10-31 — rules with weather, dates and messages

Automatic Designer rules can react to the weather, the day of the month, daylight, sunrise and sunset and a date, and send push notifications, mails and message-center entries with pauses in between. Existing user operating modes are taken over with `adopt --uuid`.

### Added
- **lox logic rules**: conditions `when weather temperature > 25` (and the other fields of the weather service), `when day >= 15`, `when daylight [off]`, `when sunrise|sunset|dawn|dusk` and `when date "2026-12-24"`; actions `then notify "<subject>" "<text>"`, `then mail "<to>" "<subject>" "<text>"`, `then wait 2.5s` and `then message warning "<subject>" "<text>" [admins]`. Compiles wire the weather, day and daylight blocks into the AutoPilot container as Loxone Config does.

### Changed
- **loxone-config 0.4.0** (on crates.io): a smaller public API. Internal modules (`autopilot`, `blockdoc`,
  `coverage`, `periphery`, `port_meta`, `scan`, `settings`) are private now, the ones only `loxl`, lox and the
  tests use (`cli`, `lsp`, `library`, `observed_defaults`, the older `semantic_fingerprint_v*` forms) are hidden
  from the docs, and `Module`, `CompileOptions` and `DocDiff` are re-exported at the top. The program-file
  modules no longer depend on the Lox Logic ones (`lock::Attr`, formerly `logic::Attr`, which still re-exports it).
- **lox logic adopt --uuid**: documented for user operating modes made in Loxone Config: the mode keeps its uuid and `Mode=` number, a new title renames it.

### Fixed
- **lox logic compile**: the "past the edge of the A3 sheet" warning lists only objects the compile placed, not ones laid out that way in Loxone Config (whose `--relayout` would rearrange the page).

## [0.46.0] — 2026-10-31

### Added
- **Example library** of real Loxone Config projects in `crates/loxone-config/examples/library/`: each with its source, license, Lox Logic view and page drawings, so you can see what a config looks like as code

### Fixed
- **`loxl scan`** (and `lox logic scan`) also finds the project's creator and phone number in the document properties, and `--redact` replaces them; before, a shared copy kept both

## [0.45.0] — 2026-10-31 — smarter rules, calendar, shareable configs

Automatic Designer rules can watch states and operating modes, combine conditions with `or` and switch modes; the *Betriebszeiten* calendar is managed from source; and `loxl scan --redact` makes a config safe to share.

### Added
- **lox logic rules**: conditions on control states (`when pumpe.active == on`, `when temp.value > 25`) and operating modes (`when mode "Heizperiode"`), `or` between condition groups, and `then mode <mode> on|off`; a `then` takes every command the app offers for the block. The codes come from the Miniserver's own Automatic Designer table. Compiles keep the AutoPilot container's inputs as Loxone Config saves them.
- **lox logic**: calendar entries (*Betriebszeiten*) from source: `CalendarEntry("Sommerferien", CalType: 2, Mode: 3, From: 6393, To: 6456)`, fixed dates, Easter-relative days, periods and yearly ranges; existing entries are taken over with `adopt --uuid`.
- **lox logic scan --redact**: `loxl scan --redact <cfg> --out <copy>` writes a copy of a config with all personal data and serials replaced, to share in an issue.

### Changed
- **lox logic adopt --uuid**: promoting an extern keeps its room, category and schedule claims (they move to the block); only a tiles claim is still refused.

### Fixed
- **lox logic decompile --out-dir**: a page whose rules drive blocks on other pages declares them as externs instead of failing with "undeclared slug".
- **lox config push**: operating modes made in source appear in the app (`operatingModes`), and renamed or deleted ones follow.

## [0.44.0] — 2026-10-31 — lox logic

The config-as-code language has a name of its own: Lox Logic, written in `.loxl` files and run as `lox logic` or `loxl`. Its library crate is loxone-config now (formerly lxir).

### Changed
- **lox logic**: `lox ir` is now `lox logic` (`lox loxl` works too); the language is called Lox Logic and its files
  end in `.loxl` instead of `.lxir`. The standalone binary is `loxl` (was `lxir`), a new project's lockfile defaults
  to `logic.lock.json`, the env vars are `LOXL`/`LOXL_CORPUS`, and the editor extension and agent skill are named
  `loxl` and `lox-logic`. Existing lockfiles and fingerprints stay valid; rename your `.lxir` files to `.loxl`.
- **loxone-config 0.3.0** (on crates.io): the lxir crate is renamed to loxone-config (`use loxone_config::…`; the
  `ir` module is `logic`, `Error::IrParse` is `Error::Parse`). The last lxir release points to it.

## [0.43.0] — 2026-10-31 — lox as libraries

The Miniserver client and the config toolchain are now two crates on crates.io, loxone-client and lxir 0.2.0, so other Rust projects can talk to a Miniserver and read its program file the way lox does.

### Added
- **lxir 0.2.0** (on crates.io): every command that reads a config (`lxir check`, `compile`, `diff`, `decompile`, …) also takes a
  Miniserver backup ZIP; the library gains `lxir::archive` (feature `archive`, on by default): the LoxCC codec,
  extracting the program file and staging an upload ZIP, moved over from lox
- **loxone-client**: lox's Miniserver client is a library crate of its own on crates.io: the HTTP API with
  control resolution and the structure cache, token auth, the WebSocket state stream, FTP backups, Statistics V2
  and the statistics files. It takes a `Connection` and reads no config files, so any Rust project can use it.

### Changed
- **lox config users/devices**, the push's user guard and `lox access check` read the program file through lxir's
  model instead of a parser of their own (same output; one parser less to keep in step)

## [0.42.1] — 2026-10-31

### Changed
- lxir: highlight extern mirrors:, no dead-outputs warning for family parents (#217)

## [0.42.0] — 2026-10-31 — device settings, bulk statements, light controllers in the app

Air and Tree devices get their own settings from source, one statement can set many objects, and light controllers shown from source appear in the app with their circuits.

### Added
- **`lxir templates`**: a template library — motion and staircase light, push-button, heating and humidity hysteresis, frost protection, CO2 ventilation, door hint, water and intrusion alarm, awning and sun protection — each tested in the simulator; `lxir templates <name>` prints the file to copy into a module, and one line calls a template
  - Template object parameters may be typed `any`, so a template takes a virtual or a physical sensor alike
- **`lxir types`**: each block type says what it is for (purpose, main inputs and outputs, the Loxone knowledge-base page where known), `lxir types --search <text>` finds types by name or purpose, and the LSP hover on a type shows the same
- **lxir**: `(a, b, c).MinTime = 600` sets a setting, title, room or category on several objects in one statement, and `fmt` keeps the one line (#199).
- **lxir**: an Air or Tree device's own description, installation place and switch board can be owned from source (`Desc`, `Installation`, `SwitchBoard`, `SwitchBoardRow`, `SwitchBoardPos`), and `device:` matches Air devices and their I/Os by serial (#197).
- **lox config push**: a light controller shown from source gets its app entry, with its circuits, masters and colour ranges (#184).

### Changed
- **lxir drift**: the fingerprint is v9 and also compares device settings; a lock with a v8 fingerprint still reads as in sync.

## [0.41.0] — 2026-10-31 — operating modes as code

Schedules can name any operating mode of the config, and a module can create its own modes; who lives in the house stays with the Miniserver.

### Added
- **`lxir` operating modes from source** (D69): a schedule line names any mode of the config by its title (`"Party" 18:00-24:00`), `heating` and `cooling` name the heating and cooling periods, and `party = Mode("Party")` creates a user-defined mode in the "Betriebsmodi" folder
  - Checked in Loxone Config: a save leaves the new mode and the schedule unchanged. Calendar entries stay with Loxone Config (#211); users stay with the Miniserver (D70)

## [0.40.0] — 2026-10-31

### Added
- **`lxir` agent cases** (`crates/lxir/tools/agent`): sixteen requests in plain German, each with hidden `lxir test` blocks and a reference solution; `run.py` lets Claude Code solve them with the lxir skill and reports the pass rate
  - `lxir test` now reads an input of a block without a model through its wires, so tests can expect on what the logic sends to an actuator (`VirtualOutCmd.I`)

## [0.39.0] — 2026-10-31 — the whole house under source

Rules can be deleted from source (`removed`), app-made rules and existing virtual inputs can be adopted, and a push no longer stops on the custom change of a rule deleted in the app. On the test house, all page logic and all virtual inputs are now managed.

### Added
- **`lox ir` / `lxir`**: `removed <slug>` deletes a named Automatic Designer rule from the program, the way it deletes a block. A rule deleted in the app thus stays deleted after the next push.
- **`lox ir adopt` / `lxir adopt`**: `--uuid <rule>` takes over one Automatic Designer rule as `rule <slug> "Name"`, and an extern rule of the project is promoted. Decompile and adopt declare rules by name when no other rule shares it.
- **`lox ir adopt` / `lxir adopt`**: `--uuid <virtual input>` takes over an existing VirtualIn and promotes an extern on it. Its place in the folder stays as it was.

### Changed
- **`lox config push`**: a custom change to an object that the upload deletes (`removed` in the source, a rule deleted in the app, for example) goes with the object. It no longer stops `--force`, and the JSON marks it `"removed": true`.

### Fixed
- **`lox ir compile` / `lxir compile`**: a managed virtual input is rebuilt in its place in the "Virtual Inputs" folder, so it no longer moves to the end of the folder.

## [0.38.0] — 2026-10-31 — tests without lox-cli

`lox ir test` simulates the logic itself, built only on what Loxone documents: a test passes on documented
behavior or tells you which block it cannot simulate.

### Added
- **`lox ir test` / `lxir test`** runs the `test … end` blocks in a simulator of its own: no lox-cli install needed,
  and `examples/ci.sh` now always runs a module's tests
  - 31 block types are modelled from Loxone's documentation (logic gates, comparators, arithmetic, formulas, memory
    flags, refs, monoflop, on/off delays, switch, push-button, flip-flop, edge detection, threshold switch,
    operating modes, schedules); `crates/lxir/docs/simulator.md` lists each with its source
  - A test that depends on any other block reports `ERROR` and names it instead of passing on a guessed value;
    setting a port between that block and the expectation cuts it out
  - Ports are addressed by UUID through the lockfile, so blocks no longer need unique titles
  - `--lox <path>` still runs the tests in lox-cli's `lox sim`; `--show-spec` now needs it

## [0.37.0] — 2026-10-31 — new blocks in the app without a Loxone Config save

A jalousie written in source now shows up in the Loxone app straight after the push, and the app catalogue knows jalousies, central blocks and meters.

### Added
- **`lox config push`** lists jalousies, central light and shading blocks and meters in the app when a compile gives them a room and a category, with entries reproduced byte for byte from Loxone Config's; a jalousie minted from source gets the special states a save would give it, so it is in the app right after the push.
- **`lox config push`** names a new control that cannot appear in the app because it has no room or category.

## [0.36.0] — 2026-10-31 — devices and their settings in source

The virtual inputs and outputs and the Modbus servers behind them are now source as well, declared, changed and adopted with their I/O like the logic on the pages. The settings of the physical sensors and actuators (description, unit, correction, timing) can be owned by the source too.

### Added
- **lxir**: virtual and Modbus devices from source (D64). The HTTP, UDP and text virtual inputs, virtual outputs and Modbus servers, together with their commands, devices, sensors and actuators, are declared like blocks. `device:` names the device an I/O hangs in, and the settings (`Address:`, `Check:`, `ModbusAddress:`, …) are parameters. `lxir adopt --uuid <device>` takes an existing device over with all its I/O and promotes the externs on them. `lxir coverage` lists the devices not managed yet.
- **lxir**: the settings of physical inputs and outputs from source (D65). On an extern of an extension's, Tree, Air or 1-Wire input or output, `x.Desc = "…"`, `x.Unit = "<v.1> °C"`, `x.MinTime = 600` and the analog correction claim that setting; the original returns when the line leaves the source. `lxir diff` and the drift fingerprint now compare these settings and those of the devices; locks written by an older lxir still read as in sync.
- **lox config push**: a new display unit is carried into the app catalogue (`LoxAPP3.json`), so the app shows the new format without a Loxone Config save.

## [0.35.0] — 2026-10-30 — the whole house in source

The alarm, smoke alarm, irrigation, room controllers, energy flow monitor and meters join the blocks lxir manages,
an extern can be promoted to a managed block in place, and `lox ir coverage` shows what is left.

### Added
- **`lox ir`: the house control blocks are managed** — alarm, smoke alarm, irrigation, the intelligent room
  controller, the energy flow monitor and the four meter types can be declared, adopted and rebuilt from source;
  Code Touch, Intercom and Touch Pure Flex adopt (Loxone Config creates them when it pairs the device)
  - Their internals (zones, timers, statistic groups) pass through unchanged, and a minted block passes a save in
    Loxone Config with an empty diff
- **`lox ir adopt --uuid <x> --as <extern>`** promotes an extern to a managed block: its wires and port values move
  into the declaration, and the other modules keep their references
- **`lox ir coverage`** lists the config's blocks by type (managed, extern, rule, untouched) and the blocks left to
  adopt (`--json`)
- **Managed blocks take `.room` and `.category`**, so a block made in source shows up in its room in the app

### Fixed
- **`lox ir compile`** keeps the flag Loxone Config set on a wire into an extern, so a compile of an unchanged
  source is byte-identical
- **`lox ir adopt --uuid`** works in modules that use templates or Automatic Designer rules (it pinned a rule as
  a new extern and rejected template slugs)

## [0.34.0] — 2026-10-30

### Changed
- **License** changed from GPL-3.0 / commercial to Apache-2.0, for `lox` and the `lxir` crate
  - No copyleft, no commercial license: use, modify and redistribute freely, including in closed-source products

## [0.33.0] — 2026-10-30

### Added
- **`lox config push`** writes app catalogue entries for more block types: memory flags, pulses at a time, digital day timers and the info displays of sensors and inputs (digital and analog, with statistics). Each one reproduces Loxone Config's entries byte for byte.

## [0.32.0] — 2026-10-30

### Added
- **`lox ir lsp`** finds references and renames module-level names (constants, externs, blocks, templates, instances) across a module's fragments. In a project, rename is `lxir rename`'s verified plan, and the lockfile is re-keyed in the same edit. "did you mean `X`?" diagnostics get a quick fix.

## [0.31.0] — 2026-10-30 — the app sees what you push

`lox config push` no longer leaves the app on the last Loxone Config save: it edits the app catalogue for the controls a push renames, moves, removes or adds.

### Added
- **`lxir::catalogue`**: carries a compile into the app catalogue `LoxAPP3.json` as text edits, with entries for push buttons and digital virtual inputs validated against Loxone Config saves.

### Changed
- **`lox config push`** updates the app catalogue (`LoxAPP3.json`) it uploads: renamed, moved, deleted and hidden controls, and new push buttons and digital virtual inputs with a room and a category, get their entries, and a new `lastModified` makes the app reload. What it cannot carry over is still named (`app_catalogue_updated`, `app_catalogue_stale` in `-o json`).

## [0.30.0] — 2026-10-30 — pushes without a reboot

`lox config push` now applies a config the way Loxone Config does: it restarts only the program, and the Miniserver runs the new config about two seconds later.

### Changed
- **`lox config push`**: applies the upload in seconds without rebooting the Miniserver. Like Loxone Config's save, it restarts only the program, which loads the new config and keeps remanent values. A push now takes a few seconds instead of over a minute. `--reboot` restarts the whole Miniserver as before

## [0.29.0] — 2026-10-30

### Added
- **lxir `rule <slug> "Name"`**: create Automatic Designer rules from source. A named rule is the config's rule of that name, or a new one the compile adds below the existing rules, inactive until its body says `active`
  - The same body as for existing rules (`active`/`inactive`, `when time`, `when weekday`, `then <block> "<command>"`); changing the name in source renames the rule
  - The lock pins the rule's UUID (lockfile v7), so compiles before the push are a fixpoint and `lxir rename` keeps the rule
  - Checked with Loxone Config: it keeps a created rule on open and save. The config needs at least one rule made in the app first

## [0.28.1] — 2026-10-30

### Fixed
- **MCP `get_control`** on a lighting controller (LightControllerV2) reports each circuit's real value and the active moods (`activeMoods`). The circuit values come from the Miniserver's live states over the WebSocket, because the HTTP read gave 383 for every circuit, whatever the mood.

## [0.28.0] — 2026-10-30 — a language server for config as code

`lox ir lsp` brings lxir into your editor: errors at the right line while you type, hover with the object an extern resolves to, port completion and formatting.

### Added
- **`lxir lsp` / `lox ir lsp`**: a language server for `.lxir` modules, so editors show errors and help while you write config as code
  - Diagnostics as you type (what `check` and `lint` report). On save in a `lox.toml` project, the module is also compiled against the base config in memory (extern resolution, lockfile, dead outputs); nothing is written
  - Go to definition across the fragments of a module; hover shows the object an extern resolves to (type, title, room, page, UUID) and a port's direction and documented range
  - Completion of ports after `name.` and of block types after `name =`, plus formatting and the outline
  - The VS Code extension in `crates/lxir/editor/vscode` starts it (run `npm install --omit=dev` once in that folder). For Neovim, see the lxir README

## [0.27.0] — 2026-10-30

### Changed
- **`lox config push`** also warns when the push adds an object the app shows (`Visu`), or shows or hides an existing one: the uploaded `LoxAPP3.json` stays as Loxone Config last saved it (its date is in the note) until the next save from Loxone Config (#155)

## [0.26.0] — 2026-10-30

### Added
- **`lxir adopt` and `lxir decompile`** now turn a config's existing lighting moods into source: a `moods` set and a claim per controller, so taking over a house no longer means typing every mood by hand (#170)
  - Controllers with the same moods share one set, ready to move into a room template
  - Adopted and compiled again, the config stays byte-identical; a controller with only the stock "Viel Licht" is left alone

### Fixed
- **`lxir compile`**: changing a controller's moods keeps the off mood "Aus" at its place when it sits between other moods, instead of moving it to the end

## [0.25.0] — 2026-10-30 — lighting moods from source

The moods of every room are code now: declare a set of moods once, and give it to each Lichtsteuerung from a room template.

### Added
- **Lighting moods from source** (`lox ir`/`lxir`): `moods <set> … end` lists a Lichtsteuerung's moods by name
  with the values of its circuits, and `<controller>.moods = <set>` makes them that controller's mood list
  - Values follow each circuit's kind: `on`/`off`, `40%`, `rgb(100, 80, 0)`, `temp(60%, 2700K)`; a value the
    circuit cannot take is a compile error
  - Claim a set inside a room template and every room gets the same moods. Existing moods keep their identity
    by name, so switch buttons wired to `Sel1`…`Sel8` keep selecting them
  - A controller minted from source gets Loxone Config's stock moods ("Viel Licht", "Aus") first, so it no
    longer needs a save in Loxone Config before its moods can be set
  - `lxir diff` shows mood changes per mood, and `lxir drift` reports moods edited in Loxone Config or the app

```text
moods wohnen
	"Viel Licht": all on
	"Lesen": AQ1 80%, AQ3 temp(60%, 2700K)
	"Kino": AQ4 rgb(0, 0, 20)
end
wohnzimmer_licht.moods = wohnen
```

## [0.24.0] — 2026-10-30

### Added
- `lox ir`: Automatic Designer rules from source (D57). `rule <slug>` on an `AutopilotRule` extern states whether the
  rule is `active`, its conditions (`when time 09:00`, `when weekday mon tue`) and its actions (`then pumpe "on"`);
  `decompile`/`adopt` lift every rule of these shapes, and the compile writes `APC=` only when the source changes the
  rule, as the app's own edit (`STA` follows `active`). Action codes come from the config itself or the observed
  PushButton/PButtonT ones; anything else is refused
- **`lox autopilot changed --explain`** (#156): says what moved the rules' `changed` time. It compares
  the newest program file in `/prog` with the one before it (or `--since FILE`), sorted into rules,
  users & access and other changes, and lists the custom changes pending in `/sys/custom`. With
  `-o json`, `rules_changed` tells a guard script whether a rule really changed, so a password set in
  the app no longer looks like an edited rule.

## [0.23.1] — 2026-10-29

### Fixed
- `lox ir diff` shows DayTimer schedule changes, per timer and day group in the terms of a `schedule` block
  (`DayTimer "Abendlicht" schedule weekdays: 17:00-22:00 -> 17:00-23:00`), also with `--summary`, `--json` and
  `--exit-code`; `lox ir drift` and the drift check of `lox config push` now see a schedule changed in Loxone Config
  or the app. Fingerprints recorded by older versions stay accepted (#162)

## [0.23.0] — 2026-10-29

### Added
- Visualization password for secured commands: `LOX_VISU_PASSWORD`, or a file named by `visu_password_file:` in the
  config (`lox setup set --visu-password-file ~/.lox/visu-password`) or `lox access learn --visu-password-file`;
  otherwise a hidden prompt. Never taken from an argument, never printed

### Fixed
- `lox access learn` and the MCP tool `learn_nfc_tag` work at a secured NFC Code Touch (`isSecured`): learn mode
  starts and ends as secured commands signed with the visualization password, instead of failing with code 500.
  A 500 on a secured command now says "wrong visualization password, or the user has none", and a tag that is
  already paired (it opens the door instead of being learned) is reported at once instead of after the timeout
  (#159)
- `lox user show` prints the last edit in local time; the Miniserver counts `lastEdit` from 2009-01-01 UTC, so it was
  off by the UTC offset (JSON keeps the raw value) (#160)

## [0.22.0] — 2026-10-29

### Added
- `page "<Title>" (room: "<Room>")` creates a page the config lacks, filed by title like "Neue Seite" in Loxone
  Config; optional `category:`. Without `room:` a missing page stays an error (lxir D56, lockfile v5)

## [0.21.0] — 2026-10-29

### Added
- **Push notifications from source**: in a `.lxir` module, `OutputRefLM(mirrors: <notification>, AI: …,
  On: "…", Off: "…", MinimumTime: 12h)` draws a push notification on a page with its message, and a
  `recipients <notification> … end` declaration lists the users and groups that get it, by title
  (`user "alice"`). The notification itself stays with Loxone Config (an extern `Notification(…)`). Message
  texts show in `lox ir diff`, and the semantic fingerprint now covers them (`lox ir drift` still accepts
  fingerprints recorded before).

## [0.20.0] — 2026-10-29

### Added
- **Switching times from source** (lxir D53): a `schedule <name> … end` block in a `.lxir` module is a DayTimer's
  week (`weekdays 06:00-07:30, 17:00-22:30`, `holiday vacation off`, values and `activate` for analog timers), and
  `<timer>.schedule = <name>` writes it onto a DayTimer, extern or managed
- `lox ir compile` mints virtual inputs (`party = VirtualIn("Partymodus")`) in the Virtual Inputs folder, with the
  next free `VI<n>` (lxir D54), and eight more block types verified against Loxone Config saves (delays,
  staircase light, edge detection, flip-flop, multiplexer, state); `lox ir types <Type>` shows each port's
  documented unit, label and range, unit suffixes scale into the port's unit, and `lox ir lint` flags constants out
  of range (lxir D52)
- `lox ir scan <file|dir>`: lists the personal data in a config (address, users, codes, credentials, remote access,
  serials) by element and attribute, never the value, and exits 1 on any finding; CI runs it over every committed
  `.Loxone`
- `lox ir diff --summary`: the semantic diff as a commit message, by block title and port name, users/access
  changes included
- `extern … (device: "<serial>")` narrows a repeated title to the objects of one Extension or Tree device (lxir D55)
- An agent skill for writing `.lxir` modules (`crates/lxir/skills/lxir/SKILL.md`)

## [0.19.1] — 2026-10-29

### Fixed
- `lox ir compile` keeps placed blocks on the page's paper sheet (A3 landscape, from the config's `PaperFormat`): a cluster short of room is packed tighter, and goes past the edge only if it fits nowhere even then. Compile warns per page about managed objects past the sheet's edge and names the `--relayout` that lays the page out again; `lox ir render` reports and draws the sheet (lxir D50)

## [0.19.0] — 2026-10-29

### Added
- **Access rights from source** (#140): an `access <code_touch> … end` declaration in a `.lxir` module lists the
  users and groups that may open an NFC Code Touch, by title, each with its access schedule
  (`user "alice": "Always"`). It owns that block's access: compiling grants, reschedules and revokes, writing the
  same `<PRM>` edit Loxone Config makes; Code Touches without a declaration are not touched. Users, groups,
  schedules, tags and codes stay with Loxone Config and the app, and a title that is missing or not unique is an
  error. `lox ir decompile` and `lox ir adopt` lift existing rights into declarations; `lox ir lint` reports
  `no-access-right` for a user whose NFC tag or access code opens no Code Touch.
- lxir reads and generates `permissions.bin`, the program ZIP's table of who may use which object
  (`lxir::permissions`), byte for byte against the files Loxone Config writes.

### Changed
- `lox config push` stages a `permissions.bin` generated for the compiled config when lxir reproduces the live
  one byte for byte, so access granted or revoked in the module reaches it. Otherwise the live file stays, and
  the push warns when the compiled access rights no longer match it. The JSON report has `permissions`
  (`unchanged`, `regenerated`, `kept`, `absent`) and `permissions_stale`.

## [0.18.0] — 2026-10-29

### Added
- `lox config push` no longer deletes user changes silently (#138). Loading an upload deletes what the Miniserver
  holds apart from its program file, users, groups, NFC tags and access codes changed in the app or with
  `lox user` among them, and the drift check only sees what a download holds. The push now compares every live
  user with the program file (exists, name, last edit, groups, NFC tag names and count, number of access codes):
  staging lists the differences (`user_changes_pending` in `-o json`), `--force` refuses with error
  `user_changes_pending` unless `--discard-custom-changes`. The check needs an admin connection; users it cannot
  read stop `--force`.

## [0.17.0] — 2026-10-29

### Added
- **NFC tags and access codes** (#139): `lox access learn <code-touch>` reads an NFC tag's ID at a Code Touch
  (learn mode over the WebSocket, ended again on success, error, timeout and Ctrl-C) and says whether the tag is
  known; with `--user` and `--name` it pairs the tag with that user. `lox user nfc add <user> <id> <name>` and
  `lox user nfc rm <user> <id|name>` pair and remove tags by ID in any spelling. `lox user code set <user>` sets
  an access code from a hidden prompt or stdin, never from the command line; the code stays out of output,
  errors and the `-v` log, and a code another user has fails with `code_in_use`. Every change asks for
  confirmation (`--yes` skips it; with `-o json`, `--non-interactive` or no terminal it needs `--yes`),
  supports `--dry-run`, is sent once without retries, and reminds you that it may live only in the
  Miniserver's custom changes until Loxone Config saves the program. New MCP tools `learn_nfc_tag`,
  `assign_nfc_tag`, `remove_nfc_tag` and `set_access_code`, all confirmed by the user.
- `lox config push --force` refuses while the Miniserver holds custom changes: what the app, the web UI or the API changed and Loxone Config has not saved yet (light moods, timer schedules, autopilot rules, kept as `/sys/custom/<UUID>.LoxCC`). A download doesn't contain them and loading the upload deletes them. The push names them (`custom_changes_pending` in `-o json`); `--discard-custom-changes` pushes anyway. `lox config upload` and `lox config restore` list them before uploading
- `lox config push` names the app controls it renames, moves to another room or category, or deletes: the upload carries Loxone Config's app catalogue (`LoxAPP3.json`) from before. After the reboot it says whether the Miniserver serves that catalogue or a rebuilt one (`app_catalogue_stale`, `app_catalogue_rebuilt` in `-o json`)

## [0.16.0] — 2026-10-29

### Added
- **Users and door access, read-only** (#136): `lox user ls`, `lox user show <user>` (state, validity, groups,
  NFC tags, number of access codes) and `lox user groups` read the Miniserver's user management.
  `lox access log <code-touch>` decodes an NFC Code Touch's access log (who opened when, with which tag or code,
  denied attempts). `lox access check <user> [<code-touch>]` says why a user may or may not open a Code Touch:
  it joins the live user with the access rights in a freshly downloaded program file and answers `granted`
  (schedule, direct or through a group), `no-access-right`, `no-credential`, `user-disabled` or
  `unknown-full-access`, with warnings where the file and the live state disagree. `lox access perms <control>`
  lists the users and groups assigned to a control. All with `-o json`; new MCP tools `list_users`, `get_user`,
  `access_log` and `access_check`.
- `lox config users` also shows each user's UUID, number of NFC tags and access codes, tag names, groups, own
  access rights and `last_edit`, and reads backup ZIPs too.
- `lox ir diff` has a users/access section: users and groups added, removed or renamed, group membership, NFC
  tags, access codes and passwords (as names, counts and short fingerprints, never the stored hashes), last
  edits, access rights per Code Touch with their schedule, and UI rights in summary. `--json` prints the whole
  diff as JSON; `--exit-code` counts the new section. `lox config diff` and the System › Config view list the
  same changes under *(users & access)*.

### Changed
- `lox config push` no longer refuses when the live config differs from the base only in users, groups,
  credentials or access rights. The push compiles against the live download, so those stay as they are live;
  the report lists them as "kept as live" (`kept_as_live` in `-o json`).

### Fixed
- `lox ir compile` writes `Text="true"` on the refs it mints for an `ApiActor` or a `VirtualTextIn`, as Loxone Config does; a save in Loxone Config used to add it (lxir D48). With this, a recompile right after a save in Loxone Config 17.2 reproduces the saved config byte for byte

## [0.15.2] — 2026-10-29

### Fixed
- `lox ir compile` keeps the order Loxone Config gave the blocks it manages: their place among the objects of their page, their attributes and their connectors (lxir D47). It used to append them at the end of the page in its own attribute and connector order, so after a save in Loxone Config, `lox config push` found bytes to upload where the logic had not changed
- `lox ir compile` writes its blocks at the block version of the config it compiles against (`V="178"` under Loxone Config 17.2), so a save in Loxone Config no longer restamps them. It used to write a fixed `V="175"`. `lox ir adopt` accepts 17.2 configs, which it refused block by block. The connector table is verified for 17.2 with the save oracle (lxir D46)
- Docs: a Config as Code (lxir) guide and a Terminal UI guide on the docs site; README, COMMANDS.md and the site
  brought in line with the CLI (license, `--serial`, `lox energy`, `config log -c`, `stream -o json`, blinds,
  charger limit in kWh, token auth, per-context scenes)
- `lox thermostat --help` lists the real operating modes; `lox ctx add --help` says what `--serial` is for

## [0.15.1] — 2026-10-29

### Fixed
- `lox ir compile` puts each wire it rebuilds back where the base had it among the connector's inputs (lxir D45). Loxone Config sorts a connector's inputs when it saves, and the compile used to move its own wires to the end, so after a save in Loxone Config, `lox config push` found bytes to upload where the logic had not changed
- `lox config upload` and `lox config restore` upload as `/prog/sps_new.zip`, the file the Miniserver loads on its next start, as Loxone Config and `lox config push` do. They used the local file's name (`upload`) or the backup's original name (`restore`), which the Miniserver very likely never loaded: the upload reported success and nothing changed. Both also refuse a file that is not a config ZIP (a `.Loxone` file, a Git LFS pointer) before connecting

## [0.15.0] — 2026-10-29

### Added
- `lox stream --reconnect` keeps streaming across Miniserver reboots and dropped connections: it waits until the Miniserver answers again (backoff 1–30 s, gives up after 30 min), re-reads the structure, logs in again and resumes the same output. The gap is reported on stderr and, with `-o json`, as `{"event":"disconnected"}` / `{"event":"reconnected"}` lines; states that changed while offline are printed after the reconnect, and `--initial` prints the full snapshot again
- `lox config lint` warns about drive blocks that move no motor (`unwired-motor`): an `AutoJalousie` or `JalousieUpDown2` with neither `OutputUp` nor `OutputDown` wired, a `Door` with no open/close/trigger output wired. Such a block still runs its position model and the app shows it moving, but nothing switches. The finding names the outputs on the same page that nothing drives, usually the relays meant for the motor. A position output into hardware, or an API link to a device that serves this block alone (a Tree/Air smart motor), counts as driving; a panel API-linked to several blocks does not
- `lox autopilot changed`: when the Automatic Designer rules were last changed on the Miniserver (the live `changed` state, unix seconds; `-o json` for scripts). Rules edited in the app can live only in the Miniserver's custom changes, which an upload discards: compare this with the time of the download an upload is based on
- `lox files get PATH --save-as -` writes the file to stdout
- **Config as code: `lox ir`** runs the lxir toolchain — `check`, `fmt`, `compile`, `test`, `rename`, `lint`, `decompile`, `adopt`, `diff`, `drift`, `observe`, `roundtrip`, with lxir's flags and exit codes (`lox lxir …` works too)
- `lox config compress FILE.Loxone [--save-as PATH]` — compresses config XML into `sps0.LoxCC`, the format inside a config ZIP, so an upload no longer needs lox-cli. The header carries the payload size, the XML length and the CRC-32 of the XML, as in Loxone-written downloads; `-o json` reports sizes and checksum
  - `--template DOWNLOAD.zip` writes an upload ZIP instead: the download's entries copied raw, only `sps0.LoxCC` replaced, and verified to extract byte-identically to `FILE` before it is written
- `lox ir repair IN --out OUT` turns numeric character references that Loxone stores as literal text (`&amp;#xA;` and the like) back into the characters they stand for (lxir D44)
- **`lox config push [PROJECT] [--force]`** deploys an lxir project: downloads the live config, refuses when its logic changed since the project's base, compiles the module against the live download (users and passwords stay as they are), shows the logic changes and stages a byte-exact upload ZIP. With `--force` it uploads it as `sps_new.zip`, reboots, waits, downloads again to verify the Miniserver runs exactly the compiled file, and only then updates the lock, `out` and `base`. `--patched-from`, `--allow-removals`, `--accept-version`, `--backup-dir`; `-o json` reports every artifact

### Changed
- `lox autopilot ls` lists the rules themselves, read from the program file (`AutopilotRule` objects) with the `lox config` sources (`--at`, `--file`, `--download`): name, UUID, active, and whether the rule JSON parses. It used to list the structure file's single container entry. `-o json` is now `{"config", "rules": [{name, uuid, active, invalid, apc_valid, apc_error}]}`
- `lox files get` names the absolute path it saved to (`✓  Downloaded /log/def.log → /home/me/def.log (51234 bytes)`), supports `-o json` and `--quiet`, and when stdout is redirected without `--save-as` says on stderr where the file went (`lox files get X > file` used to capture only the message)
- `lox blind --help` lists every action with the command it sends: `up`/`down` are full moves (`FullUp`/`FullDown`, also as `full-up`/`full-down`), not jog steps; `stop` sends `off`, `shade` `AutomaticDown`, `shade N` `manualLamella/N`, `pos N` `manualPosition/N`. The MCP `blind` tool says the same
- Releases are automatic: every push to main that passes CI becomes a release (version bump from PR labels or the changelog, binaries, Docker image, Homebrew formula), and lxir is published to crates.io whenever its version is new
- lxir now lives in this repository (`crates/lxir`, imported with its history) and lox builds it from source, so `lox config diff/show/wiring/lint/history`, the Wiring view and MCP `get_wiring` use the current lxir instead of the crates.io 0.1.0 release
- `crates/lxir` has the lxir changes that landed in the old repository after the import: a title claim also rewrites the refs that copy that title (D43), and blocks are placed in the GUI's own geometry, shared with the GUI (D42, `lox ir compile --relayout <page>`). D42 moves the lockfile to version 4; a version 3 lock is upgraded on the next compile
- License expression `GPL-3.0-or-later OR LicenseRef-Commercial` (was `AND`): the dual license is a choice between the two, as LICENSE says

### Fixed
- `lox stream` without `--initial` no longer prints the initial text-state snapshot as if it were changes (only the value-state snapshot was skipped)
- `lox autopilot state <rule>` shows when the rule last ran, from the Miniserver's rule history; it used to fail with HTTP 404 (states can't be read over `/jdev/sps/io`)
- `lox blind NAME full-up` / `full-down`, as documented in COMMANDS.md, were rejected as unknown actions

## [0.14.0] — 2026-10-04

### Added
- **Config logic insight** (read-only, built on lxir):
  - `lox config show` (logic pages; `-p PAGE` as lxir source), `lox config wiring NAME` (inputs, outputs, non-default parameters with units, devices and rooms; `--trace up|down|both`), `lox config find`, `lox config lint` (dead blocks, unwired inputs, duplicate names, broken refs), `lox config history NAME` (a block across the snapshots)
  - All of them read the config repo, the cached download, `--at SNAPSHOT`, `--file PATH` or `--download`; snapshots by commit, version (`v273`) or save date
- MCP tool `get_wiring` — the config logic around a control (what drives it, what it drives, parameters, traced paths), read-only
- TUI: System › Config snapshot browser (`⏎` on a commit: pages, blocks, lxir source, `/` search, wiring at that snapshot), compare any two commits (`m`), `w` on a diff line opens the block's wiring
- TUI wiring overlay: block type, page, room and parameters in the header, the selected wire's device/room and fan-out (`also drives`), formatted live values, `t` trace to the sensors / actuators, `o` open the page, `/` search, `H` block history, `p` all parameters

### Changed
- `lox config diff` is a logic diff (blocks, parameters, wires per page, hardware refs folded) and takes snapshots of the config repo (`lox config diff`, `diff v271`, `diff A..B`) as well as files
- `lox config pull` commit messages carry the logic diff with totals, grouped by page and capped at 80 lines; layout-only saves say "No logic changes"
- lxir is always built (also with `--no-default-features`)
- TUI keys, one pattern everywhere: `Esc` (or `⌫`) goes back one step — in the wiring the previously followed block (was `b`), in the config browser source → blocks → pages, in the chart the cursor, on a screen filter → pane → the screen a jump (`e`, `⏎` on a room or event, palette) came from (`Esc ← rooms` in the header); `q` closes a popup in one press and quits only from a screen, and a held `q` no longer quits after closing the popups
- TUI chart: `←`/`→` and `h`/`l` both move the cursor, which carries on into the previous/next period at the edges; `PgUp`/`PgDn` move a whole period (was `←`/`→`)
- TUI facet picker: `Esc` closes at once, like the other type-to-narrow popups (`C-u` clears the query)

### Fixed
- Piping output into a command that stops reading early (`lox ls | head -1`, `lox completions bash | head`) no longer panics with "Broken pipe"; `lox` exits quietly (#116)

## [0.13.0] — 2026-10-03

### Added
- **MCP server** (`lox mcp`) — turns `lox` into a Model Context Protocol server over stdio, so Claude Desktop, Claude Code, Cursor, ChatGPT connectors, n8n and other MCP clients can control a Loxone installation
  - `lox mcp serve [--read-only | --allow-risky] [--allow-raw]`, `lox mcp config` (client config snippets), `lox mcp tools`
  - Built on rmcp; speaks MCP 2026-07-28 (stateless `server/discover`, `tools/list` cache hints) and every revision back to 2024-11-05
  - Read tools (`list_rooms`, `list_controls`, `get_control`, `list_sensors`, `list_light_moods`, `list_scenes`, `system_status`), action tools (`switch`, `blind`, `light`, `thermostat`, `run_scene`) and high-risk tools (`door`, `gate`, `alarm`, raw `send_command` with `--allow-raw`)
  - Every action supports `dry_run` and returns the equivalent `lox` command line; every tool has an output schema and structured results
  - High-risk actions need the user's confirmation (multi-round-trip `input_required` on 2026-07-28, `elicitation/create` on older revisions); confirmation state is a single-use, server-side nonce bound to the exact commands
- `confirm:` list in the config — controls on it (UUID, alias or name, optional `[Room]`) always ask for confirmation, in `lox mcp` and `lox tui`

### Changed
- Door openers, locks and gates wired as generic switches or push buttons (door/gate/lock icon, `isSecured`) now need confirmation, too — in `lox mcp` and `lox tui`
- Control resolution: among several substring matches, a single exact (case-insensitive) name match wins
- MCP `get_control` returns a control's named outputs (e.g. LightControllerV2 relays) instead of flattening them into states
- `lox sensors --type temperature` classifies by unit, so CO2 and humidity sensors no longer show up as temperatures
- Dimming a LightControllerV2 to a bare level is refused with a hint to use moods (the Miniserver ignores it)

### Fixed
- Token auth: the HMAC algorithm follows the `hashAlg` the Miniserver reports (SHA-1 on firmware before 10.4), instead of always SHA-256
- WebSocket sessions (`lox stream`, `lox otel serve`, `lox tui`) reuse the stored token via `authwithtoken` instead of minting a new long-lived token on every connect
- `lox stream` and `lox otel serve` send keepalives and detect dead sockets, so the Miniserver's idle timeout no longer drops them; `lox otel serve` reconnects with backoff
- `token.json` is written with mode 0600

## [0.12.0] — 2026-09-27

### Added
- **Terminal UI** (`lox tui`) — full-screen, keyboard-first live view of your installation (built in by default; `--no-default-features` for a minimal build)
  - Home, Rooms, Events, Energy, System and Sites screens with live values
  - Control actions from the keyboard (toggle, dim, set value, moods), with confirmation for doors, alarms and gates
  - Wiring view (`w`) — the config program around a control, with live values on the wires (via [lxir](https://github.com/discostu105/lxir))
  - History view (`c`) — control statistics from 6 h to a year, with period compare
  - Fuzzy command palette (`:`), `y` copies the equivalent `lox` command
  - `--demo` (synthetic house, no Miniserver needed) and `--read-only` (wall display) modes
  - Miniserver log with full-line view and match highlighting; config history with semantic diffs

## [0.11.0] — 2026-03-23

### Added
- **Multi-Miniserver context management** (`lox ctx`) — `kubectl`-style context switching for multiple Miniservers
  - `lox ctx add <name> --host ... --user ... --pass ...` — add a named context
  - `lox ctx use <name>` / `lox ctx <name>` — switch active context
  - `lox ctx list` — list all contexts (`*` = active)
  - `lox ctx current` — show active context
  - `lox ctx remove <name>` / `lox ctx rename <old> <new>` — manage contexts
  - `lox ctx init` — create project-local `.lox/` directory (auto-discovered like `.git`)
  - `lox ctx migrate` — convert existing flat config to a `default` context
- `--ctx <name>` global flag — run any command against a specific context without switching
- Per-context data isolation — each context gets its own cache, token, and scenes directory under `~/.lox/contexts/<name>/`
- Project-local `.lox/` directory support — walks up from cwd, like `.git` resolution
- Backward-compatible config format: existing flat `~/.lox/config.yaml` files continue to work unchanged

## [0.8.0] — 2026-03-21

### Added
- **Windows support** — pre-built binaries for Windows x86_64 and aarch64 are now included in every release
- `lox config init <path>` — initialize a git repository for config version tracking (multi-Miniserver via serial subdirectories)
- `lox config pull [--quiet]` — download config via FTP, decompress LoxCC, generate semantic diff, and git-commit with meaningful change messages
- `lox config log [-n COUNT]` — show config change history from the git repository
- `lox config restore <commit> --force` — restore a previous config version from git history and upload to Miniserver
- `lox health` — device health dashboard showing battery, signal, offline status, and bus errors for Tree/Air devices (`--type tree|air`, `--problems`)
- `lox schema` — command schema introspection for AI agent discovery; lists commands with metadata, args, and valid actions
- `--dry-run` global flag — validates and resolves inputs without executing commands; returns structured JSON envelope with `-o json`
- `--non-interactive` global flag — fails instead of prompting for confirmation (implied by `-o json`)
- `--trace-id` global flag — correlation ID for tracking agent actions in logs
- `-v`/`--verbose` global flag — `-v` shows HTTP requests, `-vv` shows requests + response bodies
- `--all-in-room` flag on `lox on`/`lox off` — apply command to all controls in a room
- Structured JSON error envelopes when using `-o json` (categorized error codes: `control_not_found`, `ambiguous_control`, `unauthorized`, `connection_error`, etc.)

### Changed
- `lox extensions` now queries `/data/status` instead of `LoxApp3.json` — provides richer device information including Tree branch error counts, device parent relationships, and plugin versions
- CI: moved linting to a dedicated Ubuntu job; Windows builds no longer run redundant clippy/fmt checks

### Removed
- `lox daemon` — automation daemon (WebSocket/polling rule engine)
- `lox automation` — automation rule management
- `lox service` — systemd service management
- `timezone:` config field (was only used for automation time windows)

## [0.1.0] — 2024-01-01

### Added
- `lox ls` — list controls with optional `--type`, `--room`, `--values` filters
- `lox get <name>` — show full state of a control
- `lox on/off/pulse <name>` — send on/off/pulse commands
- `lox send <name> <cmd>` — send arbitrary raw command
- `lox blind <name> <action>` — control Jalousie blinds (up/down/stop/shade/pos)
- `lox mood <name> <action>` — control LightControllerV2 moods
- `lox set <name> <value>` — set analog/virtual input value
- `lox if <name> <op> <value>` — conditional state check (exit 0/1)
- `lox watch <name>` — poll state changes
- `lox status [--energy]` — Miniserver health; auto-discovers energy meters
- `lox rooms` — list all rooms
- `lox config set/show` — manage connection config
- `lox token fetch/info/clear` — RSA+AES token auth management
- `lox cache info/clear/refresh` — structure cache management
- `lox scene list/show/new` + `lox run <scene>` — multi-step scenes
- `--room` flag on all commands for disambiguation
- Bracket room qualifier: `"Name [Room]"` syntax
- Alias support in config (`aliases:` map)
- Structure cache (24h TTL) at `~/.lox/cache/structure.json`
- Token auth (acquired via `lox token fetch`) used for all HTTP requests
### Fixed
- `lox config set` no longer clobbers the alias list
- `lox set` percent-encodes values to avoid malformed URLs
- Token auth is now actually used for all requests (was always falling back to Basic Auth)
- `lox blind` and `lox mood` now support `--room` flag
- WebSocket nonce uses cryptographically random bytes
- Debug `eprintln!` removed from token RSA parsing

[Unreleased]: https://github.com/discostu105/lox/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/discostu105/lox/compare/v0.1.0...v0.8.0
[0.1.0]: https://github.com/discostu105/lox/releases/tag/v0.1.0
