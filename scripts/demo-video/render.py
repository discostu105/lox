#!/usr/bin/env python3
"""Render an asciicast from record.py into an MP4 with captions and key overlay.

The terminal is emulated by pyte, drawn cell by cell with Pillow in full colour,
and piped to ffmpeg. Captions come from the cast's marker events and go into the
video and into a .srt file next to it.

    uv run --with pyte --with pillow --with fonttools \\
        scripts/demo-video/render.py [demo.cast] [demo.mp4]
    ... render.py demo.cast --text    # text snapshots per caption, for review
"""

import json
import subprocess
import sys
from functools import lru_cache
from pathlib import Path

import pyte
from fontTools.ttLib import TTFont
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent.parent

W, H = 1920, 1080
FPS = 30
FONT_SIZE = 22
BG = (9, 12, 16)  # page behind the window
TERM_BG = (13, 17, 23)  # terminal default background
TERM_FG = (201, 209, 217)
CHROME = (22, 27, 34)  # window title bar
ACCENT = (126, 231, 135)

ANSI = {
    "black": (13, 17, 23), "red": (255, 123, 114), "green": (126, 231, 135),
    "brown": (227, 179, 65), "yellow": (227, 179, 65), "blue": (121, 192, 255),
    "magenta": (210, 168, 255), "cyan": (86, 212, 221), "white": (201, 209, 217),
    "brightblack": (110, 118, 129), "brightred": (255, 161, 152),
    "brightgreen": (86, 211, 100), "brightbrown": (227, 179, 65),
    "brightyellow": (227, 179, 65), "brightblue": (165, 214, 255),
    "brightmagenta": (210, 168, 255), "brightcyan": (86, 212, 221),
    "brightwhite": (240, 246, 252),
}


def fc(pattern: str) -> str | None:
    try:
        p = subprocess.run(["fc-match", "-f", "%{file}", pattern],
                           capture_output=True, text=True, check=True).stdout.strip()
        return p or None
    except (OSError, subprocess.CalledProcessError):
        return None


class Fonts:
    """A monospace font with fallbacks, chosen per glyph by the fonts' cmaps."""

    def __init__(self, size: int):
        regular = [
            "JetBrainsMono Nerd Font:style=Regular", "JetBrains Mono:style=Regular",
            "DejaVu Sans Mono", "Noto Sans Symbols2", "Noto Sans Symbols", "Symbola",
        ]
        bold = ["JetBrainsMono Nerd Font:style=Bold", "JetBrains Mono:style=Bold",
                "DejaVu Sans Mono:style=Bold"]
        self.regular = self._load(regular, size)
        self.bold = self._load(bold, size) + self.regular[1:]
        main = self.regular[0][0]
        self.cw = round(main.getlength("M"))
        asc, desc = main.getmetrics()
        self.ch = asc + desc
        self.asc = asc

    @staticmethod
    def _load(patterns, size):
        out, seen = [], set()
        for pat in patterns:
            path = fc(pat)
            if not path or path in seen:
                continue
            seen.add(path)
            try:
                cmap = TTFont(path, fontNumber=0, lazy=True).getBestCmap()
                out.append((ImageFont.truetype(path, size), set(cmap)))
            except Exception:  # noqa: BLE001 — skip unreadable fonts
                continue
        if not out:
            sys.exit("no usable monospace font found (install JetBrains Mono or DejaVu Sans Mono)")
        return out

    @lru_cache(maxsize=4096)
    def pick(self, ch: str, bold: bool):
        cp = ord(ch[0])
        for f, cmap in (self.bold if bold else self.regular):
            if cp in cmap:
                return f
        return self.regular[0][0]


def colour(c: str, default):
    if c == "default":
        return default
    if c in ANSI:
        return ANSI[c]
    if len(c) == 6:
        try:
            return tuple(int(c[i:i + 2], 16) for i in (0, 2, 4))
        except ValueError:
            pass
    return default


class Term:
    def __init__(self, cols, rows, fonts: Fonts):
        self.cols, self.rows, self.f = cols, rows, fonts
        self.screen = pyte.Screen(cols, rows)
        self.stream = pyte.Stream(self.screen)
        self.img = Image.new("RGB", (cols * fonts.cw, rows * fonts.ch), TERM_BG)
        self.screen.dirty.update(range(rows))

    def feed(self, s: str):
        self.stream.feed(s)

    def paint(self) -> bool:
        if not self.screen.dirty:
            return False
        cw, chh = self.f.cw, self.f.ch
        for y in sorted(self.screen.dirty):
            if y >= self.rows:
                continue
            # each row is drawn on its own strip: glyphs that overhang their cell
            # are clipped instead of leaving marks on the neighbouring rows
            row = Image.new("RGB", (self.cols * cw, chh), TERM_BG)
            d = ImageDraw.Draw(row)
            line = self.screen.buffer[y]
            for x in range(self.cols):
                c = line[x]
                if c.data == "":
                    continue  # second half of a wide char
                fg = colour(c.fg, TERM_FG)
                bg = colour(c.bg, TERM_BG)
                if c.reverse:
                    fg, bg = bg, fg
                wide = 2 if x + 1 < self.cols and line[x + 1].data == "" else 1
                x0 = x * cw
                if bg != TERM_BG:
                    d.rectangle([x0, 0, x0 + cw * wide - 1, chh - 1], fill=bg)
                if c.data.strip():
                    d.text((x0, 0), c.data, font=self.f.pick(c.data, c.bold), fill=fg)
                if c.underscore:
                    d.line([x0, chh - 3, x0 + cw * wide - 1, chh - 3], fill=fg)
            self.img.paste(row, (0, y * chh))
        self.screen.dirty.clear()
        return True

    def text(self) -> str:
        return "\n".join(self.screen.display)


def load(cast: Path):
    lines = cast.read_text().splitlines()
    header = json.loads(lines[0])
    return header, [json.loads(line) for line in lines[1:]]


def captions(events, t_from, t_to):
    """[(start, end, text)] from marker events, clipped to the video."""
    out, cur = [], None
    for t, kind, data in events:
        if kind != "m":
            continue
        if cur and cur[2]:
            out.append((cur[0], t, cur[2]))
        cur = (t, None, data)
    if cur and cur[2]:
        out.append((cur[0], t_to, cur[2]))
    return [(max(a, t_from) - t_from, min(b, t_to) - t_from, s) for a, b, s in out if b > t_from]


def srt(caps) -> str:
    def ts(t):
        ms = int(round(t * 1000))
        return f"{ms // 3600000:02}:{ms // 60000 % 60:02}:{ms // 1000 % 60:02},{ms % 1000:03}"

    return "\n".join(f"{i}\n{ts(a)} --> {ts(b)}\n{s}\n" for i, (a, b, s) in enumerate(caps, 1))


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    cast = Path(args[0]) if args else ROOT / "target/demo-video/demo.cast"
    out = Path(args[1]) if len(args) > 1 else cast.with_suffix(".mp4")
    header, events = load(cast)
    cols, rows = header["width"], header["height"]
    fonts = Fonts(FONT_SIZE)
    term = Term(cols, rows, fonts)

    marks = [e[0] for e in events if e[1] == "m"]
    t_from = max(0.0, marks[0] - 0.4) if marks else 0.0
    t_to = marks[-1] + 0.2 if marks else events[-1][0]
    caps = captions(events, t_from, t_to)
    keys = [(t - t_from, d) for t, k, d in events if k == "i"]

    if "--text" in sys.argv:
        i = 0
        for a, _, s in caps:
            while i < len(events) and events[i][0] - t_from <= a + 1.2:
                if events[i][1] == "o":
                    term.feed(events[i][2])
                i += 1
            print(f"\n===== {a:5.1f}s  {s}\n{term.text()}")
        return

    # layout: window (title bar + terminal) centred above a caption band
    tw, th = term.img.size
    bar = 52
    cap_h = 120
    wx = (W - tw) // 2
    wy = max(16, (H - cap_h - (th + bar)) // 2)
    base = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(base)
    d.rounded_rectangle([wx - 12, wy, wx + tw + 12, wy + bar + th + 12], radius=14, fill=TERM_BG)
    d.rounded_rectangle([wx - 12, wy, wx + tw + 12, wy + bar], radius=14, fill=CHROME)
    d.rectangle([wx - 12, wy + bar - 14, wx + tw + 12, wy + bar], fill=CHROME)
    for i, c in enumerate([(255, 95, 86), (255, 189, 46), (39, 201, 63)]):
        cx = wx + 12 + i * 26
        d.ellipse([cx - 7, wy + bar // 2 - 7, cx + 7, wy + bar // 2 + 7], fill=c)
    ui = ImageFont.truetype(fc("Fira Sans:style=Medium") or fc("DejaVu Sans"), 22)
    title = "lox tui --demo"
    d.text((W // 2 - d.textlength(title, font=ui) / 2, wy + 13), title, font=ui, fill=(139, 148, 158))
    cap_font = ImageFont.truetype(fc("Fira Sans:style=SemiBold") or fc("DejaVu Sans:style=Bold"), 40)
    key_font_path = fonts.regular[0][0].path
    key_font = ImageFont.truetype(key_font_path, 28)
    tx, ty = wx, wy + bar + 6

    ff = subprocess.Popen(
        ["ffmpeg", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24",
         "-s", f"{W}x{H}", "-r", str(FPS), "-i", "-", "-c:v", "libx264", "-preset", "slow",
         "-crf", "18", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(out)],
        stdin=subprocess.PIPE,
    )
    n = int((t_to - t_from) * FPS)
    ei = 0
    # everything before the video starts
    while ei < len(events) and events[ei][0] < t_from:
        if events[ei][1] == "o":
            term.feed(events[ei][2])
        ei += 1
    for fi in range(n):
        t = fi / FPS
        while ei < len(events) and events[ei][0] - t_from <= t:
            if events[ei][1] == "o":
                term.feed(events[ei][2])
            ei += 1
        term.paint()
        frame = base.copy()
        frame.paste(term.img, (tx, ty))
        fd = ImageDraw.Draw(frame, "RGBA")
        # caption, fading in
        for a, b, s in caps:
            if a <= t < b:
                alpha = int(255 * min(1.0, (t - a) / 0.25, (b - t) / 0.15))
                cy = H - cap_h // 2 - 26
                tl = fd.textlength(s, font=cap_font)
                fd.text((W / 2 - tl / 2, cy), s, font=cap_font, fill=(240, 246, 252, alpha))
        # recent keys, as pills in the title bar
        recent = [k for kt, k in keys if 0 <= t - kt < 1.6][-8:]
        x = wx + tw - 4
        for k in reversed(recent):
            kw = fd.textlength(k, font=key_font) + 20
            x -= kw + 8
            fd.rounded_rectangle([x, wy + 6, x + kw, wy + bar - 6], radius=9, fill=(48, 54, 61, 255))
            fd.text((x + 10, wy + 8), k, font=key_font, fill=ACCENT)
        ff.stdin.write(frame.tobytes())
        if fi % (FPS * 5) == 0:
            print(f"\r{fi / FPS:5.1f}/{n / FPS:.1f} s", end="", flush=True)
    ff.stdin.close()
    ff.wait()
    out.with_suffix(".srt").write_text(srt(caps))
    print(f"\nwrote {out} ({n / FPS:.1f} s) and {out.with_suffix('.srt').name}")


if __name__ == "__main__":
    main()
