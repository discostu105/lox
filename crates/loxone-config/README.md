# loxone-config: the Loxone Config program file, and Lox Logic

This crate reads and writes the program file of a Loxone Miniserver (`.Loxone` XML and backup ZIPs).
It also compiles **Lox Logic** (`.loxl`), a text language for Miniserver logic: blocks, wires and
parameters. You review a change as a diff in a pull request, an agent can write it, and the compiler
applies it to the `.Loxone` file without touching anything else.

```text
# Beschattung Süd: Beispielmodul.
# Externe Objekte gehören Loxone Config, Blöcke gehören dem Compiler.

page "Beschattung"

let temp_schwelle = 28

extern aussentemp = VirtualIn(iname: "VI1")
extern wind_alarm = VirtualIn(iname: "VI2")
extern sonne = VirtualIn(iname: "VI3")
extern jal_sued = AutoJalousie(title: "Beschattung Süd")

temp_hoch = GreaterEqual(
    "Temp über 28",
    Input1: aussentemp.Q,
    Input2: temp_schwelle,
)

beschatten = And(
    I1: temp_hoch.Q,
    I2: sonne.Q,
)

jal_sued.AutoShade <- beschatten.Q
jal_sued.Safety <- wind_alarm.Q

jal_sued.TargetPos = 70
```

`loxl compile` turns this into the XML Loxone Config would have written: UUIDs, counters, connectors
and canvas layout. A lockfile records every UUID the compiler creates, so the next compile changes only
what you changed.

## What the language covers

- **Templates.** `template fassade(jalousie: AutoJalousie, pos = 70) … end` declares a body once;
  `sued = fassade(jalousie: jal_sued)` creates it per facade. Each instance keeps stable identities.
- **Expressions.** `jal_sued.AutoShade <- sonne.Q and aussentemp.Q >= temp_schwelle` compiles into gate
  and comparator blocks, each labeled with its sub-expression. Arithmetic such as
  `verbrauch.AQ / 1000` becomes one `Formula` block.
- **Units.** `Time: 30min`, `2700K` and `70%` compile to the port's base unit.
- **Device assignment.** `sensor.title`, `sensor.room` and `sensor.category` set an object's title, room
  and category. A room change also moves the device's inputs and outputs that shared its room.
- **Door access.** `access haustuer … end` on an NFC Code Touch lists who may open it and when. Users
  left out lose access. Users, tags and codes stay with Loxone Config and the app.
- **Lighting moods.** `moods wohnen … end` declares moods with circuit values, and `licht.moods = wohnen`
  gives them to a lighting controller. Moods keep their identity by name, so switches keep working.
- **PicoC programs.** `Code: file("hue.c")` on a `Code16` block takes the program from a file.
- **Also:** DayTimer schedules, Automatic Designer rules, Touch Pure Flex tiles, operating modes,
  virtual inputs, push notifications and the settings of physical inputs and outputs. The full list is
  [docs/language.md](docs/language.md).

## Why

A Miniserver's behavior lives in one `.Loxone` XML file that only a Windows program edits. You cannot
review a change, reuse a rule or test logic before it reaches the house. Scripts and agents cannot edit
it safely either: its identity model (UUIDs, counters, per-port identifiers) is undocumented, and naive
edits corrupt it.

Lox Logic works like Terraform. The `.loxl` module is the source, the `.Loxone` file is the deployed
artifact, and the lockfile pins every identity in between, like `terraform.tfstate`.

The source describes only the logic you choose to manage. Hardware, rooms, visualization and other
logic pass through the compiler byte for byte. You can manage one shading rule and leave the rest alone.
See [docs/vision.md](docs/vision.md).

## How it stays safe

- **The compiler owns only what it wrote.** Loxone Config, the Miniserver (app-created rules, device
  registrations) and the compiler all write the config. The compiler owns its managed blocks, the wires
  and values it set on extern ports, and the titles, rooms and categories it assigned. Everything else
  round-trips unchanged through a byte-faithful XML layer.
- **Identity is the UUID, not the title.** Titles change with the language of Loxone Config. Externs
  match by `uuid`, `iname` or `title`, and the lockfile then pins the UUID of the object and of every
  port.
- **Deterministic output.** The same base, module, lockfile and options give the same bytes. New UUIDs
  come from a deterministic generator (no clock, no randomness) and never change once recorded.
  Compiling the compiler's own output again changes nothing.
- **Refuse, never guess.** The compiler creates only block types verified on a live Miniserver. Wiring a
  port the base config lacks is an error. A managed block that disappears from the source is an error
  unless you mark it `removed`.

## Try it on your own config

loxone-config never talks to a Miniserver. It reads and writes `.Loxone` files and backup ZIPs; every
command that reads a config also takes a backup ZIP. [`lox`](https://github.com/discostu105/lox) is
the CLI that talks to the Miniserver, and it runs every `loxl` command as `lox logic …`.

```sh
brew tap discostu105/lox https://github.com/discostu105/lox
brew install discostu105/lox/lox   # or a release binary; `cargo install loxone-config` for loxl alone

# 1. Download the deployed config (or use the .Loxone file Loxone Config saves locally).
lox config download --extract --save-as current.zip   # writes current.Loxone

# 2. Look around. Decompile is a read-only view of every page.
loxl decompile current.Loxone | less

# 3. Adopt one existing block. This writes a module and a lockfile that pins its identity.
loxl adopt current.Loxone --uuid <uuid> --as my_block \
     --module modules/ --lock modules/haus.lock.json

# 4. Edit the module, then compile it onto the current config.
loxl compile --base current.Loxone --module modules/ \
     --lock modules/haus.lock.json --out out.Loxone --serial <serial>

# 5. Read the diff. It must show exactly your change.
loxl diff current.Loxone out.Loxone

# 6. Deploy. In a lox.toml project (base, module, lock, serial):
lox config push            # drift check, compile, show the changes, stage
lox config push --force    # upload byte-exact, restart the program, verify, move base
```

You can also open `out.Loxone` in Loxone Config and upload it from there. Upload the file you diffed: a
tool that rewrites the XML on the way can change it.

After a push, keep the downloaded config as the new base; `lox config push` does both. `loxl drift`
tells you when Loxone Config, the app or the Miniserver changed the config since, before you compile
over that change. `loxl coverage --lock modules/haus.lock.json current.Loxone` shows how much of the
config the source owns and lists the blocks left to adopt.

## Quickstart (CLI)

```sh
loxl help                            # or `lox logic help`
loxl check modules/beschattung.loxl  # parse and validate, with line numbers
loxl fmt --write modules/beschattung.loxl
loxl compile                         # in a lox.toml project: no flags needed
loxl compile --base current.Loxone --module modules/beschattung.loxl \
             --lock modules/beschattung.lock.json --out out.Loxone \
             --serial 504F94112233
loxl rename alter_name neuer_name    # rename in sources and lockfile, identities kept
loxl lint                            # unused names, blocks whose outputs feed nothing
loxl diff current.Loxone out.Loxone  # semantic diff; --summary for a commit message, --json
loxl render out.Loxone --page Beschattung --out page.svg   # a page as SVG
loxl compile --relayout Beschattung  # place a page's managed blocks afresh
loxl decompile --out-dir view/ current.Loxone              # one module per page
loxl adopt current.Loxone --out-module modules/haus.loxl \
     --out-lock modules/haus.lock.json                      # take over existing blocks
loxl types [<Type>]                  # block types the compiler can create: ports, units, ranges
loxl types --search hysteresis       # search names and descriptions
loxl templates [<name>]              # the template library
loxl roundtrip current.Loxone        # byte-fidelity self-check
loxl repair current.Loxone --out fixed.Loxone   # turn `&#xA;` text back into line breaks
loxl scan configs/                   # find personal data in configs (exit 1 if any)
loxl test                            # run the module's `test … end` blocks in the simulator
```

A directory with a `lox.toml` is a **project**. Its flat `key = "value"` lines name the base config,
module, lockfile and output, and optionally the serial and page. Inside it, `loxl compile` needs no
flags and `check`, `fmt` and `drift` use its module and lockfile. The module directory may have
subdirectories (`rooms/`, `systems/`). All fragments share one namespace, so there is no `import`.

[examples/ci.sh](examples/ci.sh) is a CI check for a repo of `.loxl` sources. It checks parsing,
formatting, that the lockfile is current, deterministic output, and optionally a committed expected
output. `--sync` also requires the sources to match the deployed config. The lox
[workflow](../../.github/workflows/ci.yml) runs it on the shipped examples.

## Quickstart (library)

```rust
use loxone_config::{LoxoneDoc, Lockfile};
use loxone_config::logic::{compile, CompileOptions, Module};
use loxone_config::uuid::parse_serial;

let base = LoxoneDoc::parse(&std::fs::read("examples/configs/haus.Loxone")?)?;
let module = Module::parse(&std::fs::read_to_string("examples/logic/beschattung.loxl")?)?;
let mut lock = Lockfile::new(); // or Lockfile::load(path)

let out = compile(&base, &module, &mut lock, &CompileOptions {
    machine: parse_serial("504F94112233")?,   // stamped into new UUIDs
    mint_time_unix: 1_767_225_600,            // fixed time: reproducible output
    page_title: Some("Beschattung".into()),
    allow_removals: false,
    accept_version: None,                     // accept a new Loxone Config release
    relayout: Vec::new(),
})?;
std::fs::write("out.Loxone", out.to_bytes())?;
lock.save(std::path::Path::new("beschattung.lock.json"))?;
```

Runnable examples (`cargo run --example …`): `compile`, `decompile`, `diff`, `observe`,
`roundtrip_check`, `lifecycle`. The committed `examples/out/` files are the output of the `compile`
example, and running it again reproduces them byte for byte.

`examples/logic/pool.loxl` (against `examples/configs/pool.Loxone`) is a complete module: pages,
constants, externs, a template, expressions, units, a `Formula` block and two tests.
[examples/README.md](examples/README.md) lists the others, including `examples/project/`, a `lox.toml`
project with fragment modules.

## Tests

Tests live in the module and simulate the compiled config:

```text
test "Frostwächter schlägt an und beruhigt sich"
    wassertemp.Q = 2         # drive the sensor
    tick 2
    expect frost_alarm.Q == 1
    wassertemp.Q = 10
    tick 2
    expect frost_alarm.Q == 0
end
```

`loxl test` compiles in memory, runs each scenario in the Lox Logic simulator and shows the actual value
of each failed `expect`.

- Comparators: `== != > >= < <= ~=` (`~=` is approximately equal).
- `clock 06:30` sets the simulated time, for `DayTimer` logic.
- `--trace` prints each test cycle by cycle.
- `--lox <path>` runs the scenarios in lox-cli's `lox sim` instead.

The simulator models the blocks Loxone documents ([docs/simulator.md](docs/simulator.md)). A test that
depends on another block reports an error that names it.

## Modules

| Module | What it does |
|---|---|
| `xml` | Lossless parser and writer for `.Loxone` XML, including Loxone's deviations from XML (attribute names starting with a digit, raw newlines in attribute values). |
| `uuid` | Loxone UUIDs: creation time, counters, machine id, connector index; a deterministic generator. |
| `doc` | Read layer: objects, ports, wires, counters, pages. |
| `connectors` | Port directions: the verified builtin table (`loxl types` lists it, see [docs/connector-db.md](docs/connector-db.md)) and inference from real configs. |
| `logic` | Lox Logic: parser, formatter, `compile`, `decompile`, `adopt`, lint, `test`. |
| `lock` | The lockfile: per-object and per-port UUIDs, counters, layout, claims. |
| `diff` | Semantic diff between two configs. |
| `archive` | Backup ZIPs (feature `archive`, on by default): the LoxCC codec, extracting the program file, staging an upload. |

[docs/implementation.md](docs/implementation.md) lists every module. Transport (FTP and HTTP to the
Miniserver) and credentials are out of scope; they live in [`lox`](https://github.com/discostu105/lox).

## Documentation

| Doc | Contents |
|---|---|
| [docs/language.md](docs/language.md) | The `.loxl` language spec (v2) |
| [docs/agents.md](docs/agents.md) | How AI agents use the toolchain |
| [skills/lox-logic/SKILL.md](skills/lox-logic/SKILL.md) | The hard rules as a Claude Code skill (link into `~/.claude/skills/`) |
| [docs/vision.md](docs/vision.md) | Why config as code for Loxone |
| [docs/design.md](docs/design.md) | Architecture, ownership model, compile strategy |
| [docs/decisions/](docs/decisions/) | Design decisions |
| [docs/lockfile-spec.md](docs/lockfile-spec.md) | The lockfile format (v8) |
| [docs/loxone-format.md](docs/loxone-format.md) | Verified facts about the `.Loxone` format |
| [docs/implementation.md](docs/implementation.md) | Module map, tests, how to extend |
| [docs/connector-db.md](docs/connector-db.md) | The connector table: evidence and admission rules |
| [docs/simulator.md](docs/simulator.md) | What the simulator models |
| [docs/oracle-wine.md](docs/oracle-wine.md) | Running Loxone Config under Wine to check what a save keeps |
| [docs/roadmap.md](docs/roadmap.md) | Direction and non-goals |

## Editor support

`loxl lsp` is a language server on stdin/stdout. It offers:

- diagnostics as you type (`loxl check` and the lints), and on save in a `lox.toml` project, a compile
  against the base config that writes nothing
- go to definition across a module's fragments
- hover: the object an extern resolves to, and a type's ports
- completion of ports after `name.` and of block types after `name =`
- formatting and the outline
- find references and rename of module-level names (in a project, rename runs `loxl rename`, which
  also updates the lockfile)
- quick fixes for "did you mean `X`?"

`editor/vscode/` is the VS Code extension: highlighting, snippets and the client that starts the server
(see [editor/vscode/README.md](editor/vscode/README.md)). Other editors run `loxl lsp` (or
`lox logic lsp`) for the file type `loxl`. Neovim:

```lua
vim.filetype.add({ extension = { loxl = "loxl" } })
vim.api.nvim_create_autocmd("FileType", { pattern = "loxl", callback = function(ev)
  vim.lsp.start({ name = "loxl", cmd = { "loxl", "lsp" },
    root_dir = vim.fs.root(ev.buf, "lox.toml") })
end })
```

## Validation

- The XML layer round-trips real Miniserver configs byte for byte. Real configs contain personal data
  and are not committed. To run the corpus test, point `LOXL_CORPUS` at a directory of `.Loxone` files
  (`tools/fetch-corpus.py` downloads a public corpus):

  ```sh
  LOXL_CORPUS=~/loxone-backups cargo test --test roundtrip
  ```

- The UUID layout (epoch 2009-01-01, object and port tails, ports created before their object) comes
  from live configs and is covered by the `uuid` tests.
- Loxone Config 17 (under Wine) opens a compiled config and saves it back with an empty semantic diff:
  every new UUID, wire and parameter survives. See [docs/oracle-wine.md](docs/oracle-wine.md).
- Logic gates have two inputs. Loxone Config deletes an `I3` on save, so the compiler refuses it; cascade
  two-input gates instead.

## Relationship to `lox` and `lox-cli`

loxone-config is developed in the [`lox`](https://github.com/discostu105/lox) repository as
`crates/loxone-config`. lox uses it for `lox config diff/show/wiring/lint`, `lox config push` and the
whole toolchain as `lox logic`. The crate stays a network-free library that anyone can build on. It
started as [discostu105/lxir](https://github.com/discostu105/lxir) (crate `lxir` and `.lxir` files up to
0.2), which is now archived.

[`lox-cli`](https://github.com/eisber/lox-cli) is a separate project by Markus Cozowicz, based on an
earlier version of lox, licensed AGPL-3.0 or commercial. There, an AI agent edits the `.Loxone` XML
directly, one command per step (`lox config add`, `set-param`, `wire-connector`), and tests the result
in the `lox sim` simulator. Lox Logic instead keeps the logic as source and compiles it with pinned
identities. Neither project contains the other's code; the Lox Logic simulator is written from Loxone's
documentation. Both projects install a binary named `lox`.

## License

[Apache License 2.0](LICENSE), the same as `lox`. Published on
[crates.io](https://crates.io/crates/loxone-config) as `loxone-config`.
