#!/usr/bin/env python3
"""Measure deterministic cache keys and invalidation boundaries."""

from __future__ import annotations

import json
from importlib import import_module
from pathlib import Path

support = import_module("experiment_support")
require = support.require

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "Cargo.toml"
MEDIA = ROOT / "fixtures" / "media"


def run_state(
    edits: list[dict], media: dict[str, str]
) -> tuple[dict, list[str], dict[str, str]]:
    requests = [
        {"tool": "plan_edit", "args": {"base_revision": 0, "edits": edits}},
        {"tool": "commit_edit", "args": {"plan_token": "plan-1"}},
        {"tool": "inspect"},
        {"tool": "verify"},
        {"tool": "cache_keys", "args": {"media_hashes": media}},
    ]
    responses = support.run_jsonl_runtime(MANIFEST, requests)
    support.require(len(responses) == 5, f"unexpected response count: {responses}")
    support.require(
        responses[0]["ok"] and responses[1]["ok"] and responses[2]["ok"],
        f"state transaction failed: {responses}",
    )
    require(
        responses[3]["result"]["pass"], f"state verification failed: {responses[3]}"
    )
    require(responses[4]["ok"], f"cache key generation failed: {responses[4]}")
    return (
        responses[2]["result"],
        responses[0]["result"]["changed_clip_ids"],
        responses[4]["result"],
    )


def main() -> None:
    media = support.media_hashes(MEDIA)
    base_edits = [
        {"op": "set_audio_gain", "clip_id": "b", "gain_db_milli": -3000},
        {"op": "add_marker", "id": "marker-1", "clip_id": "b", "label": "beat drop"},
        {"op": "add_subtitle", "id": "subtitle-1", "clip_id": "b", "text": "Hello"},
    ]
    marker_only_edits = base_edits + [
        {"op": "add_marker", "id": "marker-2", "clip_id": "c", "label": "end beat"},
    ]
    gain_change_edits = [
        {"op": "set_audio_gain", "clip_id": "b", "gain_db_milli": -6000},
        {"op": "add_marker", "id": "marker-1", "clip_id": "b", "label": "beat drop"},
        {"op": "add_subtitle", "id": "subtitle-1", "clip_id": "b", "text": "Hello"},
    ]

    base, base_changed, base_keys = run_state(base_edits, media)
    marker_only, marker_changed, marker_keys = run_state(marker_only_edits, media)
    gain_change, gain_changed, gain_keys = run_state(gain_change_edits, media)

    require(
        base_keys["media_render_key"] == marker_keys["media_render_key"],
        "marker-only edit invalidated media render",
    )
    require(
        base_keys["subtitle_overlay_key"] == marker_keys["subtitle_overlay_key"],
        "marker-only edit invalidated subtitle overlay",
    )
    require(
        base_keys["metadata_key"] != marker_keys["metadata_key"],
        "marker-only edit did not change metadata key",
    )
    require(
        base_keys["media_render_key"] != gain_keys["media_render_key"],
        "audio gain edit did not invalidate media render",
    )
    require(
        base_keys["subtitle_overlay_key"] == gain_keys["subtitle_overlay_key"],
        "audio gain edit invalidated subtitle overlay",
    )
    require(
        base_changed == ["b", "b", "b"],
        f"unexpected base affected clips: {base_changed}",
    )
    require(
        marker_changed[-1] == "c", f"marker edit did not identify c: {marker_changed}"
    )
    require(gain_changed[0] == "b", f"gain edit did not identify b: {gain_changed}")

    for name, changed, render_keys in [
        ("base", base_changed, base_keys),
        ("marker_only", marker_changed, marker_keys),
        ("gain_change", gain_changed, gain_keys),
    ]:
        print(
            json.dumps(
                {"case": name, "changed_clip_ids": changed, **render_keys},
                separators=(",", ":"),
            )
        )
    print("validated marker-only cache reuse and audio-gain invalidation")


if __name__ == "__main__":
    main()
