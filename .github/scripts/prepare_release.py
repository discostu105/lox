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

`--dry-run` prints the decision and the notes and changes nothing.
`--check` only checks the fragments' names (CI runs it on every PR). Needs
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
    r"(\.md$|^docs/|^crates/lxir/docs/|^\.github/|^Formula/|^LICENSE|^codecov\.yml$|^k8s/|^scripts/demo-video/)"
)
# Commits the release flow itself pushes.
BOT_SUBJECTS = re.compile(r"^(Release v\d|brew: update formula)")
# Keep a Changelog's sections, in its order.
SECTIONS = ["Added", "Changed", "Deprecated", "Removed", "Fixed", "Security"]
FRAGMENT_DIR = "changelog.d"
FRAGMENT_NAME = re.compile(
    r"^[\w.-]+\.(" + "|".join(s.lower() for s in SECTIONS) + r")\.md$"
)


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
            sections = ", ".join(s.lower() for s in SECTIONS)
            errors.append(f"{path}: name it <topic>.<section>.md, the section one of {sections}")
            continue
        with open(path, encoding="utf-8") as f:
            text = f.read().strip()
        if not text:
            errors.append(f"{path}: empty")
            continue
        if not text.startswith("- "):
            text = "- " + text
        entries.append((path, m.group(1).capitalize(), text))
    if errors:
        sys.exit("\n".join(errors))
    entries.sort(key=lambda e: order.get(e[0], len(order)))
    return entries


def release_notes(body, entries):
    """The Unreleased section's own text merged with the fragments, by
    section."""
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
        sections.setdefault(section, []).append(text)
    parts = ["\n".join(preamble).strip()]
    for section in SECTIONS + [s for s in sections if s not in SECTIONS]:
        text = "\n".join(sections.get(section, [])).strip()
        if text:
            parts.append(f"### {section}\n{text}")
    return "\n\n".join(p for p in parts if p)


def unreleased(changelog):
    """(start, end) of the Unreleased section's body."""
    m = re.search(r"^## \[Unreleased\][^\n]*\n", changelog, re.M)
    if not m:
        sys.exit("CHANGELOG.md has no `## [Unreleased]` section")
    rest = re.search(r"^## \[", changelog[m.end():], re.M)
    return m.end(), m.end() + rest.start() if rest else len(changelog)


def release_changelog(changelog, version, units, notes):
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
    return f"{changelog[:start]}\n## [{version}] — {date}\n\n{body}\n\n{changelog[end:]}"


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
        print(f"{len(fragments())} changelog fragment(s) in {FRAGMENT_DIR}/, names OK")
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
    labeled = any(label.startswith("release:") for u in units for label in u["labels"])
    if not labeled and re.search(r"^### Added$", notes, re.M):
        level = max(level, "minor", key=LEVELS.index)
    with open("Cargo.toml", encoding="utf-8") as f:
        current = parse_version(package_version(f.read()).group(1))
    last = parse_version(tag)
    version = fmt(current if current > last else bump(last, level))
    print(f"Release v{version} ({level if current <= last else 'version from Cargo.toml'}, last {tag}).")
    if dry_run:
        print(f"\n{notes or '(no notes: the PR titles become the notes)'}")
        return
    set_versions(version)
    with open("CHANGELOG.md", "w", encoding="utf-8") as f:
        f.write(release_changelog(changelog, version, units, notes))
    for path, _, _ in entries:
        os.remove(path)
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as f:
            f.write(f"tag=v{version}\n")


if __name__ == "__main__":
    main()
