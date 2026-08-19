#!/usr/bin/env python3
"""Exercise artifact cache hit/miss behavior using typed Raccord cache keys."""

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
MEDIA = ROOT / "fixtures" / "media"
OUTPUT = ROOT / "fixtures" / "output"
CACHE = OUTPUT / "artifact-cache"
RENDERED = OUTPUT / "metadata-cached.mp4"


def state_for(edits: list[dict], media: dict[str, str]) -> tuple[dict, dict[str, str]]:
    requests = [
        {"tool": "plan_edit", "args": {"base_revision": 0, "edits": edits}},
        {"tool": "commit_edit", "args": {"plan_token": "plan-1"}},
        {"tool": "inspect"},
        {"tool": "verify"},
        {"tool": "cache_keys", "args": {"media_hashes": media}},
    ]
    responses = support.run_jsonl_runtime(MANIFEST, requests)
    require(len(responses) == 5, f"unexpected response count: {responses}")
    require(
        responses[0]["ok"] and responses[1]["ok"] and responses[2]["ok"],
        f"state transaction failed: {responses}",
    )
    require(
        responses[3]["result"]["pass"], f"state verification failed: {responses[3]}"
    )
    require(responses[4]["ok"], f"cache key generation failed: {responses[4]}")
    return responses[2]["result"], responses[4]["result"]


def cache_status(media_key: str) -> str:
    result = support.cache_command(
        MANIFEST, "lookup", str(CACHE), media_key, str(RENDERED)
    )
    status = result.get("status")
    if not isinstance(status, str) or status not in {"hit", "miss"}:
        raise RuntimeError(f"invalid cache status: {result}")
    return status


def render_or_reuse(media_key: str) -> str:
    if cache_status(media_key) == "hit":
        return "hit"

    subprocess.run([str(ROOT / "scripts" / "render-metadata.sh")], check=True)
    source = OUTPUT / "metadata.mp4"
    require(source.exists(), f"renderer did not produce {source}")
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    support.cache_command(
        MANIFEST,
        "publish",
        str(CACHE),
        media_key,
        f"sha256:{digest}",
        str(source),
    )
    require(cache_status(media_key) == "hit", "published artifact was not readable")
    return "miss"


def main() -> None:
    if CACHE.exists():
        try:
            shutil.rmtree(CACHE)
        except OSError as error:
            raise RuntimeError(f"cannot clear artifact cache: {CACHE}") from error
    media = support.media_hashes(MEDIA)
    edits = [
        {"op": "set_audio_gain", "clip_id": "b", "gain_db_milli": -3000},
        {"op": "add_marker", "id": "marker-1", "clip_id": "b", "label": "beat drop"},
        {"op": "add_subtitle", "id": "subtitle-1", "clip_id": "b", "text": "Hello"},
    ]
    marker_only_edits = edits + [
        {"op": "add_marker", "id": "marker-2", "clip_id": "c", "label": "end beat"},
    ]
    gain_change_edits = [
        {"op": "set_audio_gain", "clip_id": "b", "gain_db_milli": -6000},
        {"op": "add_marker", "id": "marker-1", "clip_id": "b", "label": "beat drop"},
        {"op": "add_subtitle", "id": "subtitle-1", "clip_id": "b", "text": "Hello"},
    ]

    _, base_keys = state_for(edits, media)
    first = render_or_reuse(base_keys["media_render_key"])
    second = render_or_reuse(base_keys["media_render_key"])
    _, marker_keys = state_for(marker_only_edits, media)
    marker = render_or_reuse(marker_keys["media_render_key"])
    _, gain_keys = state_for(gain_change_edits, media)

    require(first == "miss", f"first render should miss: {first}")
    require(second == "hit", f"second render should hit: {second}")
    require(marker == "hit", f"marker-only render should hit: {marker}")
    require(
        gain_keys["media_render_key"] != base_keys["media_render_key"],
        "gain change reused the media key",
    )
    require(
        cache_status(gain_keys["media_render_key"]) == "miss",
        "gain change unexpectedly had an artifact",
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
    print("validated artifact cache miss, hit, and marker-only reuse")


if __name__ == "__main__":
    main()
