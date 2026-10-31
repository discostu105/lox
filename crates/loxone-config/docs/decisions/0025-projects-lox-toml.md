# D25: Projects are a `lox.toml`, and there is no `import`

Status: accepted (2026-08-25)

## Context

A deployment target needs a base config, a module, a lock and an output. Passing them as flags on every run
is error-prone.

## Decision

A directory with a `lox.toml` is a project, one deployment target:

- `base` (the deployed config) and `module` (a file or a fragment directory) are required.
- `lock` and `out` have defaults. `serial` and `page` are optional.
- The file has flat `key = "value"` lines and `#` comments. Paths are relative to the file.
- `loxl compile` inside the directory needs no flags; flags override the file. `check`, `fmt` and `drift`
  default to the project's module and lock.
- Module directories are searched recursively, so `externals.loxl` can sit beside `rooms/`, `systems/` and
  `templates/`.
- The file is read by a small strict parser. It refuses tables, arrays and unquoted values with a pointed
  error.
- `module` has no default. Defaulting to `.` would pull stray `.loxl` files (decompile views, backups) into a
  compile.
- The serial is validated at parse time, so a typo fails before any file is read.

## Rejected

- An `import` statement. The fragments of a module share one namespace and one lockfile, and compile into one
  `.Loxone` document. An import would declare a dependency with no effect, on every file. A future need for
  several projects or namespaces gets its own decision.
- The `toml` crate. Six string keys do not justify three transitive dependencies.
