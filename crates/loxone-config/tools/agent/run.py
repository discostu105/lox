#!/usr/bin/env python3
"""Run an agent on the Lox Logic cases and grade its modules with `loxl test`.

Each case is a request in plain language (`cases/<id>/request.md`), hidden
tests (`tests.loxl`) and a reference solution (`reference.loxl`). Per case
the agent gets a fresh directory holding the fixture config, a `lox.toml`
and the lox-logic skill, writes `solution.loxl`, and the hidden tests then run
against it in the Lox Logic simulator. See README.md.

    run.py                         # every case, Claude Code (`claude -p`)
    run.py --case c03 --case c07   # a subset (id prefixes)
    run.py --reference             # grade the reference solutions instead
"""

import argparse
import datetime
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
CRATE = HERE.parent.parent
SERIAL = "504F94112233"

PROMPT = """\
This directory is a Lox Logic project (lox.toml) for a Loxone Miniserver.
`fixture.Loxone` is the current config: its virtual inputs are the house's
sensors and its virtual output commands the actuators. Use the lox-logic skill.
The Lox Logic documentation is in {docs}.

Write `solution.loxl` in this directory so that it implements this request:

{request}

Reference existing objects with `extern`. `loxl check`, `loxl compile` and
`loxl test` work here without flags; write `test` blocks to check your
logic in the simulator. Work on your own: there is nobody to answer
questions. When you are done, `loxl compile` must succeed.
"""

TOML = """\
base = "fixture.Loxone"
module = "solution.loxl"
lock = "solution.lock.json"
out = "out.Loxone"
serial = "{serial}"
page = "Logik"
"""


def loxl(binary, *args, cwd=None):
    return subprocess.run(
        [binary, *args], cwd=cwd, capture_output=True, text=True, check=False
    )


def build_fixture(binary, work):
    out = work / "fixture.Loxone"
    r = loxl(binary, "compile", "--base", CRATE / "examples" / "configs" / "rohbau.Loxone",
             "--module", HERE / "fixture.loxl", "--lock", work / "fixture.lock.json",
             "--serial", SERIAL, "--out", out)
    if r.returncode != 0:
        sys.exit(f"fixture: {r.stderr.strip()}")
    return out


def test_names(tests):
    return re.findall(r'^test "([^"]*)"', tests, re.M)


def strip_tests(source):
    """The module without its `test … end` blocks."""
    out, inside = [], False
    for line in source.splitlines(keepends=True):
        if not inside and line.startswith("test "):
            inside = True
        elif inside and line.rstrip() == "end":
            inside = False
        elif not inside:
            out.append(line)
    return "".join(out)


def grade(binary, fixture, case, solution, work):
    """The hidden tests against the solution: (pass, detail)."""
    tests = (case / "tests.loxl").read_text()
    module = work / "graded"
    module.mkdir()
    # The solution's own tests do not count, and a name of theirs could
    # clash with a hidden one.
    (module / "a.loxl").write_text(strip_tests(solution.read_text()))
    (module / "z.loxl").write_text(tests)
    r = loxl(binary, "test", "--base", fixture, "--module", module,
             "--lock", work / "graded.lock.json", "--serial", SERIAL)
    results = dict(re.findall(r'^test "([^"]*)" \.\.\. (\w+)', r.stdout, re.M))
    hidden = test_names(tests)
    if not results:
        return False, (r.stderr or r.stdout).strip().splitlines()[-1:] or ["no output"]
    failed = [n for n in hidden if results.get(n) != "ok"]
    detail = [] if not failed else [
        line.strip() for line in r.stdout.splitlines()
        if line.startswith("    ")
    ]
    return not failed, detail


def run_agent(args, case, work):
    shutil.copytree(CRATE / "skills" / "lox-logic", work / ".claude" / "skills" / "lox-logic")
    (work / "lox.toml").write_text(TOML.format(serial=SERIAL))
    prompt = PROMPT.format(docs=CRATE / "docs",
                           request=(case / "request.md").read_text().strip())
    env = dict(os.environ)
    env["PATH"] = f"{Path(args.loxl).resolve().parent}{os.pathsep}{env['PATH']}"
    cmd = [args.claude, "-p", prompt, "--output-format", "json",
           "--no-session-persistence", "--strict-mcp-config",
           "--setting-sources", "project",
           "--add-dir", str(CRATE / "docs"),
           "--allowedTools", "Bash(loxl *)", "Read", "Write", "Edit", "Glob",
           "Grep", "Skill",
           "--max-budget-usd", str(args.budget)]
    if args.model:
        cmd += ["--model", args.model]
    start = time.time()
    try:
        r = subprocess.run(cmd, cwd=work, env=env, capture_output=True, text=True,
                           timeout=args.timeout, check=False)
        out = r.stdout
    except subprocess.TimeoutExpired:
        out = ""
    meta = {"seconds": round(time.time() - start)}
    try:
        j = json.loads(out)
        meta.update(turns=j.get("num_turns"), cost_usd=j.get("total_cost_usd"),
                    result=(j.get("result") or "")[-400:])
    except json.JSONDecodeError:
        meta["result"] = out[-400:] or "(timeout or no output)"
    (work / "agent.json").write_text(out)
    return meta


def main():
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--loxl", default=str(CRATE.parent.parent / "target" / "release" / "loxl"))
    p.add_argument("--claude", default="claude")
    p.add_argument("--model")
    p.add_argument("--case", action="append", default=[],
                   help="id prefix; repeatable (default: every case)")
    p.add_argument("--reference", action="store_true",
                   help="grade the reference solutions, no agent")
    p.add_argument("--timeout", type=int, default=900, help="seconds per case")
    p.add_argument("--budget", type=float, default=2.0, help="USD per case")
    p.add_argument("--work", help="work directory (default: a new temp dir, kept)")
    p.add_argument("--json", help="write the results here")
    args = p.parse_args()

    work = Path(args.work or tempfile.mkdtemp(prefix="loxl-agent-"))
    work.mkdir(parents=True, exist_ok=True)
    fixture = build_fixture(args.loxl, work)
    cases = sorted(d for d in (HERE / "cases").iterdir()
                   if not args.case or any(d.name.startswith(c) for c in args.case))
    rows = []
    for case in cases:
        cw = work / case.name
        cw.mkdir()
        shutil.copy(fixture, cw / "fixture.Loxone")
        if args.reference:
            shutil.copy(case / "reference.loxl", cw / "solution.loxl")
            meta = {}
        else:
            meta = run_agent(args, case, cw)
        solution = cw / "solution.loxl"
        if solution.exists():
            ok, detail = grade(args.loxl, fixture, case, solution, cw)
        else:
            ok, detail = False, ["no solution.loxl"]
        rows.append({"case": case.name, "pass": ok, "detail": detail, **meta})
        extra = "" if not meta else (
            f"  {meta.get('turns')} turns, {meta['seconds']}s"
            + (f", ${meta['cost_usd']:.2f}" if meta.get("cost_usd") else ""))
        print(f"{'PASS' if ok else 'FAIL'}  {case.name}{extra}", flush=True)
        for line in detail:
            print(f"      {line}")
    passed = sum(r["pass"] for r in rows)
    print(f"\n{passed}/{len(rows)} cases pass — work dir {work}")
    if args.json:
        Path(args.json).write_text(json.dumps({
            "date": datetime.date.today().isoformat(),
            "model": args.model or "default",
            "reference": args.reference,
            "cases": rows,
        }, indent=2, ensure_ascii=False) + "\n")
    sys.exit(0 if passed == len(rows) else 1)


if __name__ == "__main__":
    main()
