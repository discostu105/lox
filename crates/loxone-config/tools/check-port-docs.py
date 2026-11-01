#!/usr/bin/env python3
"""Check the port descriptions in src/port_doc.rs against Loxone's pages.

Every entry in src/port_doc.rs names the port's abbreviation on the type's
knowledge-base page (reference `kb-block-pages` in docs/references.json,
the page `blockdoc::kb_page` gives). This script downloads those pages
into refs/kb/ at the repository root (gitignored, like every reference),
reads their Inputs, Outputs and Parameters tables, and checks that each
abbreviation is a row of the page:

    tools/check-port-docs.py            download missing pages, check
    tools/check-port-docs.py --refetch  download every page again, check

It prints, per type, how many ports have a description and the rows of
the page no entry uses (candidates for a new entry, once the port they
belong to is known). Exit 1 when an abbreviation is not on its page.
"""

import html
import re
import sys
import urllib.request
from pathlib import Path

CRATE = Path(__file__).resolve().parent.parent
REPO = CRATE.parent.parent
DEST = REPO / "refs" / "kb"
# loxone.com answers 403 to a bare client; a browser user agent passes.
UA = {
    "User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/130.0 Safari/537.36"
}


def builtin_ports() -> dict[str, list[str]]:
    """Type -> port keys, from src/connectors.rs's const tables."""
    src = (CRATE / "src" / "connectors.rs").read_text(encoding="utf-8")
    consts = {
        m.group(1): re.findall(r'p\("([^"]+)", \w+\)', m.group(2))
        for m in re.finditer(r"const ([A-Z0-9_]+): &\[PortSpec\] = &\[(.*?)\];", src, re.S)
    }
    start = src.index("pub fn builtin(")
    body = src[start : src.index("\n}\n", start)]
    out = {}
    for arm in re.finditer(r"((?:\"\w+\"\s*\|?\s*)+)=> Some\((\w+)\)", body):
        for t in re.findall(r'"(\w+)"', arm.group(1)):
            out[t] = consts[arm.group(2)]
    return out


def kb_pages() -> dict[str, str]:
    src = (CRATE / "src" / "blockdoc.rs").read_text(encoding="utf-8")
    body = src[src.index("pub fn kb_page(") :]
    body = body[: body.index("\n}\n")]
    out = {}
    for arm in re.finditer(r'((?:"\w+"\s*\|?\s*)+)=> "([a-z0-9-]+)"', body):
        for t in re.findall(r'"(\w+)"', arm.group(1)):
            out[t] = arm.group(2)
    return out


def port_docs() -> dict[str, list[tuple[str, str]]]:
    """Type -> [(key, abbreviation)] from src/port_doc.rs."""
    src = (CRATE / "src" / "port_doc.rs").read_text(encoding="utf-8")
    src = src[: src.index("#[cfg(test)]")]
    entry = r'\(\s*"([^"]+)",\s*"([^"]+)",'
    consts = {
        m.group(1): re.findall(entry, m.group(2))
        for m in re.finditer(r"const ([A-Z0-9_]+): &\[Entry\] = &\[(.*?)\n\];", src, re.S)
    }
    body = src[src.index("fn entries(") :]
    out = {}
    # A type's entries span lines, or fit on one (`"StateV" => &[(…)],`).
    arm = r'"(\w+)" => (?:&\[(\([^\n]*\))\](?=,\n)|&\[(.*?)\n        \]|([A-Z0-9_]+)),'
    for m in re.finditer(arm, body, re.S):
        listed = m.group(2) or m.group(3)
        out[m.group(1)] = re.findall(entry, listed) if listed else consts[m.group(4)]
    return out


def text(s: str) -> str:
    s = re.sub(r"<br\s*/?>", " ", s)
    return re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", "", s))).strip()


def rows(page: str) -> list[str]:
    """The abbreviations of the Inputs, Outputs and Parameters tables."""
    out = []
    for sec in ("Input", "Output", "Parameter"):
        m = re.search(r'<h2 id="%s">.*?</h2>(.*?)(?:<h2|$)' % sec, page, re.S)
        if not m:
            continue
        for row in re.findall(r"<tr[^>]*>(.*?)</tr>", m.group(1), re.S):
            cell = re.search(r"<td[^>]*>(.*?)</td>", row, re.S)
            if cell:
                out.append(text(cell.group(1)))
    return out


def family(abbr: str):
    m = re.fullmatch(r"(.*?)(\d+)-(\d+)", abbr)
    return (m.group(1), int(m.group(2)), int(m.group(3))) if m else None


def covered(abbr: str, row: str) -> bool:
    """`V1` is covered by the row `V1-2`, `Lc1-8` by `Lc1-8`."""
    if abbr == row:
        return True
    r = family(row)
    if not r:
        return False
    a = family(abbr)
    if a:
        return a[0] == r[0] and r[1] <= a[1] and a[2] <= r[2]
    m = re.fullmatch(re.escape(r[0]) + r"(\d+)", abbr)
    return bool(m) and r[1] <= int(m.group(1)) <= r[2]


def on_page(abbr: str, page_rows: list[str]) -> bool:
    """A row covers the abbreviation, or rows cover every member of a family."""
    if any(covered(abbr, r) for r in page_rows):
        return True
    f = family(abbr)
    return bool(f) and all(
        any(covered(f"{f[0]}{n}", r) for r in page_rows) for n in range(f[1], f[2] + 1)
    )


def fetch(slug: str, again: bool) -> str:
    path = DEST / f"{slug}.html"
    if again or not path.exists():
        req = urllib.request.Request(f"https://www.loxone.com/enen/kb/{slug}/", headers=UA)
        with urllib.request.urlopen(req, timeout=60) as r:
            path.write_bytes(r.read())
    return path.read_text(encoding="utf-8")


def main() -> int:
    args = set(sys.argv[1:])
    if args - {"--refetch"}:
        print(__doc__, file=sys.stderr)
        return 2
    DEST.mkdir(parents=True, exist_ok=True)
    ports, pages, docs = builtin_ports(), kb_pages(), port_docs()
    problems = 0
    total = described = 0
    for t, keys in ports.items():
        entries = docs.get(t, [])
        n = 0
        for k in keys:
            if any(k == key or covered(k, key) for key, _ in entries):
                n += 1
        total += len(keys)
        described += n
        if not entries:
            continue
        slug = pages.get(t)
        if not slug:
            print(f"FAIL  {t}: entries but no knowledge-base page")
            problems += 1
            continue
        page_rows = rows(fetch(slug, "--refetch" in args))
        bad = [a for _, a in entries if not on_page(a, page_rows)]
        unused = [
            r for r in page_rows if not any(covered(a, r) or on_page(r, [a]) for _, a in entries)
        ]
        status = "FAIL" if bad else "ok"
        problems += bool(bad)
        line = f"{status:<5} {t:<22} {n:>3}/{len(keys):<3} kb {slug}"
        if bad:
            line += f"  not on the page: {' '.join(bad)}"
        if unused:
            line += f"  unused rows: {' '.join(unused)}"
        print(line)
    print(f"{described} of {total} ports described, {len(docs)} of {len(ports)} types")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
