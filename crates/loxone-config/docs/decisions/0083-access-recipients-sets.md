# D83: Access rights and recipients are named sets

Status: accepted (2026-11-01), amends [D49](0049-nfc-access-rights.md) and
[D51](0051-push-notifications.md)

## Context

Five sections write content onto a block: tiles, schedules, moods, access rights and recipients. Three
of them were named sets that blocks claim (`tiles kueche … end`, `flex.tiles = kueche`). `access` and
`recipients` were attached to the block itself (`access haustuer … end`), so two Code Touches with the
same residents repeated the list, a template could not take one as a parameter, and the parser kept a
separate code path for each section.

## Decision

**One pattern.** `access <name> … end` and `recipients <name> … end` declare a named set. A claim
`<block>.access = <name>` or `<block>.recipients = <name>` makes it the block's rights or recipients.
Several blocks may claim one set; a block takes one claim of each kind. What a claim writes is what the
attached section wrote: compile output does not change.

**The old form is an error with the replacement.** A set named like an extern or block is the attached
form. It is refused with "Write `access haustuer_access` instead of `access haustuer`, and claim it with
`haustuer.access = haustuer_access`." The language server applies both edits as one quick fix. A set may
not share a name with an object anyway (one namespace), so the error never rejects a valid file.

**One section parser.** The five set kinds share one head parser, one body state and one claim rule
(`<slug>.<kind> = <name>`). The kind keyword starts a section only when no `=`, `.` or `<-` follows it,
so `access` and `recipients` stay usable as slugs, as D49 required.

**Set parameters take them.** `rechte: access` and `an: recipients` are set parameter kinds, claimed in
a template body like the other three.

**Decompile and adopt** lift each Code Touch's rights into a set `<slug>_access` and a claim. Code
Touches with the same rights share the set of the first one, as moods do.

## Rejected

- **Accepting both forms.** Two spellings for one thing is what D80 removed.
- **Reserving `access` and `recipients`.** It would turn existing slugs of that name into errors.
- **A set per block on decompile with no sharing.** Equal sets would come back as copies a reader has to
  compare by eye.
