# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Entries for the next release wait in [`changelog.d/`](changelog.d/README.md), one file per change, and move here
when it is released.

## [Unreleased]

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
