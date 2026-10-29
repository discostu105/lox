#!/usr/bin/env python3
"""Grade agent-written .lxir solutions against lox-cli's eval cases.

lox-cli (https://github.com/eisber/lox-cli) ships ~475 natural-language
automation tasks, each with the blocks, wires and parameters a solution
should contain and simulator scenarios it should pass. This harness reads
those cases from a lox-cli checkout -- nothing of it is copied into this
repository (lox-cli is AGPL-3.0) -- and grades solutions written in the
IR:

    lxir-eval.py cases  --evals <lox-cli>/tests/eval [--feasible]
    lxir-eval.py prompt --evals <dir> <case-id>
    lxir-eval.py fixture --evals <dir> --out <fixture.Loxone>
    lxir-eval.py run    --evals <dir> --solutions <dir> [--case <id>]...
                        [--lxir <bin>] [--lox <bin>] [--json <report>]

A solution is `<solutions>/<case-id>.lxir`. `run` compiles it against a
working copy of the eval fixture, grades the compiled config structurally
(expected blocks, wires, parameters) and runs the case's scenarios through
`lox sim run`. Scenarios that already pass on the untouched fixture are
reported as non-discriminating: they say nothing about the solution.

The fixture is normalized in the working copy only: XML comments are
stripped (a .Loxone written by Loxone Config has none, and lxir refuses
them) and repeated UUIDs are re-minted after their first occurrence, the
way a Loxone Config save repairs them.

See tools/eval/README.md for the agent loop.
"""

import argparse
import glob
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

SERIAL = "504F94E7A100"
MINT_TIME = "1767225600"


def load_cases(evals):
    cases = {}
    for path in sorted(glob.glob(os.path.join(evals, "cases", "*.json"))):
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
        for case in data if isinstance(data, list) else data.get("cases", []):
            if isinstance(case, dict) and "id" in case:
                case["_file"] = os.path.basename(path)
                cases[case["id"]] = case
    return cases


def lxir_types(lxir):
    out = subprocess.run([lxir, "types", "--json"], capture_output=True, check=True)
    return {t["type"] for t in json.loads(out.stdout)}


def block_types(case):
    """Each expected new block as the set of types that satisfy it."""
    for b in case.get("expected", {}).get("new_blocks", []):
        alts = b.get("type_alt") or []
        yield {b["type"], *(alts if isinstance(alts, list) else [alts])}


def feasible(case, mintable):
    return all(types & mintable for types in block_types(case))


def normalize_fixture(src, dst):
    with open(src, encoding="utf-8") as f:
        text = f.read()
    text = re.sub(r"[ \t]*<!--.*?-->[ \t]*\r?\n?", "", text, flags=re.S)
    seen = set()
    counter = 0

    def remint(m):
        nonlocal counter
        uuid = m.group(2)
        if uuid not in seen:
            seen.add(uuid)
            return m.group(0)
        counter += 1
        return f'{m.group(1)}"eeee{counter:04x}-{uuid[9:]}"'

    # Only defining occurrences (` U="…"`), never `Input=` references:
    # wires keep naming the first, as after a Loxone Config save.
    text = re.sub(r'( U=)"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{16})"', remint, text)
    with open(dst, "w", encoding="utf-8") as f:
        f.write(text)
    return counter


# --- structural grading ----------------------------------------------------


class Config:
    def __init__(self, path):
        self.root = ET.parse(path).getroot()
        self.objects = [c for c in self.root.iter("C") if c.get("U")]
        self.port_owner = {}
        for o in self.objects:
            for co in o.findall("Co"):
                if co.get("U"):
                    self.port_owner[co.get("U")] = o

    def uuids(self):
        return {o.get("U") for o in self.objects}


def fold(s):
    return (s or "").casefold()


def match_blocks(case, cfg, base_uuids):
    new = [o for o in cfg.objects if o.get("U") not in base_uuids]
    used = set()
    results = []
    for want, types in zip(case["expected"].get("new_blocks", []), block_types(case)):
        hit = None
        for o in new:
            if o.get("U") in used or o.get("Type") not in types:
                continue
            title = fold(o.get("Title"))
            if "title" in want and title != fold(want["title"]):
                continue
            if "title_contains" in want and fold(want["title_contains"]) not in title:
                continue
            hit = o
            break
        if hit is not None:
            used.add(hit.get("U"))
        results.append((describe_block(want), hit is not None))
    return results


def describe_block(want):
    t = want.get("title") or want.get("title_contains")
    return want["type"] + (f' "{t}"' if t else "")


def endpoint_objects(cfg, title, typ):
    for o in cfg.objects:
        if title is not None and o.get("Title") != title:
            continue
        if typ is not None and o.get("Type") != typ:
            continue
        if title is None and typ is None:
            continue
        yield o


def port_uuids(obj, key):
    """Connector UUIDs of `key` -- any connector when the case names none."""
    return {co.get("U") for co in obj.findall("Co") if key in (None, co.get("K"))}


def match_wires(case, cfg):
    results = []
    for w in case["expected"].get("wiring", []):
        fk = w.get("from_connector") or w.get("from_conn")
        tk = w.get("to_connector") or w.get("to_conn")
        sources = set()
        for o in endpoint_objects(cfg, w.get("from_title"), w.get("from_type")):
            sources |= port_uuids(o, fk)
        ok = False
        for o in endpoint_objects(cfg, w.get("to_title"), w.get("to_type")):
            for co in o.findall("Co"):
                if tk not in (None, co.get("K")):
                    continue
                feeds = {i.get("Input") for i in co.findall("In")}
                if feeds & sources or any(through_ref(cfg, u, sources) for u in feeds):
                    ok = True
        frm = f'{w.get("from_title") or w.get("from_type")}.{fk}'
        to = f'{w.get("to_title") or w.get("to_type")}.{tk}'
        results.append((f"{frm} -> {to}", ok))
    return results


def through_ref(cfg, port_uuid, sources):
    """A wire drawn through an Input/OutputRef mirror of the source."""
    ref = cfg.port_owner.get(port_uuid)
    if ref is None or ref.get("Type") not in ("InputRef", "OutputRef"):
        return False
    return any(
        i.get("Input") in sources for co in ref.findall("Co") for i in co.findall("In")
    )


def same_value(actual, want):
    try:
        return abs(float(actual) - float(want)) < 1e-6
    except (TypeError, ValueError):
        return fold(actual) == fold(str(want))


def match_params(case, cfg, base_uuids):
    results = []
    for p in case["expected"].get("params", []):
        key = p.get("param") or p.get("connector")
        ok = False
        for o in cfg.objects:
            if o.get("Type") != p.get("block_type"):
                continue
            if "block_title" in p and fold(p["block_title"]) not in fold(o.get("Title")):
                continue
            for co in o.findall("Co"):
                if co.get("K") == key and same_value(co.get("Def"), p.get("value")):
                    ok = True
            if same_value(o.get(key), p.get("value")):
                ok = True  # attribute parameters
        results.append((f'{p.get("block_type")}.{key} = {p.get("value")}', ok))
    return results


# --- simulation ------------------------------------------------------------


def simulate(lox, config, scenarios, work):
    """Scenario name -> pass, or an error string when the run failed."""
    if not scenarios:
        return {}
    spec = os.path.join(work, "sim.json")
    with open(spec, "w", encoding="utf-8") as f:
        json.dump(scenarios, f, ensure_ascii=False)
    out = subprocess.run(
        [lox, "sim", "run", "--json", "--sim-file", spec, config],
        capture_output=True,
        text=True,
    )
    try:
        verdict = json.loads(out.stdout)
    except json.JSONDecodeError:
        return (out.stderr or out.stdout).strip().splitlines()[-1:] or ["no output"]
    return {s["name"]: s["pass"] for s in verdict.get("scenarios", [])}


# --- commands --------------------------------------------------------------


def cmd_cases(args):
    cases = load_cases(args.evals)
    mintable = lxir_types(args.lxir)
    shown = 0
    for cid, case in cases.items():
        ok = feasible(case, mintable)
        if args.feasible and not ok:
            continue
        shown += 1
        types = ", ".join("/".join(sorted(t)) for t in block_types(case))
        print(f'{"+" if ok else "-"} {cid:48} {case.get("difficulty", ""):7} {types}')
    print(f"{shown} of {len(cases)} cases", file=sys.stderr)


def cmd_prompt(args):
    case = load_cases(args.evals).get(args.case)
    if case is None:
        sys.exit(f"no case `{args.case}`")
    exp = case.get("expected", {})
    print(f"Task ({args.case}): {case['utterance']}\n")
    print("Write the solution as one .lxir module against the eval fixture")
    print(f"({os.path.join(args.evals, 'fixture.Loxone')}; `lxir-eval.py run` normalizes it).")
    print("Reference fixture objects as externs, mint the rest as blocks.\n")
    print("The grader looks for:")
    for b in exp.get("new_blocks", []):
        print(f"  block   {describe_block(b)}")
    for w in exp.get("wiring", []):
        fk = w.get("from_connector") or w.get("from_conn")
        tk = w.get("to_connector") or w.get("to_conn")
        print(
            f'  wire    {w.get("from_title") or w.get("from_type")}.{fk} -> '
            f'{w.get("to_title") or w.get("to_type")}.{tk}'
        )
    for p in exp.get("params", []):
        print(f'  param   {p.get("block_type")}.{p.get("param") or p.get("connector")} = {p.get("value")}')
    for s in exp.get("simulation", []):
        print(f'  sim     {s.get("name")}')
        for step in s.get("steps") or [s]:
            clock = f' at {step["time"]}' if "time" in step else ""
            print(f'            set {json.dumps(step.get("inputs", {}), ensure_ascii=False)}'
                  f' tick {step.get("ticks")}{clock}'
                  f' expect {json.dumps(step.get("expected_outputs", {}), ensure_ascii=False)}')


def cmd_fixture(args):
    n = normalize_fixture(os.path.join(args.evals, "fixture.Loxone"), args.out)
    print(f"wrote {args.out} ({n} repeated UUID(s) re-minted); "
          f"`lxir decompile {args.out}` lists its objects")


def cmd_run(args):
    cases = load_cases(args.evals)
    sols = sorted(glob.glob(os.path.join(args.solutions, "*.lxir")))
    if args.case:
        sols = [s for s in sols if os.path.basename(s)[:-5] in args.case]
    work = tempfile.mkdtemp(prefix="lxir-eval-")
    fixture = os.path.join(work, "fixture.Loxone")
    reminted = normalize_fixture(os.path.join(args.evals, "fixture.Loxone"), fixture)
    base_uuids = Config(fixture).uuids()
    report = {"fixture_reminted_uuids": reminted, "cases": []}
    for sol in sols:
        cid = os.path.basename(sol)[:-5]
        case = cases.get(cid)
        if case is None:
            print(f"? {cid}: no such case", file=sys.stderr)
            continue
        cw = os.path.join(work, cid)
        os.makedirs(cw)
        out = os.path.join(cw, "out.Loxone")
        comp = subprocess.run(
            [args.lxir, "compile", "--base", fixture, "--module", sol,
             "--lock", os.path.join(cw, "lock.json"), "--out", out,
             "--serial", SERIAL, "--time", MINT_TIME],
            capture_output=True, text=True,
        )
        entry = {"case": cid, "compiled": comp.returncode == 0}
        scenarios = case.get("expected", {}).get("simulation", [])
        baseline = simulate(args.lox, fixture, scenarios, cw)
        if comp.returncode != 0:
            entry["error"] = comp.stderr.strip()
        else:
            cfg = Config(out)
            entry["blocks"] = match_blocks(case, cfg, base_uuids)
            entry["wires"] = match_wires(case, cfg)
            entry["params"] = match_params(case, cfg, base_uuids)
            entry["sim"] = simulate(args.lox, out, scenarios, cw)
        entry["baseline"] = baseline
        entry["pass"] = verdict(entry)
        report["cases"].append(entry)
        print_entry(entry)
    passed = sum(e["pass"] for e in report["cases"])
    print(f"\n{passed}/{len(report['cases'])} cases pass", end="")
    print(f" (fixture: {reminted} repeated UUID(s) re-minted in the working copy)" if reminted else "")
    if args.json:
        with open(args.json, "w", encoding="utf-8") as f:
            json.dump(report, f, ensure_ascii=False, indent=1)
    if not args.keep:
        shutil.rmtree(work)
    else:
        print(f"work dir: {work}")
    return 0 if passed == len(report["cases"]) else 1


def verdict(e):
    """Compiled, every expected block present, every scenario passing."""
    if not e["compiled"] or not isinstance(e.get("sim"), dict):
        return False
    return all(ok for _, ok in e["blocks"]) and all(e["sim"].values())


def frac(items):
    return f"{sum(ok for _, ok in items)}/{len(items)}"


def print_entry(e):
    mark = "PASS" if e["pass"] else "FAIL"
    if not e["compiled"]:
        print(f"{mark} {e['case']}: compile error: {e['error'].splitlines()[-1]}")
        return
    sim = e["sim"]
    if isinstance(sim, dict):
        base = e["baseline"] if isinstance(e["baseline"], dict) else {}
        disc = [n for n in sim if not base.get(n, False)]
        simtxt = f"sim {sum(sim.values())}/{len(sim)} ({sum(sim[n] for n in disc)}/{len(disc)} discriminating)"
    else:
        simtxt = f"sim error: {' '.join(sim)}"
    print(
        f"{mark} {e['case']}: blocks {frac(e['blocks'])}, wires {frac(e['wires'])}, "
        f"params {frac(e['params'])}, {simtxt}"
    )
    for kind in ("blocks", "wires", "params"):
        for what, ok in e[kind]:
            if not ok:
                print(f"       missing {kind[:-1]}: {what}")
    if isinstance(sim, dict):
        for n, ok in sim.items():
            if not ok:
                print(f"       failed scenario: {n}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    here = os.path.dirname(os.path.abspath(__file__))
    default_lxir = os.path.join(here, "..", "..", "target", "release", "lxir")
    for name in ("cases", "prompt", "fixture", "run"):
        p = sub.add_parser(name)
        p.add_argument("--evals", required=True, help="lox-cli tests/eval directory")
        p.add_argument("--lxir", default=os.environ.get("LXIR") or (
            default_lxir if os.path.exists(default_lxir) else "lxir"))
        if name == "cases":
            p.add_argument("--feasible", action="store_true",
                           help="only cases whose blocks lxir can mint")
        if name == "prompt":
            p.add_argument("case")
        if name == "fixture":
            p.add_argument("--out", required=True)
        if name == "run":
            p.add_argument("--solutions", required=True)
            p.add_argument("--case", action="append", help="only this case (repeatable)")
            p.add_argument("--lox", default=os.environ.get("LOX", "lox"))
            p.add_argument("--json", help="write the full report here")
            p.add_argument("--keep", action="store_true", help="keep the work dir")
    args = ap.parse_args()
    commands = {"cases": cmd_cases, "prompt": cmd_prompt, "fixture": cmd_fixture, "run": cmd_run}
    return commands[args.cmd](args) or 0


if __name__ == "__main__":
    sys.exit(main())
