#!/usr/bin/env python3
"""Extract lox-sim's `block_signature` table into the legacy-db JSON shape.

lox-sim (in the lox-cli repo) hard-codes its connector knowledge as a Rust
match in `lox-sim/src/parser.rs`. This script lifts it into the
`connector-map.json` shape (`type -> {c: [keys], t: {key: "I"|"O"}}`) so
`lxir observe --crosscheck` can compare corpus evidence against it:

    tools/extract-sim-signature.py ~/repos/my/lox-cli > sim-signature.json
    lxir observe corpus/*.Loxone --crosscheck sim-signature.json

Inputs and params both map to "I" (params are Def-settable inputs); the
`c` order is inputs+outputs+params as the sim lists them, which is NOT the
connector-index order — expect `order_mismatch` noise against this db and
use connector-map.json (or corpus indexes) for ordering questions.
"""

import json
import re
import sys
from pathlib import Path


def extract(parser_rs: str) -> dict:
    start = parser_rs.index("fn block_signature(")
    end = parser_rs.index("\n}", start)
    body = parser_rs[start:end]

    arm_re = re.compile(
        r'((?:"[A-Za-z0-9_]+"\s*\|\s*)*"[A-Za-z0-9_]+")\s*=>\s*\((.*?)\),\n',
        re.DOTALL,
    )
    group_re = re.compile(r"&\[(.*?)\]", re.DOTALL)

    db = {}
    for m in arm_re.finditer(body):
        types = re.findall(r'"([A-Za-z0-9_]+)"', m.group(1))
        groups = group_re.findall(m.group(2))
        if len(groups) != 3:
            continue
        inputs, outputs, params = (re.findall(r'"([^"]+)"', g) for g in groups)
        entry = {
            "c": inputs + outputs + params,
            "t": {
                **{k: "I" for k in inputs},
                **{k: "O" for k in outputs},
                **{k: "I" for k in params},
            },
        }
        for t in types:
            db[t] = entry
    return db


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    parser = Path(sys.argv[1]) / "lox-sim" / "src" / "parser.rs"
    db = extract(parser.read_text())
    print(f"{len(db)} types", file=sys.stderr)
    json.dump(db, sys.stdout, indent=1, sort_keys=True)
    print()


if __name__ == "__main__":
    main()
