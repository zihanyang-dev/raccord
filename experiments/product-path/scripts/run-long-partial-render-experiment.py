#!/usr/bin/env python3
"""Measure per-clip artifact reuse and partial re-concatenation on the long fixture."""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
from importlib import import_module
from pathlib import Path

support = import_module("experiment_support")
require = support.require

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "Cargo.toml"
TIMELINE = ROOT / "fixtures" / "long-timeline.json"
MEDIA = ROOT / "fixtures" / "long-media"
OUTPUT = ROOT / "fixtures" / "output"
CACHE = OUTPUT / "long-clip-cache"
OVERLAY_CACHE = OUTPUT / "long-overlay-cache"
SEGMENTS = OUTPUT / "long-partial-segments"


def runtime_state(edits: list[dict], media: dict[str, str]) -> tuple[dict, dict]:
    requests = [
        {"tool": "plan_edit", "args": {"base_revision": 0, "edits": edits}},
        {"tool": "commit_edit", "args": {"plan_token": "plan-1"}},
        {"tool": "inspect"},
        {"tool": "verify"},
        {"tool": "cache_keys", "args": {"media_hashes": media}},
    ]
    responses = support.run_jsonl_runtime(
        MANIFEST, requests, ["--timeline", str(TIMELINE)]
    )
    require(len(responses) == 5, f"unexpected response count: {responses}")
    require(
        all(response["ok"] for response in responses[:3]), f"state failed: {responses}"
    )
    require(
        responses[3]["result"] == {"pass": True, "version": 1},
        f"verify failed: {responses[3]}",
    )
    require(responses[4]["ok"], f"cache key generation failed: {responses[4]}")
    return responses[2]["result"], responses[4]["result"]


def cache_status(cache_root: Path, key: str, destination: Path) -> str:
    result = support.cache_command(
        MANIFEST, "lookup", str(cache_root), key, str(destination)
    )
    status = result.get("status")
    if not isinstance(status, str) or status not in {"hit", "miss"}:
        raise RuntimeError(f"invalid cache status: {result}")
    return status


def render_segment(clip: dict, destination: Path) -> None:
    source_name = clip["source"].removeprefix("asset://")
    source = MEDIA / f"{source_name}.mp4"
    gain = clip.get("audio_gain_db_milli", 0)
    factor = 10 ** (gain / 20000)
    subprocess.run(
        [
            "ffmpeg",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            str(source),
            "-af",
            f"volume={factor:.8f}",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-shortest",
            str(destination),
        ],
        check=True,
    )


def publish_artifact(cache_root: Path, key: str, rendered: Path) -> None:
    digest = hashlib.sha256(rendered.read_bytes()).hexdigest()
    support.cache_command(
        MANIFEST, "publish", str(cache_root), key, f"sha256:{digest}", str(rendered)
    )


def concat_segments(name: str, segment_paths: list[Path]) -> Path:
    concat_file = OUTPUT / f"long-partial-{name}.txt"
    concat_file.write_text(
        "".join(f"file '{path}'\n" for path in segment_paths),
        encoding="utf-8",
    )
    output = OUTPUT / f"long-partial-{name}.mp4"
    subprocess.run(
        [
            "ffmpeg",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            str(concat_file),
            "-c",
            "copy",
            str(output),
        ],
        check=True,
    )
    return output


def render_state(name: str, state: dict, keys: dict) -> tuple[dict[str, int], Path]:
    hits = {"hit": 0, "miss": 0}
    segment_paths = []
    for clip in state["clips"]:
        clip_id = clip["id"]
        key = keys["clip_media_render_keys"][clip_id]
        destination = SEGMENTS / f"{name}-{clip_id}.mp4"
        status = cache_status(CACHE, key, destination)
        if status == "miss":
            temporary = SEGMENTS / f"{name}-{clip_id}.rendered.mp4"
            render_segment(clip, temporary)
            publish_artifact(CACHE, key, temporary)
            temporary.unlink()
            require(
                cache_status(CACHE, key, destination) == "hit",
                f"published clip {clip_id} was not readable",
            )
        hits[status] += 1
        segment_paths.append(destination)
    return hits, concat_segments(name, segment_paths)


GLYPHS = {
    "E": ["11111", "10000", "10000", "11110", "10000", "10000", "11111"],
    "H": ["10001", "10001", "10001", "11111", "10001", "10001", "10001"],
    "L": ["10000", "10000", "10000", "10000", "10000", "10000", "11111"],
    "O": ["01110", "10001", "10001", "10001", "10001", "10001", "01110"],
}


def write_subtitle_image(path: Path, text: str) -> None:
    normalized = text.upper()
    if not normalized or any(character not in GLYPHS for character in normalized):
        raise RuntimeError(f"unsupported bitmap subtitle: {text!r}")
    scale = 8
    width = 640
    height = 80
    pixels = bytearray(width * height * 3)
    text_width = (len(normalized) * 5 + (len(normalized) - 1) * 2) * scale
    origin_x = (width - text_width) // 2
    origin_y = (height - 7 * scale) // 2
    for character_index, character in enumerate(normalized):
        for row, bitmap_row in enumerate(GLYPHS[character]):
            for column, value in enumerate(bitmap_row):
                if value != "1":
                    continue
                for y in range(row * scale, (row + 1) * scale):
                    for x in range(column * scale, (column + 1) * scale):
                        pixel = (
                            (origin_y + y) * width
                            + origin_x
                            + character_index * 7 * scale
                            + column * scale
                            + x
                        ) * 3
                        pixels[pixel : pixel + 3] = b"\xff\xff\xff"
    path.write_bytes(f"P6\n{width} {height}\n255\n".encode() + pixels)


def render_overlay(
    name: str, media_output: Path, state: dict, keys: dict
) -> tuple[str, Path]:
    subtitle = state["subtitles"][0]
    placement = next(
        placement
        for placement in state["placements"]
        if placement["id"] == subtitle["clip_id"]
    )
    start = placement["start_frame"] / 24
    end = (placement["start_frame"] + placement["duration_frames"]) / 24
    destination = OUTPUT / f"long-overlay-{name}.mp4"
    key = keys["subtitle_overlay_key"]
    status = cache_status(OVERLAY_CACHE, key, destination)
    if status == "miss":
        subtitle_path = OUTPUT / f"long-overlay-{name}.ppm"
        temporary = OUTPUT / f"long-overlay-{name}.rendered.mp4"
        write_subtitle_image(subtitle_path, subtitle["text"])
        subprocess.run(
            [
                "ffmpeg",
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-i",
                str(media_output),
                "-loop",
                "1",
                "-framerate",
                "24",
                "-t",
                "48",
                "-i",
                str(subtitle_path),
                "-filter_complex",
                f"[1:v]setpts=PTS-STARTPTS[subtitle];[0:v][subtitle]overlay=0:280:enable='between(t,{start:.3f},{end:.3f})'[video]",
                "-map",
                "[video]",
                "-map",
                "0:a",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "copy",
                "-shortest",
                str(temporary),
            ],
            check=True,
        )
        publish_artifact(OVERLAY_CACHE, key, temporary)
        temporary.unlink()
        subtitle_path.unlink()
        require(
            cache_status(OVERLAY_CACHE, key, destination) == "hit",
            f"published overlay {name} was not readable",
        )
    return status, destination


def duration_and_frames(path: Path) -> tuple[float, int]:
    probe = subprocess.check_output(
        [
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,nb_frames",
            "-of",
            "json",
            str(path),
        ],
        text=True,
    )
    try:
        data = json.loads(probe)
        duration = float(data["format"]["duration"])
        video = next(
            stream for stream in data["streams"] if stream["codec_type"] == "video"
        )
        frames = int(video["nb_frames"])
    except (KeyError, StopIteration, TypeError, ValueError) as error:
        raise RuntimeError(f"invalid ffprobe response for {path}") from error
    return duration, frames


def mean_volume(path: Path, start: str) -> float:
    result = subprocess.run(
        [
            "ffmpeg",
            "-hide_banner",
            "-ss",
            start,
            "-t",
            "3",
            "-i",
            str(path),
            "-af",
            "volumedetect",
            "-f",
            "null",
            "-",
        ],
        capture_output=True,
        text=True,
        check=True,
    )
    marker = "mean_volume:"
    for line in result.stderr.splitlines():
        if marker not in line:
            continue
        try:
            return float(line.split(marker, 1)[1].split("dB", 1)[0])
        except (IndexError, ValueError) as error:
            raise RuntimeError(f"invalid mean volume for {path}") from error
    raise RuntimeError(f"mean volume missing for {path}")


def main() -> None:
    for path in (CACHE, OVERLAY_CACHE, SEGMENTS):
        if path.exists():
            try:
                shutil.rmtree(path)
            except OSError as error:
                raise RuntimeError(
                    f"cannot clear partial-render directory: {path}"
                ) from error
    CACHE.mkdir(parents=True, exist_ok=True)
    OVERLAY_CACHE.mkdir(parents=True, exist_ok=True)
    SEGMENTS.mkdir(parents=True, exist_ok=True)

    media = support.media_hashes(MEDIA, expected_count=6)
    base_edits = [
        {"op": "set_audio_gain", "clip_id": "d", "gain_db_milli": -3000},
        {
            "op": "add_marker",
            "id": "long-marker-1",
            "clip_id": "d",
            "label": "middle beat",
        },
        {
            "op": "add_subtitle",
            "id": "long-subtitle-1",
            "clip_id": "b",
            "text": "HELLO",
        },
    ]
    marker_edits = base_edits + [
        {
            "op": "add_marker",
            "id": "long-marker-2",
            "clip_id": "c",
            "label": "end beat",
        },
    ]
    gain_edits = [
        {"op": "set_audio_gain", "clip_id": "d", "gain_db_milli": -6000},
        *base_edits[1:],
    ]
    subtitle_edits = [
        *base_edits[:2],
        {"op": "add_subtitle", "id": "long-subtitle-1", "clip_id": "b", "text": "E"},
    ]

    base_state, base_keys = runtime_state(base_edits, media)
    base_counts, base_output = render_state("base", base_state, base_keys)
    marker_state, marker_keys = runtime_state(marker_edits, media)
    marker_counts, marker_output = render_state("marker", marker_state, marker_keys)
    gain_state, gain_keys = runtime_state(gain_edits, media)
    gain_counts, gain_output = render_state("gain", gain_state, gain_keys)
    subtitle_state, subtitle_keys = runtime_state(subtitle_edits, media)
    subtitle_counts, subtitle_output = render_state(
        "subtitle", subtitle_state, subtitle_keys
    )
    base_overlay, base_overlay_output = render_overlay(
        "base", base_output, base_state, base_keys
    )
    marker_overlay, marker_overlay_output = render_overlay(
        "marker", marker_output, marker_state, marker_keys
    )
    subtitle_overlay, subtitle_overlay_output = render_overlay(
        "subtitle", subtitle_output, subtitle_state, subtitle_keys
    )

    require(
        base_counts == {"hit": 0, "miss": 6},
        f"unexpected base cache counts: {base_counts}",
    )
    require(
        marker_counts == {"hit": 6, "miss": 0},
        f"unexpected marker cache counts: {marker_counts}",
    )
    require(
        gain_counts == {"hit": 5, "miss": 1},
        f"unexpected gain cache counts: {gain_counts}",
    )
    require(
        subtitle_counts == {"hit": 6, "miss": 0},
        f"unexpected subtitle cache counts: {subtitle_counts}",
    )
    require(
        (base_overlay, marker_overlay, subtitle_overlay) == ("miss", "hit", "miss"),
        f"unexpected overlay cache statuses: {(base_overlay, marker_overlay, subtitle_overlay)}",
    )
    require(
        base_keys["subtitle_overlay_key"] == marker_keys["subtitle_overlay_key"],
        "marker changed subtitle overlay key",
    )
    require(
        base_keys["subtitle_overlay_key"] != subtitle_keys["subtitle_overlay_key"],
        "subtitle change reused overlay key",
    )
    for output in (base_output, marker_output, gain_output, subtitle_output):
        duration, frames = duration_and_frames(output)
        require(47.7 <= duration <= 48.3, f"unexpected partial duration: {duration}")
        require(frames == 1152, f"unexpected partial frame count: {frames}")
    for output in (base_overlay_output, marker_overlay_output, subtitle_overlay_output):
        duration, frames = duration_and_frames(output)
        require(47.7 <= duration <= 48.3, f"unexpected overlay duration: {duration}")
        require(1150 <= frames <= 1152, f"unexpected overlay frame count: {frames}")

    source_volume = mean_volume(MEDIA / "d.mp4", "0")
    gain_volume = mean_volume(gain_output, "24")
    require(
        -7.2 <= gain_volume - source_volume <= -4.8,
        "gain segment was not re-rendered at -6 dB",
    )

    print(
        json.dumps(
            {
                "base": base_counts,
                "marker_only": marker_counts,
                "gain_change": gain_counts,
                "subtitle_change": subtitle_counts,
                "overlay": {
                    "base": base_overlay,
                    "marker_only": marker_overlay,
                    "subtitle_change": subtitle_overlay,
                },
            },
            separators=(",", ":"),
        )
    )
    print(
        "validated per-clip reuse, gain-local invalidation, and subtitle overlay reuse"
    )


if __name__ == "__main__":
    main()
