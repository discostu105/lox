# D87: Tooling for v2: unit-suffix lint, lint --fix, scoped names in the language server

Status: accepted (2026-11-01), amended by [D125](0125-fmt-short-values-comments-fix-in-place.md)

## Context

The v2 changes ([D80](0080-v2-one-spelling.md) to [D86](0086-v2-polish.md)) gave the language its
final spelling, but the tools lagged behind:

- Decompile writes time values with their unit ([D85](0085-defaults.md)), hand-written modules kept
  bare seconds (`Delay: 10800`), and nothing pointed at them.
- `loxl lint` printed findings in source order, mixed by kind. On a house config with two hundred
  dead outputs the one finding that mattered was hard to see.
- An error of a directory module's check named no file, so finding the line meant a search.
- The language server knew `inst.Port` but not `inst.block` or `inst.block.Port`
  ([D82](0082-template-instance-names.md)): no hover, no definition, no port completion.
  It completed no argument keys.

## Decision

**The `unit-suffix` lint uses decompile's rule.** A bare number on a port documented in seconds or
minutes is a finding when decompile would write it with a suffix: the largest of `h`, `min`, `s`,
`ms` that gives a whole number, and only when the suffixed value compiles back to the same digits.
Zero and ports in other units stay bare. One rule means a decompiled module has no findings and a
fixed module reads like a decompiled one. The lint covers blocks, `defaults` and template bodies.

**`loxl lint --fix` writes the suffixes.** It rewrites each fragment in canonical form (the form
`loxl fmt` writes). In a `lox.toml` project it compiles before and after and writes nothing if the
config or the lockfile would differ, since a suffix only changes the spelling. Outside a project the
rule's exactness is the guarantee. Only `unit-suffix` is fixable: the other findings need a decision
(delete the extern, or keep it).

**Findings grouped by kind.** `loxl lint` prints each kind with its count, then its findings.
`--summary` prints the counts only, for a quick look at a large config and for CI logs. The last
line says how many findings `--fix` would fix.

**Module errors carry file and line.** A check or compile error of a directory module is located
the way the language server locates diagnostics: the backtick span of the message is searched in the
fragments. `check --json` gives `path` and `line`. Parse errors already had both.

**The language server follows scoped names.** Hover and go to definition resolve `inst.block` and
`inst.block.Port` to the template body's block; hover says which instance and the compiled name.
Port completion works after `inst.block.`. Inside an argument list it completes the keys: a block
type's inputs and parameters, a template's parameters, a `defaults` statement's parameters; in
value position it offers signal names. Two lints get quick fixes: `unit-suffix` replaces the value,
`default-page` inserts the page line of the file before.

The compile output does not change.

## Rejected

- **Fixing in place without `fmt`.** Rewriting just the number would keep the file's layout, but
  needs spans the AST does not keep. The canonical form is what `fmt` writes anyway.
- **Leaving seconds bare.** `Delay: 58` already is seconds on most ports, but not on a minutes port,
  and the reader cannot tell which. `58s` says it; decompile writes it too, so there is one rule.
- **Fixing other lints.** Deleting an unused extern or a dead block is a change of intent.
