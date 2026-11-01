# D84: A `page` statement governs its own file

Status: accepted (2026-11-01), amends [D28](0028-page-statement.md)

## Context

A directory module joins its fragments in path order. A `page` statement governed the blocks after it
until the next `page`, across that join: a file without its own `page` took the page the previous file
named. Where a block lands then depends on the file names around it. Renaming `garten.loxl` to
`aussen.loxl`, or adding a file that sorts in front, moves blocks between pages, and nothing in the
file itself says so. The convention (every fragment opens with its `page`) made this rare, but a reader
or an agent writing one file had to know the files before it.

## Decision

**A `page` statement governs the rest of its file.** It ends at the next `page` or at the end of the
fragment. Blocks above a fragment's first `page` take the default page (`--page`, the project's
`page`, or the document's first page), as they would in a module of that one file. A single-file
module does not change. `mirrors:` narrowing, which looks at the `page` in force at the extern, follows
the same scope.

**The merge marks the boundary.** `Module::from_fragments` joins the fragments with an
`Item::FragmentEnd` between them. Expansion and desugaring keep it in place. It is never parsed or
printed. Every place that joins fragments (the CLI, the language server, adopt's check) goes through
it.

**Lint, not an error.** `loxl lint` reports `default-page` for a fragment whose page blocks come before
its first `page` while another fragment names one. Such a file can be deliberate (blocks for the
default page), so it stays advisory. Object-tree items (virtual inputs, modes, device I/O) have no page
and are not counted.

**Migration.** A file that relied on the carry-over gets the previous file's `page` line on top. The
shipped examples and the r50 house config had none: every fragment with page blocks already opened
with its `page`, and the files without one declare only object-tree items. Their output is
byte-identical.

## Rejected

- **Requiring a `page` in every fragment.** Files of externs, templates or tests have no page, and a
  module may well put everything on the default page.
- **An error for a page-less fragment in a paged module.** Blocks on the default page are a valid
  choice; the lint names the likely mistake without refusing the file.
- **Fragment boundaries as a field of `Module`.** Every place that builds a module would have to keep a
  second list in step with `items`. A marker item stays in order through expansion and desugaring for
  free.
