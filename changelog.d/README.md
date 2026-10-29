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

An `added` fragment makes the release a minor one unless a `release:*` PR label says otherwise. A PR that
changes only docs or CI is not released by itself; its fragment waits for the next release. CI checks the file
names (`prepare_release.py --check`); `python3 .github/scripts/prepare_release.py --dry-run` shows the next
release and its notes.
