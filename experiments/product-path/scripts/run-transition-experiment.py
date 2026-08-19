#!/usr/bin/env python3
"""Validate semantic crossfade planning, rendering, and cache boundaries."""

from __future__ import annotations

import json
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
STATE = OUTPUT / "transition-state.json"
RENDERED = OUTPUT / "transition-crossfade.mp4"


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
    require(len(responses) == 5, f"unexpected transition response count: {responses}")
    require(
        all(response["ok"] for response in responses[:3]),
        f"transition state failed: {responses}",
    )
    require(
        responses[3]["result"] == {"pass": True, "version": 1},
        f"transition verify failed: {responses[3]}",
    )
    require(responses[4]["ok"], f"transition cache keys failed: {responses[4]}")
    return responses[2]["result"], responses[4]["result"]


def render_crossfade() -> None:
    sources = [MEDIA / f"{name}.mp4" for name in ("a", "b", "c")]
    filters = [
        "[0:v]setpts=PTS-STARTPTS,format=yuv420p[v0]",
        "[1:v]setpts=PTS-STARTPTS,format=yuv420p[v1]",
        "[2:v]setpts=PTS-STARTPTS,format=yuv420p[v2]",
        "[0:a]asetpts=PTS-STARTPTS[a0]",
        "[1:a]asetpts=PTS-STARTPTS[a1]",
        "[2:a]asetpts=PTS-STARTPTS[a2]",
        "[v1][v2]xfade=transition=fade:duration=1:offset=7[vbc]",
        "[a1][a2]acrossfade=d=1[a_bc]",
        "[v0][vbc]concat=n=2:v=1:a=0[vout]",
        "[a0][a_bc]concat=n=2:v=0:a=1[audio_out]",
    ]
    command = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y"]
    for source in sources:
        command.extend(["-i", str(source)])
    command.extend(
        [
            "-filter_complex",
            ";".join(filters),
            "-map",
            "[vout]",
            "-map",
            "[audio_out]",
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
            str(RENDERED),
        ]
    )
    subprocess.run(command, check=True)


def write_state(state: dict) -> None:
    output_root = OUTPUT.resolve()
    state_path = STATE.resolve()
    if state_path.parent != output_root:
        raise RuntimeError(f"state path escapes output directory: {STATE}")
    state_path.write_text(json.dumps(state, indent=2) + "\n", encoding="utf-8")


def probe_output() -> tuple[float, int]:
    try:
        data = json.loads(
            subprocess.check_output(
                [
                    "ffprobe",
                    "-v",
                    "error",
                    "-show_entries",
                    "format=duration:stream=codec_type,nb_frames",
                    "-of",
                    "json",
                    str(RENDERED),
                ],
                text=True,
            )
        )
        duration = float(data["format"]["duration"])
        video = next(
            stream for stream in data["streams"] if stream["codec_type"] == "video"
        )
        return duration, int(video["nb_frames"])
    except (
        KeyError,
        StopIteration,
        TypeError,
        ValueError,
        json.JSONDecodeError,
    ) as error:
        raise RuntimeError(
            f"invalid transition ffprobe response for {RENDERED}"
        ) from error


def main() -> None:
    media = support.media_hashes(MEDIA, expected_count=6)
    base_state, base_keys = runtime_state([], media)
    transition_state, transition_keys = runtime_state(
        [
            {
                "op": "add_transition",
                "id": "crossfade-b-c",
                "from_clip_id": "b",
                "to_clip_id": "c",
                "kind": "crossfade",
                "duration_frames": 24,
            }
        ],
        media,
    )
    require(
        base_state["transitions"] == [], f"unexpected base transitions: {base_state}"
    )
    require(
        transition_state["transitions"]
        == [
            {
                "id": "crossfade-b-c",
                "from_clip_id": "b",
                "to_clip_id": "c",
                "kind": "crossfade",
                "duration_frames": 24,
            }
        ],
        f"unexpected transition state: {transition_state}",
    )
    require(
        base_keys["media_render_key"] != transition_keys["media_render_key"],
        "transition did not change media key",
    )
    require(
        base_keys["transition_render_key"] != transition_keys["transition_render_key"],
        "transition key did not change",
    )
    require(
        base_keys["clip_media_render_keys"]
        == transition_keys["clip_media_render_keys"],
        "transition changed clip-local keys",
    )

    render_crossfade()
    duration, frames = probe_output()
    require(22.7 <= duration <= 23.3, f"unexpected crossfade duration: {duration}")
    require(550 <= frames <= 552, f"unexpected crossfade video frames: {frames}")
    write_state(transition_state)
    print(
        json.dumps(
            {"duration": duration, "frames": frames, "transition": "crossfade-b-c"},
            separators=(",", ":"),
        )
    )
    print("validated semantic transition planning, rendering, and cache boundaries")


if __name__ == "__main__":
    main()
