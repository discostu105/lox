#!/usr/bin/env bash
# The save oracle, one command: open a compiled .Loxone in Loxone Config
# under Wine on a headless Xvfb display, save it, and semantically diff
# the result. Every divergence is either a compiler bug or a new fact
# about the format (see docs/oracle-wine.md for everything learned so
# far, including the save fingerprint that is NOT a divergence).
#
#   oracle.sh run <file.Loxone> [--out <saved>] [--keep]   full cycle
#   oracle.sh up                     start Xvfb + checks
#   oracle.sh open <file.Loxone>     open a COPY in the GUI
#   oracle.sh save                   Ctrl+S, poll until written
#   oracle.sh shot [out.png]         screenshot the framebuffer
#   oracle.sh status                 what is running, what is open
#   oracle.sh down                   wineserver -k + stop Xvfb
#
# The GUI must be driven — LoxoneConfig.exe has no headless mode. The
# rig only ever opens a copy inside its own work dir; the input file is
# never touched. Opening never contacts a Miniserver (connecting is a
# separate explicit GUI action this script never performs).
#
# Configuration (defaults match the rig this was built on):
#   ORACLE_PREFIX   isolated wineprefix — NEVER the desktop one: a
#                   launch into a prefix whose wineserver already runs
#                   on another display joins THAT instance and opens
#                   the file as a tab on the desktop
#   ORACLE_EXE      LoxoneConfig.exe to run (cwd is its directory)
#   ORACLE_DISPLAY  X display for Xvfb (default :5)
#   ORACLE_DIR      work dir (copies, framebuffer, pidfile, state)
#   LXIR            lxir binary for the closing semantic diff
set -eu

prefix=${ORACLE_PREFIX:-$HOME/.local/share/loxone-config/wine-oracle}
exe=${ORACLE_EXE:-/opt/loxone-config-bin/LoxoneConfig.exe}
display=${ORACLE_DISPLAY:-:5}
work=${ORACLE_DIR:-$HOME/.cache/lxir-oracle}
lxir=${LXIR:-lxir}
fbdir="$work/fb"

die() { echo "oracle: $*" >&2; exit 1; }
note() { echo "oracle: $*"; }

xd() { DISPLAY=$display xdotool "$@"; }

# --- Safety: never join a foreign instance -------------------------------
# A wine launch attaches to an existing wineserver for the same prefix.
# If that server serves another display, the file would open as a tab in
# THAT GUI (observed: the user's desktop instance) — refuse instead.
check_isolation() {
    [ -d "$prefix" ] || die "wineprefix $prefix does not exist"
    local pid env_disp
    for pid in $(pgrep -f 'LoxoneConfig\.exe' || true); do
        [ -r "/proc/$pid/environ" ] || continue
        if tr '\0' '\n' <"/proc/$pid/environ" | grep -qx "WINEPREFIX=$prefix"; then
            env_disp=$(tr '\0' '\n' <"/proc/$pid/environ" | sed -n 's/^DISPLAY=//p')
            if [ "$env_disp" != "$display" ]; then
                die "a LoxoneConfig (pid $pid) already runs in $prefix on DISPLAY=$env_disp — down that instance first"
            fi
        fi
    done
}

# --- ProgramData -----------------------------------------------------------
# Loxone Config reads its icon library, block images and templates from
# ProgramData/Loxone/Loxone Config <version>. The packaged launcher
# (/usr/bin/loxone-config of loxone-config-bin) copies them into its
# prefix whenever the package version changes; the oracle runs the exe
# directly, so it does the same for its own prefix, with the same stamp.
# Without them Loxone Config loads for minutes at full CPU and then says
# "Die Icon Library konnte nicht geladen werden".
seed_programdata() {
    local dir full dest stamp
    dir=$(dirname "$exe")
    if [ ! -d "$dir/programdata" ] || [ ! -f "$dir/VERSION" ]; then
        note "no programdata/ + VERSION next to $exe — ProgramData not seeded"
        return 0
    fi
    full=$(cat "$dir/VERSION")
    dest="$prefix/drive_c/ProgramData/Loxone/Loxone Config ${full%-*}"
    stamp="$prefix/.loxone-config-bin-version"
    [ "$(cat "$stamp" 2>/dev/null)" = "$full" ] && return 0
    mkdir -p "$dest"
    cp -r "$dir/programdata/." "$dest/"
    printf '%s' "$full" >"$stamp"
    note "seeded ProgramData for Loxone Config $full"
}

# --- Xvfb ----------------------------------------------------------------
xvfb_pid() { [ -f "$work/xvfb.pid" ] && cat "$work/xvfb.pid" || true; }

up() {
    mkdir -p "$fbdir"
    local pid; pid=$(xvfb_pid)
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
        note "Xvfb already up on $display (pid $pid)"
    else
        if xd getdisplaygeometry >/dev/null 2>&1; then
            die "display $display is served by something that is not our Xvfb — pick another ORACLE_DISPLAY"
        fi
        Xvfb "$display" -screen 0 2560x1440x24 -fbdir "$fbdir" \
            +extension GLX +render -noreset >/dev/null 2>&1 &
        echo $! >"$work/xvfb.pid"
        local i=0
        until xd getdisplaygeometry >/dev/null 2>&1; do
            i=$((i + 1)); [ "$i" -gt 50 ] && die "Xvfb did not come up on $display"
            sleep 0.2
        done
        note "Xvfb up on $display (pid $(xvfb_pid))"
    fi
    check_isolation
    seed_programdata
}

# --- GUI helpers ---------------------------------------------------------
# Window ids churn right after launch — always re-search, never cache.
# Loxone Config's message boxes share the main window's caption; at
# least 400 px high is the main window, anything lower a message box.
main_window() {
    local id h
    for id in $(xd search --onlyvisible --name 'Loxone Config' 2>/dev/null || true); do
        h=$(xd getwindowgeometry --shell "$id" 2>/dev/null | sed -n 's/^HEIGHT=//p')
        [ "${h:-0}" -ge 400 ] && { echo "$id"; return 0; }
    done
    return 0 # none yet; `id=$(main_window)` must not trip set -e
}

# Message boxes are the low ones; the id of the first, if any is up.
message_box() {
    local id h
    for id in $(xd search --onlyvisible --name 'Loxone Config' 2>/dev/null || true); do
        h=$(xd getwindowgeometry --shell "$id" 2>/dev/null | sed -n 's/^HEIGHT=//p')
        [ "${h:-400}" -lt 400 ] && { echo "$id"; return 0; }
    done
    return 0
}

# The auto-backup recovery dialog's caption is "LoxoneConfig" (no
# space). Answer No — the file must open exactly as compiled. Alt+N is
# unreliable on Xvfb; fall back to clicking where a Wine message box
# puts its No button (right half, bottom row).
dismiss_recovery() {
    local id g x y w h
    id=$(xd search --onlyvisible --name '^LoxoneConfig$' 2>/dev/null | head -1 || true)
    [ -n "$id" ] || return 0
    note "recovery dialog up — answering No"
    xd windowfocus "$id" 2>/dev/null || true
    xd key alt+n
    sleep 1
    id=$(xd search --onlyvisible --name '^LoxoneConfig$' 2>/dev/null | head -1 || true)
    [ -n "$id" ] || return 0
    g=$(xd getwindowgeometry --shell "$id")
    x=$(echo "$g" | sed -n 's/^X=//p'); y=$(echo "$g" | sed -n 's/^Y=//p')
    w=$(echo "$g" | sed -n 's/^WIDTH=//p'); h=$(echo "$g" | sed -n 's/^HEIGHT=//p')
    xd mousemove $((x + w * 62 / 100)) $((y + h * 84 / 100)) click 1
    sleep 1
}

# The QtWebEngine news panel renders as an unnamed white overlay child
# (~630x500) that swallows clicks — unmap it. Popups ignore Escape on
# Xvfb; unmapping is the reliable close for those too.
unmap_overlay() {
    local id g w h
    for id in $(xd search --onlyvisible --name '.*' 2>/dev/null || true); do
        g=$(xd getwindowgeometry --shell "$id" 2>/dev/null) || continue
        w=$(echo "$g" | sed -n 's/^WIDTH=//p'); h=$(echo "$g" | sed -n 's/^HEIGHT=//p')
        if [ "$w" -ge 600 ] && [ "$w" -le 700 ] && [ "$h" -ge 450 ] && [ "$h" -le 550 ]; then
            xd windowunmap "$id" 2>/dev/null || true
            note "unmapped ${w}x${h} overlay window $id"
        fi
    done
}

open_file() {
    local src=$1 copy winpath
    [ -f "$src" ] || die "no such file: $src"
    up
    mkdir -p "$work"
    copy="$work/$(basename "${src%.Loxone}").oracle.Loxone"
    cp -f "$src" "$copy"
    printf '%s\n' "$copy" >"$work/open.path"
    winpath="Z:$(realpath "$copy" | tr / '\\')"
    note "opening copy $copy"
    # The launcher must not inherit our stdout: it lives as long as the
    # GUI, so `oracle.sh run … | tail` would wait for Loxone Config to
    # exit and hold back every message, the failure included.
    (cd "$(dirname "$exe")" &&
        DISPLAY=$display WINEPREFIX=$prefix LIBGL_ALWAYS_SOFTWARE=1 \
        nohup wine "$exe" "$winpath" &) </dev/null >/dev/null 2>&1
    # Poll for the main window; answer the recovery dialog whenever it
    # shows (it can precede or follow the main window).
    local i=0
    while :; do
        dismiss_recovery
        [ -n "$(main_window)" ] && break
        i=$((i + 1)); [ "$i" -gt 90 ] && die "no Loxone Config window after 90s"
        sleep 1
    done
    # Loading a big config continues after the window exists; give the
    # dialog one more chance, then clear the news overlay.
    sleep 8
    dismiss_recovery
    unmap_overlay
    note "open: $(basename "$copy")"
}

save_file() {
    local copy sum id i
    [ -f "$work/open.path" ] || die "nothing open (no $work/open.path)"
    copy=$(cat "$work/open.path")
    sum=$(md5sum "$copy" | cut -d' ' -f1)
    id=$(main_window); [ -n "$id" ] || die "no Loxone Config window on $display"
    xd windowfocus "$id" 2>/dev/null || true
    xd key ctrl+s
    i=0
    while [ "$(md5sum "$copy" | cut -d' ' -f1)" = "$sum" ]; do
        i=$((i + 1))
        if [ "$i" -gt 30 ]; then
            [ -n "$(message_box)" ] &&
                die "file unchanged 60s after Ctrl+S — a Loxone Config message box is up; check 'oracle.sh shot'"
            die "file unchanged 60s after Ctrl+S — check 'oracle.sh shot'"
        fi
        sleep 2
    done
    sleep 2 # let the write finish
    note "saved: $copy"
}

shot() {
    local out=${1:-$work/shot.png}
    magick "xwd:$fbdir/Xvfb_screen0" "$out"
    note "screenshot: $out"
}

status() {
    local pid; pid=$(xvfb_pid)
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
        echo "Xvfb: up on $display (pid $pid)"
    else
        echo "Xvfb: down"
    fi
    pgrep -f 'LoxoneConfig\.exe' | while read -r p; do
        [ -r "/proc/$p/environ" ] || continue
        echo "LoxoneConfig pid $p: $(tr '\0' '\n' <"/proc/$p/environ" | grep -E '^DISPLAY=|^WINEPREFIX=' | tr '\n' ' ')"
    done
    [ -f "$work/open.path" ] && echo "open copy: $(cat "$work/open.path")" || true
}

down() {
    WINEPREFIX=$prefix wineserver -k 2>/dev/null || true
    local pid; pid=$(xvfb_pid)
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
        # Exact pid from our pidfile — never pkill by pattern (pattern
        # kills have matched the invoking shell itself before).
        kill "$pid" 2>/dev/null || true
    fi
    rm -f "$work/xvfb.pid"
    note "down"
}

run() {
    local src=$1 out= keep=0 copy
    shift
    while [ $# -gt 0 ]; do
        case $1 in
        --out) out=$2; shift 2 ;;
        --keep) keep=1; shift ;;
        *) die "unknown run option: $1" ;;
        esac
    done
    open_file "$src"
    save_file
    copy=$(cat "$work/open.path")
    [ -n "$out" ] && { cp -f "$copy" "$out"; note "saved copy: $out"; copy=$out; }
    if command -v "$lxir" >/dev/null 2>&1; then
        echo "--- semantic diff (compiled -> GUI-saved) ---"
        "$lxir" diff "$src" "$copy" || true
    else
        note "lxir not found — diff yourself: lxir diff '$src' '$copy'"
    fi
    [ "$keep" = 1 ] || down
}

case ${1:-} in
run) shift; [ $# -ge 1 ] || die "usage: oracle.sh run <file.Loxone> [--out <saved>] [--keep]"; run "$@" ;;
up) up ;;
open) shift; [ $# -eq 1 ] || die "usage: oracle.sh open <file.Loxone>"; open_file "$1" ;;
save) save_file ;;
shot) shift; shot "${1:-}" ;;
status) status ;;
down) down ;;
*) sed -n '2,16p' "$0"; exit 1 ;;
esac
