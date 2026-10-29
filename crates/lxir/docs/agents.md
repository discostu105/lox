# Working with lxir as an AI agent

Operational guidance for AI agents (and automation generally) authoring
Loxone logic through this toolchain. The IR exists so you never have to edit
`.Loxone` XML — and you must not.

## Hard rules

1. **Never edit `.Loxone` files directly.** All config changes go through
   `lxir compile`. The XML's identity model (UUIDs, counters, per-port
   identifiers, three concurrent writers) makes manual edits corrupting.
2. **Never hand-edit the lockfile**, except through the documented
   operations (`remove_object`, `rename_object` — exposed via the library;
   see [lockfile-spec.md](lockfile-spec.md)). Commit the lockfile together
   with the module.
3. **Never invent block types or port names.** Valid types for a block
   declaration (`slug = Type(…)`) are exactly the builtin table
   (`src/connectors.rs`: gates, the comparator
   family, `Formula`, `Monoflop`, `PulseGen`, `AnalogThresholdTrigger`).
   Valid ports on an extern are whatever the base config actually has —
   discover them with `lxir decompile` / `lxir observe`, or read them from
   the error, which lists them (and suggests the closest name for typos).
   `lxir check` catches wrong types/ports/directions on managed blocks
   without needing the base config.
4. **Do not upload.** Compiling produces a file; sending it to a Miniserver
   is a separate, human-gated step (lox's `config` commands, outside this
   crate).
   Treat everything here as read-only toward the house.
5. **Prefer `iname`/`uuid` matching over `title`** for externs — titles are
   locale-volatile and may change under you.
6. Treat `NoMatch` / `AmbiguousMatch` as **stop-and-ask**: choosing among
   candidates is a human decision unless instructions already cover it.

## The loop

```sh
# 0. Orientation: what does the config contain, what is already managed?
lxir decompile current.Loxone            # full IR view, `page` sections
lxir decompile --out-dir view/ current.Loxone   # one module per page
lxir observe current.Loxone              # port evidence per block type

# 1. Edit the module (.lxir). Whole-line # comments are preserved.

# 2. Validate cheaply before compiling (no base config needed — includes
#    block types, port names, and wire directions on managed blocks):
lxir check modules/beschattung.lxir       # human-readable, line numbers
lxir check --json modules/beschattung.lxir # machine-readable diagnostics
lxir fmt --write modules/beschattung.lxir # canonicalize (non-destructive)

# 3. Compile against the current base:
lxir compile --base current.Loxone \
            --module modules/beschattung.lxir \
            --lock modules/beschattung.lock.json \
            --out out.Loxone
# --serial only needed the first time (recorded in the lock afterwards);
# --time only for reproducible builds (lock pins minted UUIDs regardless).
# In a directory with a lox.toml (a project: base/module/lock/out/serial/
# page as flat key = "value" lines), `lxir compile` alone does all of
# this, and check/fmt/drift default to the project's module and lock.

# 4. Show your work as a SEMANTIC diff, never an XML diff:
lxir diff current.Loxone out.Loxone

# 5. Sanity: the output must round-trip.
lxir roundtrip out.Loxone
```

Present step 4's output when proposing a change: it shows added blocks,
wires, and parameter changes in reviewable form, and flags locale-rename
noise (`[locale?]`) so it isn't mistaken for a real edit.

The decompile view is for **reading, not compiling**: compiling it against
the same base would mint duplicates of every managed block and claim
ownership of every shown wire. To take over existing logic, use `adopt` —
it produces the compilable pair:

```sh
# One-time takeover of existing managed-type blocks (never modifies the
# config; refuses to overwrite existing outputs):
lxir adopt current.Loxone --out-module modules/haus.lxir \
                          --out-lock modules/haus.lock.json
# Blocks whose rebuild would not be faithful are SKIPPED with a warning
# (they stay unmanaged and appear as pinned externs) — read the warnings.
# Then verify the takeover is a semantic no-op before editing anything:
lxir compile --base current.Loxone --module modules/haus.lxir \
             --lock modules/haus.lock.json --serial <serial> --out out.Loxone
lxir diff --exit-code current.Loxone out.Loxone   # must be empty
```

## The language in 30 seconds

See [ir-spec.md](ir-spec.md) for the full spec.

```text
# comment (all comments survive formatting)
let schwelle = 28                    # named constant

extern sonne = VirtualIn(iname: "VI3")           # existing object, not yours
extern jal = AutoJalousie(title: "Beschattung Süd")

temp_hoch = GreaterEqual(            # yours, compiler-owned; the argument
	"Temp über 28",                  # list is the block's whole input side:
	Input1: sonne.Q,                 # port reference  -> wire into Input1
	Input2: schwelle,                # literal or let  -> Def= parameter
)
beschatten = And(                    # gates are fixed 2-input
	I1: temp_hoch.Q,
	I2: sonne.Q,
)

jal.AutoShade <- beschatten.Q        # wire onto an extern port
jal.TargetPos = 70                   # Def= on an extern port; original
                                     # auto-restored when the line is removed
extern sensor = LoxAIRDevice(uuid: "…")  # a device (created in Loxone Config)
sensor.title = "Raumklima Bad"       # assign title, room, category (D39);
sensor.room = "Bad"                  # children in the device's room follow

removed old_gate                     # confirmed delete of a managed block
moved beschatten -> schatten_gate    # slug rename, UUIDs survive
```

The meaning of an argument is decided by its value: `Port: slug.Port`
wires that source in, anything else sets the port's parameter. `<-` and
port-`=` work only on extern targets — everything about a managed block is
in its own argument list.

Removing a block declaration makes the next compile **fail on purpose**. When
deletion is the confirmed intent, add `removed <slug>` to the module — it
is scoped to that one block and shows the intent in the diff. For a slug
rename, rename the declaration and add `moved <old> -> <new>`; both
statements become no-ops once applied. (`--allow-removals` still exists but
authorizes *every* pending removal at once — prefer `removed`.)

## Errors and remedies

Many wrong-name errors end with a ``did you mean `…`?`` suggestion — trust
it only when it matches what you intended.

| Error contains | Meaning | Remedy |
|---|---|---|
| `line N` (from `check`) | syntax/reference error in the module | fix that line; `lxir fmt` shows canonical form |
| `no match for extern` | spec matched nothing of that type | check type + spelling; `lxir decompile` the base to see candidates |
| `ambiguous` + candidate UUIDs | several objects match | switch to `uuid: "<one of the candidates>"` — ask if unclear |
| `not in the verified builtin table` | block type can't be minted in v0 | use a verified type, or declare the object `extern` (create it once in Loxone Config) |
| `unknown port … known ports: …` | wrong port name on a managed block | use a listed port (caught by `check`, no base needed) |
| `has no port … present ports: …` | extern port's `<Co>` missing from base | use a listed port, or have the port wired/set once in Loxone Config so it exists |
| `in the lockfile but not in the source` | managed block vanished from source | typo → restore the line; delete → add `removed <slug>`; rename → `moved <old> -> <new>`; unmanage → `remove_object` |
| `changed type` | slug reused for a different block type | new slug, or `remove_object` first |
| `is an output port and cannot be used as …` | wire direction wrong | swap source and sink |
| `targets managed block` | `<-` or port-`=` used on a block you declared | move the wire/parameter into that block's argument list |
| `is v0 syntax` | `block`/`wire`/`set`/`match` keyword from the old grammar | the error shows the v1 spelling — rewrite the line |
| `duplicate parameter` / `duplicate wire` | same argument twice in one argument list | delete one (fan-in with *different* sources is allowed) |
| `undeclared constant` | bare identifier in value position | declare `let <name> = …`, or quote the value if a string was intended |
| `neither slug is in the lockfile` (from `moved`) | rename references unknown identity | check both spellings against the lock; a first compile needs no `moved` |
| `is an attribute parameter, not a connector` | wire aimed at e.g. `Formula:` | pass a value instead: `Formula: "I1+I2"` |
| `warning: cannot adopt` (from `adopt`) | that block's rebuild would lose something (e.g. GUI input inversion `Inv=`) | leave it unmanaged, or remove the offending state in Loxone Config and re-adopt |
| `base carries N duplicate UUID(s)` | two objects/connectors in the base share one identity (a push tool re-keyed refs) | open and save the config once in Loxone Config — it re-mints the later twins — then use that save as the base; never hand-edit UUIDs |
| `output would carry duplicate UUID(s)` | this compile would emit a repeat: a fresh mint hit a base UUID minted in the same second, or the lock maps two identities onto one UUID | compile again (or another `--time`); if it persists, inspect the lock entries carrying the named UUIDs |
| `targets managed block` + `give it a label` / `D19` | `.title`/`.room`/`.category` on a block you declared | title → the block's label; room/category stay with Loxone Config |
| `rewrites what extern … is matched by` | the extern is matched by the attribute it claims | pin it by `uuid:` (or `iname:`) |
| `no room titled` / `no category titled` | `.room`/`.category` names no `Place`/`Category` in the base | use a listed title, or create the room/category in Loxone Config first |
| `carries no room binding` / `carries no category binding` | the object has no `<IoData Pr=/Cr=>` to rewrite | assign a room/category once in Loxone Config, then claim it from source |
| `claim the … of the same object` | two externs resolve to one object and claim the same attribute | keep one assignment |
| `numeric character references as literal text` (from `drift`) | a foreign XML writer left `&#xA;` text in programs, rules or descriptions; Loxone reads it verbatim | `lxir repair <cfg> --out <fixed>`, then compile against `<fixed>` as the base |
| `recorded for block … no longer exists` | the pinned page was deleted from the base | re-pin: edit the lock entry's `page_uuid`, or `remove_object` the block to place it afresh |

After any failed `compile`, the lockfile on disk is untouched (the CLI saves
only on success) — just fix and re-run.

## Invariants you can rely on

- Compilation is deterministic: unchanged inputs → byte-identical output
  and lockfile. If you see a diff, something actually changed.
- Recompiling the compiler's own output is a no-op (fixpoint) — safe to
  re-run freely.
- The compiler touches only: its managed blocks, wires it drew onto extern
  ports (`<-`), `Def=` values it assigned on extern ports, and the
  title/room/category it claimed on externs (plus that claim's followers).
  Everything else in the config is preserved byte-for-byte.
- Extern-port originals and extern wires are tracked in the lock, so
  deleting IR lines cleanly reverts their effects on the next compile.
