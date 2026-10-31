#!/usr/bin/env python3
"""Fetch the public Loxone config corpus into corpus/web/ (gitignored).

Most files carry no license that allows redistributing them, so the corpus
stays local. What is committed is its manifest, tools/corpus-manifest.json:
for every file the source (pinned to a commit for GitHub), the sha256, the
size and the license the source states. Sources:

  1. Official Loxone KB sample files (zips, one .Loxone/.LoxPLAN each),
     scraped from the videos-sample-files index page.
  2. LoxWiki (Confluence) attachments, enumerated via the REST API.
  3. GitHub repositories with real projects/templates.

Usage:
  tools/fetch-corpus.py [dest-dir]
        Reproduce the corpus from the manifest: download every entry that
        is missing in dest-dir and verify each file's sha256. Exit 1 when a
        file could not be fetched or does not match.
  tools/fetch-corpus.py --discover [dest-dir]
        Search the sources afresh (newest GitHub commits, current KB and
        wiki attachments), download into dest-dir and rewrite the
        manifest. Review the manifest diff before committing it.

dest-dir defaults to corpus/web. GitHub API calls use GITHUB_TOKEN or
GH_TOKEN when set (unauthenticated: 60 requests an hour, enough for one
discovery run).
"""

import hashlib
import io
import json
import os
import re
import sys
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
MANIFEST = TOOLS / "corpus-manifest.json"
# loxone.com answers 403 to a bare client; a browser user agent passes.
UA = {
    "User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/130.0 Safari/537.36"
}
KB_INDEX = "https://www.loxone.com/enen/kb/videos-sample-files/"
WIKI = "https://loxwiki.atlassian.net/wiki"
EXTS = (".loxone", ".loxplan")

# repo -> known paths, the fallback when the tree listing fails. Every
# repo's tree is listed (catches renames and collections); ONOKOM/Templates
# and eisber/lox-cli are only listed (lox-cli's golden configs are Git-LFS
# blobs, resolved via media.githubusercontent.com).
GITHUB_FILES = {
    "benjaminellmer/mc-hba": [
        "Loxone/HB_SS23.Loxone",
        "Loxone/Hotel Room.Loxone",
        "KNX/HB_SS23.Loxone",
    ],
    "PXLDigital/Loxone-Mini-Server": ["Src/Loxone/LoxoneBoys House.Loxone"],
    "stefanpenzinger/hba-project": ["loxone/HBA-Hotel.Loxone"],
    "Jhonnay/Qt_MiniserverUpdater": ["EmptyProject.Loxone"],
    "dec112/sensors_iot": ["loxone/DEC4IoT-Demo.Loxone"],
    "5iggi/vlx2mqtt": ["LoxoneStatusTemplates/StatusBausteine_VLX2MQTT.Loxone"],
    "tobsch/lox-config": ["examples/minimal.Loxone"],
    "Smarteon/community": ["loxcall/loxcall.Loxone"],
    "mr-manuel/Loxone": ["SEVentilation/SEVentilation.Loxone"],
    "sevelm/InnoTune": ["InnoTune.Loxone"],
    "Project51At/PluggitAP190": ["PluggitAP190.Loxone"],
    "marcelzoller/loxberry-plugin-sureflap": ["SureFlap.Loxone"],
    "DerFlash/Loxone-Nibe-Gateway": ["Nibe.Loxone"],
    "dusanmsk/railduino-udp": ["railduino.Loxone"],
    "netdata-be/loxone": ["applamp.Loxone"],
    "ogglobi/FreeAir2Lox": ["FreeAir2Lox.Loxone"],
    "jonas-claes/growatt-inverter": ["growatt.Loxone"],
}
# Licenses GitHub cannot classify, read from the repository by hand.
LICENSE_OVERRIDES = {"eisber/lox-cli": "AGPL-3.0-only OR commercial (dual-licensed, see LICENSE)"}
GITHUB_TREE_REPOS = ["ONOKOM/Templates", "eisber/lox-cli", *GITHUB_FILES.keys()]

# LoxWiki pages known (2026-08-25) to carry .Loxone/.LoxPLAN attachments.
# CQL attachment search turned out unreliable, so the pages are pinned;
# each page's attachment list is enumerated via the REST API.
WIKI_PAGES = [
    1316061809,  # Fronius PV via Modbus
    1516634217,  # irrigation
    1517355031,  # Geofency presence
    1517355186,  # Kombi-Taster
    1517355333,  # Philips Hue
    1517355637,  # Helios ventilation via Modbus TCP
    1520140305,  # "Loxone Config Beispiele" index (TVs, AV receivers, Sonos)
    1520763236,  # RGB color cycle
    1520763536,  # KNV heat pump
    1521975986,  # Eintastenbedienung
    1522696264,  # aWATTar energy prices
    1522696540,  # intelligent heating curve
    1529021083,  # Lichtsteuerung Gen1
    1536591392,  # wind direction averaging
    1650327637,  # Sonos doorbell
    1755021316,  # Orno OR-WE-516 Modbus meter
]


def get(url: str) -> bytes:
    headers = dict(UA)
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if token and url.startswith("https://api.github.com/"):
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def get_json(url: str):
    return json.loads(get(url))


def looks_like_config(data: bytes) -> bool:
    head = data[:4096].lstrip(b"\xef\xbb\xbf")
    return head.startswith(b"<?xml") and b"<ControlList" in data[:65536]


def split_docs(data: bytes) -> list:
    """Some samples concatenate several documents into one file (e.g. the
    two-Miniserver communication sample: one XML declaration, two
    <ControlList> roots) — split so each part parses on its own."""
    parts = re.split(rb"(?=<ControlList[ >])", data)
    prolog, roots = parts[0], parts[1:]
    return [data] if len(roots) <= 1 else [prolog + r for r in roots]


def out_name(name: str, part: int, parts: int) -> str:
    return name if parts == 1 else f"{name}.doc{part + 1}.Loxone"


# --- source access, shared by discovery and reproduction -----------------


def github_raw(repo: str, commit: str, path: str) -> bytes:
    quoted = urllib.parse.quote(path)
    data = get(f"https://raw.githubusercontent.com/{repo}/{commit}/{quoted}")
    if data.startswith(b"version https://git-lfs"):
        data = get(f"https://media.githubusercontent.com/media/{repo}/{commit}/{quoted}")
    return data


def zip_member(url: str, member: str) -> bytes:
    with zipfile.ZipFile(io.BytesIO(get(url))) as z:
        return z.read(member)


def fetch_source(src: dict) -> bytes:
    kind = src["kind"]
    if kind == "github":
        return github_raw(src["repo"], src["commit"], src["path"])
    if kind == "kb":
        return zip_member(src["url"], src["member"])
    if kind == "wiki":
        return get(src["url"])
    raise ValueError(f"unknown source kind {kind!r}")


# --- discovery -----------------------------------------------------------


class Discovery:
    def __init__(self, dest: Path):
        self.dest = dest
        self.entries: list = []
        self.report: list = []

    def save(self, name: str, data: bytes, source: dict, license: str, alt: str = "") -> None:
        """alt: the name to use when `name` is taken (same file name in two
        directories of one repository)."""
        if alt and any(e["file"].startswith(name) for e in self.entries):
            name = alt
        if not looks_like_config(data):
            self.report.append(f"SKIP (not a ControlList XML): {name}")
            return
        if b"<!--" in data:
            # Loxone Config never writes comments: a tool rewrote the file.
            self.report.append(f"SKIP (XML comment, not a Loxone Config save): {name}")
            return
        docs = split_docs(data)
        for i, doc in enumerate(docs):
            fname = out_name(name, i, len(docs))
            out = self.dest / fname
            state = "have" if out.exists() and out.read_bytes() == doc else "ok  "
            out.write_bytes(doc)
            self.report.append(f"{state} {len(doc):>8} {fname}")
            src = dict(source)
            if len(docs) > 1:
                src["part"] = i
            self.entries.append(
                {
                    "file": fname,
                    "source": src,
                    "license": license,
                    "sha256": hashlib.sha256(doc).hexdigest(),
                    "bytes": len(doc),
                }
            )

    def kb_samples(self) -> None:
        page = get(KB_INDEX).decode("utf-8", "replace")
        zips = sorted(
            set(re.findall(r'https://www\.loxone\.com/enen/wp-content/uploads/[^"\']+\.zip', page))
        )
        self.report.append(f"-- KB index: {len(zips)} zip links")
        for url in zips:
            try:
                with zipfile.ZipFile(io.BytesIO(get(url))) as z:
                    for member in z.namelist():
                        if member.lower().endswith(EXTS):
                            self.save(
                                f"kb_{Path(member).name}",
                                z.read(member),
                                {"kind": "kb", "url": url, "member": member},
                                "© Loxone (no redistribution license)",
                            )
            except Exception as e:  # noqa: BLE001 - report and continue
                self.report.append(f"FAIL {url}: {e}")

    def loxwiki(self) -> None:
        for page_id in WIKI_PAGES:
            try:
                listing = get_json(f"{WIKI}/rest/api/content/{page_id}/child/attachment?limit=200")
            except Exception as e:  # noqa: BLE001
                self.report.append(f"FAIL wiki page {page_id}: {e}")
                continue
            for att in listing.get("results", []):
                title = att["title"]
                dl = att.get("_links", {}).get("download")
                if not title.lower().endswith(EXTS) or not dl:
                    continue
                try:
                    url = WIKI + dl
                    self.save(
                        f"wiki_{title.replace('/', '_')}",
                        get(url),
                        {"kind": "wiki", "page": page_id, "url": url},
                        "unknown (LoxWiki upload)",
                    )
                except Exception as e:  # noqa: BLE001
                    self.report.append(f"FAIL {title}: {e}")

    def github(self) -> None:
        for repo in GITHUB_TREE_REPOS:
            try:
                info = get_json(f"https://api.github.com/repos/{repo}")
                commit = get_json(f"https://api.github.com/repos/{repo}/commits/HEAD")["sha"]
            except Exception as e:  # noqa: BLE001
                self.report.append(f"FAIL {repo}: {e} (repository gone?)")
                continue
            license = (info.get("license") or {}).get("spdx_id") or "none"
            if license == "NOASSERTION":
                license = LICENSE_OVERRIDES.get(repo, "unrecognized (see the repository)")
            try:
                tree = get_json(f"https://api.github.com/repos/{repo}/git/trees/{commit}?recursive=1")
                paths = [t["path"] for t in tree.get("tree", []) if t["path"].lower().endswith(EXTS)]
            except Exception as e:  # noqa: BLE001
                self.report.append(f"FAIL tree {repo}: {e} — falling back to known paths")
                paths = GITHUB_FILES.get(repo, [])
            for path in paths:
                try:
                    self.save(
                        f"gh_{repo.replace('/', '_')}_{Path(path).name}",
                        github_raw(repo, commit, path),
                        {"kind": "github", "repo": repo, "commit": commit, "path": path},
                        license,
                        alt=f"gh_{repo.replace('/', '_')}_{path.replace('/', '_')}",
                    )
                except Exception as e:  # noqa: BLE001
                    self.report.append(f"FAIL {repo}/{path}: {e}")


def discover(dest: Path) -> int:
    d = Discovery(dest)
    d.kb_samples()
    d.loxwiki()
    d.github()
    d.entries.sort(key=lambda e: e["file"])
    manifest = {
        "about": "The public Loxone config corpus (tools/fetch-corpus.py). Files are "
        "downloaded, never committed: most carry no license to redistribute them.",
        "files": d.entries,
    }
    MANIFEST.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    d.report.append(f"-- {len(d.entries)} files, manifest: {MANIFEST}")
    print("\n".join(d.report))
    return 0


# --- reproduction --------------------------------------------------------


def reproduce(dest: Path) -> int:
    entries = json.loads(MANIFEST.read_text(encoding="utf-8"))["files"]
    cache: dict = {}  # one download per source blob (multi-document files)
    bad = 0
    for e in entries:
        out = dest / e["file"]
        if not out.exists():
            src = {k: v for k, v in e["source"].items() if k != "part"}
            key = json.dumps(src, sort_keys=True)
            try:
                if key not in cache:
                    cache[key] = split_docs(fetch_source(src))
                out.write_bytes(cache[key][e["source"].get("part", 0)])
            except Exception as ex:  # noqa: BLE001
                print(f"FAIL     {e['file']}: {ex}")
                bad += 1
                continue
        if hashlib.sha256(out.read_bytes()).hexdigest() != e["sha256"]:
            print(f"MISMATCH {e['file']}: differs from the manifest")
            bad += 1
    print(f"-- {len(entries) - bad}/{len(entries)} files verified in {dest}")
    return 1 if bad else 0


def main() -> int:
    args = sys.argv[1:]
    discovering = "--discover" in args
    rest = [a for a in args if a != "--discover"]
    if any(a.startswith("-") for a in rest) or len(rest) > 1:
        print(__doc__, file=sys.stderr)
        return 2
    dest = Path(rest[0] if rest else "corpus/web")
    dest.mkdir(parents=True, exist_ok=True)
    return discover(dest) if discovering else reproduce(dest)


if __name__ == "__main__":
    sys.exit(main())
