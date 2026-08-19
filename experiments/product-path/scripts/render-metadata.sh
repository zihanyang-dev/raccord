#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
media_dir="$root/fixtures/media"
output_dir="$root/fixtures/output"
state_file="$output_dir/metadata-state.json"
output_file="$output_dir/metadata.mp4"
frame_file="$output_dir/metadata-subtitle-frame.png"
mkdir -p "$output_dir"

mise exec -- cargo run --quiet \
    --manifest-path "$root/Cargo.toml" \
    --bin jsonl_api >"$state_file" <<'JSONL'
{"tool":"plan_edit","args":{"base_revision":0,"edits":[{"op":"set_audio_gain","clip_id":"b","gain_db_milli":-3000},{"op":"add_marker","id":"marker-1","clip_id":"b","label":"beat drop"},{"op":"add_subtitle","id":"subtitle-1","clip_id":"b","text":"Hello"}]}}
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
responses = [json.loads(line) for line in state_path.read_text(encoding="utf-8").splitlines() if line.strip()]

if not responses[0]["ok"] or not responses[1]["ok"] or not responses[2]["ok"]:
    raise SystemExit(f"metadata state transaction failed: {responses}")
if responses[3]["result"] != {"pass": True, "version": 1}:
    raise SystemExit(f"metadata state verification failed: {responses[3]}")

state = responses[2]["result"]
clips = state["clips"]
placements = {placement["id"]: placement for placement in state["placements"]}
subtitles = state["subtitles"]
if state["markers"] != [{"id": "marker-1", "clip_id": "b", "label": "beat drop"}]:
    raise SystemExit(f"unexpected markers: {state['markers']}")
if subtitles != [{"id": "subtitle-1", "clip_id": "b", "text": "Hello"}]:
    raise SystemExit(f"unexpected subtitles: {subtitles}")

filters = []
video_labels = []
audio_labels = []
for index, clip in enumerate(clips):
    video_labels.append(f"[v{index}]")
    audio_labels.append(f"[a{index}]")
    filters.append(f"[{index}:v]setpts=PTS-STARTPTS[v{index}]")
    gain = clip.get("audio_gain_db_milli", 0)
    factor = 10 ** (gain / 20000)
    filters.append(f"[{index}:a]asetpts=PTS-STARTPTS,volume={factor:.8f}[a{index}]")

filters.append("".join(video_labels) + f"concat=n={len(clips)}:v=1:a=0[vcat]")
filters.append("".join(audio_labels) + f"concat=n={len(clips)}:v=0:a=1[acat]")
video_output = "[vcat]"
subtitle_images = []
glyphs = {
    " ": ["00000"] * 7,
    "A": ["01110", "10001", "10001", "11111", "10001", "10001", "10001"],
    "B": ["11110", "10001", "10001", "11110", "10001", "10001", "11110"],
    "D": ["11110", "10001", "10001", "10001", "10001", "10001", "11110"],
    "E": ["11111", "10000", "10000", "11110", "10000", "10000", "11111"],
    "H": ["10001", "10001", "10001", "11111", "10001", "10001", "10001"],
    "L": ["10000", "10000", "10000", "10000", "10000", "10000", "11111"],
    "O": ["01110", "10001", "10001", "10001", "10001", "10001", "01110"],
    "P": ["11110", "10001", "10001", "11110", "10000", "10000", "10000"],
    "T": ["11111", "00100", "00100", "00100", "00100", "00100", "00100"],
}

def write_subtitle_image(path: Path, text: str) -> None:
    scale = 8
    normalized = text.upper()
    if any(character not in glyphs for character in normalized):
        raise SystemExit(f"subtitle contains unsupported bitmap glyph: {text!r}")
    width = max(640, (len(normalized) * 5 + max(0, len(normalized) - 1) * 2) * scale + 32)
    height = 80
    pixels = bytearray(width * height * 3)
    text_width = (len(normalized) * 5 + max(0, len(normalized) - 1) * 2) * scale
    origin_x = (width - text_width) // 2
    origin_y = (height - 7 * scale) // 2
    for char_index, character in enumerate(normalized):
        glyph = glyphs[character]
        char_x = origin_x + char_index * 7 * scale
        for row, bitmap_row in enumerate(glyph):
            for column, value in enumerate(bitmap_row):
                if value != "1":
                    continue
                for y in range(row * scale, (row + 1) * scale):
                    for x in range(column * scale, (column + 1) * scale):
                        pixel = ((origin_y + y) * width + char_x + x) * 3
                        pixels[pixel : pixel + 3] = b"\xff\xff\xff"
    path.write_bytes(f"P6\n{width} {height}\n255\n".encode() + pixels)

for index, subtitle in enumerate(subtitles):
    placement = placements[subtitle["clip_id"]]
    start = placement["start_frame"] / 24
    end = (placement["start_frame"] + placement["duration_frames"]) / 24
    subtitle_image = output_path.parent / f"{subtitle['id']}.ppm"
    write_subtitle_image(subtitle_image, subtitle["text"])
    subtitle_images.append(subtitle_image)
    input_index = len(clips) + index
    subtitle_label = f"[subtitle{index}]"
    filters.append(f"[{input_index}:v]setpts=PTS-STARTPTS{subtitle_label}")
    next_label = f"[vsub{index}]"
    filters.append(
        f"{video_output}{subtitle_label}overlay=0:280:"
        f"enable='between(t,{start:.3f},{end:.3f})'{next_label}"
    )
    video_output = next_label

command = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y"]
for clip in clips:
    source = clip["source"].removeprefix("asset://")
    command.extend(["-i", str(media_dir / f"{source}.mp4")])
for subtitle_image in subtitle_images:
    command.extend(["-loop", "1", "-framerate", "24", "-t", "9", "-i", str(subtitle_image)])
command.extend(
    [
        "-filter_complex",
        ";".join(filters),
        "-map",
        video_output,
        "-map",
        "[acat]",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "128k",
        "-shortest",
        str(output_path),
    ]
)
subprocess.run(command, check=True)

probe = subprocess.check_output(
    [
        "ffprobe",
        "-v",
        "error",
        "-show_entries",
        "format=duration:stream=codec_type,nb_frames",
        "-of",
        "json",
        str(output_path),
    ],
    text=True,
)
probe_data = json.loads(probe)
duration = float(probe_data["format"]["duration"])
video_stream = next(stream for stream in probe_data["streams"] if stream["codec_type"] == "video")
if not 8.7 <= duration <= 9.3:
    raise SystemExit(f"unexpected metadata duration: {duration}")
if int(video_stream["nb_frames"]) != 216:
    raise SystemExit(f"unexpected metadata video frames: {video_stream['nb_frames']}")

subprocess.run(
    ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-ss", "4.5", "-i", str(output_path), "-frames:v", "1", str(frame_path)],
    check=True,
)

def mean_volume(path: Path, start: str | None = None) -> float:
    command = ["ffmpeg", "-hide_banner"]
    if start is not None:
        command.extend(["-ss", start, "-t", "3"])
    command.extend(["-i", str(path), "-af", "volumedetect", "-f", "null", "-"])
    result = subprocess.run(command, capture_output=True, text=True, check=True)
    match = re.search(r"mean_volume:\s+(-?[0-9.]+) dB", result.stderr)
    if not match:
        raise SystemExit(f"mean volume was not reported for {path}")
    return float(match.group(1))

source_volume = mean_volume(media_dir / "b.mp4")
rendered_volume = mean_volume(output_path, "3")
volume_delta = rendered_volume - source_volume
if not -4.2 <= volume_delta <= -1.8:
    raise SystemExit(f"unexpected clip b gain delta: {volume_delta:.2f} dB")

print(
    f"validated metadata render duration={duration:.3f}s "
    f"video_frames={video_stream['nb_frames']} gain_delta={volume_delta:.2f}dB "
    f"subtitle_frame={frame_path}"
)
PY

ffprobe -v error \
    -show_entries format=duration:stream=codec_type,codec_name,nb_frames \
    -of json "$output_file"
