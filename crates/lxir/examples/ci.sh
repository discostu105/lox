#!/bin/sh
# The CI check path for a repo that holds lxir sources — tree-untouched,
# equally runnable locally and in CI.
#
# Invariants (always checked):
#   check          modules parse and validate
#   fmt --check    sources are canonical
#   lock currency  compiling changes nothing in the committed lockfile
#   determinism    a second compile is byte-identical
#
# Modes:
#   (default)      additionally show diff and drift against the base,
#                  informative only
#   --sync         require the sources to equal the deployed state: the
#                  semantic diff against the base must be empty and drift
#                  green — the state right after a push or download
#
# Optional:
#   EXPECTED=...   compiled output must be byte-identical to this
#                  committed file
#   LOX=...        lox binary (eisber/lox-cli); when set — or when `lox`
#                  is on PATH — the module's `test … end` blocks run
#                  through the simulator (`lxir test`)
#
# Configuration via environment:
#   LXIR=...       lxir binary                 (default: lxir on PATH)
#   BASE=...       pinned .Loxone base         (required)
#   MODULE=...     module file or directory    (required)
#   LOCK=...       committed lockfile          (required)
#   SERIAL=...     Miniserver serial           (default: from the lockfile)
set -eu
lxir=${LXIR:-lxir}
base=${BASE:?set BASE to the pinned .Loxone base config}
module=${MODULE:?set MODULE to the module file or directory}
lock=${LOCK:?set LOCK to the committed lockfile}

"$lxir" check "$module"
"$lxir" fmt --check "$module" >/dev/null && echo "fmt: canonical"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# Compile against a COPY of the lock: if the result differs from the
# committed lock, either the lock was not committed together with the
# sources, or the base is not the one of the last compile (drift
# suspicion — see the diff/drift output below).
cp "$lock" "$tmp/lock.json"
"$lxir" compile --base "$base" --module "$module" --lock "$tmp/lock.json" \
    ${SERIAL:+--serial "$SERIAL"} --out "$tmp/out.Loxone"
if ! cmp -s "$lock" "$tmp/lock.json"; then
    echo "ERROR: compile updates $lock — compile locally and commit the lock" >&2
    exit 1
fi
echo "lock: current"

# Determinism: a second compile must be byte-identical.
cp "$lock" "$tmp/lock2.json"
"$lxir" compile --base "$base" --module "$module" --lock "$tmp/lock2.json" \
    ${SERIAL:+--serial "$SERIAL"} --out "$tmp/out2.Loxone" >/dev/null
cmp "$tmp/out.Loxone" "$tmp/out2.Loxone"
echo "compile: deterministic"

if [ -n "${EXPECTED:-}" ]; then
    cmp "$EXPECTED" "$tmp/out.Loxone"
    echo "expected output: byte-identical"
fi

# Simulated tests: run when a lox binary is available (LOX env or PATH),
# skip with a note otherwise — repos without a simulator stay green.
if [ -n "${LOX:-}" ] || command -v lox >/dev/null 2>&1; then
    "$lxir" test --base "$base" --module "$module" --lock "$lock" \
        ${SERIAL:+--serial "$SERIAL"}
else
    echo "sim: skipped (no lox binary — set LOX= or put lox on PATH)"
fi

if [ "${1:-}" = "--sync" ]; then
    "$lxir" diff --exit-code "$base" "$tmp/out.Loxone"
    "$lxir" drift "$base" --lock "$lock"
else
    "$lxir" diff "$base" "$tmp/out.Loxone" || true
    "$lxir" drift "$base" --lock "$lock" || true
fi
echo "ci: OK"
