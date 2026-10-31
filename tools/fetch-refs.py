#!/usr/bin/env python3
"""Download the external references of docs/references.json into refs/.

The references stay with their publishers: refs/ is gitignored, only the
manifest (URL, sha256, size, date checked) is committed. For every entry
marked "pinned" the download is checked against the pinned sha256, so a
silently republished document is noticed:

  tools/fetch-refs.py            download what is missing, verify pins
  tools/fetch-refs.py --check    re-download everything pinned and verify
                                 (exit 1 on a changed or missing document)
  tools/fetch-refs.py --update   re-download everything pinned and write
                                 the new hashes into the manifest

Unpinned entries (web pages, repositories) are only listed: they change
all the time and are read in place.
"""

import datetime
import hashlib
import json
import sys
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "docs" / "references.json"
DEST = ROOT / "refs"
# loxone.com answers 403 to a bare client; a browser user agent passes.
UA = {
    "User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/130.0 Safari/537.36"
}


def get(url: str) -> bytes:
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def local_name(ref: dict) -> str:
    suffix = Path(urllib.parse.urlparse(ref["url"]).path).suffix or ".bin"
    return ref["id"] + suffix


def main() -> int:
    args = set(sys.argv[1:])
    unknown = args - {"--check", "--update"}
    if unknown:
        print(__doc__, file=sys.stderr)
        return 2
    refetch = bool(args)
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    DEST.mkdir(exist_ok=True)
    today = datetime.date.today().isoformat()
    problems = 0
    for ref in manifest["references"]:
        if not ref.get("pinned"):
            print(f"link  {ref['id']:<26} {ref['url']}")
            continue
        path = DEST / local_name(ref)
        if refetch or not path.exists():
            try:
                path.write_bytes(get(ref["url"]))
            except Exception as e:  # noqa: BLE001 - report and continue
                print(f"FAIL  {ref['id']:<26} {e}")
                problems += 1
                continue
        data = path.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        pinned = ref.get("sha256")
        if "--update" in args:
            if digest != pinned:
                print(f"new   {ref['id']:<26} {len(data):>9} bytes (was {pinned or 'unpinned'})")
            ref.update(sha256=digest, bytes=len(data), checked=today)
        elif pinned is None:
            print(f"UNPIN {ref['id']:<26} run --update to pin {digest[:12]}")
            problems += 1
        elif digest != pinned:
            print(f"DRIFT {ref['id']:<26} publisher changed the file: re-read it, then --update")
            problems += 1
        else:
            print(f"ok    {ref['id']:<26} {path.relative_to(ROOT)}")
    if "--update" in args:
        MANIFEST.write_text(
            json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )
        print(f"wrote {MANIFEST.relative_to(ROOT)}")
        return 0
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
