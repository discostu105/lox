#!/usr/bin/env python3
"""Fail on design decision IDs (D42) in text users read. See WRITING_GUIDELINES.md, rule 4.

Checked: Rust string literals and /// //! docs under src/ and crates/*/src/, and the user docs.
Not checked: // comments, tests, the decision files themselves, CHANGELOG.md (history).
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
# D42, but not a hex byte in an NFC ID like "04 D4 E5".
ID = re.compile(r"(?<![0-9A-F] )\bD\d{1,3}\b(?! [0-9A-F]{2}\b)")

DOCS = [
    "README.md",
    "COMMANDS.md",
    "docs/**/*.md",
    "changelog.d/*.md",
    "crates/*/README.md",
    "crates/loxone-config/docs/language.md",
    "crates/loxone-config/docs/agents.md",
    "crates/loxone-config/skills/**/*.md",
]
SKIP = {"changelog.d/README.md"}


def line_of(text, pos):
    return text.count("\n", 0, pos) + 1


def rust_text(src):
    """Yield (offset, text) for each string literal and /// or //! doc line; skip // comments and chars."""
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith("//", i):
            end = src.find("\n", i)
            end = n if end < 0 else end
            if src.startswith(("///", "//!"), i):
                yield i, src[i:end]
            i = end
        elif src.startswith("/*", i):
            end = src.find("*/", i + 2)
            i = n if end < 0 else end + 2
        elif c == "'":
            # A char literal ('"', '\'', 'x') or a lifetime ('a).
            m = re.match(r"'(?:\\.|[^'\\])'", src[i:])
            i += len(m.group(0)) if m else 1
        elif c == "r" and re.match(r'r#*"', src[i:]) and (i == 0 or not src[i - 1].isalnum()):
            hashes = re.match(r"r(#*)\"", src[i:]).group(1)
            close = '"' + hashes
            start = i + 2 + len(hashes)
            end = src.find(close, start)
            end = n if end < 0 else end
            yield start, src[start:end]
            i = end + len(close)
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            yield i, src[i : j + 1]
            i = j + 1
        else:
            i += 1


def check_rust(path, text, hits):
    # Test modules may quote IDs; stop at the first #[cfg(test)].
    cut = text.find("#[cfg(test)]")
    body = text if cut < 0 else text[:cut]
    for pos, chunk in rust_text(body):
        for i in ID.finditer(chunk):
            hits.append(f"{path}:{line_of(body, pos + i.start())}: {i.group(0)}")


def main():
    hits = []
    for path in sorted(ROOT.glob("src/**/*.rs")) + sorted(ROOT.glob("crates/*/src/**/*.rs")):
        check_rust(path.relative_to(ROOT), path.read_text(encoding="utf-8"), hits)
    for pattern in DOCS:
        for path in sorted(ROOT.glob(pattern)):
            rel = path.relative_to(ROOT)
            if str(rel) in SKIP:
                continue
            for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                for i in ID.finditer(line):
                    hits.append(f"{rel}:{n}: {i.group(0)}")
    if hits:
        print("Design decision IDs in user-facing text (state the rule instead):")
        print("\n".join(hits))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
