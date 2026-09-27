# Demo video

A scripted, reproducible screen recording of `lox tui --demo` with captions,
for the README and social posts.

```bash
scripts/demo-video/make.sh
# → target/demo-video/lox-tui-demo.mp4   1920×1080, 30 fps, captions burned in
#   target/demo-video/lox-tui-demo.srt   the captions as subtitles
#   target/demo-video/lox-tui-demo.cast  the raw session (asciicast v2)
```

## How it works

1. **`script.toml`** is the storyboard. Each step has a caption, the keys to
   press, and how long to wait.
2. **`record.py`** runs `lox tui --demo` in a pseudo-terminal of a fixed size
   (120×28). It sends the keys on schedule and logs the output with timestamps
   as an asciicast. Captions and keys go into the cast as marker and input
   events. It uses a fresh `HOME`, so your saved TUI state and config are never
   read, and no Miniserver is contacted.
3. **`render.py`** replays the cast through a terminal emulator (pyte). It draws
   every frame in full colour with Pillow, in JetBrains Mono, falling back to
   DejaVu Sans Mono. It adds a window frame, the captions and a key overlay, and
   pipes the frames to ffmpeg (x264).

`LOX_DEMO_HOUR=12.5` pins the demo's simulated time of day, and with it the
clock, the PV output and the charts. That keeps the video the same whenever it
is recorded.

Needs: Python ≥ 3.11, [uv](https://docs.astral.sh/uv/), ffmpeg, fontconfig
(`fc-match`).

To review a script quickly without rendering, print a text snapshot for each
caption:

```bash
uv run --with pyte --with pillow --with fonttools \
    scripts/demo-video/render.py target/demo-video/lox-tui-demo.cast --text
```
