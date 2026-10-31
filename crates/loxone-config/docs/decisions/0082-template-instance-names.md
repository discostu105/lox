# D82: Template instances are named by the instance

Status: accepted (2026-11-01), amends [D23](0023-templates.md)

## Context

A template instance's blocks were named by their expanded slugs: body block `ausgang` of instance `flur`
was `flur_ausgang` in source as in the lock. One real config had about 110 such references. They leak
the expansion into source, a rename of the body slug breaks every caller, and the reader has to know
the joining rule. Sets (`moods`, `schedule`, `tiles`) went into a template as string value parameters,
so a typo surfaced only at compile.

## Decision

**Scoped names.** Source names an instance's blocks through the instance: `flur.taster.Q` for body
block `taster`. The lexer joins `a.b.` into one scoped name, and the resolver maps `inst.block` to the
lock key `inst_block`. Lock keys are unchanged, so existing locks and compile output stay identical.

**Primary block.** A template has a primary block: its only block, or the one `template t(…) -> <block>`
names. The instance itself names it: `flur.Q`. There is one spelling; `flur.ausgang.Q` for the primary
block is an error that names `flur.Q`.

**The old spelling is an error.** `flur_ausgang.Q` in source is refused with "Write `flur.Q` instead of
`flur_ausgang.Q`." The language server offers the replacement as a quick fix and completes `flur.` with
the instance's other blocks and the primary block's ports. `removed` and `moved` name lock keys and
keep accepting the expanded slug.

**Instance names.** An instance name that collides with another slug or with an expanded slug is an error.

**Set parameters.** `name: moods|schedule|tiles [= <set>]` declares a set parameter. The argument is the
set's bare name and is checked against the sets of that kind. A string for a set is an error with the
replacement. Object parameters take no default. Schedule and tile claims work in template bodies.

**Tools follow.** `loxl adopt` and `loxl rename` write scoped names, and `loxl test` reports blocks as
source spells them. Compile errors still name blocks by their lock keys.

## Rejected

- **Accepting both spellings.** Two names for one block is what D80 removed for values.
- **`inst.primary.Port` as a second spelling** of the primary block. Same reason.
- **Changing the lock keys** to the scoped form. It would re-key every existing lock for no gain.

Next: `access` and `recipients` sets as parameter kinds.
