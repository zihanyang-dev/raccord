#!/usr/bin/env python3
"""Deterministic policy Agent for testing feedback-driven tool loops."""

from __future__ import annotations

import json
import sys
from collections.abc import Callable
from typing import cast


def require(condition: bool, detail: str) -> None:
    if not condition:
        raise RuntimeError(detail)


def send(message: dict) -> None:
    print(json.dumps(message, separators=(",", ":")), flush=True)


def receive_response() -> dict:
    line = sys.stdin.readline()
    require(bool(line), "router ended before the policy response")
    try:
        message = json.loads(line)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"router emitted invalid JSON: {line}") from error
    require(message.get("type") == "response", f"unexpected router message: {message}")
    response = message.get("response")
    require(isinstance(response, dict), "router response must be an object")
    return response


def call(tool: str, args: dict | None = None) -> dict:
    request: dict[str, object] = {"tool": tool}
    if args is not None:
        request["args"] = args
    send({"type": "request", "request": request})
    return receive_response()


def require_ok(response: dict, label: str) -> dict:
    require(response.get("ok", False), f"{label} failed: {response}")
    result = response.get("result")
    require(isinstance(result, dict), f"{label} returned no result")
    return cast(dict, result)


def find_clip(query: str) -> tuple[str | None, dict]:
    response = call("find", {"query": query})
    result = require_ok(response, "find")
    matches = result.get("matches", [])
    require(isinstance(matches, list), "find matches must be a list")
    if not matches:
        return None, result
    clip_id = matches[0].get("id")
    require(isinstance(clip_id, str), "find result has no clip id")
    return clip_id, result


def inspect_clip(clip_id: str) -> tuple[int, dict]:
    result = require_ok(call("inspect", {"ids": [clip_id]}), "inspect")
    revision = result.get("revision")
    require(isinstance(revision, int), "inspect result has no revision")
    return cast(int, revision), result


def apply_edit(revision: int, edits: list[dict]) -> dict:
    return call("plan_edit", {"base_revision": revision, "edits": edits})


def commit(plan: dict) -> dict:
    plan_token = plan.get("plan_token")
    require(isinstance(plan_token, str), "plan result has no plan token")
    return require_ok(call("commit_edit", {"plan_token": plan_token}), "commit")


def finish() -> None:
    result = require_ok(call("verify"), "verify")
    require(bool(result.get("pass")), "policy Agent verification failed")
    send({"type": "done"})


def require_clip_id(clip_id: str | None, detail: str) -> str:
    require(clip_id is not None, detail)
    return cast(str, clip_id)


def run_delete_b() -> None:
    clip_id = require_clip_id(
        find_clip("asset://b")[0], "policy Agent could not find b"
    )
    revision, _ = inspect_clip(clip_id)
    plan = require_ok(
        apply_edit(revision, [{"op": "ripple_delete", "clip_id": clip_id}]), "plan"
    )
    commit(plan)
    finish()


def run_replace_and_trim_b() -> None:
    clip_id = require_clip_id(
        find_clip("asset://b")[0], "policy Agent could not find b"
    )
    revision, _ = inspect_clip(clip_id)
    plan = require_ok(
        apply_edit(
            revision,
            [
                {
                    "op": "replace_source",
                    "clip_id": clip_id,
                    "source": "asset://b-take-2",
                },
                {"op": "trim", "clip_id": clip_id, "duration_frames": 48},
            ],
        ),
        "plan",
    )
    commit(plan)
    finish()


def run_insert_d() -> None:
    anchor = require_clip_id(find_clip("asset://a")[0], "policy Agent could not find a")
    revision, _ = inspect_clip(anchor)
    plan = require_ok(
        apply_edit(
            revision,
            [
                {
                    "op": "insert_after",
                    "after": anchor,
                    "clip": {"id": "d", "source": "asset://d", "duration_frames": 24},
                }
            ],
        ),
        "plan",
    )
    commit(plan)
    finish()


def run_metadata_b() -> None:
    clip_id = require_clip_id(
        find_clip("asset://b")[0], "policy Agent could not find b"
    )
    revision, _ = inspect_clip(clip_id)
    plan = require_ok(
        apply_edit(
            revision,
            [
                {
                    "op": "set_audio_gain",
                    "clip_id": clip_id,
                    "gain_db_milli": -3000,
                },
                {
                    "op": "add_marker",
                    "id": "marker-1",
                    "clip_id": clip_id,
                    "label": "beat drop",
                },
                {
                    "op": "add_subtitle",
                    "id": "subtitle-1",
                    "clip_id": clip_id,
                    "text": "Hello",
                },
            ],
        ),
        "metadata plan",
    )
    commit(plan)
    finish()


def run_move_c() -> None:
    clip_id = require_clip_id(
        find_clip("asset://c")[0], "policy Agent could not find c"
    )
    revision, _ = inspect_clip(clip_id)
    plan = require_ok(
        apply_edit(revision, [{"op": "move_after", "clip_id": clip_id, "after": None}]),
        "plan",
    )
    commit(plan)
    finish()


def run_missing_recovery() -> None:
    missing_id, _ = find_clip("missing")
    if missing_id is None:
        response = apply_edit(0, [{"op": "ripple_delete", "clip_id": "missing"}])
        require(
            response.get("error", {}).get("code") == "MISSING_CLIP",
            "missing recovery did not observe MISSING_CLIP",
        )
    clip_id = require_clip_id(
        find_clip("asset://b")[0], "policy Agent could not recover b"
    )
    revision, _ = inspect_clip(clip_id)
    plan = require_ok(
        apply_edit(revision, [{"op": "ripple_delete", "clip_id": clip_id}]),
        "recovery plan",
    )
    commit(plan)
    finish()


def run_stale_recovery() -> None:
    plan = require_ok(
        apply_edit(0, [{"op": "ripple_delete", "clip_id": "b"}]), "initial plan"
    )
    commit(plan)
    stale = apply_edit(
        0, [{"op": "replace_source", "clip_id": "c", "source": "asset://c-alt"}]
    )
    require(
        stale.get("error", {}).get("code") == "STALE_REVISION",
        "stale recovery did not observe STALE_REVISION",
    )
    revision, _ = inspect_clip("c")
    plan = require_ok(
        apply_edit(
            revision,
            [{"op": "replace_source", "clip_id": "c", "source": "asset://c-alt"}],
        ),
        "recovery plan",
    )
    commit(plan)
    finish()


def run_task(task_id: str) -> None:
    handlers = {
        "delete-b": run_delete_b,
        "replace-and-trim-b": run_replace_and_trim_b,
        "insert-d-after-a": run_insert_d,
        "metadata-on-b": run_metadata_b,
        "move-c-to-start": run_move_c,
        "recover-from-missing-clip": run_missing_recovery,
        "recover-from-stale-revision": run_stale_recovery,
    }
    handler = handlers.get(task_id)
    require(handler is not None, f"policy Agent does not know task: {task_id}")
    cast(Callable[[], None], handler)()


def main() -> None:
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError as error:
            raise RuntimeError(f"Agent task is invalid JSON: {line}") from error
        require(message.get("type") == "task", f"unexpected session message: {message}")
        task_id = message.get("id")
        require(isinstance(task_id, str), "Agent task has no id")
        run_task(task_id)


if __name__ == "__main__":
    main()
