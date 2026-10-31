# D13: `removed` and `moved` statements

Status: accepted (2026-08-25), amended by [D31](0031-removal-tombstones.md)

## Context

Every change should be a reviewable one-line diff. A delete through a CLI flag or a rename through a library
call never shows up in a diff. Terraform closed the same gap by moving `state rm` and `state mv` into
`removed` and `moved` blocks.

## Decision

Deleting and renaming a managed block are statements in the source:

- `removed <slug>` deletes one block. `--allow-removals` instead allows every removal in a run.
- `moved <old> -> <new>` renames a slug and keeps its object and port UUIDs.

Both are no-ops once applied, so the source can keep them. The flag and the library calls
(`Lockfile::remove_object`, `Lockfile::rename_object`) stay as escape hatches.
