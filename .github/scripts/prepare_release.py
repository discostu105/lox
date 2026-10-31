#!/usr/bin/env python3
"""Prepare an automatic release (run by .github/workflows/auto-release.yml).

Looks at what landed on main since the last vX.Y.Z tag (first-parent
commits: one per merged PR or direct push) and decides whether it needs a
release, and how big a bump:

- Labels on the merged PR decide, or the same words in brackets in a
  direct commit's message (`[release:minor]`): `release:major`,
  `release:minor`, `release:patch`, `no-release`.
- Without one, a change that touches only docs, CI or the Homebrew formula
  needs no release; anything else does.
- The bump is the largest one asked for. Without any `release:*` label it
  is minor when the release notes have an `### Added` entry (a new
  feature), else patch. A Cargo.toml version already above the last tag is
  released as it is.

The release notes are the changelog fragments, one file per change in
changelog.d/ (`<topic>.<section>.md`, see changelog.d/README.md), merged
by section with whatever CHANGELOG.md still has under Unreleased; the PR
titles when both are empty. New files never conflict, so PRs do not edit
CHANGELOG.md.

Then it sets the version in Cargo.toml and Cargo.lock, writes the notes
into CHANGELOG.md under the new version and deletes the fragments. It
prints the tag and, in Actions, writes `tag=…` to $GITHUB_OUTPUT. With
nothing to release it leaves every file alone.

A `<topic>.summary.md` fragment is optional: its first line is the
release's tagline (the version heading and the GitHub release title,
`vX.Y.Z — tagline`), the rest the intro above the sections.

`--notes vX.Y.Z` prints the GitHub release notes of a released version
from CHANGELOG.md (release.yml runs it): the intro, then the sections
under the release headings (`### Added` → `## ✨ New`, …), lines
unwrapped. In Actions it writes the title to $GITHUB_OUTPUT as `name=…`.

`--dry-run` prints the decision and the notes and changes nothing.
`--check` only checks the fragments' names and that CHANGELOG.md has
nothing under Unreleased (CI runs it on every PR). Needs
`gh` (with GH_TOKEN in Actions) for the PR labels and titles.
"""

import datetime
import json
import os
import re
import subprocess
import sys

LEVELS = ["none", "patch", "minor", "major"]
# Changes to these alone do not change the binary.
NO_RELEASE_PATHS = re.compile(
    r"(\.md$|^docs/|^crates/loxone-config/docs/|^\.github/|^Formula/|^LICENSE|^codecov\.yml$|^k8s/|^scripts/demo-video/)"
)
# Commits the release flow itself pushes.
BOT_SUBJECTS = re.compile(r"^(Release v\d|brew: update formula)")
# Keep a Changelog's sections, in its order.
SECTIONS = ["Added", "Changed", "Deprecated", "Removed", "Fixed", "Security"]
FRAGMENT_DIR = "changelog.d"
# The optional tagline and intro of a release, not a changelog section.
SUMMARY = "Summary"
FRAGMENT_NAME = re.compile(
    r"^[\w.-]+\.(" + "|".join(s.lower() for s in SECTIONS + [SUMMARY]) + r")\.md$"
)
# The headings the GitHub release uses for the changelog's sections.
RELEASE_HEADINGS = {
    "Added": "✨ New",
    "Changed": "🚀 Improvements",
    "Deprecated": "⚠️ Deprecated",
    "Removed": "🗑️ Removed",
    "Fixed": "🐛 Bug Fixes",
    "Security": "🔒 Security",
}


def git(*args):
    return subprocess.run(
        ["git", *args], check=True, capture_output=True, text=True
    ).stdout


def parse_version(v):
    return tuple(int(x) for x in v.lstrip("v").split("."))


def bump(version, level):
    major, minor, patch = version
    if level == "major":
        return (major + 1, 0, 0)
    if level == "minor":
        return (major, minor + 1, 0)
    return (major, minor, patch + 1)


def fmt(version):
    return ".".join(str(x) for x in version)


def pull_requests(sha):
    """The merged PR(s) that brought this commit to main."""
    repo = os.environ.get("GITHUB_REPOSITORY") or "{owner}/{repo}"
    result = subprocess.run(
        [
            "gh", "api", f"repos/{repo}/commits/{sha}/pulls",
            "--jq", "[.[] | select(.merged_at != null) | {number, title, labels: [.labels[].name]}]",
        ],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        # In Actions a missed label must not go unnoticed; locally, commits
        # not pushed yet have no PR.
        if os.environ.get("GITHUB_ACTIONS"):
            sys.exit(f"Looking up the PR of {sha} failed: {result.stderr.strip()}")
        print(f"(no PR lookup for {sha[:8]}: {result.stderr.strip()})", file=sys.stderr)
        return []
    return json.loads(result.stdout or "[]")


def changes_since(tag):
    """One entry per first-parent commit: its PR (if any), labels, files."""
    log = git("log", "--first-parent", "--format=%H%x1f%s%x1f%b%x1e", f"{tag}..HEAD")
    units = []
    seen_prs = set()
    for record in log.split("\x1e"):
        record = record.strip("\n")
        if not record:
            continue
        sha, subject, body = record.split("\x1f")
        if BOT_SUBJECTS.match(subject):
            continue
        prs = pull_requests(sha)
        if any(p["number"] in seen_prs for p in prs):
            continue  # a further commit of a rebase-merged PR, counted already
        seen_prs.update(p["number"] for p in prs)
        labels = {name for p in prs for name in p["labels"]}
        labels |= set(re.findall(r"\[(release:(?:major|minor|patch)|no-release)\]", subject + "\n" + body))
        files = git("diff", "--name-only", f"{sha}^1", sha).split()
        title = prs[0]["title"] if prs else subject
        number = prs[0]["number"] if prs else None
        units.append({"sha": sha, "title": title, "pr": number, "labels": labels, "files": files})
    return units


def level_of(unit):
    labels = unit["labels"]
    for level in ("major", "minor", "patch"):
        if f"release:{level}" in labels:
            return level
    if "no-release" in labels:
        return "none"
    if all(NO_RELEASE_PATHS.search(f) for f in unit["files"]):
        return "none"
    return "patch"


def fragments():
    """The changelog.d entries as (path, section, text), oldest first (in
    the order they were added to git; files not committed yet last)."""
    if not os.path.isdir(FRAGMENT_DIR):
        return []
    added = git("log", "--reverse", "--diff-filter=A", "--format=", "--name-only", "--", FRAGMENT_DIR)
    order = {path: i for i, path in enumerate(added.split())}
    entries, errors = [], []
    for name in sorted(os.listdir(FRAGMENT_DIR)):
        if name == "README.md":
            continue
        path = f"{FRAGMENT_DIR}/{name}"
        m = FRAGMENT_NAME.match(name)
        if not m:
            sections = ", ".join(s.lower() for s in SECTIONS + [SUMMARY])
            errors.append(f"{path}: name it <topic>.<section>.md, the section one of {sections}")
            continue
        with open(path, encoding="utf-8") as f:
            text = f.read().strip()
        if not text:
            errors.append(f"{path}: empty")
            continue
        if m.group(1) != SUMMARY.lower() and not text.startswith("- "):
            text = "- " + text
        entries.append((path, m.group(1).capitalize(), text))
    if errors:
        sys.exit("\n".join(errors))
    entries.sort(key=lambda e: order.get(e[0], len(order)))
    return entries


def summary(entries):
    """(tagline, intro) from the summary fragments: their first lines joined
    into one tagline, the rest into the intro."""
    taglines, intros = [], []
    for _, section, text in entries:
        if section == SUMMARY:
            first, _, rest = text.partition("\n")
            taglines.append(first.strip().rstrip("."))
            if rest.strip():
                intros.append(rest.strip())
    return "; ".join(taglines), "\n\n".join(intros)


def release_notes(body, entries):
    """The Unreleased section's own text merged with the fragments, by
    section; the summary's intro goes first."""
    preamble, sections, current = [], {}, None
    for line in body.splitlines():
        m = re.match(r"^### (.+?)\s*$", line)
        if m:
            current = m.group(1)
            sections.setdefault(current, [])
        elif current is None:
            preamble.append(line)
        else:
            sections[current].append(line)
    for lines in sections.values():
        while lines and not lines[-1].strip():
            lines.pop()
    for _, section, text in entries:
        if section != SUMMARY:
            sections.setdefault(section, []).append(text)
    parts = [summary(entries)[1], "\n".join(preamble).strip()]
    for section in SECTIONS + [s for s in sections if s not in SECTIONS]:
        text = "\n".join(sections.get(section, [])).strip()
        if text:
            parts.append(f"### {section}\n{text}")
    return "\n\n".join(p for p in parts if p)


def check():
    """What CI checks on every PR: the fragments' names, and that nothing is
    written under [Unreleased]. A PR that adds entries there is merged by
    text, so once a release commit moved the section under a version, the
    entries land in that version's notes, without a conflict."""
    entries = fragments()
    with open("CHANGELOG.md", encoding="utf-8") as f:
        changelog = f.read()
    start, end = unreleased(changelog)
    if changelog[start:end].strip():
        sys.exit(
            "CHANGELOG.md: [Unreleased] must stay empty. Put the entry in a new file "
            f"{FRAGMENT_DIR}/<branch>.<section>.md instead (see {FRAGMENT_DIR}/README.md)."
        )
    print(f"{len(entries)} changelog fragment(s) in {FRAGMENT_DIR}/, names OK; [Unreleased] empty")


def unreleased(changelog):
    """(start, end) of the Unreleased section's body."""
    m = re.search(r"^## \[Unreleased\][^\n]*\n", changelog, re.M)
    if not m:
        sys.exit("CHANGELOG.md has no `## [Unreleased]` section")
    rest = re.search(r"^## \[", changelog[m.end():], re.M)
    return m.end(), m.end() + rest.start() if rest else len(changelog)


def release_changelog(changelog, version, units, notes, tagline):
    start, end = unreleased(changelog)
    body = notes
    if not body:
        lines = [
            f"- {u['title']}" + (f" (#{u['pr']})" if u["pr"] else "")
            for u in units
            if level_of(u) != "none"
        ]
        body = "### Changed\n" + "\n".join(lines)
    date = datetime.datetime.now(datetime.timezone.utc).date().isoformat()
    heading = f"## [{version}] — {date}" + (f" — {tagline}" if tagline else "")
    return f"{changelog[:start]}\n{heading}\n\n{body}\n\n{changelog[end:]}"


def unwrap(text):
    """Join the hard-wrapped continuation lines of the changelog: GitHub
    keeps every line break of a release body. Code blocks, sub-bullets and
    blank lines stay as they are."""
    out, in_code = [], False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("```"):
            in_code = not in_code
            out.append(line)
        elif in_code or not stripped:
            out.append(line)
        elif (
            line[0] == " "
            and out
            and out[-1].strip()
            and not out[-1].strip().startswith("```")
            and not re.match(r"^\s*([-*+]|\d+\.)\s", line)
        ):
            out[-1] = out[-1].rstrip() + " " + stripped
        else:
            out.append(line)
    return "\n".join(out)


def github_notes(changelog, tag):
    """(title, body) of the GitHub release of a version in CHANGELOG.md."""
    version = re.escape(tag.lstrip("v"))
    m = re.search(
        rf"^## \[{version}\]([^\n]*)\n(.*?)(?=^## \[|^\[[^\]]+\]: |\Z)", changelog, re.M | re.S
    )
    if not m:
        return tag, ""
    # "— 2026-10-30" or "— 2026-10-30 — tagline"
    tagline = m.group(1).split(" — ", 2)[2].strip() if m.group(1).count(" — ") >= 2 else ""
    title = f"{tag} — {tagline}" if tagline else tag
    lines = []
    for line in m.group(2).strip().splitlines():
        h = re.match(r"^### (.+?)\s*$", line)
        if h:
            line = f"## {RELEASE_HEADINGS.get(h.group(1), h.group(1))}"
            if lines and lines[-1].strip():
                lines.append("")
            lines.append(line)
            lines.append("")
        else:
            lines.append(line)
    return title, unwrap("\n".join(lines)).strip()


def package_version(cargo_toml):
    section = re.search(r"^\[package\]\n(.*?)(?=^\[|\Z)", cargo_toml, re.M | re.S)
    return re.search(r'^version = "([^"]+)"', section.group(1), re.M)


def set_versions(version):
    with open("Cargo.toml", encoding="utf-8") as f:
        toml = f.read()
    section = re.search(r"^\[package\]\n", toml, re.M)
    m = package_version(toml)
    at = section.end() + m.start(1)
    toml = toml[:at] + version + toml[at + len(m.group(1)):]
    with open("Cargo.toml", "w", encoding="utf-8") as f:
        f.write(toml)
    with open("Cargo.lock", encoding="utf-8") as f:
        lock = f.read()
    lock, n = re.subn(
        r'(\[\[package\]\]\nname = "lox"\nversion = ")[^"]+(")', rf"\g<1>{version}\g<2>", lock
    )
    if n != 1:
        sys.exit("Cargo.lock: no single lox package entry")
    with open("Cargo.lock", "w", encoding="utf-8") as f:
        f.write(lock)


def main():
    dry_run = "--dry-run" in sys.argv[1:]
    if "--check" in sys.argv[1:]:
        check()
        return
    if "--notes" in sys.argv[1:]:
        args = sys.argv[sys.argv.index("--notes") + 1:]
        if not args:
            sys.exit("--notes needs a tag (vX.Y.Z)")
        with open("CHANGELOG.md", encoding="utf-8") as f:
            title, body = github_notes(f.read(), args[0])
        print(body)
        print(f"Title: {title}", file=sys.stderr)
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as f:
                f.write(f"name={title}\n")
        return
    try:
        tag = git("describe", "--tags", "--abbrev=0", "--match", "v[0-9]*").strip()
    except subprocess.CalledProcessError:
        sys.exit("No vX.Y.Z tag to release from")
    units = changes_since(tag)
    for u in units:
        print(f"{level_of(u):>5}  {u['sha'][:8]}  {u['title']}" + (f"  [{', '.join(sorted(u['labels']))}]" if u["labels"] else ""))
    level = max((level_of(u) for u in units), key=LEVELS.index, default="none")
    if level == "none":
        print(f"Nothing to release since {tag}.")
        return
    with open("CHANGELOG.md", encoding="utf-8") as f:
        changelog = f.read()
    start, end = unreleased(changelog)
    entries = fragments()
    notes = release_notes(changelog[start:end], entries)
    tagline = summary(entries)[0]
    labeled = any(label.startswith("release:") for u in units for label in u["labels"])
    if not labeled and re.search(r"^### Added$", notes, re.M):
        level = max(level, "minor", key=LEVELS.index)
    with open("Cargo.toml", encoding="utf-8") as f:
        current = parse_version(package_version(f.read()).group(1))
    last = parse_version(tag)
    version = fmt(current if current > last else bump(last, level))
    print(f"Release v{version} ({level if current <= last else 'version from Cargo.toml'}, last {tag}).")
    if dry_run:
        if not notes:
            print("\n(no notes: the PR titles become the notes)")
            return
        preview = release_changelog(changelog, version, units, notes, tagline)
        title, body = github_notes(preview, f"v{version}")
        print(f"\nGitHub release: {title}\n\n{body}")
        return
    set_versions(version)
    with open("CHANGELOG.md", "w", encoding="utf-8") as f:
        f.write(release_changelog(changelog, version, units, notes, tagline))
    for path, _, _ in entries:
        os.remove(path)
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as f:
            f.write(f"tag=v{version}\n")


if __name__ == "__main__":
    main()
