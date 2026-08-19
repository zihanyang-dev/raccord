#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
media_dir="$root/fixtures/long-media"
timeline_file="$root/fixtures/long-timeline.json"

mkdir -p "$media_dir"

make_clip() {
  local name="$1"
  local color="$2"
  local frequency="$3"

  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "color=c=${color}:s=640x360:r=24:d=8" \
    -f lavfi -i "sine=frequency=${frequency}:sample_rate=48000:duration=8" \
    -shortest \
    -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
    -c:a aac -b:a 128k \
    -movflags +faststart \
    "$media_dir/${name}.mp4"
}

make_clip a red 440
make_clip b green 550
make_clip c blue 660
make_clip d yellow 770
make_clip e purple 880
make_clip f orange 990

python3 - "$timeline_file" <<'PY'
from __future__ import annotations

import json
import sys
from pathlib import Path

output = Path(sys.argv[1])
clips = [
    {"id": name, "source": f"asset://{name}", "duration_frames": 192, "audio_gain_db_milli": 0}
    for name in "abcdef"
]
output.write_text(
    json.dumps(
        {"revision": 0, "clips": clips, "markers": [], "subtitles": []},
        indent=2,
    )
    + "\n",
    encoding="utf-8",
)
PY

printf 'Generated 48-second fixture media in %s\n' "$media_dir"
printf 'Wrote timeline fixture to %s\n' "$timeline_file"
