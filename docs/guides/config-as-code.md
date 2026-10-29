---
title: Config as Code
layout: default
parent: Guides
nav_order: 5
---

# Config as Code (lxir)
{: .no_toc }

Write Miniserver logic as text, review it in a pull request, deploy it verified.
{: .fs-5 .fw-300 }

## Table of contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## How it works

A Miniserver's logic lives in one `.Loxone` XML file that only Loxone Config can edit. `lox ir` runs the
[lxir](https://github.com/discostu105/lox/tree/main/crates/lxir) toolchain, which treats it like Terraform treats
infrastructure:

| | |
|:--|:--|
| **Module** (`*.lxir`) | Your source: the blocks, wires and parameters you manage |
| **Base** (`.Loxone`) | The config the Miniserver runs now (a download) |
| **Lockfile** (`*.lock.json`) | Pins every UUID the compiler minted, so the next compile changes only what you changed |
| **Out** (`.Loxone`) | Base + module, written exactly as Loxone Config would have drawn it |

You manage only the logic you choose. Hardware, rooms, visualization, users and everyone else's logic pass through
byte for byte. Nothing in `lox ir` talks to the Miniserver; `lox config push` does the transport.

{: .note }
`lox ir …` and the standalone `lxir …` binary are the same CLI, so lxir's docs apply word for word.
`lox ir help` lists every command and flag.

## The language in one example

```text
page "Beschattung"

let temp_schwelle = 28

extern aussentemp = VirtualIn(iname: "VI1")          # owned by Loxone Config
extern sonne      = VirtualIn(iname: "VI3")
extern jal_sued   = AutoJalousie(title: "Beschattung Süd")

temp_hoch = GreaterEqual("Temp über 28", Input1: aussentemp.Q, Input2: temp_schwelle)

jal_sued.AutoShade <- temp_hoch.Q and sonne.Q        # a wire; the `and` becomes an And block
jal_sued.TargetPos = 70                              # a parameter
```

- `extern` names an object Loxone Config owns (matched by `uuid`, `iname` or `title`); you can wire to it, not
  create it. A title that repeats can be narrowed by `room:`, `category:` or `device:` (the hardware serial of the
  Extension or Tree device it hangs off).
- A plain declaration (`temp_hoch = GreaterEqual(…)`) is a block the compiler owns.
- `<-` draws a wire, `=` sets a parameter. Expressions (`and`, `>=`, `/ 1000`) compile to real blocks, units read as
  written (`Time: 30min`, `70%`), and `template … end` stamps out repeated rules.

### Who may open a door

Access rights on an NFC Code Touch can live in source too. An `access` declaration lists who may open it, each user
or group by its title with an access schedule:

```text
extern haustuer = NfcCodeTouch(uuid: "…")

access haustuer
	user "alice": "Always"
	group "Residents": "Workdays"
end
```

The declaration owns that Code Touch's access: whoever it leaves out loses the right on the next push. Users, groups,
schedules, NFC tags and codes are still created in Loxone Config or the app; a title the config doesn't have stops
the compile. `lox ir adopt` and `lox ir decompile` write the declarations for the rights the config has, and
`lox ir lint` names users whose tag or code opens no Code Touch.

### Switching times and app switches

A `schedule` is a DayTimer's week in source; a claim writes it onto a timer, extern or managed:

```text
schedule abendlicht
	weekdays 06:00-07:30, 17:00-22:30
	weekend 08:00-23:00
	holiday vacation off
end

schaltuhr.schedule = abendlicht

party = VirtualIn("Partymodus")          # a new switch in the app, in the Virtual Inputs folder
```

The claim replaces the timer's switching times and nothing else; edits made in the app are overwritten on the next
push, and removing the claim hands the times back to the app. A new `VirtualIn` gets the next free `VI<n>`.

The full language: [ir-spec.md](https://github.com/discostu105/lox/blob/main/crates/lxir/docs/ir-spec.md).
Worked examples: [crates/lxir/examples](https://github.com/discostu105/lox/tree/main/crates/lxir/examples).

## Set up a project

A directory with a `lox.toml` is a project: one Miniserver, one deployment target. Keep it in git.

```bash
mkdir haus && cd haus && git init
lox config download --extract --save-as base.zip     # → base.zip + base.Loxone
lox ir decompile base.Loxone | less                  # read-only view of every page
```

```toml
# lox.toml — paths are relative to this file
base   = "base.Loxone"       # the deployed config (required)
module = "pages"             # a .lxir file or a directory of them (required)
lock   = "lxir.lock.json"    # default: lxir.lock.json
out    = "out.Loxone"        # default: out.Loxone
serial = "504F94112233"      # in minted UUIDs; push checks it
```

Then start in one of two ways:

```bash
# Take over the existing blocks lxir can manage (one file per page)
lox ir adopt base.Loxone --out-dir pages --out-lock lxir.lock.json

# …or adopt one block, e.g. one you just drew in Loxone Config
lox ir adopt base.Loxone --uuid <uuid> --as stiegenlicht \
    --module pages --lock lxir.lock.json
```

Or write a new page from scratch in `pages/`. Find UUIDs and names with `lox config find` and
`lox config wiring`.

{: .tip }
Only block types whose ports are verified can be managed. Anything else stays unmanaged (adopt says so with a
warning) and keeps working, untouched, as part of the base.

## The edit loop

```bash
lox ir check                          # parse + validate, errors with line numbers
lox ir fmt --write                    # canonical formatting
lox ir compile                        # base + module + lock → out.Loxone, lock updated
lox config diff base.Loxone out.Loxone   # what changes, by name, page by page
lox ir lint                           # unused names, blocks whose outputs feed nothing
```

`lox config diff` is the readable review: names, pages and values. `lox ir diff --exit-code` is the same
comparison by UUID, with exit code 1 when anything differs (for scripts and CI). `lox ir diff --summary` says it
as a commit message, by title and port name: its first line is the subject.

```
base.Loxone → out.Loxone

= 2 added · 1 parameter · wires +4 −0

Beschattung
  + block  Temp über 28 (GreaterEqual)
  + block  temp_hoch.Q and sonne.Q (And)
  ~ param  Beschattung Süd · TargetPos  100 → 70
  + wire   Außentemperatur.Q → Temp über 28.Input1
  + wire   Sonnenschein.Q → temp_hoch.Q and sonne.Q.I2
  + wire   Temp über 28.Q → temp_hoch.Q and sonne.Q.I1
  + wire   temp_hoch.Q and sonne.Q.Q → Beschattung Süd.AutoShade
```

That is the example above compiled: the expression became an `And` block, labeled with the expression so it stays
readable in Loxone Config.

## Deploy with `lox config push`

```bash
lox config push              # dry run: download live, drift check, compile, show the changes, stage
lox config push --force      # … then upload, reboot, wait, verify, move base forward
git add -A && git commit -m "Beschattung: Süd fährt bei 28° auf 70%"
```

Each step stops the push when it fails:

1. Download what the Miniserver runs now (`backups/<stamp>-pre-push.zip`). **Custom changes:** if the app, the web
   UI or the API changed something Loxone Config hasn't saved yet (light moods, timers, autopilot rules), the
   Miniserver holds it in `/sys/custom/`. The download doesn't contain it and the upload would delete it, so
   `--force` refuses and names what is there.
2. **Drift check:** the live logic must still be your `base`. If someone saved from Loxone Config or the app since,
   the push refuses and names the `lox ir diff` to run. Changed users, groups, tags, codes or access rights don't
   stop it: the push lists them as kept as live.
3. Compile against the live download, so users, passwords, access rights and layout stay as they are live.
4. Show the changes and check the users: users, groups, NFC tags and codes changed in the app after the program file
   was saved exist only on the Miniserver, and loading the upload would delete them. The push compares the live
   users with the program file and lists what differs (needs an admin connection).
5. Stage the upload: the downloaded ZIP with `sps0.LoxCC` replaced, verified to extract to exactly the compiled
   file. The app's catalogue in it stays as Loxone Config wrote it, so the push names the app controls it renames,
   moves or deletes: the app may keep showing them as before until Loxone Config saves. If your module grants or
   revokes access ([above](#who-may-open-a-door)), `permissions.bin` is regenerated to match, provided lxir
   reproduces the live one exactly; otherwise the push warns that it stays stale. Without `--force` the push ends
   here.
6. Upload as `/prog/sps_new.zip`, reboot, wait until the Miniserver answers (up to 5 minutes). Users changed live
   stop the upload: load and save the program in Loxone Config first, or pass `--discard-custom-changes`.
7. Download again: the Miniserver must run exactly the compiled file, or the push fails and the project stays as it
   was.
8. Update the lock, `out`, and `base` in `lox.toml`. Commit them.

| Flag | When |
|:-----|:-----|
| `--allow-removals` | You deleted managed blocks from the module and want them gone from the config |
| `--accept-version <v>` | The base was saved by a newer Loxone Config release |
| `--patched-from <dl.Loxone>` | Your base is a byte patch of that download |
| `--backup-dir <dir>` | Downloads and artifacts elsewhere (default `backups/`, git-ignored: configs hold personal data) |
| `--discard-custom-changes` | Push although the Miniserver holds custom changes, deleting them |
| `-o json` | A machine-readable report on stdout |

{: .note }
To keep custom changes, load the project from the Miniserver in Loxone Config and save it. The live config then
differs from your `base`: bring the project up to date as below, then push.

**Roll back** by uploading the download from before the push:

```bash
lox config upload backups/<stamp>-pre-push.zip --force && lox reboot --yes
```

## When someone else changed the config

Loxone Config, the app and the Miniserver itself also write the config. Check before you edit:

```bash
lox config download --extract --save-as live.zip
lox ir drift live.Loxone              # exit 0: in sync with the last compile; 1: someone wrote since
lox config diff base.Loxone live.Loxone   # what they changed
```

If they changed something you don't manage, point `base` in `lox.toml` at `live.Loxone` and compile again. If they
touched a managed block, bring the module up to date first (or `lox ir adopt --uuid` a block they added).

## Tests, CI and editors

- **Tests:** `test "…" … end` blocks in a module set inputs, advance time and `expect` outputs. `lox ir test` runs
  them against the compiled config through [lox-cli](https://github.com/eisber/lox-cli)'s simulator (`--lox <path>`
  or `$LOX`).
- **CI:** [examples/ci.sh](https://github.com/discostu105/lox/blob/main/crates/lxir/examples/ci.sh) checks
  parsing, formatting, lock currency and byte-determinism; `--sync` also requires the sources to match the deployed
  config.
- **No personal data in git:** a real config carries the address, users, codes and serials. `lox ir scan <file|dir>`
  lists where (element and attribute, never the value) and exits 1, so a CI step or pre-commit hook can refuse it.
- **Rename safely:** `lox ir rename old_slug new_slug` renames across all files and the lock, identities kept.
- **VS Code:** syntax highlighting and snippets in
  [editor/vscode](https://github.com/discostu105/lox/tree/main/crates/lxir/editor/vscode).
- **AI agents:** `lox ir check --json` gives machine-readable errors; the
  [agent guide](https://github.com/discostu105/lox/blob/main/crates/lxir/docs/agents.md) has the rules.

## Reference

| Doc | Contents |
|:----|:---------|
| [lxir README](https://github.com/discostu105/lox/blob/main/crates/lxir/README.md) | Overview, every CLI command, library use |
| [ir-spec.md](https://github.com/discostu105/lox/blob/main/crates/lxir/docs/ir-spec.md) | The `.lxir` language |
| [lockfile-spec.md](https://github.com/discostu105/lox/blob/main/crates/lxir/docs/lockfile-spec.md) | The lockfile format |
| [vision.md](https://github.com/discostu105/lox/blob/main/crates/lxir/docs/vision.md) | Why, and the "three writers, one file" model |
