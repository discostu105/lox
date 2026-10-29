#!/usr/bin/env python3
"""Fetch the public Loxone config corpus into corpus/web/ (gitignored).

Sources were verified 2026-08-25 (see docs/connector-db.md). Most files
carry no explicit license, so the corpus stays local — this script is
committed instead, making the corpus reproducible:

  1. Official Loxone KB sample files (zips, one .Loxone/.LoxPLAN each),
     scraped from the videos-sample-files index page.
  2. LoxWiki (Confluence) attachments, enumerated via the REST API.
  3. GitHub repositories with real projects/templates, fetched raw.

Idempotent: existing target files are skipped. Failures are reported and
skipped — sources are third-party and rot over time.

Usage: tools/fetch-corpus.py [dest-dir]   (default: corpus/web)
"""

import io
import json
import re
import sys
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path

UA = {"User-Agent": "Mozilla/5.0 (corpus fetch for lxir format research)"}
KB_INDEX = "https://www.loxone.com/enen/kb/videos-sample-files/"
WIKI_API = "https://loxwiki.atlassian.net/wiki/rest/api"

# repo -> paths (raw.githubusercontent.com). ONOKOM/Templates and
# eisber/lox-cli are enumerated via the git tree API instead of listing
# their dozens of paths here (lox-cli's golden configs are Git-LFS
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
GITHUB_TREE_REPOS = ["ONOKOM/Templates", "eisber/lox-cli", *GITHUB_FILES.keys()]


def get(url: str) -> bytes:
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def looks_like_config(data: bytes) -> bool:
    head = data[:4096].lstrip(b"\xef\xbb\xbf")
    return head.startswith(b"<?xml") and b"<ControlList" in data[:65536]


def save(dest: Path, name: str, data: bytes, report: list) -> None:
    if not looks_like_config(data):
        report.append(f"SKIP (not a ControlList XML): {name}")
        return
    # Some samples concatenate several documents into one file (e.g. the
    # two-Miniserver communication sample: one XML declaration, two
    # <ControlList> roots) — split so each part parses on its own.
    parts = re.split(rb"(?=<ControlList[ >])", data)
    prolog, roots = parts[0], parts[1:]
    docs = [data] if len(roots) <= 1 else [prolog + r for r in roots]
    for i, doc in enumerate(docs):
        out = dest / (name if len(docs) == 1 else f"{name}.doc{i + 1}.Loxone")
        if out.exists():
            report.append(f"have {out.stat().st_size:>8} {out.name}")
            continue
        out.write_bytes(doc)
        report.append(f"ok   {len(doc):>8} {out.name}")


def fetch_kb_samples(dest: Path, report: list) -> None:
    page = get(KB_INDEX).decode("utf-8", "replace")
    zips = sorted(
        set(re.findall(r'https://www\.loxone\.com/enen/wp-content/uploads/[^"\']+\.zip', page))
    )
    report.append(f"-- KB index: {len(zips)} zip links")
    for url in zips:
        name = urllib.parse.unquote(url.rsplit("/", 1)[1])
        try:
            blob = get(url)
            with zipfile.ZipFile(io.BytesIO(blob)) as z:
                for member in z.namelist():
                    if member.lower().endswith((".loxone", ".loxplan")):
                        save(dest, f"kb_{Path(member).name}", z.read(member), report)
        except Exception as e:  # noqa: BLE001 - report and continue
            report.append(f"FAIL {name}: {e}")


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


def fetch_loxwiki(dest: Path, report: list) -> None:
    for page_id in WIKI_PAGES:
        try:
            listing = json.loads(
                get(f"{WIKI_API}/content/{page_id}/child/attachment?limit=200")
            )
        except Exception as e:  # noqa: BLE001
            report.append(f"FAIL wiki page {page_id}: {e}")
            continue
        for att in listing.get("results", []):
            title = att["title"]
            if not title.lower().endswith((".loxone", ".loxplan")):
                continue
            dl = att.get("_links", {}).get("download")
            if not dl:
                continue
            try:
                data = get("https://loxwiki.atlassian.net/wiki" + dl)
                save(dest, f"wiki_{title.replace('/', '_')}", data, report)
            except Exception as e:  # noqa: BLE001
                report.append(f"FAIL {title}: {e}")


def fetch_github(dest: Path, report: list) -> None:
    # Enumerate each repo's tree once: catches renames and collections.
    for repo in GITHUB_TREE_REPOS:
        listed = []
        try:
            tree = json.loads(
                get(f"https://api.github.com/repos/{repo}/git/trees/HEAD?recursive=1")
            )
            listed = [
                t["path"]
                for t in tree.get("tree", [])
                if t["path"].lower().endswith((".loxone", ".loxplan"))
            ]
        except Exception as e:  # noqa: BLE001
            report.append(f"FAIL tree {repo}: {e} — falling back to known paths")
            listed = GITHUB_FILES.get(repo, [])
        for path in listed:
            name = f"gh_{repo.replace('/', '_')}_{Path(path).name}"
            try:
                url = (
                    f"https://raw.githubusercontent.com/{repo}/HEAD/"
                    + urllib.parse.quote(path)
                )
                data = get(url)
                if data.startswith(b"version https://git-lfs"):
                    url = (
                        f"https://media.githubusercontent.com/media/{repo}/HEAD/"
                        + urllib.parse.quote(path)
                    )
                    data = get(url)
                save(dest, name, data, report)
            except Exception as e:  # noqa: BLE001
                report.append(f"FAIL {repo}/{path}: {e}")


def main() -> None:
    dest = Path(sys.argv[1] if len(sys.argv) > 1 else "corpus/web")
    dest.mkdir(parents=True, exist_ok=True)
    existing = {p.name for p in dest.iterdir()}
    report: list = [f"dest: {dest} ({len(existing)} files already present)"]
    fetch_kb_samples(dest, report)
    fetch_loxwiki(dest, report)
    fetch_github(dest, report)
    fresh = [p for p in dest.iterdir() if p.name not in existing]
    report.append(f"-- done: {len(fresh)} new files, {len(list(dest.iterdir()))} total")
    print("\n".join(report))


if __name__ == "__main__":
    main()
