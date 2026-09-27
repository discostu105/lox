#!/usr/bin/env bash
# Build lox, record the scripted demo session, render the video.
#   scripts/demo-video/make.sh [script.toml]
# Output: target/demo-video/lox-tui-demo.{mp4,srt,cast,gif}
# (the README embeds a copy of the GIF: docs/media/lox-tui-demo.gif)
set -euo pipefail
cd "$(dirname "$0")/../.."
script="${1:-scripts/demo-video/script.toml}"
out=target/demo-video
cargo build --release
python3 scripts/demo-video/record.py "$script" "$out/lox-tui-demo.cast"
uv run -q --with pyte==0.8.2 --with pillow==12.3.0 --with fonttools==4.66.0 \
    scripts/demo-video/render.py "$out/lox-tui-demo.cast" "$out/lox-tui-demo.mp4"
# the README preview: 1280 px, 10 fps, a 128-colour palette (~4 MB)
ffmpeg -loglevel error -y -i "$out/lox-tui-demo.mp4" \
    -vf "fps=10,scale=1280:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle" \
    "$out/lox-tui-demo.gif"
echo "wrote $out/lox-tui-demo.gif"
