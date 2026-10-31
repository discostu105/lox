# D32: A ref matched by what it mirrors

Status: accepted (2026-08-25)

## Context

A ref's `Ref=` attribute names the UUID of the object it mirrors (see
[loxone-format.md](../loxone-format.md#refs-c-typeinputrefoutputref-ref)). A `uuid:` pin on a ref extern says
nothing a reader can check. The house sources had 113 such pinned `InputRef`/`OutputRef` externs.

## Decision

`extern status_alarm_ref = InputRef(mirrors: status_alarm)` finds the ref through its target.

- The target must be nameable: a managed block with a locked identity, or an extern matched by name.
- Duplicate refs of one target usually sit on different pages. The declaring file's `page` statement
  ([D28](0028-page-statement.md)) narrows the candidates.
- A match that is still ambiguous is refused. Two mirrors of one flag on one page keep their `uuid:` pins.
- Changing the matcher kind on a pinned extern confirms the pinned object before the lock records the new kind.
  Converting a pin to `mirrors:` is verified, not taken on faith.
- A pinned `mirrors:` extern re-confirms on every compile. A title may drift under its pin, but "this ref
  mirrors X" is a claim in source. A ref that the GUI re-pointed elsewhere is drift worth stopping on.

Decompile and adopt emit `mirrors:` when the target has a slug in the module and the ref is the target's only
mirror of that type. For periphery targets that no slug can name, the `# mirrors …` note stays.

[D33](0033-minted-mirrors.md) uses the same binding to create refs.
