# D124: No `page default`

Status: accepted (2026-11-01), follows [D84](0084-page-per-fragment.md)

## Context

#303 asked for `page default` to return to the default page in the middle of a file, after a
`page "…"` statement.

## Decision

**No such statement.** The ways that exist cover it:

- Blocks above a file's first `page` take the default page. Since D84 that holds for every file of a
  directory module, so a file of its own is the usual answer.
- `page "<title>"` names any page, the default one too.

The default page is not a fixed page. It is `--page`, the project's `page` or the document's first
page, so the same file would land on different pages under different flags. A named page says where
the blocks go in the file itself, which is what D28 asks of placement.

## Rejected

- **`page default`**: a second spelling for "above the first `page`", and its meaning depends on
  the command line. `default` would also become a word that cannot be a page title.
