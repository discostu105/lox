# Vision: Loxone config as code

## The problem

A Loxone Miniserver's entire behavior lives in one `.Loxone` XML file that is
practically opaque:

- It is only editable in **Loxone Config**, a Windows GUI. There is no
  reviewable diff, no pull request, no blame, no rollback story beyond
  whole-file backups.
- Logic cannot be **reused**. Ten shading controllers for ten windows are ten
  hand-drawn copies; fixing a bug means fixing it ten times.
- There is no way to **test** logic before uploading it to the house.
- Automation and AI agents cannot safely author logic: the file's identity
  model (UUIDs, counters, per-port identifiers) is undocumented, and naive
  editing corrupts it.

## The idea

Treat the config the way Terraform treats infrastructure:

| Terraform | lxir |
|---|---|
| `.tf` source | `.lxir` IR modules (text, git-friendly) |
| provider state (cloud) | the `.Loxone` config on the Miniserver |
| `terraform.tfstate` | the lockfile (`*.lock.json`) |
| `terraform plan` / `apply` | `lxir diff` / `lxir compile` |
| `terraform state rm` | `Lockfile::remove_object` |
| `terraform import` | `lxir decompile` |

The IR describes **only the logic you choose to manage**. Everything else —
hardware, rooms, users, visualization, other people's logic — passes through
the compiler byte-for-byte untouched. This is what makes the approach safe to
adopt incrementally: manage one shading rule today, and the rest of the
config never notices.

## What this unlocks

- **Review**: a wire change is a one-line diff in a pull request, not a
  screenshot of a diagram.
- **Reuse** (roadmap): templates instantiate the same logic for N windows.
- **Testing** (roadmap): compile against a base config, run the result
  through lxir's simulator (`lxir test`, D68) before it ever reaches the
  house.
- **Agents**: an AI agent can read the IR, propose a change, compile, show a
  semantic diff, and never touch the fragile XML directly. See
  [agents.md](agents.md).
- **Locale immunity**: identity is pinned by UUID in the lockfile, so the
  111-renames-per-save locale churn that plagues title-based tooling is
  irrelevant.

## Living with two masters

Loxone Config remains a legitimate writer — hardware, visualization, the
occasional manual edit. Drift is not prevented; it becomes an explicit,
reviewable step:

```text
everyday:             edit .lxir → check → compile → diff → upload
after a GUI session:  download → decompile
    ├─ managed logic changed?    → review it as a source diff, commit
    ├─ only layout or hardware?  → lockfile update, source untouched
    └─ new unknown objects?      → stay unmanaged, pass through untouched;
                                   adoptable into source later (roadmap)
```

Gradual adoption is also the migration path for existing installations: a
real config moves block-by-block from unmanaged to managed — never
big-bang.

## Non-goals

- Replacing Loxone Config. Hardware setup, device pairing, creating rooms
  and categories, and visualization stay in the GUI. The IR manages *logic
  blocks and their wiring* — the part that benefits from being code — plus
  the *assignment* of existing objects: an extern's title, room and
  category are plain facts a source file can hold (D39).
- Transport. Talking to the Miniserver (FTP/HTTP, credentials) belongs to the `lox` / `lox-cli` CLIs, which consume this
  crate. The library stays pure: bytes in, bytes out.
- Simulating what Loxone does not document. `lxir test` simulates the
  blocks whose behavior Loxone documents (D68, [simulator.md](simulator.md))
  and refuses the rest by name; it does not reimplement the Miniserver.

## Where this came from

The concept began as a German design sketch ("lox-ir — Design-Skizze",
preserved in this repo's git history) proposing the IR, the
lockfile-as-tfstate idea, templates, expression sugar, and sim-backed
tests. A live investigation against a real Miniserver (2026-08-24, config
v273, ~1900 objects, six config generations spanning two years) validated
its core assumptions and sharpened the identity model. The findings are
condensed in [loxone-format.md](loxone-format.md); the resulting
architecture in [design.md](design.md); the sketch ideas not yet
implemented live on in [roadmap.md](roadmap.md).
