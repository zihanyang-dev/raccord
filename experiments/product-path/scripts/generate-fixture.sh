#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
media_dir="$root/fixtures/media"

mkdir -p "$media_dir"

make_clip() {
  local name="$1"
  local color="$2"
  local frequency="$3"

  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "color=c=${color}:s=640x360:r=24:d=3" \
    -f lavfi -i "sine=frequency=${frequency}:sample_rate=48000:duration=3" \
    -shortest \
    -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
    -c:a aac -b:a 128k \
    -movflags +faststart \
    "$media_dir/${name}.mp4"
}

make_clip a red 440
make_clip b green 660
make_clip c blue 880

printf 'Generated fixture media in %s\n' "$media_dir"
