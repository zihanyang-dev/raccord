"""Shared process and cache helpers for product-path experiments."""

from __future__ import annotations

import hashlib
import json
import subprocess
from pathlib import Path
from typing import Any


def require(condition: bool, detail: str) -> None:
    if not condition:
        raise RuntimeError(detail)


def media_hashes(media_dir: Path, expected_count: int | None = None) -> dict[str, str]:
    hashes = {
        path.stem: hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(media_dir.glob("*.mp4"))
    }
    require(bool(hashes), f"fixture media is missing: {media_dir}")
    if expected_count is not None:
        require(
            len(hashes) == expected_count,
            f"expected {expected_count} media clips, found {len(hashes)}",
        )
    return hashes


def run_jsonl_runtime(
    manifest: Path,
    requests: list[dict[str, Any]],
    runtime_args: list[str] | None = None,
) -> list[dict[str, Any]]:
    payload = (
        "\n".join(json.dumps(request, separators=(",", ":")) for request in requests)
        + "\n"
    )
    command = [
        "mise",
        "exec",
        "--",
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(manifest),
        "--bin",
        "jsonl_api",
    ]
    if runtime_args:
        command.extend(["--", *runtime_args])
    completed = subprocess.run(
        command,
        input=payload,
        text=True,
        check=True,
        capture_output=True,
    )
    responses: list[dict[str, Any]] = []
    for line in completed.stdout.splitlines():
        if not line.strip():
            continue
        try:
            response = json.loads(line)
        except json.JSONDecodeError as error:
            raise RuntimeError(f"invalid runtime response: {line}") from error
        if not isinstance(response, dict):
            raise RuntimeError(f"runtime response must be an object: {line}")
        responses.append(response)
    return responses


def cache_command(manifest: Path, *arguments: str) -> dict[str, Any]:
    completed = subprocess.run(
        [
            "mise",
            "exec",
            "--",
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            str(manifest),
            "--bin",
            "artifact_cache",
            "--",
            *arguments,
        ],
        text=True,
        check=True,
        capture_output=True,
    )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(
            f"invalid artifact cache response: {completed.stdout}"
        ) from error
    if not isinstance(result, dict):
        raise RuntimeError(f"artifact cache response must be an object: {result}")
    return result
