#!/usr/bin/env python3
"""Record `lox tui --demo` following script.toml into an asciicast v2 file.

The TUI runs in a pseudo-terminal of a fixed size. Keys are sent on a fixed
schedule, and all output is logged with timestamps. Captions and key presses go
into the cast as marker ("m") and input ("i") events, so render.py (or
`asciinema play`) can replay the exact session.

    python3 scripts/demo-video/record.py [script.toml] [out.cast]
"""

import fcntl
import json
import os
import pty
import select
import signal
import struct
import sys
import termios
import time
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

KEYS = {
    "Enter": b"\r",
    "Esc": b"\x1b",
    "Tab": b"\t",
    "S-Tab": b"\x1b[Z",
    "Space": b" ",
    "Up": b"\x1b[A",
    "Down": b"\x1b[B",
    "Right": b"\x1b[C",
    "Left": b"\x1b[D",
    "Backspace": b"\x7f",
}

# How keys are shown in the video's key overlay
LABELS = {"Enter": "⏎", "Esc": "Esc", "Tab": "⇥", "S-Tab": "⇤", "Space": "space",
          "Up": "↑", "Down": "↓", "Left": "←", "Right": "→", "Backspace": "⌫"}

# Terminal queries the TUI makes at startup, and what a plain xterm answers.
# Without an answer crossterm waits ~2 s for the keyboard-enhancement probe.
REPLIES = {
    b"\x1b[c": b"\x1b[?62;22c",  # primary device attributes (no kitty keyboard)
    b"\x1b[6n": b"\x1b[1;1R",  # cursor position
    b"\x1b]11;?": b"\x1b]11;rgb:0d0d/1111/1717\x1b\\",  # background colour
}


def key_bytes(k: str) -> bytes:
    if k in KEYS:
        return KEYS[k]
    if k.startswith("C-") and len(k) == 3:
        return bytes([ord(k[2].lower()) & 0x1F])
    return k.encode()


def main() -> None:
    script = Path(sys.argv[1]) if len(sys.argv) > 1 else HERE / "script.toml"
    out = Path(sys.argv[2]) if len(sys.argv) > 2 else ROOT / "target/demo-video/demo.cast"
    cfg = tomllib.loads(script.read_text())
    term = cfg["terminal"]
    cols, rows = term["cols"], term["rows"]
    binary = ROOT / term.get("binary", "target/release/lox")
    args = [str(binary), *term.get("args", ["tui", "--demo"])]
    key_gap = cfg.get("timing", {}).get("key_gap", 0.35)
    type_gap = cfg.get("timing", {}).get("type_gap", 0.09)
    default_wait = cfg.get("timing", {}).get("wait", 1.5)

    env = dict(os.environ)
    env.update(TERM="xterm-256color", COLORTERM="truecolor", LANG="C.UTF-8")
    env.update(term.get("env", {}))
    # a fresh HOME: no saved TUI state or preferences, no real Miniserver config
    home = out.parent / "home"
    home.mkdir(parents=True, exist_ok=True)
    env["HOME"] = str(home)
    env.pop("LOX_CONFIG", None)

    pid, fd = pty.fork()
    if pid == 0:
        os.execvpe(args[0], args, env)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))

    t0 = time.monotonic()
    events: list[list] = []
    tail = b""

    def now() -> float:
        return time.monotonic() - t0

    def pump(until: float) -> None:
        """Read output (answering queries) until the absolute time `until`."""
        nonlocal tail
        while True:
            left = until - now()
            if left <= 0:
                return
            r, _, _ = select.select([fd], [], [], min(left, 0.02))
            if not r:
                continue
            try:
                data = os.read(fd, 65536)
            except OSError:
                return
            if not data:
                return
            events.append([round(now(), 4), "o", data.decode("utf-8", "replace")])
            scan = tail + data
            for q, a in REPLIES.items():
                if q in scan:
                    os.write(fd, a)
            tail = scan[-8:]

    def send(k: str, label: str | None = None) -> None:
        os.write(fd, key_bytes(k))
        events.append([round(now(), 4), "i", label if label is not None else LABELS.get(k, k)])

    pump(now() + cfg.get("timing", {}).get("startup", 2.0))
    for step in cfg["step"]:
        if "caption" in step:
            events.append([round(now(), 4), "m", step["caption"]])
        for k in step.get("keys", []):
            send(k)
            pump(now() + step.get("key_gap", key_gap))
        if "type" in step:
            for ch in step["type"]:
                send(ch if ch != " " else "Space", ch if ch != " " else "␣")
                pump(now() + type_gap)
        pump(now() + step.get("wait", default_wait))

    events.append([round(now(), 4), "m", ""])
    os.kill(pid, signal.SIGTERM)
    try:
        os.waitpid(pid, 0)
    except ChildProcessError:
        pass

    out.parent.mkdir(parents=True, exist_ok=True)
    header = {
        "version": 2,
        "width": cols,
        "height": rows,
        "timestamp": int(time.time()),
        "env": {"TERM": "xterm-256color"},
        "title": cfg.get("title", "lox tui"),
    }
    with out.open("w") as f:
        f.write(json.dumps(header) + "\n")
        for e in events:
            f.write(json.dumps(e, ensure_ascii=False) + "\n")
    print(f"recorded {now():.1f} s, {len(events)} events → {out}")


if __name__ == "__main__":
    main()
