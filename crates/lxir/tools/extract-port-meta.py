#!/usr/bin/env python3
"""Documented per-port metadata for the builtin block types (D52).

Loxone documents every function block's connectors in its TechDoc: the
label the canvas shows (`Tr`, `Don`), the parameter's unit (`s`, `min`,
`%`, `K`), its default, and its range. lox-cli scraped that
documentation into `docs/schemas/loxone-block-types-full.json`
(github.com/eisber/lox-cli); this script reads such a file from a path
you give it — it is never copied into this repository — and keeps the
facts for the ports of lxir's own builtin table:

    tools/extract-port-meta.py <loxone-block-types-full.json>

regenerates src/port_meta.rs. The Rust file is generated: edit this
script, never the table.

Documentation is the weakest of lxir's evidence tiers — it never admits
a type or decides a port's direction (those need corpus evidence and the
save oracle, docs/connector-db.md). What it is good for is knowledge the
corpus cannot give: which unit a parameter is *in*. Two cross-checks run
on every generation and print a report on stdout:

  * every documented key must exist on the builtin port table, and a
    documented output must be an output there (direction agreement);
  * the documented default is compared with the corpus-observed GUI
    default (docs/data/param-defaults.json, D30) — agreement
    corroborates the unit's scale (an `AutoShadeTime` documented in
    minutes with default 120 matches a corpus mode of 120, not 7200).
"""

import json
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Units the generated table keeps. Anything else documented (°, mm, lux,
# kW, …) is kept too: a known unit lxir has no suffix for still refuses
# a mismatched suffix.
DIRS = {"Input": "I", "Output": "O", "Parameter": "P"}


def builtin_ports() -> dict[str, list[tuple[str, str]]]:
    """Type -> [(key, dir)] from src/connectors.rs's const tables."""
    src = (REPO / "src" / "connectors.rs").read_text(encoding="utf-8")
    consts: dict[str, list[tuple[str, str]]] = {}
    for m in re.finditer(r"const ([A-Z0-9_]+): &\[PortSpec\] = &\[(.*?)\];", src, re.S):
        consts[m.group(1)] = re.findall(r'p\("([^"]+)", (\w+)\)', m.group(2))
    start = src.index("pub fn builtin(")
    body = src[start : src.index("\n}\n", start)]
    out: dict[str, list[tuple[str, str]]] = {}
    for arm in re.finditer(r"((?:\"\w+\"\s*\|?\s*)+)=> Some\((\w+)\)", body):
        for t in re.findall(r'"(\w+)"', arm.group(1)):
            out[t] = consts[arm.group(2)]
    return out


def norm_number(s):
    if s is None or s == "":
        return None
    s = s.strip().replace(",", ".")
    return s if re.fullmatch(r"-?\d+(\.\d+)?", s) else None


def expand(name: str, keys: list[str]):
    """A documented key, or a `%d` pattern, matched against real keys."""
    if "%d" not in name:
        return [(name, None)] if name in keys else []
    rx = re.compile(re.escape(name).replace("%d", r"(\d+)") + "$")
    return [(k, m.group(1)) for k in keys if (m := rx.match(k))]


def label_for(short, index):
    if not short:
        return None
    s = short.replace("-%e", "")
    if "%d" in s:
        if index is None:
            return None
        s = s.replace("%d", index)
    return s or None


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    doc = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    defaults_path = REPO / "docs" / "data" / "param-defaults.json"
    corpus = json.loads(defaults_path.read_text(encoding="utf-8"))
    ports = builtin_ports()

    rows = []  # (type, key, unit, label, min, max, default)
    report = []
    for btype in sorted(ports):
        keys = [k for k, _ in ports[btype]]
        dirs = dict(ports[btype])
        entry = doc.get(btype)
        if not entry:
            report.append(f"{btype}: not documented")
            continue
        seen = set()
        for c in entry.get("connectors", []):
            matches = expand(c["name"], keys)
            if not matches:
                report.append(f"{btype}.{c['name']}: documented, not on the builtin table")
                continue
            for key, index in matches:
                seen.add(key)
                ddir = DIRS.get(c.get("type"))
                if (ddir == "O") != (dirs[key] == "Output") and dirs[key] != "Api":
                    report.append(
                        f"{btype}.{key}: direction disagrees (documented {ddir}, table {dirs[key]})"
                    )
                unit = c.get("unit") or None
                default = norm_number(c.get("default"))
                cd = corpus.get(btype, {}).get(key)
                if default is not None and cd and cd.get("accepted"):
                    verdict = "agrees" if float(cd["value"]) == float(default) else "DIFFERS"
                    report.append(
                        f"{btype}.{key}: default documented {default}{unit or ''}, "
                        f"corpus {cd['value']} — {verdict}"
                    )
                row = (
                    btype,
                    key,
                    unit,
                    label_for(c.get("short"), index),
                    norm_number(c.get("min")),
                    norm_number(c.get("max")),
                    default,
                )
                if any(row[2:]):
                    rows.append(row)
        undocumented = [k for k in keys if k not in seen]
        if undocumented:
            report.append(f"{btype}: undocumented keys {', '.join(undocumented)}")

    def lit(v):
        return "None" if v is None else f'Some("{v}")'

    lines = [
        "//! Documented per-port metadata for the builtin block types (D52).",
        "//! GENERATED by `tools/extract-port-meta.py` — do not edit; regenerate",
        "//! with:",
        "//!",
        "//! ```sh",
        "//! tools/extract-port-meta.py <lox-cli>/docs/schemas/loxone-block-types-full.json",
        "//! ```",
        "//!",
        "//! Source: Loxone's TechDoc block documentation as scraped by lox-cli",
        "//! (github.com/eisber/lox-cli) — read from that path at generation",
        "//! time, filtered to the ports of `connectors::builtin`, and",
        "//! cross-checked against the table's directions and the",
        "//! corpus-observed defaults (the generator prints the report).",
        "//!",
        "//! Documentation is the weakest evidence tier: it never admits a type",
        "//! or decides a direction. It supplies what the corpus cannot — the",
        "//! unit a parameter is in (value suffixes scale into it, D52), the",
        "//! canvas label (`Tr` for `InputTrigger`; error hints only), and the",
        "//! documented range (`lxir lint`).",
        "",
        "/// One port's documented facts. Numbers are kept as the exact",
        "/// decimal strings the documentation gives.",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub struct PortMeta {",
        "    /// Unit as documented (`s`, `min`, `%`, `K`, `°`, …).",
        "    pub unit: Option<&'static str>,",
        "    /// The label Loxone Config shows on the canvas.",
        "    pub label: Option<&'static str>,",
        "    pub min: Option<&'static str>,",
        "    pub max: Option<&'static str>,",
        "    pub default: Option<&'static str>,",
        "}",
        "",
        "const fn m(",
        "    unit: Option<&'static str>,",
        "    label: Option<&'static str>,",
        "    min: Option<&'static str>,",
        "    max: Option<&'static str>,",
        "    default: Option<&'static str>,",
        ") -> PortMeta {",
        "    PortMeta {",
        "        unit,",
        "        label,",
        "        min,",
        "        max,",
        "        default,",
        "    }",
        "}",
        "",
        "/// The documented metadata of a builtin block's port, if any.",
        "pub fn port_meta(block_type: &str, port: &str) -> Option<PortMeta> {",
        "    Some(match (block_type, port) {",
    ]
    for t, k, unit, label, lo, hi, default in rows:
        lines.append(
            f'        ("{t}", "{k}") => m({lit(unit)}, {lit(label)}, {lit(lo)}, {lit(hi)}, {lit(default)}),'
        )
    lines += ["        _ => return None,", "    })", "}", ""]
    out = REPO / "src" / "port_meta.rs"
    out.write_text("\n".join(lines), encoding="utf-8")
    subprocess.run(["rustfmt", "--edition", "2024", str(out)], check=True)

    print(f"src/port_meta.rs: {len(rows)} ports over {len(ports)} builtin types")
    for line in report:
        print(line)


if __name__ == "__main__":
    main()
