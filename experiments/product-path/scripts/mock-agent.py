#!/usr/bin/env python3
"""Replay reference requests through the interactive Agent session protocol."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def load_tasks(path: Path) -> dict[str, dict]:
    try:
        with path.open(encoding="utf-8") as stream:
            entries = json.load(stream)
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeError(f"cannot load mock-agent fixture: {path}") from error
    if not isinstance(entries, list):
        raise RuntimeError("mock-agent fixture must contain a list")
    tasks = {}
    for entry in entries:
        if not isinstance(entry, dict) or not isinstance(entry.get("id"), str):
            raise RuntimeError("mock-agent fixture contains an invalid task")
        tasks[entry["id"]] = entry
    return tasks


def send(message: dict) -> None:
    print(json.dumps(message, separators=(",", ":")), flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, required=True)
    args = parser.parse_args()
    tasks = load_tasks(args.fixture)

    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError as error:
            raise RuntimeError(f"mock agent received invalid JSON: {line}") from error
        if message.get("type") != "task":
            raise RuntimeError(f"mock agent received unexpected message: {message}")

        task_id = message.get("id")
        if task_id not in tasks:
            raise RuntimeError(f"mock agent does not know task: {task_id}")
        for request in tasks[task_id]["requests"]:
            send({"type": "request", "request": request})
            response_line = sys.stdin.readline()
            if not response_line:
                raise RuntimeError("session ended before the tool response")
            try:
                json.loads(response_line)
            except json.JSONDecodeError as error:
                raise RuntimeError(
                    f"mock agent received invalid tool response: {response_line}"
                ) from error
        send({"type": "done"})


if __name__ == "__main__":
    main()
