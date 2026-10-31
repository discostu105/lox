# Writing Guidelines

How we write text people read: CLI help, errors, command output, MCP tool descriptions, code comments, docs,
changelog entries, commits and PRs. [API_DESIGN_GUIDELINES.md](API_DESIGN_GUIDELINES.md) covers command
names, flags and output formats.

Short and plain beats complete. If a reader needs more, link to it.

## Rules

1. **Say what the reader does or gets.** Explain the internals only when the reader needs them to act.
2. **One idea per sentence, about 25 words at most.** If a sentence needs a `;`, two `—`, or a parenthesis
   inside a parenthesis, split it.
3. **Fact first, evidence last.** Put the source (corpus counts, a config version, a date) after the fact, or
   in the commit message.
4. **No internal IDs in user-facing text.** Leave design decision numbers (`D42`), oracle sessions, issue
   numbers and Rust paths out of help, errors and the changelog. Name the rule instead.
5. **Describe the current state.** Don't write "now", "used to", "since v0.40" or "previously". When
   behavior changes, rewrite the paragraph rather than appending a correction. History belongs in git and
   the changelog.
6. **Use the user's words.** Write "create" instead of "mint", and "Loxone Config" instead of "the oracle".
   Also avoid "fingerprint", "projection", "another writer", "corpus-observed" and "v0" in user text.
7. **Say it once.** A fact lives in one file; other files link to it.
8. **Lists and tables for lists.** Options, steps, cases and enumerations go in a list or table, not a
   sentence with semicolons.

## By kind

**CLI help.** The first line is a verb phrase. Give each flag one line: what it does and its default. Put
the reasons in the docs.

```
Before: Placement (D42): a new block goes where its wires want it on its page; a placed block keeps its
        position, and a move made in Loxone Config is taken over into the lockfile. --relayout <page>
        (repeatable) places that page's managed blocks afresh — except those wired to refs another
        writer owns, which stay with them.
After:  --relayout <page>  Re-place this page's blocks (repeatable). Blocks moved in Loxone Config stay put.
```

**Errors.** Say what is wrong, then what to do, in two sentences at most. Don't mention `docs/*.md`, since
binary users don't have the repo.

```
Before: base written by Loxone Config 16.1 but the lock pins 15.3 — qualify the release first
        (docs/design.md D22): one oracle open+save, then `loxl compile --accept-version`
After:  Config saved by Loxone Config 16.1, but the lockfile expects 15.3. Run `loxl compile --accept-version 16.1`.
```

**Code comments.** Explain why, not what; delete a comment that repeats the next line. Put evidence next to
the entry it backs (`// seen in Config 16 saves`), not in a paragraph above it. A module doc stays under
about 10 lines; the details go on the items.

**Public `///` docs** in `crates/` are rendered on docs.rs. Don't use decision IDs, session numbers or block
names from real configs there.

**Decision IDs in code.** Use them only in `//` comments, and only where the reasoning is too long to
repeat. Never use one as a noun ("the D34 pin").

**Docs.** Docs describe what is. When a plan or design doc is done, delete it, because git keeps it. Open
work goes to GitHub issues, not markdown checklists. A decision records the context, the choice and the
rejected alternatives. Spec text goes to `language.md` and format facts to `loxone-format.md`.

**Changelog fragments.** Keep each bullet under about 300 characters and write it in the user's words. Put
details in sub-bullets. See [changelog.d/README.md](changelog.d/README.md).

## Tells to avoid

These mark text as machine-written or padded:

- Clauses chained with `—`, and "X — except Y, which Z" constructions.
- Openers and intensifiers: "Crucially", "Note that", "Honest limits", "It's worth noting", "simply",
  "without ceremony", "not just X but Y".
- Sales words: robust, seamless, powerful, comprehensive, AI-ready, "zero dependencies".
- Hedging every statement, or ending a paragraph with a line that sums it up.
- Bold labels on every bullet, and lists of three where one item would do.
- Status banners and dates in the body of a doc.

## Before you commit

Read the text once as a new user would. Cut what they would skip. If you can delete a third of it and lose
nothing, delete it.
