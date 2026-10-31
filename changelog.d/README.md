# Changelog fragments

Every PR puts its changelog entry here as a new file instead of editing `CHANGELOG.md`. New files never
conflict, so PRs no longer collide in the changelog. The next release (`.github/scripts/prepare_release.py`,
run for every green push to main) writes the entries into `CHANGELOG.md` under the new version, grouped by
section, and deletes the files.

**Name:** `<topic>.<section>.md`. The topic only has to be unique; use the branch name (`/` → `-`). The section
is one of `added`, `changed`, `deprecated`, `removed`, `fixed`, `security`:

```
changelog.d/fix-upload-sps-new.fixed.md
changelog.d/stream-reconnect.added.md
```

**Content:** the entry as it should read in the changelog, for users: one bullet (several, or sub-bullets, are
fine too).

```markdown
- `lox config upload` uploads as `/prog/sps_new.zip`, the file the Miniserver loads on its next start
```

**Style** (the GitHub release shows the entries as they are, under `## ✨ New`, `## 🚀 Improvements`,
`## 🐛 Bug Fixes`, …; wrapped lines are joined): lead with the command or feature in bold, then say in a sentence
or two what users get. Keep a bullet under about 300 characters and leave out internal IDs (`D42`, oracle
sessions); see [WRITING_GUIDELINES.md](../WRITING_GUIDELINES.md). Sub-bullets and a short code block of example
commands are welcome for a bigger feature.

```markdown
- **`lox stream --reconnect`** keeps streaming across Miniserver reboots and dropped connections
  - The gap is reported on stderr and, with `-o json`, as `{"event":"disconnected"}` lines
```

**Summary (optional):** `<topic>.summary.md` gives the release its tagline and intro. The first line is the
tagline, short and lowercase (`users and door access`); it becomes the version heading and the GitHub release
title `vX.Y.Z — users and door access`. The rest is the intro, one or two sentences on what the release means to
users. Several summaries in one release are joined. Without one the title is the bare version.

```markdown
users and door access

See who can open which door. `lox` reads the Miniserver's user management and the access logs of NFC Code
Touches.
```

`python3 .github/scripts/prepare_release.py --notes vX.Y.Z` prints a released version's GitHub notes.

An `added` fragment makes the release a minor one unless a `release:*` PR label says otherwise. A PR that
changes only docs or CI is not released by itself; its fragment waits for the next release.

CI (`prepare_release.py --check`) checks the file names and fails a PR that writes under `[Unreleased]` in
`CHANGELOG.md`. Git merges such a PR by text, so after a release it would put the entry into the version just
released, without a conflict. Corrections to released sections are fine.
`python3 .github/scripts/prepare_release.py --dry-run` shows the next release and its notes.
