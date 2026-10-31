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

| Terraform | Lox Logic |
|---|---|
| `.tf` source | `.loxl` Lox Logic modules (text, git-friendly) |
| provider state (cloud) | the `.Loxone` config on the Miniserver |
| `terraform.tfstate` | the lockfile (`*.lock.json`) |
| `terraform plan` / `apply` | `loxl diff` / `loxl compile` |
| `terraform state rm` | `Lockfile::remove_object` |
| `terraform import` | `loxl adopt` |

Lox Logic describes **only the logic you choose to manage**. Hardware, rooms, users, visualization and
other people's logic pass through the compiler byte for byte. That makes it safe to adopt step by
step: manage one shading rule, and the rest of the config stays as it is.

## What this unlocks

- **Review**: a wire change is a one-line diff in a pull request, not a
  screenshot of a diagram.
- **Reuse**: templates create the same logic for N windows.
- **Testing**: `loxl test` runs the compiled result in the Lox Logic simulator before it reaches the
  house.
- **Agents**: an AI agent can read the source, propose a change, compile and show a semantic diff
  without touching the XML. See [agents.md](agents.md).
- **Locale changes don't matter**: the lockfile pins identity by UUID, so a save that renames 111
  built-in objects into another language changes nothing.

## Living with two masters

Loxone Config stays a legitimate writer: hardware, visualization, the occasional manual edit. Drift
is not prevented; it becomes an explicit step you review (`loxl drift`):

```text
everyday:             edit .loxl → check → compile → diff → upload
after a GUI session:  download → decompile
    ├─ managed logic changed?    → review it as a source diff, commit
    ├─ only layout or hardware?  → lockfile update, source untouched
    └─ new unknown objects?      → stay unmanaged and pass through;
                                   `loxl adopt` takes them over later
```

An existing installation moves to source the same way, block by block. `loxl coverage` shows how far
it has come.

## Non-goals

- Replacing Loxone Config. Hardware setup, device pairing, creating rooms and categories, and
  visualization stay there. Lox Logic manages logic blocks and their wiring, plus the assignment of
  existing objects: an extern's title, room and category are facts a source file can hold.
- Transport. Talking to the Miniserver (FTP, HTTP, credentials) belongs to `lox`. The library stays
  pure: bytes in, bytes out.
- Simulating what Loxone does not document. `loxl test` simulates the blocks whose behavior Loxone
  documents ([simulator.md](simulator.md)) and refuses the rest by name.

## Further reading

The verified format facts are in [loxone-format.md](loxone-format.md), the architecture in
[design.md](design.md), and the direction in [roadmap.md](roadmap.md).
