#!/usr/bin/env python3
"""Validate cache reuse on the 48-second semantic timeline fixture."""

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
CACHE = OUTPUT / "long-artifact-cache"
RENDERED = OUTPUT / "long-metadata-cached.mp4"
SOURCE_ARTIFACT = OUTPUT / "long-metadata.mp4"


def run_state(edits: list[dict], media: dict[str, str]) -> dict[str, str]:
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
        all(response["ok"] for response in responses[:3]),
        f"long state failed: {responses}",
    )
    require(
        responses[3]["result"] == {"pass": True, "version": 1},
        f"long verify failed: {responses[3]}",
    )
    require(responses[4]["ok"], f"long cache key generation failed: {responses[4]}")
    return responses[4]["result"]


def cache_status(key: str) -> str:
    result = support.cache_command(MANIFEST, "lookup", str(CACHE), key, str(RENDERED))
    status = result.get("status")
    if not isinstance(status, str) or status not in {"hit", "miss"}:
        raise RuntimeError(f"invalid cache status: {result}")
    return status


def render_or_reuse(key: str) -> str:
    if cache_status(key) == "hit":
        return "hit"

    subprocess.run(
        ["bash", str(ROOT / "scripts" / "render-long-fixture.sh")],
        check=True,
    )
    require(SOURCE_ARTIFACT.exists(), f"renderer did not produce {SOURCE_ARTIFACT}")
    digest = hashlib.sha256(SOURCE_ARTIFACT.read_bytes()).hexdigest()
    support.cache_command(
        MANIFEST, "publish", str(CACHE), key, f"sha256:{digest}", str(SOURCE_ARTIFACT)
    )
    require(cache_status(key) == "hit", "published long artifact was not readable")
    return "miss"


def main() -> None:
    if CACHE.exists():
        try:
            shutil.rmtree(CACHE)
        except OSError as error:
            raise RuntimeError(f"cannot clear long artifact cache: {CACHE}") from error
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
    marker_only_edits = base_edits + [
        {
            "op": "add_marker",
            "id": "long-marker-2",
            "clip_id": "c",
            "label": "end beat",
        },
    ]
    gain_change_edits = [
        {"op": "set_audio_gain", "clip_id": "d", "gain_db_milli": -6000},
        *base_edits[1:],
    ]

    base_keys = run_state(base_edits, media)
    first = render_or_reuse(base_keys["media_render_key"])
    second = render_or_reuse(base_keys["media_render_key"])
    marker_keys = run_state(marker_only_edits, media)
    marker = render_or_reuse(marker_keys["media_render_key"])
    gain_keys = run_state(gain_change_edits, media)

    require(first == "miss", f"long first render should miss: {first}")
    require(second == "hit", f"long second render should hit: {second}")
    require(marker == "hit", f"long marker-only render should hit: {marker}")
    require(
        base_keys["media_render_key"] == marker_keys["media_render_key"],
        "marker changed long media key",
    )
    require(
        base_keys["subtitle_overlay_key"] == marker_keys["subtitle_overlay_key"],
        "marker changed long subtitle key",
    )
    require(
        base_keys["media_render_key"] != gain_keys["media_render_key"],
        "gain reused long media key",
    )
    require(
        cache_status(gain_keys["media_render_key"]) == "miss",
        "gain unexpectedly had a long artifact",
    )

    print(
        json.dumps(
            {
                "base_key": base_keys["media_render_key"],
                "gain_key": gain_keys["media_render_key"],
                "first": first,
                "second": second,
                "marker_only": marker,
            },
            separators=(",", ":"),
        )
    )
    print("validated long artifact miss, hit, marker-only reuse, and gain invalidation")


if __name__ == "__main__":
    main()
