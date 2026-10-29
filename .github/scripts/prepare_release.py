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
  is minor when the CHANGELOG's Unreleased section has an `### Added`
  entry (a new feature), else patch. A Cargo.toml version already above
  the last tag is released as it is.

Then it sets the version in Cargo.toml and Cargo.lock and moves the
Unreleased section under the new version (listing the PR titles when
Unreleased is empty). It prints the tag and, in Actions, writes `tag=…` to
$GITHUB_OUTPUT. With nothing to release it leaves every file alone.

`--dry-run` prints the decision and changes nothing. Needs `gh` (with
GH_TOKEN in Actions) for the PR labels and titles.
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


def unreleased(changelog):
    """(start, end) of the Unreleased section's body."""
    m = re.search(r"^## \[Unreleased\][^\n]*\n", changelog, re.M)
    if not m:
        sys.exit("CHANGELOG.md has no `## [Unreleased]` section")
    rest = re.search(r"^## \[", changelog[m.end():], re.M)
    return m.end(), m.end() + rest.start() if rest else len(changelog)


def release_changelog(changelog, version, units):
    start, end = unreleased(changelog)
    body = changelog[start:end].strip()
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
    labeled = any(label.startswith("release:") for u in units for label in u["labels"])
    if not labeled and re.search(r"^### Added\s*\n+- ", changelog[start:end], re.M):
        level = max(level, "minor", key=LEVELS.index)
    with open("Cargo.toml", encoding="utf-8") as f:
        current = parse_version(package_version(f.read()).group(1))
    last = parse_version(tag)
    version = fmt(current if current > last else bump(last, level))
    print(f"Release v{version} ({level if current <= last else 'version from Cargo.toml'}, last {tag}).")
    if dry_run:
        return
    set_versions(version)
    with open("CHANGELOG.md", "w", encoding="utf-8") as f:
        f.write(release_changelog(changelog, version, units))
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as f:
            f.write(f"tag=v{version}\n")


if __name__ == "__main__":
    main()
