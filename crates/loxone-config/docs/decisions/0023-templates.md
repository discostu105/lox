# D23: Templates are macro expansion with locked identities

Status: accepted (2026-08-25)

## Context

Real configs repeat the same block group many times, such as 16 AutoJalousie blocks in one house, each with
its own title and feeds.

## Decision

`template <name>(<params>)` … `end` declares a reusable body of block, wire and assignment statements.
`<slug> = <name>(<param>: <arg>, …)` instantiates it. The lowercase callee tells an instance from a block
declaration: an instance is a composite block.

- **Parameters.** An object parameter (`jalousie: AutoJalousie`) takes an extern or block slug. Its type is
  checked when the slug's declared type is known. A value parameter has a default (`pos = 70`), which an
  instance may override with a literal or a `let` reference.
- **Expansion** is a pure source-to-source pass before compilation:
  - body slug `b` in instance `sued` becomes `sued_b`;
  - object parameters become the passed slugs;
  - value parameters substitute like a shadowing `let`;
  - free identifiers resolve in the module namespace after expansion, so a template may use module externs.
- **Identity.** The lockfile keys the expanded slugs. `sued_hoch` is pinned like a hand-written block.
  Instantiating again never mints again, and editing a body mints only what it adds. `removed sued_alt` and
  `moved` work per instance. Other statements name an instance's blocks by their expanded names; the
  instance slug itself names no object.
- **Nothing downstream knows templates.** Compile, lockfile, diff, drift and the oracle see plain blocks, so
  templates need no oracle run.
- **Port forwarding.** A call-site binding that names no template parameter is forwarded verbatim as a port
  binding onto the body's single block. Per-instance feeds differ in real configs: one house's AutoJalousie
  rows have 0 to 2 `EndUp` sources of mixed types, and reverse-recorded `OutputAPI` entries. Neither typed
  object parameters nor module-level wires can express that. Forwards may repeat like block feeds. A body
  with several blocks takes no forwards.
- **Validation** of references runs on the expanded module, as in compile.
- **Title interpolation.** A body title may embed `{param}`, which expansion replaces with a string value
  parameter. Without it every instance would share one title in the app. A placeholder that names no value
  parameter is an error; braces that are not a slug pass verbatim.
- **`any`.** An object parameter typed `any` takes every type. After expansion, the ports the body uses are
  checked against the type passed. A library template over "the motion sensor" cannot know whether the house
  has a virtual or a physical input.
- **The library** (`templates/`, `loxl templates`) is plain `.loxl`. A project copies a file into its module
  directory, so the instance's identity is the project's. A library change reaches a project only when
  someone copies the file again. Every library template is tested in the simulator against
  `examples/configs/rohbau.Loxone`, so it uses only simulated block types. Its result is a block named
  `ausgang`, which the caller wires to the actuator. Issue: #203.

Deferred: nested templates, template-local `let`/`extern`, and a qualified `<body_slug>.<Port>` forward for
bodies with several blocks.

## Rejected

- Brace-delimited bodies. Language v1 removed braces.
- Indentation-sensitive bodies. The lexer ignores whitespace everywhere else; `end` keeps the grammar
  line-oriented.
- A `use` or `instance` keyword. One call syntax, told apart by case, is enough.
- A label position typed as a value in the grammar, for interpolation. It ripples through parser, fmt and
  decompile for no added power.
- `import` of the library by name. A library update would change every project's compile without a diff in
  its source.
