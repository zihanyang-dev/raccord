#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
media_dir="$root/fixtures/long-media"
output_dir="$root/fixtures/output"
timeline_file="$root/fixtures/long-timeline.json"
state_file="$output_dir/long-metadata-state.json"
output_file="$output_dir/long-metadata.mp4"
frame_file="$output_dir/long-metadata-subtitle-frame.png"
mkdir -p "$output_dir"

mise exec -- cargo run --quiet \
    --manifest-path "$root/Cargo.toml" \
    --bin jsonl_api -- --timeline "$timeline_file" >"$state_file" <<'JSONL'
{"tool":"plan_edit","args":{"base_revision":0,"edits":[{"op":"set_audio_gain","clip_id":"d","gain_db_milli":-3000},{"op":"add_marker","id":"long-marker-1","clip_id":"d","label":"middle beat"},{"op":"add_subtitle","id":"long-subtitle-1","clip_id":"b","text":"HELLO"}]}}
{"tool":"commit_edit","args":{"plan_token":"plan-1"}}
{"tool":"inspect"}
{"tool":"verify"}
JSONL

python3 - "$state_file" "$media_dir" "$output_file" "$frame_file" <<'PY'
from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

state_path, media_dir, output_path, frame_path = map(Path, sys.argv[1:])
responses = [
    json.loads(line)
    for line in state_path.read_text(encoding="utf-8").splitlines()
    if line.strip()
]
if not all(response["ok"] for response in responses[:3]):
    raise SystemExit(f"long metadata transaction failed: {responses}")
if responses[3]["result"] != {"pass": True, "version": 1}:
    raise SystemExit(f"long metadata verification failed: {responses[3]}")

state = responses[2]["result"]
if len(state["clips"]) != 6:
    raise SystemExit(f"unexpected long clip count: {len(state['clips'])}")
if state["markers"] != [{"id": "long-marker-1", "clip_id": "d", "label": "middle beat"}]:
    raise SystemExit(f"unexpected long markers: {state['markers']}")
if state["subtitles"] != [{"id": "long-subtitle-1", "clip_id": "b", "text": "HELLO"}]:
    raise SystemExit(f"unexpected long subtitles: {state['subtitles']}")

clips = state["clips"]
placements = {placement["id"]: placement for placement in state["placements"]}
filters = []
video_labels = []
audio_labels = []
for index, clip in enumerate(clips):
    video_labels.append(f"[v{index}]")
    audio_labels.append(f"[a{index}]")
    filters.append(f"[{index}:v]setpts=PTS-STARTPTS[v{index}]")
    factor = 10 ** (clip.get("audio_gain_db_milli", 0) / 20000)
    filters.append(f"[{index}:a]asetpts=PTS-STARTPTS,volume={factor:.8f}[a{index}]")
filters.append("".join(video_labels) + "concat=n=6:v=1:a=0[vcat]")
filters.append("".join(audio_labels) + "concat=n=6:v=0:a=1[acat]")
video_output = "[vcat]"

glyphs = {
    "E": ["11111", "10000", "10000", "11110", "10000", "10000", "11111"],
    "H": ["10001", "10001", "10001", "11111", "10001", "10001", "10001"],
    "L": ["10000", "10000", "10000", "10000", "10000", "10000", "11111"],
    "O": ["01110", "10001", "10001", "10001", "10001", "10001", "01110"],
}
subtitle_path = output_path.parent / "long-subtitle-1.ppm"
text = "HELLO"
scale = 8
width = 640
height = 80
pixels = bytearray(width * height * 3)
text_width = (len(text) * 5 + (len(text) - 1) * 2) * scale
origin_x = (width - text_width) // 2
origin_y = (height - 7 * scale) // 2
for char_index, character in enumerate(text):
    for row, bitmap_row in enumerate(glyphs[character]):
        for column, value in enumerate(bitmap_row):
            if value != "1":
                continue
            for y in range(row * scale, (row + 1) * scale):
                for x in range(column * scale, (column + 1) * scale):
                    pixel = ((origin_y + y) * width + origin_x + char_index * 7 * scale + column * scale + x) * 3
                    pixels[pixel : pixel + 3] = b"\xff\xff\xff"
subtitle_path.write_bytes(f"P6\n{width} {height}\n255\n".encode() + pixels)

subtitle = state["subtitles"][0]
placement = placements[subtitle["clip_id"]]
start = placement["start_frame"] / 24
end = (placement["start_frame"] + placement["duration_frames"]) / 24
filters.append("[6:v]setpts=PTS-STARTPTS[subtitle]")
filters.append(
    f"{video_output}[subtitle]overlay=0:280:enable='between(t,{start:.3f},{end:.3f})'[vout]"
)

command = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y"]
for clip in clips:
    source = clip["source"].removeprefix("asset://")
    command.extend(["-i", str(media_dir / f"{source}.mp4")])
command.extend(["-loop", "1", "-framerate", "24", "-t", "48", "-i", str(subtitle_path)])
command.extend([
    "-filter_complex", ";".join(filters),
    "-map", "[vout]", "-map", "[acat]",
    "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
    "-c:a", "aac", "-b:a", "128k", "-shortest", str(output_path),
])
subprocess.run(command, check=True)

probe = subprocess.check_output(
    ["ffprobe", "-v", "error", "-show_entries", "format=duration:stream=codec_type,nb_frames", "-of", "json", str(output_path)],
    text=True,
)
data = json.loads(probe)
duration = float(data["format"]["duration"])
video = next(stream for stream in data["streams"] if stream["codec_type"] == "video")
if not 47.7 <= duration <= 48.3:
    raise SystemExit(f"unexpected long duration: {duration}")
if int(video["nb_frames"]) != 1152:
    raise SystemExit(f"unexpected long video frames: {video['nb_frames']}")
subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-ss", "10", "-i", str(output_path), "-frames:v", "1", str(frame_path)], check=True)

def mean_volume(path: Path, start: str) -> float:
    result = subprocess.run(["ffmpeg", "-hide_banner", "-ss", start, "-t", "3", "-i", str(path), "-af", "volumedetect", "-f", "null", "-"], capture_output=True, text=True, check=True)
    match = re.search(r"mean_volume:\s+(-?[0-9.]+) dB", result.stderr)
    if not match:
        raise SystemExit(f"mean volume missing for {path}")
    return float(match.group(1))

source_volume = mean_volume(media_dir / "d.mp4", "0")
rendered_volume = mean_volume(output_path, "24")
delta = rendered_volume - source_volume
if not -4.2 <= delta <= -1.8:
    raise SystemExit(f"unexpected long gain delta: {delta:.2f} dB")
print(f"validated long render duration={duration:.3f}s frames={video['nb_frames']} gain_delta={delta:.2f}dB subtitle_frame={frame_path}")
PY
