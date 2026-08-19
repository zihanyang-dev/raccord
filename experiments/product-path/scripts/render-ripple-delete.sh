#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
media_dir="$root/fixtures/media"
output_dir="$root/fixtures/output"
concat_file="$output_dir/concat.txt"
output_file="$output_dir/ripple-delete.mp4"

mkdir -p "$output_dir"

mise exec -- cargo run --quiet \
  --manifest-path "$root/Cargo.toml" \
  --bin raccord-product-path-experiment \
  -- write-concat "$concat_file" "$media_dir"

ffmpeg -hide_banner -loglevel error -y \
  -f concat -safe 0 -i "$concat_file" \
  -c copy \
  "$output_file"

ffprobe -v error \
  -show_entries format=duration:stream=codec_type,codec_name,nb_frames \
  -of json \
  "$output_file"

duration="$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$output_file")"
video_frames="$(ffprobe -v error -select_streams v:0 -show_entries stream=nb_frames -of csv=p=0 "$output_file")"

python3 - "$duration" "$video_frames" <<'PY'
import sys

duration = float(sys.argv[1])
video_frames = int(sys.argv[2])

if not 5.9 <= duration <= 6.2:
    raise SystemExit(f"unexpected duration: {duration}")
if video_frames != 144:
    raise SystemExit(f"unexpected video frame count: {video_frames}")

print(f"validated duration={duration:.3f}s video_frames={video_frames}")
PY
