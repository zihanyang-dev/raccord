#!/usr/bin/env python3
"""Run compact semantic-edit scenarios against the JSONL experiment API."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "Cargo.toml"


def require(condition: bool, detail: str) -> None:
    if not condition:
        raise RuntimeError(detail)


def require_equal(actual: object, expected: object, detail: str) -> None:
    if actual != expected:
        raise RuntimeError(f"{detail}: expected {expected!r}, got {actual!r}")


def run(requests: list[dict]) -> list[dict]:
    command = [
        "mise",
        "exec",
        "--",
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(MANIFEST),
        "--bin",
        "jsonl_api",
    ]
    payload = "\n".join(
        json.dumps(request, separators=(",", ":")) for request in requests
    )
    completed = subprocess.run(
        command,
        input=payload + "\n",
        text=True,
        check=True,
        capture_output=True,
    )

    responses = []
    for line in completed.stdout.splitlines():
        if not line.strip():
            continue
        try:
            responses.append(json.loads(line))
        except json.JSONDecodeError as error:
            raise RuntimeError(f"invalid JSONL response: {line}") from error
    return responses


def placements(response: dict) -> list[dict]:
    return response["result"]["placements"]


def assert_ok(response: dict) -> None:
    require(response.get("ok", False), f"expected successful response: {response}")


def scenario_ripple_delete() -> None:
    responses = run(
        [
            {
                "tool": "plan_edit",
                "args": {
                    "base_revision": 0,
                    "edits": [{"op": "ripple_delete", "clip_id": "b"}],
                },
            },
            {"tool": "commit_edit", "args": {"plan_token": "plan-1"}},
            {"tool": "verify"},
        ]
    )
    assert_ok(responses[0])
    require_equal(
        placements(responses[0]),
        [
            {"id": "a", "start_frame": 0, "duration_frames": 72},
            {"id": "c", "start_frame": 72, "duration_frames": 96},
        ],
        "ripple placements",
    )
    assert_ok(responses[1])
    require(responses[2]["result"]["pass"], "ripple verification failed")


def scenario_replace_and_trim() -> None:
    responses = run(
        [
            {
                "tool": "plan_edit",
                "args": {
                    "base_revision": 0,
                    "edits": [
                        {
                            "op": "replace_source",
                            "clip_id": "b",
                            "source": "asset://b-take-2",
                        },
                        {"op": "trim", "clip_id": "b", "duration_frames": 48},
                    ],
                },
            },
            {"tool": "commit_edit", "args": {"plan_token": "plan-1"}},
        ]
    )
    assert_ok(responses[0])
    require_equal(
        placements(responses[0]),
        [
            {"id": "a", "start_frame": 0, "duration_frames": 72},
            {"id": "b", "start_frame": 72, "duration_frames": 48},
            {"id": "c", "start_frame": 120, "duration_frames": 96},
        ],
        "replace and trim placements",
    )
    assert_ok(responses[1])


def scenario_insert_and_move() -> None:
    responses = run(
        [
            {
                "tool": "plan_edit",
                "args": {
                    "base_revision": 0,
                    "edits": [
                        {
                            "op": "insert_after",
                            "after": "a",
                            "clip": {
                                "id": "d",
                                "source": "asset://d",
                                "duration_frames": 24,
                            },
                        }
                    ],
                },
            },
            {"tool": "commit_edit", "args": {"plan_token": "plan-1"}},
            {
                "tool": "plan_edit",
                "args": {
                    "base_revision": 1,
                    "edits": [{"op": "move_after", "clip_id": "c", "after": None}],
                },
            },
            {"tool": "commit_edit", "args": {"plan_token": "plan-2"}},
        ]
    )
    assert_ok(responses[0])
    assert_ok(responses[1])
    assert_ok(responses[2])
    require_equal(
        placements(responses[2]),
        [
            {"id": "c", "start_frame": 0, "duration_frames": 96},
            {"id": "a", "start_frame": 96, "duration_frames": 72},
            {"id": "d", "start_frame": 168, "duration_frames": 24},
            {"id": "b", "start_frame": 192, "duration_frames": 72},
        ],
        "insert and move placements",
    )
    assert_ok(responses[3])


def scenario_structured_error() -> None:
    responses = run(
        [
            {
                "tool": "plan_edit",
                "args": {
                    "base_revision": 0,
                    "edits": [{"op": "ripple_delete", "clip_id": "missing"}],
                },
            }
        ]
    )
    require(not responses[0]["ok"], "missing clip should fail")
    require_equal(
        responses[0]["error"]["code"], "MISSING_CLIP", "missing clip error code"
    )


def main() -> None:
    scenarios = [
        scenario_ripple_delete,
        scenario_replace_and_trim,
        scenario_insert_and_move,
        scenario_structured_error,
    ]
    for scenario in scenarios:
        scenario()
        print(f"passed: {scenario.__name__}")
    print(f"passed: {len(scenarios)} semantic API scenarios")


if __name__ == "__main__":
    main()
