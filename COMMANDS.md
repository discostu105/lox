# Command Reference

Every command and flag, by topic. `lox <command> --help` shows the same flags, and `lox schema -o json` describes
them for scripts.

Controls can be referenced by name (fuzzy matched), UUID or alias. When several controls share a name, add the room
with `-r`/`--room` or in brackets:

```bash
lox get "Temperatur" -r "Schlafzimmer"
lox get "Temperatur [OG Schlafzimmer]"
```

Resolution order: alias, exact UUID, bracket room, `--room`, then substring match. Among several matches, a single
exact name wins (`Zentral` over `Jalousie Zentral`).

Global flags:

| Flag | Effect |
|------|--------|
| `-o`, `--output` | Output format: `table` (default), `json`, `csv` |
| `-q`, `--quiet` | Drop non-essential output |
| `--no-color` | No colors (also `NO_COLOR`) |
| `--no-header` | No table headers |
| `-v`, `--verbose` | Log HTTP requests (`-vv` adds response bodies) |
| `--dry-run` | Resolve and validate, send nothing |
| `--non-interactive` | Fail instead of prompting (implied by `-o json`) |
| `--trace-id` | Correlation ID for agent tracing |
| `--ctx <name>` | Use this context instead of the active one |

Scripting: every command exits `0` on success and non-zero on error, and `lox if` exits `1` when the condition is
false. With `-o json`, errors are a JSON envelope (see [Error Envelopes](#error-envelopes)).

```bash
lox ls -t Jalousie -r OG -o json | jq -r '.[].uuid' | while read -r b; do lox blind "$b" up; done
```

---

## Verbose & Dry-Run

```bash
lox -v ls                              # show HTTP requests
lox -vv get "Temperatur"               # show requests + response bodies
lox --dry-run on "Licht Wohnzimmer"    # preview without executing
lox --dry-run on "Licht" -o json       # dry-run returns JSON envelope
lox --non-interactive reboot           # fail instead of prompting (implied by -o json)
lox --trace-id "abc-123" on "Licht"    # correlation ID for agent tracing
```

`--dry-run` validates and resolves inputs without sending commands. With `-o json`, it returns a structured envelope:
```json
{
  "ok": true,
  "dry_run": true,
  "would_execute": {
    "uuid": "1d8af56e-...",
    "command": "on",
    "control": "Licht Wohnzimmer",
    "room": "Wohnzimmer"
  }
}
```

---

## Setup

```bash
lox setup set --host https://192.168.1.100 --user admin --pass secret
lox setup set --serial 504F94112233   # names the config repo folder, OTel device.id; checked by config push
lox setup set --verify-ssl             # enable cert verification
lox setup set --no-verify-ssl          # disable (default, self-signed)
lox setup set --visu-password-file ~/.lox/visu-password  # visualization password for secured controls
lox setup show                         # show config (password redacted)
```

`setup set` changes only the fields you pass. All of them also read env vars (`LOX_HOST`, `LOX_USER`, `LOX_PASS`,
`LOX_SERIAL`). `read -rs LOX_PASS && export LOX_PASS` keeps the password out of the process list and your shell
history:
```bash
LOX_HOST=https://192.168.1.100 LOX_USER=admin LOX_PASS=secret lox status
```

Secured controls (`isSecured`, e.g. an NFC Code Touch) take some commands only signed with the **visualization
password** of the user lox connects with. lox reads it from `LOX_VISU_PASSWORD` or from a file, never from an
argument: put the password on the first line of a file only you can read and point the config at it:
```bash
(umask 077; read -rs pw && printf '%s\n' "$pw" > ~/.lox/visu-password)
lox setup set --visu-password-file ~/.lox/visu-password   # stores the path: visu_password_file: in config.yaml
```
The file is read only when a secured command needs it, and the password is never printed or logged.

### Aliases

```bash
lox alias add wz "1d8af56e-036e-e9ad-ffffed57184a04d2"
lox alias remove wz
lox alias ls
```

Then use directly: `lox on wz`

---

## Discovery & Inspection

```bash
lox ls                                 # all controls
lox ls -t Jalousie                     # filter by type
lox ls -r "Wohnzimmer"                 # filter by room
lox ls -c "Beleuchtung"                # filter by category
lox ls -f                              # only favorites
lox ls --values                        # include live values (one request per control: slower)

lox get "Licht Wohnzimmer"            # full state of one control
lox info "Licht Wohnzimmer"           # detailed: sub-controls, states, moods

lox rooms                             # list all rooms
lox categories                        # list all categories
lox globals                           # global states (operating mode, sunrise/sunset, wind/rain warnings)
lox modes                             # operating modes
lox extensions                        # connected extensions and devices
lox discover                          # find Miniservers on local network (UDP)
lox discover --timeout 5

lox health                            # device health dashboard (battery, signal, offline)
lox health --type tree                # filter by device type (tree or air)
lox health --problems                 # show only devices with issues

lox schema                            # list all commands with metadata (for AI agents)
lox schema blind                      # show schema for a specific command
```

---

## Sensors & Energy

```bash
lox sensors                           # all readings: temperature, door/window, motion, smoke
lox sensors --type door-window        # what's open (also: temperature, motion, smoke)
lox sensors -r "Wohnzimmer"           # one room
lox energy                            # energy meters and the energy flow monitor (current value)
lox energy -r Keller
lox energy flow                       # energy flow now: production, consumption, grid, battery
lox energy flow -o json               # the same, plus every node of the flow monitor
lox energy --top                      # the 10 meters with the highest current power
lox energy --top 5 -r Keller -o json  # top 5 in one room
```

`lox energy flow` reads the Energy Flow Monitor (`/dev/sps/io/{uuid}/all`: its `Gpwr`/`Ppwr`/`Spwr` outputs and the
power of each node) and the battery meter's state of charge. Values are in the Miniserver's unit (the monitor's
format, usually kW). Grid: imported / exported; battery: charging / discharging (JSON `battery`: + charging,
`grid`: + import); self-use is the share of production not exported. Without a flow monitor, meters named like PV,
grid (Netz, Bezug) or a storage meter stand in; the `roles:` overrides in `~/.lox/tui.yaml` apply here as in
`lox tui`.

```
Energiefluss (Zentral)

Production       0.078 kW  ░░░░░░░░░░░░░░░░
Consumption      2.593 kW  ████████████████
Grid             2.526 kW  ████████████████  importing
Battery          0.011 kW  ░░░░░░░░░░░░░░░░  9 % charging
Self-use            100 %
```

---

## Lights

```bash
lox on "Licht Wohnzimmer"
lox off "Licht Wohnzimmer"
lox on --all-in-room "Wohnzimmer"     # turn on all controls in a room
lox off --all-in-room "Schlafzimmer"  # turn off all controls in a room
lox light mood "Licht Wohnzimmer" plus      # next mood
lox light mood "Licht Wohnzimmer" minus     # previous mood
lox light mood "Licht Wohnzimmer" off       # turn off (mood 778)
lox light mood "Licht Wohnzimmer" 704       # set by mood ID
lox light moods "Licht Wohnzimmer"          # list available moods (via WebSocket)
lox light moods "Licht" -o json             # moods as JSON with control metadata
lox light dim "Stehlampe" 75                # set dimmer 0-100% (Dimmer controls; use a mood for a lighting controller)
lox light color "LED Strip" "#FF0000"       # hex RGB
lox light color "LED Strip" "hsv(120,100,100)"  # HSV
lox light color "LED Strip" "temp(80,2700)"     # white: brightness %, color temperature K
```

---

## Blinds

```bash
lox blind "Beschattung Süd" up          # all the way up (FullUp); also: open, full-up
lox blind "Beschattung Süd" down        # all the way down (FullDown); also: close, full-down
lox blind "Beschattung Süd" stop        # stop moving (off)
lox blind "Beschattung Süd" pos 50      # to a position, 0 = open, 100 = closed (manualPosition/50)
lox blind "Beschattung Süd" 50          # same as pos 50
lox blind "Beschattung Süd" shade       # automatic shading (AutomaticDown); also: auto
lox blind "Beschattung Süd" shade 40    # slat angle, 0 = horizontal, 100 = vertical (manualLamella/40)
```

`up` and `down` are full moves: the blind runs to its end position unless it is stopped. They are not jog steps; for those send the raw commands (`lox send "Beschattung Süd" up`, then `lox send "Beschattung Süd" UpOff`).

---

## Gates

```bash
lox gate "Garagentor" open
lox gate "Garagentor" close
lox gate "Garagentor" stop
```

---

## Climate

```bash
lox thermostat "Heizung" temp 22.5                 # set comfort temp
lox thermostat "Heizung" mode auto                  # auto|manual|auto-heat|auto-cool|manual-heat|manual-cool
lox thermostat "Heizung" override 24 120            # override 24°C for 120 min (default 60)
lox thermostat "Heizung"                            # show current state
lox weather                                         # current weather data
lox weather --forecast                              # 7-day forecast
```

---

## Security

```bash
lox alarm "Alarmanlage" arm            # arm alarm
lox alarm "Alarmanlage" arm --no-motion  # arm without motion detection
lox alarm "Alarmanlage" arm-home       # arm in home mode
lox alarm "Alarmanlage" disarm --code 1234
lox alarm "Alarmanlage" quit           # acknowledge/silence
lox door "Haustür" lock                # lock a door lock
lox door "Haustür" unlock              # unlock
lox door "Haustür" open                # open (e.g. electric strike)
lox intercom "Türklingel" answer        # also: hangup, open
lox lock "Heizung" --reason "Wartung"  # lock a control against changes (admin, firmware 11.3.2.11+)
lox unlock "Heizung"
```

---

## EV Charger

```bash
lox charger "Wallbox" start
lox charger "Wallbox" start --limit 20   # stop after 20 kWh
lox charger "Wallbox" pause              # also: stop
```

---

## Statistics & History

```bash
lox stats                              # controls with statistics (legacy or Statistics V2)
lox history "Temperatur" --month 2025-01
lox history "Temperatur" --day 2025-01-15
lox history "Temperatur" -o csv        # CSV output
```

---

## Conditions

```bash
lox if "Temperatur Außen" gt 25        # exit 0=true, 1=false
lox if "Schalter" eq 1 && lox on "Licht"
lox if "Temperatur" lt 19 -r Bad && lox thermostat "Heizung Bad" override 23 30
```

Operators: `eq`, `ne`, `gt`, `ge`, `lt`, `le`

---

## Analog / Virtual Inputs

```bash
lox input set "Sollwert Heizung" 21.5
lox input set "Sollwert Heizung" 21.5 -r "Wohnzimmer"  # disambiguate with room
lox input pulse "Taster"
lox input pulse "Test Frostwarnung"   # a virtual input not shown in the app
```

Virtual inputs that are not shown in the app are missing from the structure file, but the Miniserver takes
commands for them all the same. A name that matches no control is looked up among the `VirtualIn` and
`VirtualTextIn` blocks of the newest config available offline (config repo, else the download cached by
`lox config … --download`).

---

## Music Server

```bash
lox music play                         # play zone 1
lox music pause 2                      # pause zone 2
lox music volume 50                    # volume 0-100 (zone 1)
lox music volume 30 2                  # volume 30 in zone 2
lox music next                         # also: prev, stop, mute, unmute
```

Every music command takes an optional zone number (default 1). It talks to the Music Server's unofficial API on
port 7091 of the host.

---

## Scenes

```bash
lox run abend                          # run a scene
lox run abend --dry-run                # preview without executing
lox scene ls                           # list all scenes
lox scene show abend                   # print YAML definition
lox scene new abend                    # create empty scene file
```

Scenes are YAML files in `~/.lox/scenes/`:
```yaml
name: Abend
steps:
  - control: "Licht Wohnzimmer"
    cmd: on
  - control: "Beschattung Süd"
    cmd: "pos 70"
    delay_ms: 500
```

---

## Autopilot (Automatic Rules)

```bash
lox autopilot ls                       # rules in the program file: active, JSON valid
lox autopilot ls --file backup.Loxone  # same sources as `lox config` (--at, --file, --download)
lox autopilot state "Rule Name"        # when a rule last ran (the Miniserver's rule history)
lox autopilot changed                  # when the rules were last changed on the Miniserver
lox autopilot changed -o json          # {"changed": <unix seconds>, "changed_at": …, "uuid": …}
lox autopilot changed --explain        # …and what changed: newest /prog file vs the one before, /sys/custom
lox autopilot changed --explain --since F  # …against F, e.g. the download after the last push
```

Rules created or edited in the app can live only in the Miniserver's *custom changes*, not in the
program file: `lox autopilot ls` and a config download don't see them, and uploading a config discards
them (the Miniserver logs `Remove old custom changes`). `lox config push --force` refuses while the
Miniserver holds custom changes, and `lox config upload` / `restore` list them. Before another kind of
upload, check that `lox autopilot changed` is older than the download the upload is based on.

The `changed` time also moves when the Miniserver writes its program for another reason, a
visualization password set in the app for example. `--explain` says what it was (read-only, over
FTP). It compares the newest program file in `/prog` with the one before it, or with `--since FILE`,
and sorts the changes into rules, users & access and the rest. It also lists the custom changes in
`/sys/custom`, where rules edited in the app wait. With `-o json`, `rules_changed` is true when either
holds a rule change, which suits a guard script before an upload.

---

## Loxone Config

```bash
lox config download                    # download latest config ZIP via FTP
lox config download --extract          # download + decompress to .Loxone XML
lox config download --save-as config.zip  # custom output filename
lox config ls                          # list all configs on the Miniserver
lox config extract config.zip          # decompress LoxCC → .Loxone XML
lox config extract config.zip --save-as out.Loxone
lox config compress out.Loxone         # compress .Loxone XML → out.LoxCC (sps0.LoxCC format)
lox config compress out.Loxone --save-as sps0.LoxCC
lox config compress out.Loxone --template download.zip  # upload ZIP: download.zip with only sps0.LoxCC replaced → out.zip
lox config upload config.zip --force   # upload as sps_new.zip, loaded on the next reboot (dangerous)
lox config push --force                # deploy a Lox Logic project, verified (see Config as Code)
lox config users file.Loxone           # users in a .Loxone file: NFC tags, codes, groups, access rights
lox config devices file.Loxone         # hardware devices in it (Tree/Air/Network)

# Git-based config versioning
lox config init ~/loxone-config        # initialize a git repo for config tracking
lox config pull                        # download, decompress, diff & git-commit
lox config pull --quiet                # cron-friendly (no output unless error)
lox config log                         # show config change history
lox config log -c 5                    # last 5 entries (default 20)
lox config restore abc123 --force      # upload a config from git history (applied on the next reboot)
```

The `pull` workflow: FTP download → LoxCC decompress → logic diff → git commit with meaningful message.
Multi-Miniserver: each serial gets its own subdirectory in the repo.

`upload` and `restore` write the ZIP to `/prog/sps_new.zip`, the file the Miniserver loads on its next start; `lox reboot` applies it.
Both refuse a file that is not a config ZIP. A `.Loxone` file becomes one with `lox config compress FILE --template DOWNLOAD.zip`.
Both list the Miniserver's custom changes first (see `lox config push`): loading the upload deletes them.
To upload, load (a program restart, no reboot) and verify in one go, deploy with `lox config push` instead.

### Inspecting the logic (read-only)

The program inside a config — blocks, parameters and wires — read with [loxone-config](crates/loxone-config).
Hardware references on the pages are folded away: a wire goes from the motion sensor to the light, not to an input ref.

```bash
lox config diff                        # the last change in the config repo
lox config diff v271                   # from version 271 to the newest config
lox config diff 2026-08-25 HEAD        # snapshots by save date, commit, version or file
lox config diff a1b2c3d..e4f5a6b       # a range of commits
lox config diff old.Loxone new.zip     # two files (.Loxone or backup ZIP)
lox config show                        # the logic pages: blocks and wires per page
lox config show -p Wohnzimmer          # one page as Lox Logic source (--all: every page)
lox config wiring "Licht Wohnzimmer"   # inputs, outputs and non-default parameters of a block
lox config wiring Licht -r Küche --trace up      # the whole path up to the sensors (down, both)
lox config wiring Licht --trace down --depth 6   # hops to follow (default 4)
lox config wiring Licht --all-params   # parameters at their default value, too
lox config find bewegung küche         # blocks by name, type, device, room or page (all words match)
lox config lint                        # dead blocks, unwired inputs, duplicate names, broken refs
lox config history "Licht Wohnzimmer"  # how one block changed across the snapshots
lox config history Licht -n 200        # scan more snapshots (default 50)
```

`show`, `wiring`, `find` and `lint` read the newest config without the network: the config repo's working copy,
else the cached download (`lox tui`'s wiring view keeps one). `--at SNAPSHOT` reads a snapshot of the repo
(commit, `v273`, `2026-08-25`), `--file PATH` a `.Loxone` or backup ZIP, `--download` fetches the current config
via FTP first (read-only). Names match like controls do: substring, `Name [Room]` (room, page or device) or UUID.

The diff is per logic page, with totals first; layout-only saves (moved blocks) show as "No logic changes":

```
b11f0c7 · saved 2026-08-25 21:25:27 → 303191c · saved 2026-08-28 06:39:25

= 1 added · 1 renamed · 1 parameter · wires +2 −0

Hallway
  + block  Stairs pulse (Monoflop)
  ~ rename Night → Night mode
  ~ param  Hallway light · MoveOn  600 s (10 min) → 900 s (15 min)
  + wire   Motion.Q → Stairs pulse.InputTrigger
  + wire   Night mode.Q → Hallway light.DisP
```

All of them take `-o json`.

---

## Config as Code (Lox Logic)

`lox logic` runs the [Lox Logic](crates/loxone-config) toolchain: logic written as `.loxl` text, compiled into the `.Loxone`
config with a lockfile that pins every UUID. The arguments go to `loxl` as they are (`lox logic …` works too),
so the Lox Logic docs apply word for word: `loxl compile` is `lox logic compile`. Nothing here talks to the Miniserver.

```bash
lox logic help                            # one line per command; `lox logic <cmd> --help` for its flags
lox logic check pages/                    # parse + validate a module (line-numbered errors; --json)
lox logic fmt --write pages/              # canonical formatting (--check for CI)
lox logic compile                         # in a lox.toml project: base + module + lock → out.Loxone
lox logic compile --base current.Loxone --module pages/ --lock haus.lock.json \
               --out out.Loxone --serial 504F94112233
lox logic diff --exit-code current.Loxone out.Loxone  # semantic diff, exit 1 if different
lox logic diff --json old.Loxone new.Loxone  # the same as JSON, users/access section included
lox logic drift current.Loxone            # did someone change the config since the last compile?
lox logic coverage                        # in a lox.toml project: blocks by type (managed, extern, untouched), attributes carried from the base
lox logic adopt current.Loxone --out-dir pages/ --out-lock haus.lock.json  # take over existing blocks
lox logic adopt current.Loxone --uuid <uuid> --as <slug> --module pages/ --lock haus.lock.json
lox logic decompile current.Loxone        # the Lox Logic view of every page (read-only)
lox logic rename old_name new_name        # rename across sources and lockfile, identities kept
lox logic lint                            # findings by kind: unused names, dead outputs, tags that open nothing, bare durations; --summary: counts
lox logic lint --fix                      # write unit suffixes (`Delay: 3h`); the compiled config stays byte-identical
lox logic test                            # `test … end` blocks in the Lox Logic simulator
lox logic test --filter nacht --trace     # … cycle by cycle: what a test sets, every value change, each expect
lox logic types --search delay            # block types by name or purpose; `lox logic types OnDelay`: its ports
lox logic templates                       # tested templates (motion light, hysteresis, alarms, …) to copy
lox logic test --lox ~/lox-cli/lox        # … or in lox-cli's simulator (`lox sim`)
lox logic lsp                             # language server for editors (stdio): diagnostics, hover, completion, rename
```

Exit codes: 0 ok, 1 a finding (check errors, a diff, drift, a failed test), 2 a usage error.

Access rights on an NFC Code Touch can live in the module too: an `access` set lists the users and groups
that may open it, by title, each with its access schedule, and takes the right away from everyone else. Users,
groups, schedules, tags and codes stay with Loxone Config and the app. `decompile` and `adopt` lift the rights the
config has. Spec: the `access` section of the [language spec](crates/loxone-config/docs/language.md).

```text
extern haustuer = NfcCodeTouch(uuid: "…")

access bewohner
	user "alice": "Always"
	group "Residents": "Workdays"
end

haustuer.access = bewohner               # several Code Touches may share the set
```

Push notifications work the same way. The notification is created in Loxone Config and named as an extern. An
`OutputRefLM` block sends its message when its input turns on, and a `recipients` set, claimed with
`frost.recipients = …`, lists who gets it.
Spec: the push notifications section of the [language spec](crates/loxone-config/docs/language.md).

```text
extern frost = Notification(title: "Frostwarnung Garten")
warn = OutputRefLM(mirrors: frost, AI: zu_kalt.Q, On: "Frost im Garten", MinimumTime: 12h)

recipients familie
	user "alice"
end

frost.recipients = familie
```

A directory with a `lox.toml` is a project: inside it `compile` needs no flags and `check`/`fmt`/`drift`/`lint`
use its module and lock. Paths are relative to the file:

```toml
base   = "base.Loxone"       # the deployed config (required) — lox config download --extract --save-as base.zip
module = "pages"             # a .loxl file or a directory of them (required)
lock   = "logic.lock.json"    # default
out    = "out.Loxone"        # default
serial = "504F94112233"      # optional; config push refuses a Miniserver with another serial
```

Review a compile by name with `lox config diff base.Loxone out.Loxone` (`lox logic diff` lists UUIDs).
Walkthrough: [Config as Code guide](docs/guides/config-as-code.md). The language and the lockfile:
[crates/loxone-config](crates/loxone-config/README.md).

### Deploying a project: `lox config push`

```bash
lox config push                        # in a lox.toml project: download, drift check, compile, stage
lox config push --force                # … then upload, restart the program, verify and move the base forward
lox config push --force --reboot       # load the upload by rebooting the Miniserver instead (about a minute)
lox config push ~/r50 --force -o json  # another project dir; JSON report on stdout
lox config push --patched-from backups/dl.Loxone --force  # base is a byte patch of that download
lox config push --allow-removals --force   # delete managed blocks gone from the module
lox config push --accept-version 17010727  # base saved by a new Loxone Config release
lox config push --backup-dir /tmp/push # downloads and artifacts elsewhere (default: PROJECT/backups)
lox config push --discard-custom-changes --force  # push although the Miniserver holds custom changes
lox config push --allow-app-changes --force  # push although the rules changed since the last push
lox config sync                        # read-only: project, lock and Miniserver agree (exit 1 if not)
```

Each step stops the push when it fails:

1. Download the running config to `backups/<stamp>-pre-push.zip` and `.Loxone`, and list its custom changes.
   Check that the Automatic Designer's rules didn't change since the last verified push.
2. Drift check: the live logic must still match the project's `base`.
3. Compile the module against the live download, with a scratch copy of the lock.
4. Print the logic changes (live → compiled) and compare the live users with the program file.
5. Stage the upload as `<stamp>-push.zip` and check that it extracts to exactly the compiled file. Without
   `--force`, or with `--dry-run`, the push ends here.
6. Upload it as `/prog/sps_new.zip`, restart the program and wait until it runs again (at most a minute).
7. Download again and check that the Miniserver runs exactly the compiled file (`<stamp>-post-push.*`).
8. Write the lock and `out`, point `base` in lox.toml at the verified download, and record the rules' change time
   in `autopilot.changed`. Commit those files.

If live and compiled are identical, there is nothing to push. If step 7 fails, the project is left as it was.

The compile in step 3 keeps users, passwords, access rights, layout and visualization as they are live. The drift
check ignores users, groups, credentials and access rights: they are listed as kept as live. If the logic changed
since the last push, the push refuses and names the `lox logic diff` to run.

The program restart works like a save from Loxone Config. The Miniserver loads the file within seconds and keeps
remanent values. `--reboot` reboots instead and waits up to 5 minutes for the Miniserver to answer.

The upload is the downloaded ZIP with only `sps0.LoxCC` replaced, so nothing re-serializes the XML on the way.
A new backup directory gets a `.gitignore` (`*`), because real configs carry personal data.

#### Custom changes

Custom changes are what the app, the web UI or the API changed and Loxone Config has not saved yet. Examples are
light moods, timer schedules, automatic rules, users, groups, NFC tags and access codes. The download doesn't
contain them, and loading the upload deletes them.

- With custom changes present, `--force` refuses and names them. A staging run only warns.
- If `/sys/custom` cannot be listed, or users cannot be read, `--force` refuses too.
- Reading users needs an admin connection (`lox setup set --user <admin>`). The push compares each user's name,
  last edit, groups, NFC tag names and count, and number of access codes.
- A change to an object that the upload deletes goes with the object and doesn't stop the push.
- The notification settings of a phone the program already knows belong to the app. The push carries them into
  the upload. A newly registered phone still refuses.

To keep the changes, open the project from the Miniserver in Loxone Config, save it, then bring the project up to
date. To delete them on purpose, pass `--discard-custom-changes`.

#### Rules changed in the app

Rules edited in the app can live only on the Miniserver, and an upload deletes them. After a verified push, the
push writes the Automatic Designer's change time (`lox autopilot changed`) to `autopilot.changed` next to lox.toml.
The next `--force` refuses when the time moved, and names the `lox autopilot changed --explain` to run. A staging
run only warns.

The time also moves on any other program write, a password set in the app for example. When `--explain` shows no
rule change, pass `--allow-app-changes`. Without `autopilot.changed` the push says so and goes ahead. On a
Miniserver without Automatic Designer the check is skipped.

#### Checking the project: `lox config sync`

`lox config sync` checks, read-only, that the project and the Miniserver agree. It passes right after a verified
push and exits 1 when a check fails:

| Check | Passes when |
|-------|-------------|
| `live` | The live config is the project's `base` (else it saves the download as `backups/<stamp>-sync.Loxone`) |
| `source` | The module compiles against `base` without changes |
| `lock` | The compile leaves the lockfile as it is |
| `drift` | The live logic matches the lock |
| `app-changes` | The rules' change time is the one in `autopilot.changed` |

`-o json` prints `{in_sync, base, live, saved, checks: [{name, ok, detail}]}`.

#### The app catalogue and permissions

The upload ZIP also holds `LoxAPP3.json`, the catalogue the app shows. The Miniserver never rebuilds it, so the push
updates it for the controls it changes:

- New names, rooms, categories and favourites, deleted and hidden controls.
- New entries for push buttons, digital virtual inputs, memory flags, pulses at a time, digital day timers and
  sensor info displays that have a room and a category.
- `lastModified` is set to the push time, so the app reloads.

Other block types, a room the catalogue doesn't list and icons wait for a save from Loxone Config. Step 7 checks
that the Miniserver serves the uploaded catalogue.

`permissions.bin` lists who may use each object. The push regenerates it when loxone-config reproduces the live
file byte for byte. Otherwise the live file stays, and the push warns if the compiled config assigns users and
groups differently.

#### JSON report

`-o json` reports these keys:

| Key | Meaning |
|-----|---------|
| `custom_changes_pending` | Custom changes the upload would delete (error with `--force`) |
| `custom_changes_kept` | Phone notification settings carried into the upload |
| `removed: true` | On a custom change: its object is deleted by the upload |
| `user_changes_pending` | Live user changes the upload would delete (error with `--force`) |
| `kept_as_live` | Users, groups, credentials and access rights that differ from `base` and stay as they are live |
| `app_catalogue_updated` | What the push changed in the app catalogue |
| `app_catalogue_stale` | What it couldn't carry over until a save from Loxone Config |
| `app_catalogue_rebuilt: true` | The Miniserver serves a catalogue other than the uploaded one |
| `permissions` | `unchanged`, `regenerated` or `kept` |
| `permissions_stale: true` | The kept `permissions.bin` no longer matches the compiled users and groups |
| `app_changes` | `{changed, recorded, moved}`: the rules' change time, the recorded one, and whether it moved |
| `fixpoint` | The source compiles against the verified download without changes, so the saved lock matches it |

To go back to the config from before a push, upload its download again:
`lox config upload backups/<stamp>-pre-push.zip --force && lox reboot`.

---

## System

```bash
lox status                             # firmware, PLC state, memory
lox status --diag                      # CPU, tasks, interrupts, SD card health
lox status --net                       # network config (IP, MAC, DNS1/2, DHCP, NTP)
lox status --bus                       # CAN bus statistics (incl. parity errors)
lox status --lan                       # LAN statistics (incl. exhaustion, no-buffer)
lox status --all                       # all diagnostic sections
lox time                               # Miniserver system date/time
lox log                                # system log (admin only)
lox log -n 100                         # custom line count
```

---

## Users & Door Access

The Miniserver's user management. The user lox connects with needs the right to manage users
(an admin); otherwise the commands fail with `forbidden` and say so.

```bash
lox user ls                            # users: state (enabled, disabled, enabled until …), admin, UUID
lox user show alice                    # one user: validity, last edit (local time), groups, NFC tags (name + ID), number of access codes
lox user groups                        # user groups and their type (normal, full access, …)
lox access log "Code Touch"            # NFC Code Touch access log: who opened when, denied attempts (last 50)
lox access log "Code Touch" -n 0       # the whole log
lox access check alice                 # why alice may (not) open each NFC Code Touch
lox access check alice "Code Touch"    # … one Code Touch
lox access check alice --cached        # use the cached config instead of downloading it
lox access check alice --file sps.zip  # … or a config file (--at <snapshot>: the config repo)
lox access perms "Code Touch"          # users and groups the Miniserver lists as assigned to a control
```

Users and Code Touches are found by exact name (any case), a unique part of the name, or UUID.
`access log` shows time (local), result (`granted`, `denied`, `first-factor` for the first step of
two-factor access), the output opened, how (NFC tag, code, app, external) and the user and tag or code name.

`access check` joins the live user (state and validity, groups, NFC tags, access codes) with the access rights
in the program file, which it downloads fresh from the Miniserver (read-only FTP) unless `--cached`, `--file` or
`--at` says otherwise. An access right is a user's or group's right on the Code Touch with a schedule; group
rights count through the live group membership. Verdicts:

| Verdict | Meaning |
|---|---|
| `granted` | an access right, direct or through a group, with its schedule (a right on the built-in "never" schedule does not count) |
| `no-access-right` | neither the user nor one of their groups has an access right on this Code Touch |
| `no-credential` | the user has no NFC tag and no access code |
| `user-disabled` | disabled, expired (`enabled until`) or not yet valid (`enabled from`) |
| `unknown-full-access` | the only path is the full-access group, which is not verified to open Code Touches: give the user an access right to be sure |

Warnings say where the program file and the live state disagree: the user changed after the file was saved
(`lastEdit`), the user or a group is missing from it, or the Miniserver's assignment list for the Code Touch
(`access perms`) contradicts the file. With NFC tags, a granted check reminds that a phone must be unlocked
when held to the reader. `-o json` returns the verdict, reason, rights, warnings and hints per Code Touch.

`access perms` lists direct assignments only, UI and access rights alike (the Miniserver does not say which),
and not the members of the listed groups.

### NFC tags and access codes

```bash
lox access learn "Code Touch"                              # read a tag's ID: hold the tag or the unlocked phone to the reader
lox access learn "Code Touch" --user alice --name "alice phone"  # … and pair it with alice
lox user nfc add alice "04 A1 B2 C3 9F E5 F6" "alice card" # pair a tag by ID (spaces, case and : or - don't matter)
lox user nfc rm alice "alice card"                         # remove a tag, by name or ID
lox user code set alice                                    # new access code: hidden prompt, asked twice
echo 4711 | lox user code set alice --yes                  # … or the first line of stdin (never an argument)
lox user nfc add alice 04A1B2C3 "spare" --dry-run          # show the request, send nothing
```

`access learn` starts the Code Touch's learn mode (it shows a cyan running light), waits for a tag (`--timeout`,
default 60s, at most 5m), prints its ID and whether a user or Code Touch has it already, and always ends learn mode
again, on timeout and Ctrl-C too. Read errors (`…E8`), authentication errors (`…EE`, e.g. a locked phone) and a reader
that does not start (`…EF`) are named; a Code Touch that cannot learn (offline, not set up, an Air model on battery)
answers `423`. With `--user` and `--name` the tag is then paired after a confirmation; a tag another user has is refused.

A Code Touch marked `isSecured` takes learn mode only as a secured command (`jdev/sps/ios/<hash>/<uuid>/nfc/startlearn`,
the same for `stoplearn`), each signed with a fresh key from `getvisusalt` and the visualization password of the user
lox connects with. The password comes from `--visu-password-file <path>`, `LOX_VISU_PASSWORD`, `visu_password_file:`
in the config (see [Setup](#setup)), or, in a terminal, a hidden prompt. A refusal with code 500 then means a wrong
visualization password, or a user without one. A tag a user has already opens the door in learn mode instead of
being learned; the Miniserver then reports a result without an ID, and `access learn` says so instead of waiting
for the timeout.

Every change asks for confirmation; `--yes` skips the question, and without a terminal (or with `-o json` or
`--non-interactive`) a change needs `--yes`. `--dry-run` prints the requests instead, with the access code shown as
`****`. The code is 2 to 8 digits and never appears in output, errors or the `-v`/`-vv` log; a code another user has
already fails with `code_in_use`. Each change is sent once, without retries.

User-management changes may live only in the Miniserver's custom changes until Loxone Config saves the program;
uploading an older program (Loxone Config, `lox config push`) can discard them. lox reminds you after each change.

---

## Device Health

```bash
lox health                            # device health dashboard
lox health --type tree                # filter: tree or air devices only
lox health --problems                 # show only devices with warnings/offline
lox health -o json                    # JSON output
```

Shows battery levels, signal strength, online/offline status, and bus errors for Tree and Air devices.

### Hardware against the program: `lox hw status`

```bash
lox hw status                         # every device on the bus against the program file
lox hw status --problems              # only devices with a finding
lox hw status --file backup.Loxone    # same sources as `lox config` (--at, --file, --download)
lox hw status -o json                 # {config, devices: [...], counts}
```

Compares the Miniserver's `/data/status` (serial, type, online, firmware) with the hardware in the program file.
Devices match by serial: Tree, Air, extensions, network devices (MAC), 1-Wire devices (ROM ID) and the Miniserver.
Each device gets one status:

| Status | Meaning |
|--------|---------|
| `serial-changed` | A device of that name and kind has another serial live: replaced, not yet updated in the program |
| `missing` | In the program, not on the bus |
| `not-in-program` | On the bus, not in the program |
| `offline` | In the program and on the bus, but offline |
| `ok` | In the program and online |

---

## Command Schema (AI Agents)

```bash
lox schema                            # list all commands with metadata
lox schema blind                      # schema for a specific command
lox schema -o json                    # JSON output for programmatic use
```

Returns command structure, arguments, subcommands, control type hints, and valid actions. Designed for AI agent discovery — agents can introspect what commands exist and what parameters they accept.

---

## MCP Server (AI Assistants)

```bash
lox mcp serve                         # Model Context Protocol server on stdio (MCP 2026-07-28 + older)
lox mcp serve --read-only             # read tools only, no actions
lox mcp serve --allow-risky           # doors, gates, alarm, user changes without asking the user first
lox mcp serve --allow-raw             # also expose send_command (raw Loxone commands)
lox mcp config                        # print config for Claude Desktop / Cursor and Claude Code
lox mcp config -o json                # just the JSON "mcpServers" snippet
lox mcp tools                         # list the tools a client would see (-o json: full definitions)
lox --ctx home mcp serve              # pin the server to one Miniserver context
lox --dry-run mcp serve               # every action is a dry run
```

MCP clients launch `lox mcp serve` themselves; you normally only run `lox mcp config` and paste the result.
Tools: `list_rooms`, `list_controls`, `get_control`, `list_sensors`, `get_energy_flow` (the energy flow, `top` ranks
the meters), `list_light_moods`, `list_scenes`,
`system_status`, `get_wiring` (the config logic around a control: what drives it, what it drives, parameters, `trace` up/down;
`live=true` adds the current value on each wire whose source the app streams, e.g. `22000lx`),
`list_users`, `get_user`, `access_log`, `access_check` (see [Users & Door Access](#users--door-access); `access_check`
downloads the config unless `cached=true`), `switch`, `blind`, `light`, `thermostat`, `gate`, `alarm`, `door`, `run_scene`,
`learn_nfc_tag`, `assign_nfc_tag`, `remove_nfc_tag`, `set_access_code` (and `send_command` with `--allow-raw`). Every tool
has input and output schemas; every action tool accepts `dry_run`.

Read-only tools for hardware and config as code: `hw_status` (see [`lox hw status`](#hardware-against-the-program-lox-hw-status)),
`logic_check`, `logic_types`, `logic_coverage`, `logic_decompile`, `logic_diff`, `logic_drift`, `logic_lint`,
`logic_compile`, `logic_test`, `config_sync` and `config_push_dry_run`. They take a `project` directory (default: the
server's working directory) and run the matching `lox logic` or `lox config` command. Each result carries that command
line, its exit code and its output. `logic_compile` writes into a scratch directory and leaves the project and its
lock as they are. `config_push_dry_run` stages a push and never uploads.
High-risk actions (the ones `lox tui` asks to confirm, any action on a door lock, gate or alarm, and every change to users
and NFC learning) are confirmed by the user
through the MCP client; clients that cannot ask are refused with `action_not_allowed` unless the server runs with `--allow-risky`.
Door openers wired as push-buttons are recognized by their door/gate/lock/key icons. To have any other control confirmed
(e.g. a pool cover), list it under `confirm:` in the config (UUID, alias, or name with an optional `[Room]`; shown by `lox setup show`).
`lox tui` follows the same rules and asks before sending:

```yaml
confirm:
  - Pool Abdeckung [Pool]
```

Tool errors use the same envelope as `-o json`, plus `invalid_arguments`, `action_not_allowed`, `declined_by_user`,
`confirmation_expired` and `confirmation_mismatch`.

---

## Error Envelopes

When using `-o json`, errors return structured envelopes instead of plain text:

```json
{
  "ok": false,
  "error": "control_not_found",
  "message": "No control matching 'Nonexistent'"
}
```

Error codes: `control_not_found`, `ambiguous_control`, `user_not_found`, `ambiguous_user`, `tag_not_found`, `code_in_use`, `learn_timeout`, `config_not_found`, `user_changes_pending`, `confirmation_required`, `declined_by_user`, `unauthorized`, `forbidden`, `not_found`, `http_error`, `connection_error`, `error`.

---

## Filesystem

```bash
lox files ls /                         # browse Miniserver filesystem
lox files get /log/def.log             # download into the current directory (./def.log)
lox files get /log/def.log --save-as local.log
lox files get /log/def.log --save-as - | grep -i error   # write the file to stdout
```

Without `--save-as` the file is saved under its own name in the current directory (overwriting a file of that name); the confirmation shows the absolute path (`✓  Downloaded /log/def.log → /home/me/def.log (51234 bytes)`, `-o json`: `{"ok": true, "path": …, "saved_as": …, "bytes": …}`). `--save-as -` writes the raw bytes to stdout and prints nothing else there.

---

## Cache

```bash
lox cache info                         # show cache age and path
lox cache check                        # check if cache is current
lox cache refresh                      # force re-fetch
lox cache clear                        # delete local cache
```

---

## Token Auth

More secure than Basic Auth. Token is valid ~20 days.

```bash
lox token fetch                        # fetch & save token
lox token info                         # show token status
lox token check                        # verify token on Miniserver
lox token refresh                      # extend validity
lox token revoke                       # revoke on Miniserver
lox token clear                        # delete local token file
```

---

## Raw Commands

```bash
lox send <uuid> <command>              # send raw command to a control
lox send <uuid> <command> --secured <hash>  # secured command
lox watch "Temperatur"                 # poll state and print changes (Ctrl+C to stop)
lox watch "Temperatur" -i 5            # custom poll interval in seconds
```

---

## Terminal UI

A full-screen, live view of the house that you can act on. The guide is
[docs/guides/tui.md](docs/guides/tui.md): screens, safety and preferences in `~/.lox/tui.yaml`.

```bash
lox tui                                # start on the last screen (Home the first time)
lox tui --screen energy                # start on a screen: home|rooms|events|energy|system|sites
lox tui -r Kitchen                     # start in Rooms with a room selected
lox tui --read-only                    # no actions at all (wall display, screen sharing)
lox tui --theme mono                   # theme: night (default) | day | mono | neon
lox tui --icons nerd                   # Nerd Font icons (plain Unicode by default)
lox tui --no-mouse --no-motion         # no mouse capture, no animations
lox tui --demo                         # a synthetic demo house, no Miniserver needed
lox --ctx office tui                   # another context; switch live with C or on the Sites screen
```

### Keys

`?` shows the keys for where you are, and every pane's bottom border shows the ones in reach. `:` (or `Ctrl-K`)
opens the palette: fuzzy go-to for rooms, controls and scenes, plus any `lox` command.

- **`Esc` (or `⌫`) goes back one step.** In a popup it returns to where you were, then closes the popup. On a
  screen it clears the filter, facets or marks, then returns to the screen a jump came from. `Esc` never quits.
- **`q` closes** the popup in one press. On a screen with no popup open, `q` quits.

In text fields (`/`, `:`, `=`, facets, search), letters are text. `Esc` cancels and `⏎` confirms.

<!-- keys:start (generated from src/tui/keymap.rs) -->
| Global | Action |
|---|---|
| `1` | Home |
| `2` | Rooms |
| `3` | Events |
| `4` | Energy |
| `5` | System |
| `6` | Sites |
| `⇥` | next pane |
| `⇤` | previous pane |
| `h` `←` | focus pane left |
| `l` `→` | focus pane right |
| `j` `↓` | move down |
| `k` `↑` | move up |
| `g` `Home` | first |
| `G` `End` | last |
| `^d` `PageDown` | half page down |
| `^u` `PageUp` | half page up |
| `[` | previous sub-view |
| `]` | next sub-view |
| `/` | filter |
| `:` `^k` | palette: go to / run |
| `?` | help |
| `C` | switch context |
| `!` | message log |
| `p` | pause live updates |
| `^r` | refresh structure + reconnect |
| `Esc` `⌫` | back: one step (popup → filter → pane → the screen you came from); never quits |
| `q` `^c` | close the popup; on a screen: quit |

| Selected item | Action |
|---|---|
| `␣` | primary action |
| `+` | step up |
| `-` | step down |
| `<` | minimum |
| `>` | maximum |
| `=` | set exact value |
| `s` | stop |
| `m` | mode / mood |
| `a` | all actions |
| `⏎` | inspect / open |
| `w` | wiring |
| `*` | pin to Home |
| `e` | events of this item |
| `c` | history chart (timeframes) |
| `y` | copy lox command |
| `Y` | copy UUID |
| `v` | mark |
| `V` | mark all visible |

| Rooms | Action |
|---|---|
| `b` | group by room · category · type |
| `f` | facets: room, type, state… |
| `o` | sort rooms: name · activity · temperature |

| Events | Action |
|---|---|
| `f` | facets: room, control, type… |
| `x` | mute this control |
| `X` | show muted / noisy |
| `F` | follow newest |

| System › Log | Action |
|---|---|
| `⏎` | full line |
| `n` | next match |
| `N` | previous match |

| System › Config | Action |
|---|---|
| `⏎` | browse the snapshot (diff pane: the full line) |
| `m` | mark: compare another commit with this one |
| `w` | wiring of the diff line's block at that snapshot |
| `n` | next change |
| `N` | previous change |
| `P` | pull: download the newest backup, commit it to git (read-only) |

| System › Update | Action |
|---|---|
| `R` | reboot (typed confirmation) |
| `U` | install update (typed confirmation) |

<!-- keys:end -->

---

## Streaming

```bash
lox stream                             # stream all state changes via WebSocket
lox stream --room "Kitchen"            # filter by room
lox stream --type LightControllerV2    # filter by control type
lox stream --control "Kitchen Light"   # filter by control name
lox stream --initial                   # include initial state snapshot
lox stream -o json                     # NDJSON: one JSON object per line (pipe into jq)
lox stream --reconnect                 # keep streaming across Miniserver reboots
```

Without `--reconnect` the stream ends when the Miniserver goes offline (reboot after a config upload, firmware update) or drops the connection. With `--reconnect` it waits until the Miniserver answers again (retries back off from 1 s to 30 s, gives up after 30 min), re-reads the structure (the reboot may have loaded a new program), logs in again and resumes the same output:

- The gap is reported on stderr (`✗ disconnected: …`, `✓ reconnected after 2m 14s offline`; not with `-q`). With `-o json` it is also part of the output, as event lines between the state lines:
  ```json
  {"timestamp":"…","event":"disconnected","reason":"out_of_service","message":"Miniserver going offline (reboot or firmware update)"}
  {"timestamp":"…","event":"reconnected","offline_secs":134}
  ```
  `reason` is `out_of_service` (announced by the Miniserver), `closed` or `error`. CSV output has no marker rows.
- States that changed while the stream was down are printed right after the reconnect (compared with the last value seen), as are states that are new in the reloaded program.
- With `--initial` the full snapshot is printed again after every reconnect, so a consumer that rebuilds its state from it starts over from the rebooted Miniserver.
- The first connection must succeed; a wrong host or credentials still fail at once.

---

## OpenTelemetry Export

Pushes metrics, logs, and traces to any OTLP-compatible backend (Dynatrace, Datadog, Grafana Cloud, etc.).

**Metrics**: control state gauges, system diagnostics (CPU, heap, tasks), network counters (CAN/LAN), weather data.
**Logs**: state change events, text-state messages, Miniserver system log (`def.log`).
**Traces**: synthetic automation traces — correlates autopilot rule fires with temporally-close state changes.

```bash
# Continuous daemon — push metrics + logs + traces via OTLP every 30s
lox otel serve --endpoint http://localhost:4318 --interval 30s

# With auth header (Dynatrace, Datadog, etc.)
lox otel serve --endpoint https://otlp.example.com:4318 \
  --header "Authorization=Bearer xxx" --interval 1m

# Filter by room or control type
lox otel serve --endpoint ... --room "Kitchen" --type LightControllerV2

# Metrics only (disable logs and traces)
lox otel serve --endpoint ... --no-logs --no-traces

# Metrics + logs only (disable traces)
lox otel serve --endpoint ... --no-traces

# One-shot push (for cron jobs)
lox otel push --endpoint http://localhost:4318

# One-shot push, metrics only
lox otel push --endpoint http://localhost:4318 --no-logs
```

`--endpoint` defaults to `http://localhost:4318`. Dynatrace and some other backends need `--delta` (delta
temporality for counters). See the [OpenTelemetry guide](docs/guides/opentelemetry.md) for the metric list.

---

## Context Management

Manage multiple Miniserver connections, similar to `kubectl config use-context`:

```bash
lox ctx add home --host https://192.168.1.100 --user admin --pass secret
lox ctx add office --host https://10.0.0.50 --user admin --pass secret --serial 504F94AABBCC
lox ctx use home                    # switch active context
lox ctx home                        # shortcut for `lox ctx use home`
lox ctx list                        # list contexts (* = active)
lox ctx current                     # show active context
lox ctx remove office               # remove a context
lox ctx rename home house           # rename a context
```

### One-off context override

```bash
lox --ctx office status             # run against 'office' without switching
lox --ctx home ls -o json           # query 'home' context
```

### Project-local config

```bash
lox ctx init                        # create .lox/ in current directory
lox ctx init --host ... --user ... --pass ...  # with connection details
```

Project-local `.lox/config.yaml` is auto-discovered by walking up from cwd (like `.git`). Secrets are excluded via `.lox/.gitignore`.

### Migration

```bash
lox ctx migrate                     # convert flat config to 'default' context
```

Existing flat `~/.lox/config.yaml` files continue to work unchanged. Multi-context format:

```yaml
active_context: home
contexts:
  home:
    host: https://192.168.1.100
    user: admin
    pass: secret
  office:
    host: https://10.0.0.50
    user: admin
    pass: secret
```

### Config resolution order

1. `LOX_CONFIG` env var (absolute priority)
2. Project-local `.lox/config.yaml` (walk up from cwd)
3. Global `~/.lox/config.yaml` (flat or multi-context)
4. `--ctx` flag overrides context selection within global config

---

## Firmware Update

```bash
lox update check                       # check for updates
lox update install --yes               # install update
lox reboot --yes                       # reboot Miniserver (refuses without --yes)
```

---

## Shell Completions

```bash
lox completions --install              # detect your shell and install (Homebrew does this for you)
lox completions bash                   # print: bash | zsh | fish | powershell | elvish

# PowerShell: add to $PROFILE
lox completions powershell | Out-String | Invoke-Expression

# Install (bash):
lox completions bash > /etc/bash_completion.d/lox

# Install (zsh):
lox completions zsh > ~/.zfunc/_lox

# Install (fish):
lox completions fish > ~/.config/fish/completions/lox.fish
```
