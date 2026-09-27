#!/usr/bin/env bash
# Build lox, record the scripted demo session, render the video.
#   scripts/demo-video/make.sh [script.toml]
# Output: target/demo-video/lox-tui-demo.{mp4,srt,cast}
set -euo pipefail
cd "$(dirname "$0")/../.."
script="${1:-scripts/demo-video/script.toml}"
out=target/demo-video
cargo build --release
python3 scripts/demo-video/record.py "$script" "$out/lox-tui-demo.cast"
uv run -q --with pyte==0.8.2 --with pillow==12.3.0 --with fonttools==4.66.0 \
    scripts/demo-video/render.py "$out/lox-tui-demo.cast" "$out/lox-tui-demo.mp4"
