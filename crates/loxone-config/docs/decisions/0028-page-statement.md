# D28: `page "Title"` places blocks, and it is authoritative

Status: accepted (2026-08-25), amended by [D56](0056-creating-pages.md) and
[D84](0084-page-per-fragment.md)

## Context

The page a block is drawn on was a compile option plus a lockfile pin. Neither shows in a review.

## Decision

A `page "Title"` statement names the base page, by display title, for the block declarations that follow it,
until the next `page` statement.

- Blocks above the first `page` statement get the `--page` or project default.
- Placement is positional. Blocks from an expression land on the page the expression is written under.
  Blocks from a template land on the page of the instance. `page` is not allowed in a template body.
- The statement is authoritative on every compile:
  - A pin that still matches a page with the declared title is kept. Titles need not be unique, and this
    keeps adopted output byte-faithful across the corpus.
  - Any other pin moves to the first matching page in document order.
  - A title that names no page is an error. `page "Title" (room: …)` creates the page
    ([D56](0056-creating-pages.md)).
- Decompile writes `page` statements as section headers. Per-page modules and fragments open with theirs.
  The periphery is not a page and keeps a comment.
- The order of fragments matters for `page`. The convention is that every fragment opens with its `page`
  statement ([language.md](../language.md)). [D84](0084-page-per-fragment.md) ends a `page` at the end
  of its fragment.
