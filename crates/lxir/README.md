# lxir - loxone intermediate language

**lxir** (pronounced like *elixir*) is **a git-friendly and AI-friendly
text representation of Loxone config**. Miniserver logic — blocks, wires,
parameters — becomes plain text that humans can review in a pull request
and agents can safely write, and a deterministic compiler applies it to
the real `.Loxone` config without disturbing anything else in the file.

```text
# Beschattung Süd — Beispielmodul.
# Externe Objekte gehören Loxone Config; Blöcke gehören dem Compiler.

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

`lxir compile` turns this into exactly the XML Loxone Config would have
drawn — UUIDs, counters, per-port connectors, canvas layout — and records
every identity it minted in a lockfile, so the next compile changes only
what you changed.

Rules you repeat become templates: `template fassade(jalousie:
AutoJalousie, pos = 70) … end` declares the body once, `sued =
fassade(jalousie: jal_sued)` stamps it out per facade. Each instance's
blocks get stable expanded slugs (`sued_hoch`) in the lockfile, so
re-instantiating never re-mints identities.

Boolean logic can skip the blocks entirely:
`jal_sued.AutoShade <- sonne.Q and aussentemp.Q >= temp_schwelle`
desugars into the same verified gate and comparator blocks — each
labeled with its sub-expression, so the rule stays readable in Loxone
Config — and editing the expression re-derives them without ceremony.
Arithmetic goes the other way: `verbrauch_kw.AI <- verbrauch.AQ / 1000`
folds the whole calculation into ONE `Formula` block (`I1/1000`), ports
as inputs, constants inlined. Physical values read as written —
`Time: 30min`, `2700K`, `70%` — and compile to the port's base unit.

Devices stay Loxone Config's to create, but their assignment can live
in source: `sensor.title = "Raumklima Bad"`, `sensor.room = "Bad"`,
`sensor.category = "Klima"` on an extern rewrite its title and its
room/category binding — and a room change carries along the device's
inputs and outputs that shared its room (D39).

PicoC programs live in their own files: `hue = Code16("Hue", Code:
file("hue.c"), AI1: rgb.AQ)` compiles the file into the program block,
written the way Loxone Config writes it (D40).

## Why

A Miniserver's entire behavior lives in one opaque `.Loxone` XML file,
editable only in a Windows GUI. There is no reviewable diff, no reuse (ten
shading rules are ten hand-drawn copies), no way to test before uploading
to the house, and no safe way for scripts or AI agents to author logic —
the file's identity model (UUIDs, counters, per-port identifiers) is
undocumented, and naive editing corrupts it.

lxir treats the config the way Terraform treats infrastructure: the `.lxir`
module is source, the `.Loxone` file is the deployed artifact, and the
lockfile — like `terraform.tfstate` — pins every identity in between. A
wire change becomes a one-line diff in a pull request.

Crucially, the language describes **only the logic you choose to manage**.
Everything else — hardware, rooms, visualization, other people's logic —
passes through the compiler byte-for-byte untouched. Manage one shading
rule today; the rest of the config never notices. See
[docs/vision.md](docs/vision.md).

## How it stays safe

- **Three writers, one file.** Loxone Config, the Miniserver itself
  (app-created autopilots, device registrations), and this compiler all
  write the config. The compiler owns *only* its managed blocks, the wires
  it drew onto extern ports, the `Def=` values it assigned there, and the
  title/room/category it assigned to externs — everything else
  round-trips untouched through a byte-faithful XML layer.
- **Identity is UUID, not title.** Titles are locale-volatile (one observed
  save renamed 111 built-ins). Externs match by `uuid` > `iname` > `title`,
  or a ref by its target (`mirrors: <slug>`, D32); once resolved, the
  lockfile pins the UUID — object *and* every port.
- **Determinism.** Same base + module + lock + options → same output bytes.
  New UUIDs come from a deterministic minter (no clock, no RNG), are
  recorded in the lock, and never change again. Recompiling the compiler's
  own output is a byte-level fixpoint (tested).
- **Refuse, never guess.** Only live-verified block types can be minted;
  wiring a port the base config doesn't have is an error, not an invented
  UUID; a managed block vanishing from source is an error unless removal is
  explicit.

## Try it on your own config

lxir never talks to a Miniserver — it only reads and writes `.Loxone`
files. Transport is [`lox`](https://github.com/discostu105/lox)'s job,
the CLI this crate lives in: every `lxir` command below also runs as
`lox ir …`. A careful first loop:

```sh
brew tap discostu105/lox https://github.com/discostu105/lox
brew install discostu105/lox/lox   # or a release binary of lox;
                                   # `cargo install lxir` for lxir alone

# 1. Get the deployed config as XML (or use the .Loxone file
#    Loxone Config saves locally):
lox config download --extract --save-as current.zip   # writes current.Loxone

# 2. Look around — decompile is a read-only view of every page:
lxir decompile current.Loxone | less

# 3. Start small: adopt ONE existing block you own mentally,
#    or add a page-placed block of your own. This writes a module
#    and a lockfile pinning the block's identity:
lxir adopt current.Loxone --uuid <uuid> --as my_block \
     --module modules/ --lock modules/haus.lock.json

# 4. Edit the module, then compile onto the *current* base:
lxir compile --base current.Loxone --module modules/ \
     --lock modules/haus.lock.json --out out.Loxone --serial <serial>

# 5. Trust nothing — read the diff. It must show exactly your change:
lxir diff current.Loxone out.Loxone

# 6. Deploy. In a lox.toml project (base, module, lock, serial):
lox config push            # drift check, compile, show the changes, stage
lox config push --force    # upload byte-exact, reboot, verify, move base
#    Or open out.Loxone in Loxone Config and upload from there.
#    Upload exactly the file you diffed — a tool that re-serializes
#    or re-wires on the way (lox-cli's `config push`) can change it.
```

After a push, download again and keep that file as the new base
(`lox config push` does both) — `lxir drift` tells you when someone else
(GUI, app, Miniserver) has written to the config since, *before* you
compile over their change.
Everything you did not adopt passes through byte-for-byte; the diff in
step 5 is the proof, every time.

## Quickstart (CLI)

```sh
cargo run --bin lxir -- help         # in the lox repo; or `lox ir help`

lxir check modules/beschattung.lxir  # parse + validate (line-numbered errors)
lxir fmt --write modules/beschattung.lxir
lxir compile                         # in a lox.toml project: no flags needed
lxir compile --base current.Loxone --module modules/beschattung.lxir \
             --lock modules/beschattung.lock.json --out out.Loxone \
             --serial 504F94112233
lxir rename alter_name neuer_name    # refactor: sources + lockfile together,
                                     # identities kept, output verified
lxir lint                            # advisory: unused names, blocks whose
                                     # outputs feed nothing (GUI wires count)
lxir diff current.Loxone out.Loxone  # semantic diff, locale noise flagged,
                                     # users/access section; --json
lxir render out.Loxone --page Beschattung --out page.svg
                                     # a page as SVG + layout metrics
lxir compile --relayout Beschattung  # place the page's managed blocks
                                     # afresh (GUI drags are kept otherwise)
lxir decompile current.Loxone        # full IR view, grouped by page
lxir decompile --out-dir view/ current.Loxone  # one module per page
lxir adopt current.Loxone --out-module modules/haus.lxir \
     --out-lock modules/haus.lock.json  # take over existing blocks in place
lxir adopt current.Loxone --uuid <uuid> --as <slug> --module modules/ \
     --lock modules/haus.lock.json  # adopt one freshly drawn block
lxir observe current.Loxone          # port-direction evidence (JSON)
lxir roundtrip current.Loxone        # byte-fidelity self-check
lxir repair current.Loxone --out fixed.Loxone  # `&#xA;` text from a foreign
                                     # writer back to raw line breaks
lxir test                            # run the module's `test … end` blocks
                                     # through lox-cli's `lox sim` (--lox/$LOX)
```

A directory with a `lox.toml` — flat `key = "value"` lines naming the
base config, module, lockfile, output, and optionally serial and page —
is a **project**: `lxir compile` inside it needs no flags,
`check`/`fmt`/`drift` default to its module and lock, and the module
directory may nest subdirectories (`rooms/`, `systems/`). Fragments
merge into one namespace, so there is no `import` statement to write
(decision D25).

For a repo holding your `.lxir` sources, [examples/ci.sh](examples/ci.sh)
is the CI check path: parse/validate, canonical formatting, lock currency,
byte-determinism, optionally byte-comparison against a committed expected
output — and `--sync` mode, which additionally requires the sources to
equal the deployed state (empty semantic diff, drift green). The lox
repo's [workflow](../../.github/workflows/ci.yml) runs it against the
shipped examples.

## Quickstart (library)

```rust
use lxir::{LoxoneDoc, Lockfile};
use lxir::ir::{compile, CompileOptions, Module};
use lxir::uuid::parse_serial;

let base = LoxoneDoc::parse(&std::fs::read("examples/configs/haus.Loxone")?)?;
let module = Module::parse(&std::fs::read_to_string("examples/ir/beschattung.lxir")?)?;
let mut lock = Lockfile::new(); // or Lockfile::load(path)

let out = compile(&base, &module, &mut lock, &CompileOptions {
    machine: parse_serial("504F94112233")?,   // stamped into minted UUIDs
    mint_time_unix: 1_767_225_600,            // caller-provided → reproducible
    page_title: Some("Beschattung".into()),
    allow_removals: false,
    accept_version: None,                     // ConfigVersion pin (D22)
})?;
std::fs::write("out.Loxone", out.to_bytes())?;
lock.save(std::path::Path::new("beschattung.lock.json"))?;
```

Runnable examples (`cargo run --example …`): `compile`, `decompile`,
`diff`, `observe`, `roundtrip_check`, `lifecycle`. The committed
`examples/out/` files are the output of the `compile` example; running it
again reproduces them byte-for-byte.

For a complete end-to-end module, see `examples/ir/pool.lxir` against
`examples/configs/pool.Loxone`: it exercises the core language — `page`
placement, named constants, externs matched by iname and by composite
`title` + `room:`, a template with an instance, an expression as an
argument, a unit-suffixed value (`Time: 30min`), a boolean expression
wired onto an extern port, an arithmetic binding folded into a
`Formula` block, and two `test … end` scenarios runnable with
`lxir test` — the sketch's pool idea, kept compiling by the
test suite. [examples/README.md](examples/README.md) indexes the rest:
`examples/ir/sprachtour.lxir` walks every remaining construct (matchers,
mirrors, templates in full, units, fan-in), `examples/ir/lifecycle/`
stages `moved`/`removed`, and `examples/project/` is a committed
`lox.toml` project with fragment modules.

Tests live in the module itself and simulate the *compiled* config:

```text
test "Frostwächter schlägt an und beruhigt sich"
	wassertemp.Q = 2         # inject: drive the sensor
	tick 2
	expect frost_alarm.Q == 1
	wassertemp.Q = 10
	tick 2
	expect frost_alarm.Q == 0
end
```

`lxir test` compiles in memory, hands the scenario to
[`lox sim`](https://github.com/eisber/lox-cli) (found via `--lox`,
`$LOX`, or PATH), and reports each `expect` with its actual value on
failure. Comparators: `== > >= < <= ~=` (approximately); `clock "06:30"`
sets the simulated time for `DayTimer`-style logic.

## Scope

**In scope** — the pure model, reusable by any tool:

| Module | What it does |
|---|---|
| `xml` | Lossless concrete-syntax parser/writer for `.Loxone` XML. Handles Loxone's spec-violations (attribute names starting with digits, raw newlines inside attribute values) that break conforming XML parsers. |
| `uuid` | The anatomy of Loxone UUIDs — creation time, mint counters, minting-machine id, connector index — plus a deterministic minter (no clock, no RNG). |
| `doc` | Semantic read layer: objects, ports, wires, counters, pages. |
| `connectors` | Port-direction knowledge: a **verified** builtin table (gates, comparators, `Formula`, `Monoflop`, `PulseGen`, `AnalogThresholdTrigger` — see [docs/connector-db.md](docs/connector-db.md)) and evidence-based inference (`observe`, corpus merge, legacy-db crosscheck) over real configs. |
| `ir` | The text language: constructor-style block declarations with inline wires/parameters, `extern`, `let`, `page` placement, templates with instances, boolean/comparison/arithmetic expressions (as arguments and on wires; arithmetic folds into `Formula` blocks), unit-suffixed values, lifecycle statements, `test … end` scenarios; parser, canonical printer, `compile` (base + module + lockfile → config), `decompile` (config → IR view), `adopt` (existing blocks → module + identity-pinning lockfile), `test` (compile in memory → `lox sim` → per-expect results). |
| `lock` | The lockfile: slug → object *and per-port* UUIDs, counters, layout, extern-wire ownership, extern-port `Def=` originals, attribute claims. |
| `diff` | Semantic diff between two configs, with locale-rename noise flagged. |

**Out of scope** — deliberately: transport (FTP/HTTP to the Miniserver),
LoxCC compression, credentials. Those live in the `lox` / `lox-cli` CLIs,
which are the intended consumers of this crate.

## Documentation

| Doc | Contents |
|---|---|
| [docs/vision.md](docs/vision.md) | Why config-as-code for Loxone; the Terraform analogy; the two-masters workflow |
| [docs/design.md](docs/design.md) | Architecture, ownership model, compile strategy, decisions D1–D45 |
| [docs/ir-spec.md](docs/ir-spec.md) | Normative spec of the `.lxir` language (v1) |
| [docs/lockfile-spec.md](docs/lockfile-spec.md) | The lockfile format (v4) and its invariants |
| [docs/loxone-format.md](docs/loxone-format.md) | Validated reverse-engineering notes on the `.Loxone` format |
| [docs/implementation.md](docs/implementation.md) | Module map, testing strategy, how to extend |
| [docs/connector-db.md](docs/connector-db.md) | The consolidated connector database: methodology, findings, admission rules |
| [docs/oracle-wine.md](docs/oracle-wine.md) | The save oracle: driving Loxone Config under Wine, and what a save changes |
| [docs/agents.md](docs/agents.md) | Operational guide for AI agents using the toolchain |
| [docs/roadmap.md](docs/roadmap.md) | Stufen −1…4: hardening, templates, expressions, verification, multi-module |

[`AGENTS.md`](AGENTS.md) at the repo root gives agents the build/test
commands and repo rules at a glance.

## Editor support

`editor/vscode/` contains a declarative VS Code extension for `.lxir`
files: syntax highlighting, comment/bracket support, and snippets for all
statement forms. Install by symlinking it into `~/.vscode/extensions/`
(see [editor/vscode/README.md](editor/vscode/README.md)). A language server
is scoped on the roadmap; until then `lxir check` / `lxir fmt --check`
cover the validation loop.

## Validation

- The XML layer round-trips **byte-identically** on six real Miniserver
  configs spanning two years of history and three writers (117 KB–1.34 MB)
  — verified against a live installation. Real configs contain personal
  data and are not committed; point `LXIR_CORPUS` at a directory of
  `.Loxone` files to run the corpus test:

  ```sh
  LXIR_CORPUS=~/loxone-backups cargo test --test roundtrip
  ```

- UUID anatomy (epoch 2009-01-01, `ffff` + machine-id object tails,
  `<index>ff` + entity port tails, ports minted before their object) was
  established from live evidence and is encoded in `uuid`'s tests.
- The **save oracle passed**: Loxone Config 17.1 (under Wine) opened a
  compiled config and re-saved it with an empty semantic diff — every
  minted UUID, wire, and parameter survived. See
  [docs/oracle-wine.md](docs/oracle-wine.md).
- The former grown-gate assumption is **resolved, negatively**: Loxone
  Config 17 gates are fixed two-input, and an off-descriptor `I3` is
  silently deleted on save (oracle-verified). The compiler refuses `I3`+
  instead of minting it — see design decision D8.

## Relationship to `lox` / `lox-cli`

lxir lives in the [`lox`](https://github.com/discostu105/lox) repository
as `crates/lxir`. lox (server API, transport, LoxCC) depends on it for
everything config-model-related — `lox config diff/show/wiring/lint`, and
the whole toolchain as `lox ir`. The crate stays a separate, network-free
library, published on crates.io for anyone else to build on.
[`lox-cli`](https://github.com/eisber/lox-cli) (Markus Cozowicz's fork of
lox: config manipulation CLI, DOM-level writer, `lox sim` simulator) is
where `lxir test` gets its simulator.

## License

Dual-licensed, the same scheme as `lox`: use it under the
[GPL-3.0](LICENSE-GPL), or obtain a
[commercial license](LICENSE-COMMERCIAL) for proprietary redistribution —
see [LICENSE](LICENSE). Early v0, published on
[crates.io](https://crates.io/crates/lxir) as `lxir`.
